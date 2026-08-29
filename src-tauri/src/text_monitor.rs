//! Native focused-field text monitor (PLAN-03).
//!
//! Reads the focused editable field's text via UI Automation on a
//! change-detection loop, honoring process exclusions and password skips,
//! and feeds the check engine. The widget that displays results is P4;
//! this module only emits `wb://issues` + `wb://field-focus`
//! (CONTRACTS §3).
//!
//! Loop conventions copied from the base repo's journal recorder:
//! `RUNNING`/`GENERATION` statics, idempotent start, generation-checked
//! ticks. All COM/UIA work runs inside `spawn_blocking` (COM MTA
//! isolation — FRICTION 2026-08-21).
//!
//! INV-MON-001: every UIA failure degrades to skip-and-backoff; the loop
//! never panics; per-target logging is rate-capped (once per minute).
//!
//! INV-PRIV-001: password fields are detected BEFORE the value read and
//! never read, checked, or logged.
//!
//! INV-PRIV-002: field text exists only in memory for the check. Logs
//! carry at most the process name and a hash prefix.
//!
//! INV-EXCL-001: excluded processes are rejected before any value read;
//! the loop sleeps long while the foreground process is excluded.
//!
//! Offsets stay UTF-16 end-to-end (UIA is UTF-16 native; INV-OFFSET-001);
//! the only conversion is harper's char boundary inside the engine.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::Emitter;

pub const TICK_MS: u64 = 250;
pub const EXCLUDED_SLEEP_MS: u64 = 1_000;
pub const UNSUPPORTED_BACKOFF_MS: u64 = 2_000;
pub const DEBOUNCE_MS: u64 = 300;

static RUNNING: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Wall-clock ms until which the monitor stays idle ("Snooze 1 h").
/// 0 = active. Checked before any read; expires on its own.
static SNOOZE_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

/// Target identity: process + field position. Keying the debounce by
/// target identity (not just text hash) survives focus flicker between
/// two fields with identical text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TargetKey {
    pub process: String,
    /// Field bounding rect (left, top, right, bottom) — stable enough to
    /// distinguish fields within one process for v1.
    pub field_rect: (i32, i32, i32, i32),
}

impl TargetKey {
    pub fn as_string(&self) -> String {
        format!(
            "native:{}@{},{}",
            self.process, self.field_rect.0, self.field_rect.1
        )
    }
}

/// One observation of the focused field. Text is present ONLY for
/// eligible fields (never for passwords; never for excluded processes —
/// those never get this far).
#[derive(Debug, Clone)]
pub struct FieldSnapshot {
    pub key: TargetKey,
    pub is_password: bool,
    pub value: Option<String>,
    /// Top-level window handle (isize ABI; FRICTION) captured while the
    /// field was focused — apply.rs re-acquires the element from it when
    /// the widget card itself holds focus at apply time.
    pub hwnd: isize,
}

/// Last-checked field identity (Send-safe; no COM pointers).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct LastField {
    pub hwnd: isize,
    pub process: String,
}

static LAST_FIELD: Mutex<Option<LastField>> = Mutex::new(None);

fn remember_field(hwnd: isize, process: String) {
    let mut guard = LAST_FIELD.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(LastField { hwnd, process });
}

/// The last-checked field identity when its process matches (INV-APPLY-001
/// identity check happens in the caller).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn last_field_for(process: &str) -> Option<(isize, String)> {
    let guard = LAST_FIELD.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .filter(|f| f.process.eq_ignore_ascii_case(process))
        .map(|f| (f.hwnd, f.process.clone()))
}

/// What a reader reports for one tick.
#[derive(Debug)]
pub enum ReadOutcome {
    Snapshot(FieldSnapshot),
    /// Foreground process is excluded. The reader resolved the process
    /// identity ONLY — no field text, patterns, or values were read
    /// (INV-EXCL-001: the check precedes any read, enforced at the
    /// reader boundary, not after the fact; verifier finding 0008).
    Excluded(String),
    /// Focused element is browser/app CHROME (address bar, find bar,
    /// tab-search) — a navigation input, not a user document. Identity
    /// and control name/class were read; NO field text was. Suggestions
    /// on search queries are noise by definition.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    UiChrome(String),
    /// No focused editable field / pattern unavailable.
    Unsupported,
    /// Transient COM failure — skip this tick, keep normal cadence.
    Transient(String),
}

/// UIA boundary, isolated behind a trait so the tick logic is testable
/// with fakes (no COM in unit tests — PLAN-03 task 1).
pub trait FocusedFieldReader: Send + Sync {
    /// The reader owns the exclusion decision because only it can
    /// guarantee the ordering: resolve the foreground process identity,
    /// and if it is excluded return [`ReadOutcome::Excluded`] WITHOUT
    /// touching any field value or pattern.
    fn read_field(&self, excluded: &[String]) -> ReadOutcome;
}

// ── Pure tick state machine ─────────────────────────────────────────

#[derive(Debug)]
enum Pending {
    /// Text changed at `changed_at`; commit a check once it has been
    /// quiet for DEBOUNCE_MS (re-reading just before commit). `hash` is
    /// the text the quiet period started on: new text restarts the
    /// window instead of firing mid-sentence (audit M5).
    Debounce {
        key: TargetKey,
        changed_at: Instant,
        hash: [u8; 32],
    },
}

pub struct MonitorState {
    last_key: Option<TargetKey>,
    last_hash: Option<[u8; 32]>,
    pending: Option<Pending>,
    unsupported_until: Option<Instant>,
    /// Per-target log rate cap (INV-MON-001): last log instant per key.
    log_gate: HashMap<String, Instant>,
    /// Diagnostics: last tick result per process (never text).
    pub diagnostics: HashMap<String, String>,
}

/// What the loop should do after one tick.
#[derive(Debug, PartialEq)]
pub enum Decision {
    /// Foreground excluded — sleep EXCLUDED_SLEEP_MS.
    Excluded(String),
    /// Password field seen — no read, normal cadence.
    PasswordSkipped,
    /// Nothing to do — normal cadence.
    Quiet,
    /// Target unsupported — back off UNSUPPORTED_BACKOFF_MS.
    UnsupportedBackoff,
    /// Debounce timer still running.
    Debouncing,
    /// Quiet period elapsed — run the check with the given text.
    Check { key: TargetKey, text: String },
}

impl MonitorState {
    pub fn new() -> Self {
        Self {
            last_key: None,
            last_hash: None,
            pending: None,
            unsupported_until: None,
            log_gate: HashMap::new(),
            diagnostics: HashMap::new(),
        }
    }

    fn note_diag(&mut self, process: &str, note: &str) {
        // Cap the map so pathological focus churn can't grow it.
        if self.diagnostics.len() > 32 && !self.diagnostics.contains_key(process) {
            if let Some(k) = self.diagnostics.keys().next().cloned() {
                self.diagnostics.remove(&k);
            }
        }
        self.diagnostics
            .insert(process.to_string(), note.to_string());
    }

    /// INV-MON-001 log gate: returns true when a log line for this key
    /// is allowed (max once per minute per target).
    fn may_log(&mut self, key: &str, now: Instant) -> bool {
        match self.log_gate.get(key) {
            Some(t) if now.duration_since(*t) < Duration::from_secs(60) => false,
            _ => {
                self.log_gate.insert(key.to_string(), now);
                true
            }
        }
    }

    pub fn on_tick(
        &mut self,
        outcome: Result<ReadOutcome, String>,
        excluded: &[String],
        now: Instant,
    ) -> Decision {
        // INV-EXCL-001, enforced twice by design: the reader returns
        // `Excluded` having read nothing (authoritative path), and this
        // belt-and-suspenders arm covers a misbehaving reader that
        // still produced a snapshot for an excluded process.
        match &outcome {
            Ok(ReadOutcome::Excluded(process)) => {
                self.pending = None;
                self.last_key = None;
                self.note_diag(process, "excluded");
                return Decision::Excluded(process.clone());
            }
            Ok(ReadOutcome::Snapshot(snap)) if process_excluded(&snap.key.process, excluded) => {
                self.pending = None;
                self.last_key = None;
                self.note_diag(&snap.key.process, "excluded");
                return Decision::Excluded(snap.key.process.clone());
            }
            _ => {}
        }

        match outcome {
            Ok(ReadOutcome::Excluded(process)) => Decision::Excluded(process),
            Ok(ReadOutcome::UiChrome(process)) => {
                // Chrome input, not a document: clear pending state so a
                // half-finished debounce doesn't commit against the
                // omnibox when focus snaps back to a real field.
                self.pending = None;
                self.last_key = None;
                self.note_diag(&process, "ui-chrome");
                Decision::Quiet
            }
            Err(e) => {
                // Transient COM-level failure: degrade, never panic.
                if self.may_log("read-error", now) {
                    eprintln!("[monitor] read error: {e}");
                }
                Decision::Quiet
            }
            Ok(ReadOutcome::Transient(e)) => {
                if self.may_log("transient", now) {
                    eprintln!("[monitor] transient: {e}");
                }
                Decision::Quiet
            }
            Ok(ReadOutcome::Unsupported) => {
                self.pending = None;
                if let Some(until) = self.unsupported_until {
                    if now < until {
                        return Decision::UnsupportedBackoff;
                    }
                }
                self.unsupported_until = Some(now + Duration::from_millis(UNSUPPORTED_BACKOFF_MS));
                Decision::UnsupportedBackoff
            }
            Ok(ReadOutcome::Snapshot(snap)) => {
                self.unsupported_until = None;
                if snap.is_password {
                    // INV-PRIV-001: never read, never check, never log text.
                    self.pending = None;
                    self.note_diag(&snap.key.process, "password-skipped");
                    return Decision::PasswordSkipped;
                }
                let Some(text) = snap.value else {
                    // Password-gate passed but no text pattern — treat as
                    // unsupported for this tick.
                    self.note_diag(&snap.key.process, "no-text-pattern");
                    self.unsupported_until =
                        Some(now + Duration::from_millis(UNSUPPORTED_BACKOFF_MS));
                    return Decision::UnsupportedBackoff;
                };
                self.note_diag(&snap.key.process, "reading");

                let hash = text_hash(&text);
                let same_target = self.last_key.as_ref() == Some(&snap.key);
                if same_target && self.last_hash == Some(hash) {
                    // Unchanged (focus juggling included) — any pending
                    // debounce for older text stays; nothing new to do.
                    return Decision::Quiet;
                }

                // New target or changed text → (re)start the quiet period.
                match &mut self.pending {
                    Some(Pending::Debounce {
                        key,
                        changed_at,
                        hash: pending_hash,
                    }) if key == &snap.key => {
                        if *pending_hash != hash {
                            // Text changed since the quiet period began —
                            // restart it. (Audit M5: changed_at used to be
                            // set once per transition, so continuous typing
                            // degenerated to a throttle and checks fired
                            // mid-sentence against stale text.)
                            *changed_at = now;
                            *pending_hash = hash;
                            return Decision::Debouncing;
                        }
                        // Same text the debounce started on: commit once
                        // quiet long enough.
                        if now.duration_since(*changed_at) >= Duration::from_millis(DEBOUNCE_MS) {
                            self.pending = None;
                            self.last_key = Some(snap.key.clone());
                            self.last_hash = Some(hash);
                            return Decision::Check {
                                key: snap.key,
                                text,
                            };
                        }
                        Decision::Debouncing
                    }
                    _ => {
                        self.pending = Some(Pending::Debounce {
                            key: snap.key.clone(),
                            changed_at: now,
                            hash,
                        });
                        Decision::Debouncing
                    }
                }
            }
        }
    }
}

impl Default for MonitorState {
    fn default() -> Self {
        Self::new()
    }
}

/// Case-insensitive process-name match (with or without .exe).
pub fn process_excluded(process: &str, excluded: &[String]) -> bool {
    let p = process.trim().to_ascii_lowercase();
    let p = p.strip_suffix(".exe").unwrap_or(&p);
    excluded.iter().any(|e| {
        let e = e.trim().to_ascii_lowercase();
        let e = e.strip_suffix(".exe").unwrap_or(&e);
        !e.is_empty() && e == p
    })
}

/// Browser/app chrome controls that must never be monitored: address
/// bars, find bars, tab-search inputs. Suggestions on search queries
/// are noise by definition. Matched on UIA Name (provider-authored,
/// English on stock Chromium) with ClassName as the structural
/// fallback — extend the list rather than loosening a match.
#[cfg_attr(not(any(test, target_os = "windows")), allow(dead_code))]
pub fn is_browser_chrome_control(name: &str, classname: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    let c = classname.trim().to_ascii_lowercase();
    n.contains("address and search bar")          // Chromium omnibox
        || n == "search or type a url"            // older Chromium
        || n.starts_with("search tabs")           // tab-search popup
        || c.contains("omnibox") // class fallback
}

#[cfg(test)]
mod chrome_tests {
    use super::is_browser_chrome_control as chrome;

    #[test]
    fn matches_known_chrome_inputs() {
        assert!(chrome("Address and search bar", "Chrome_OmniboxView"));
        assert!(chrome("address and search bar", "ViewsTextfield"));
        assert!(chrome("Search or type a url", "omnibox thing"));
        assert!(chrome("Search tabs", "ViewsTextfield"));
        assert!(chrome("whatever", "Chrome_OmniboxView"));
    }

    #[test]
    fn does_not_match_real_fields() {
        assert!(!chrome("", ""));
        assert!(!chrome("text area", "Edit"));
        assert!(!chrome("Message", "RichEditD2DPT")); // Notepad
        assert!(!chrome("Windows Terminal", "CASCADIA_HOSTING_WINDOW_CLASS"));
    }
}

fn text_hash(text: &str) -> [u8; 32] {
    // Plan text says SHA-1; SHA-256 serves the same change-detection
    // purpose with the crate we already ship (no new dependency).
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    h.finalize().into()
}

// ── Shared loop state ───────────────────────────────────────────────

static STATE: Mutex<Option<MonitorState>> = Mutex::new(None);

fn with_state<T>(f: impl FnOnce(&mut MonitorState) -> T) -> T {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner()); // poison recovery
    let mut state = guard.take().unwrap_or_default();
    let out = f(&mut state);
    *guard = Some(state);
    out
}

// ── Windows UIA reader ──────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod windows_reader {
    use super::{FieldSnapshot, FocusedFieldReader, ReadOutcome, TargetKey};

    pub struct UiaFieldReader;

    impl FocusedFieldReader for UiaFieldReader {
        fn read_field(&self, excluded: &[String]) -> ReadOutcome {
            read_focused_field(excluded)
        }
    }

    fn read_focused_field(excluded: &[String]) -> ReadOutcome {
        use uiautomation::patterns::{UITextPattern, UIValuePattern};
        use uiautomation::UIAutomation;

        let automation = match UIAutomation::new() {
            Ok(a) => a,
            Err(e) => return ReadOutcome::Transient(format!("UIAutomation::new: {e}")),
        };
        let element = match automation.get_focused_element() {
            Ok(el) => el,
            Err(e) => return ReadOutcome::Transient(format!("get_focused_element: {e}")),
        };

        // INV-EXCL-001 (verifier finding 0008): resolve the process
        // identity FIRST and bail before any pattern or value read when
        // the foreground process is excluded.
        let pid = element.get_process_id().unwrap_or(0);
        let process = match process_name_for_pid(pid) {
            Some(name) => name,
            None => {
                // Fail closed (Greptile P1, applied cross-platform):
                // an unresolvable identity cannot be proven
                // non-excluded, so reject before any pattern or value
                // read. The synthetic label feeds diagnostics only.
                return ReadOutcome::Excluded(format!("pid-{pid}"));
            }
        };
        if super::process_excluded(&process, excluded) {
            return ReadOutcome::Excluded(process);
        }

        // Browser/app chrome (address bar, find bars, tab search) is a
        // navigation input, not a user document. Name/class are metadata
        // reads; the check happens BEFORE any value or pattern read so
        // chrome text never leaves the app (INV-EXCL-001 spirit).
        let control_name = element.get_name().unwrap_or_default();
        let classname = element.get_classname().unwrap_or_default();
        if super::is_browser_chrome_control(&control_name, &classname) {
            return ReadOutcome::UiChrome(process);
        }

        // INV-PRIV-001: password check BEFORE the value read. A failed
        // property query must fail CLOSED — common on non-conforming
        // UIA providers — so treat the field as a password and skip it
        // rather than risk reading one.
        let is_password = element.is_password().unwrap_or(true);
        let rect = element
            .get_bounding_rectangle()
            .map(|r| (r.get_left(), r.get_top(), r.get_right(), r.get_bottom()))
            .unwrap_or((0, 0, 0, 0));
        let key = TargetKey {
            process,
            field_rect: rect,
        };

        if is_password {
            // Value intentionally NOT read.
            let hwnd = foreground_hwnd();
            return ReadOutcome::Snapshot(FieldSnapshot {
                key,
                is_password: true,
                value: None,
                hwnd,
            });
        }

        // ValuePattern first (classic edit controls), then TextPattern
        // document range (Chromium/Electron).
        let value = if let Ok(vp) = element.get_pattern::<UIValuePattern>() {
            vp.get_value().ok()
        } else {
            None
        };
        let value = match value {
            Some(v) => Some(v),
            None => element
                .get_pattern::<UITextPattern>()
                .ok()
                .and_then(|tp| tp.get_document_range().ok())
                .and_then(|range| range.get_text(-1).ok()),
        };

        let hwnd = foreground_hwnd();
        match value {
            Some(text) => ReadOutcome::Snapshot(FieldSnapshot {
                key,
                is_password: false,
                value: Some(text),
                hwnd,
            }),
            None => ReadOutcome::Unsupported,
        }
    }

    fn foreground_hwnd() -> isize {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        unsafe { GetForegroundWindow().0 as isize }
    }

    pub fn process_name_for_pid_pub(pid: u32) -> Option<String> {
        process_name_for_pid(pid)
    }

    fn process_name_for_pid(pid: u32) -> Option<String> {
        use windows::core::PWSTR;
        use windows::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER};
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };

        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let result = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(handle);
            if result.is_err() && result.err() != Some(ERROR_INSUFFICIENT_BUFFER.into()) {
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            let name = path.rsplit(['\\', '/']).next().unwrap_or(&path);
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub use windows_reader::UiaFieldReader;

/// PID → process image basename (shared with apply.rs).
#[cfg(target_os = "windows")]
pub fn process_name_for_pid(pid: u32) -> Option<String> {
    windows_reader::process_name_for_pid_pub(pid)
}

// ── macOS AX reader ──────────────────────────────────────────────────

/// Thin adapter: all AX API usage lives in `a11y/macos_impl.rs` (which owns
/// the INV-EXCL-001/INV-PRIV-001 ordering); this maps its neutral outcome
/// onto [`ReadOutcome`].
#[cfg(target_os = "macos")]
mod ax_reader {
    use super::{FieldSnapshot, FocusedFieldReader, ReadOutcome, TargetKey};

    pub struct AxFieldReader;

    impl FocusedFieldReader for AxFieldReader {
        fn read_field(&self, excluded: &[String]) -> ReadOutcome {
            let hwnd = 0isize; // pid identity travels in TargetKey.process
            match crate::a11y::macos_impl::read_focused_field(excluded) {
                crate::a11y::FieldRead::Excluded(process) => ReadOutcome::Excluded(process),
                crate::a11y::FieldRead::Password { process, rect } => {
                    // Value intentionally NOT read.
                    ReadOutcome::Snapshot(FieldSnapshot {
                        key: TargetKey {
                            process,
                            field_rect: rect.unwrap_or((0, 0, 0, 0)),
                        },
                        is_password: true,
                        value: None,
                        hwnd,
                    })
                }
                crate::a11y::FieldRead::Text {
                    process,
                    text,
                    rect,
                } => ReadOutcome::Snapshot(FieldSnapshot {
                    key: TargetKey {
                        process,
                        field_rect: rect.unwrap_or((0, 0, 0, 0)),
                    },
                    is_password: false,
                    value: Some(text),
                    hwnd,
                }),
                crate::a11y::FieldRead::NoField => ReadOutcome::Unsupported,
                crate::a11y::FieldRead::Transient(e) => ReadOutcome::Transient(e),
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use ax_reader::AxFieldReader;

// ── Remaining platforms (BSD etc.) stay explicit stubs ──────────────

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod stub_reader {
    use super::{FieldSnapshot, FocusedFieldReader, ReadOutcome};

    pub struct StubFieldReader;

    impl FocusedFieldReader for StubFieldReader {
        fn read_field(&self, _excluded: &[String]) -> ReadOutcome {
            ReadOutcome::Unsupported
        }
    }

    #[allow(unused)]
    fn _shape(snapshot: FieldSnapshot) -> FieldSnapshot {
        snapshot
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub use stub_reader::StubFieldReader;

// ── Loop ────────────────────────────────────────────────────────────
/// Platform reader selection. Each backend implements the same
/// [`FocusedFieldReader`] contract (exclusion-before-read, password
/// fail-closed) — see the platform modules for the OS specifics.
#[cfg(target_os = "windows")]
fn field_reader() -> impl FocusedFieldReader {
    UiaFieldReader
}

#[cfg(target_os = "macos")]
fn field_reader() -> impl FocusedFieldReader {
    AxFieldReader
}

#[cfg(target_os = "linux")]
fn field_reader() -> impl FocusedFieldReader {
    AtspiFieldReader
}

/// Thin adapter: all AT-SPI/D-Bus usage lives in `a11y/linux_impl.rs`
/// (which owns the INV-EXCL-001/INV-PRIV-001 ordering); this maps its
/// neutral outcome onto [`ReadOutcome`].
#[cfg(target_os = "linux")]
mod atspi_reader {
    use super::{FieldSnapshot, FocusedFieldReader, ReadOutcome, TargetKey};

    pub struct AtspiFieldReader;

    impl FocusedFieldReader for AtspiFieldReader {
        fn read_field(&self, excluded: &[String]) -> ReadOutcome {
            let hwnd = 0isize; // pid identity travels in TargetKey.process
            match crate::a11y::linux_impl::read_focused_field(excluded) {
                crate::a11y::FieldRead::Excluded(process) => ReadOutcome::Excluded(process),
                crate::a11y::FieldRead::Password { process, rect } => {
                    // Value intentionally NOT read.
                    ReadOutcome::Snapshot(FieldSnapshot {
                        key: TargetKey {
                            process,
                            field_rect: rect.unwrap_or((0, 0, 0, 0)),
                        },
                        is_password: true,
                        value: None,
                        hwnd,
                    })
                }
                crate::a11y::FieldRead::Text {
                    process,
                    text,
                    rect,
                } => ReadOutcome::Snapshot(FieldSnapshot {
                    key: TargetKey {
                        process,
                        field_rect: rect.unwrap_or((0, 0, 0, 0)),
                    },
                    is_password: false,
                    value: Some(text),
                    hwnd,
                }),
                crate::a11y::FieldRead::NoField => ReadOutcome::Unsupported,
                crate::a11y::FieldRead::Transient(e) => ReadOutcome::Transient(e),
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub use atspi_reader::AtspiFieldReader;
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn field_reader() -> impl FocusedFieldReader {
    StubFieldReader
}

/// Start the monitor loop. Idempotent.
pub fn start(app: tauri::AppHandle) {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return; // already running
    }
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let reader: std::sync::Arc<dyn FocusedFieldReader> = std::sync::Arc::new(field_reader());
    tauri::async_runtime::spawn(async move {
        run_loop(app, reader, generation).await;
    });
    eprintln!("[monitor] started (generation {generation})");
}

pub fn stop() {
    RUNNING.store(false, Ordering::Release);
    GENERATION.fetch_add(1, Ordering::AcqRel);
    eprintln!("[monitor] stopped");
}

/// Pause ALL monitoring (reads, checks, widget) for `minutes`. Global
/// and wall-clock based: expires on its own, no restart needed. The
/// widget's "Snooze 1 h" button is the only caller today.
#[tauri::command]
pub fn snooze_monitor(minutes: u32) -> Result<(), String> {
    let minutes = minutes.clamp(1, 24 * 60);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    SNOOZE_UNTIL_MS.store(
        now_ms.saturating_add(u64::from(minutes) * 60_000),
        Ordering::Release,
    );
    eprintln!("[monitor] snoozed for {minutes} min");
    Ok(())
}

async fn run_loop(
    app: tauri::AppHandle,
    reader: std::sync::Arc<dyn FocusedFieldReader>,
    generation: u64,
) {
    loop {
        if !RUNNING.load(Ordering::Acquire) || GENERATION.load(Ordering::Acquire) != generation {
            return;
        }

        let (enabled, excluded) = crate::config::with_config_pub(|c| {
            (c.native_monitoring_enabled, c.excluded_processes.clone())
        });
        if !enabled {
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }

        // Global snooze: no reads, no checks, no widget. Cheap wall-
        // clock check every tick; expires without any bookkeeping.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        if now_ms < SNOOZE_UNTIL_MS.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }

        // COM isolation: the reader runs on a blocking thread. The
        // exclusion list travels IN so the reader honors read-before-
        // check ordering (verifier finding 0008 fix).
        let reader = reader.clone();
        let excluded_for_reader = excluded.clone();
        let outcome = match tokio::task::spawn_blocking(move || {
            reader.read_field(&excluded_for_reader)
        })
        .await
        {
            Ok(o) => Ok(o),
            Err(e) => Err(format!("join: {e}")),
        };

        let sleep_ms;
        let mut last_field_hint: Option<(isize, String)> = None;
        if let Ok(ReadOutcome::Snapshot(snap)) = &outcome {
            last_field_hint = Some((snap.hwnd, snap.key.process.clone()));
        }
        let decision = with_state(|state| state.on_tick(outcome, &excluded, Instant::now()));

        match decision {
            Decision::Excluded(_) => sleep_ms = EXCLUDED_SLEEP_MS,
            Decision::UnsupportedBackoff => sleep_ms = UNSUPPORTED_BACKOFF_MS,
            Decision::Quiet | Decision::PasswordSkipped | Decision::Debouncing => {
                sleep_ms = TICK_MS
            }
            Decision::Check { key, text } => {
                sleep_ms = TICK_MS;
                if let Some((hwnd, process)) = &last_field_hint {
                    remember_field(*hwnd, process.clone());
                }
                // Native surface → correctness-only under AutoBySurface.
                let req = crate::engine::CheckRequest {
                    text: text.clone(),
                    surface: crate::engine::Surface::Native,
                    target: crate::engine::TargetId {
                        kind: crate::engine::TargetKind::NativeProcess {
                            process: key.process.clone(),
                        },
                    },
                    // Settings-authored goals reach harper correctness on
                    // the native surface too (PLAN-06 task 1; verifier F1).
                    goals: crate::config::with_config_pub(|c| c.writing_goals),
                    // Native never runs the style pass regardless.
                    style_enabled: None,
                };
                let dict = crate::engine::PersonalDictionary {
                    words: crate::config::with_config_pub(|c| c.personal_dictionary.clone()),
                };
                match crate::engine::check_text_with(
                    req,
                    dict,
                    crate::engine::StylePolicy::AutoBySurface,
                    None, // native = correctness-only by policy
                    &[],
                )
                .await
                {
                    Ok(resp) => {
                        // Counts only — never text (INV-PRIV-002). Used as
                        // the behavioral-gate evidence channel.
                        with_state(|state| {
                            if state.may_log("emit", Instant::now()) {
                                eprintln!(
                                    "[monitor] issues={} target={}",
                                    resp.issues.len(),
                                    key.as_string()
                                );
                            }
                        });
                        let _ = app.emit(
                            "wb://issues",
                            serde_json::json!({
                                "targetKey": key.as_string(),
                                "issues": resp.issues,
                                "revoked": false,
                            }),
                        );
                        // The widget's apply requests need the exact
                        // text these spans index. INV-PRIV-002: this is
                        // an in-memory IPC event to our own windows,
                        // never persisted; only sent when issues exist.
                        if !resp.issues.is_empty() {
                            let _ = app.emit(
                                "wb://field-text",
                                serde_json::json!({
                                    "targetKey": key.as_string(),
                                    "text": text,
                                }),
                            );
                        }
                        // The widget docks to the field rect (caret rect
                        // unavailable in uiautomation 0.24; documented
                        // P3 deviation). Rect travels as [l, t, r, b].
                        let (l, t, r, b) = key.field_rect;
                        let _ = app.emit(
                            "wb://field-focus",
                            serde_json::json!({
                                "targetKey": key.as_string(),
                                "caret": null,
                                "fieldRect": [l, t, r, b],
                            }),
                        );
                    }
                    Err(e) => {
                        // Oversized field text is expected on huge
                        // documents; degrade quietly.
                        let msg = e.chars().take(80).collect::<String>();
                        with_state(|state| {
                            if state.may_log("engine", Instant::now()) {
                                eprintln!("[monitor] engine rejected field: {msg}");
                            }
                        });
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
    }
}

// ── Tauri commands ──────────────────────────────────────────────────

#[derive(Serialize)]
pub struct MonitorStatus {
    pub running: bool,
    pub enabled: bool,
    /// Last tick result per foreground process (never text).
    pub diagnostics: HashMap<String, String>,
}

#[tauri::command]
pub fn monitor_start(app: tauri::AppHandle) -> Result<(), String> {
    start(app);
    Ok(())
}

#[tauri::command]
pub fn monitor_stop() -> Result<(), String> {
    stop();
    Ok(())
}

#[tauri::command]
pub fn monitor_status() -> Result<MonitorStatus, String> {
    let enabled = crate::config::with_config_pub(|c| c.native_monitoring_enabled);
    let diagnostics = with_state(|state| state.diagnostics.clone());
    Ok(MonitorStatus {
        running: RUNNING.load(Ordering::Acquire),
        enabled,
        diagnostics,
    })
}

// ── Tests (pure state machine — no COM) ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn key(process: &str) -> TargetKey {
        TargetKey {
            process: process.into(),
            field_rect: (10, 20, 30, 40),
        }
    }

    fn snap(process: &str, text: &str) -> ReadOutcome {
        ReadOutcome::Snapshot(FieldSnapshot {
            key: key(process),
            is_password: false,
            value: Some(text.into()),
            hwnd: 7,
        })
    }

    fn snap_full(process: &str, text: &str) -> ReadOutcome {
        match snap(process, text) {
            ReadOutcome::Snapshot(mut s) => {
                s.hwnd = 4242;
                ReadOutcome::Snapshot(s)
            }
            other => other,
        }
    }

    #[test]
    fn excluded_process_sleeps_long_and_never_checks() {
        let mut st = MonitorState::new();
        let now = Instant::now();
        // Reader-level exclusion (authoritative path): no snapshot was
        // ever produced, so no text could have been read.
        let d = st.on_tick(
            Ok(ReadOutcome::Excluded("notepad.exe".into())),
            &["notepad".into()],
            now,
        );
        assert_eq!(d, Decision::Excluded("notepad.exe".into()));
        // Belt-and-suspenders: a misbehaving reader that DID produce a
        // snapshot for an excluded process still gets excluded here.
        let d2 = st.on_tick(
            Ok(snap_full("notepad.exe", "teh")),
            &["notepad".into()],
            now,
        );
        assert_eq!(d2, Decision::Excluded("notepad.exe".into()));
    }

    /// Verifier finding 0008 regression: the reader contract — when the
    /// exclusion list matches, read_field returns Excluded without ever
    /// touching the value path.
    struct NoReadWhenExcludedFake;
    impl FocusedFieldReader for NoReadWhenExcludedFake {
        fn read_field(&self, excluded: &[String]) -> ReadOutcome {
            if process_excluded("notepad.exe", excluded) {
                return ReadOutcome::Excluded("notepad.exe".to_string());
            }
            ReadOutcome::Snapshot(FieldSnapshot {
                key: key("app"),
                is_password: false,
                value: Some("teh".into()),
                hwnd: 1,
            })
        }
    }

    #[test]
    fn reader_boundary_excludes_before_value_read() {
        let reader = NoReadWhenExcludedFake;
        assert!(matches!(
            reader.read_field(&["notepad".into()]),
            ReadOutcome::Excluded(_)
        ));
        assert!(matches!(reader.read_field(&[]), ReadOutcome::Snapshot(_)));
    }

    #[test]
    fn password_field_is_never_checked() {
        let mut st = MonitorState::new();
        let d = st.on_tick(
            Ok(ReadOutcome::Snapshot(FieldSnapshot {
                key: key("app"),
                is_password: true,
                value: None,
                hwnd: 9,
            })),
            &[],
            Instant::now(),
        );
        assert_eq!(d, Decision::PasswordSkipped);
        assert_eq!(
            st.diagnostics.get("app").map(String::as_str),
            Some("password-skipped")
        );
    }

    #[test]
    fn change_debounces_then_checks() {
        let mut st = MonitorState::new();
        let t0 = Instant::now();
        // First sighting: start debounce.
        assert_eq!(
            st.on_tick(Ok(snap("app", "teh")), &[], t0),
            Decision::Debouncing
        );
        // Still typing (text changed): debounce restarts via key match +
        // changed text — second tick inside quiet window keeps Debouncing.
        assert_eq!(
            st.on_tick(
                Ok(snap("app", "teh recieve")),
                &[],
                t0 + Duration::from_millis(100)
            ),
            Decision::Debouncing
        );
        // Quiet for 300ms with STABLE text: commit.
        let d = st.on_tick(
            Ok(snap("app", "teh recieve")),
            &[],
            t0 + Duration::from_millis(450),
        );
        match d {
            Decision::Check { key, text } => {
                assert_eq!(key.process, "app");
                assert_eq!(text, "teh recieve");
            }
            other => panic!("expected Check, got {other:?}"),
        }
    }

    #[test]
    fn continued_typing_restarts_debounce_instead_of_throttling() {
        let mut st = MonitorState::new();
        let t0 = Instant::now();
        assert_eq!(
            st.on_tick(Ok(snap("app", "teh")), &[], t0),
            Decision::Debouncing
        );
        // User keeps typing at t0+200: quiet period restarts from here.
        assert_eq!(
            st.on_tick(
                Ok(snap("app", "teh recieve")),
                &[],
                t0 + Duration::from_millis(200)
            ),
            Decision::Debouncing
        );
        // t0+350 is 350ms after the FIRST change but only 150ms after the
        // last one — must still be debouncing, not a mid-sentence Check.
        assert_eq!(
            st.on_tick(
                Ok(snap("app", "teh recieve")),
                &[],
                t0 + Duration::from_millis(350)
            ),
            Decision::Debouncing
        );
        // Quiet past the restarted window: now it commits.
        assert!(matches!(
            st.on_tick(
                Ok(snap("app", "teh recieve")),
                &[],
                t0 + Duration::from_millis(600)
            ),
            Decision::Check { .. }
        ));
    }
    #[test]
    fn unchanged_text_never_rechecks() {
        let mut st = MonitorState::new();
        let t0 = Instant::now();
        st.on_tick(Ok(snap("app", "teh")), &[], t0);
        st.on_tick(Ok(snap("app", "teh")), &[], t0 + Duration::from_millis(400));
        // Second full quiet cycle with same text: Quiet, not Check.
        assert_eq!(
            st.on_tick(Ok(snap("app", "teh")), &[], t0 + Duration::from_millis(900)),
            Decision::Quiet
        );
    }

    #[test]
    fn focus_flicker_between_fields_keeps_identity() {
        let mut st = MonitorState::new();
        let t0 = Instant::now();
        let mut other = key("app");
        other.field_rect = (99, 99, 199, 199);
        st.on_tick(Ok(snap("app", "teh")), &[], t0);
        // Flicker to another field with the SAME text: different key →
        // new debounce for that key, no cross-field commit.
        let d = st.on_tick(
            Ok(ReadOutcome::Snapshot(FieldSnapshot {
                key: other,
                is_password: false,
                value: Some("teh".into()),
                hwnd: 8,
            })),
            &[],
            t0 + Duration::from_millis(400),
        );
        assert_eq!(d, Decision::Debouncing);
    }

    #[test]
    fn unsupported_backs_off() {
        let mut st = MonitorState::new();
        assert_eq!(
            st.on_tick(Ok(ReadOutcome::Unsupported), &[], Instant::now()),
            Decision::UnsupportedBackoff
        );
        // Second unsupported tick within the window: still backing off.
        assert_eq!(
            st.on_tick(Ok(ReadOutcome::Unsupported), &[], Instant::now()),
            Decision::UnsupportedBackoff
        );
    }

    #[test]
    fn transient_and_error_never_panic() {
        let mut st = MonitorState::new();
        assert_eq!(
            st.on_tick(Ok(ReadOutcome::Transient("x".into())), &[], Instant::now()),
            Decision::Quiet
        );
        assert_eq!(
            st.on_tick(Err("boom".into()), &[], Instant::now()),
            Decision::Quiet
        );
    }

    #[test]
    fn exclusion_match_is_case_insensitive_without_exe() {
        assert!(process_excluded("Notepad.EXE", &["notepad".into()]));
        assert!(process_excluded("notepad", &["NOTEPAD.exe".into()]));
        assert!(!process_excluded("notepad++", &["notepad".into()]));
        assert!(!process_excluded("code", &[] as &[String]));
    }
}
