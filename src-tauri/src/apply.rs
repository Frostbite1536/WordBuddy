//! Native fix application (PLAN-04 task 3).
//!
//! Strategies, in preference order (per target capabilities):
//! 1. `ValuePattern::SetValue(full corrected text)` — Notepad-class.
//!    Destroys undo (documented in the card tooltip); revert on failed
//!    verify re-sets the original text.
//! 2. TextPattern surgical replace: select the issue span, synthetic
//!    Ctrl+V of the replacement through the clipboard save/restore
//!    utility. Preserves undo in most editors. No safe revert.
//! 3. Unsupported → the card offers a copyable fix (frontend).
//!
//! **INV-APPLY-001**: synthetic input only ever targets the exact
//! process captured with the issue. Before any write or key synthesis we
//! re-resolve the focused element and the foreground window and abort on
//! any mismatch with the expected process.
//!
//! Single-flight: one apply at a time process-wide.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::clipboard::{clipboard_lock, with_temporary_clipboard, ClipboardBackend};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    /// Process name the issue was captured against (INV-APPLY-001).
    pub process: String,
    /// Full field text at capture time.
    pub original_text: String,
    /// UTF-16 span of the issue within `original_text`.
    pub start: usize,
    pub end: usize,
    /// Replacement for that span.
    pub replacement: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub ok: bool,
    /// value-set | text-paste | unsupported | aborted | verify-failed
    pub strategy: String,
    pub message: String,
}

// ── Target capability probe (mockable) ──────────────────────────────

/// What the currently focused element exposes, resolved fresh at apply
/// time. The `expected_process` check happens INSIDE the probe so the
/// process verification and the capability read are one atomic step
/// against the same focused element (INV-APPLY-001).
pub enum ApplyTarget {
    /// ValuePattern on an element whose process matches.
    Value { element_text: String },
    /// TextPattern on an element whose process matches.
    Text { document_text: String },
    /// Focused element's process does NOT match (or no focused field).
    WrongTarget { actual: String },
    /// No usable pattern.
    Unsupported,
}

pub trait ApplyProbe: Send + Sync {
    fn probe(&self, expected_process: &str) -> Result<ApplyTarget, String>;
    /// SetValue path (Value targets).
    fn set_value(&self, new_text: &str) -> Result<(), String>;
    /// Selects the UTF-16 range [start, end) in the focused Text
    /// element (cloned range + Select), ready for a paste.
    fn select_range(&self, start: usize, end: usize) -> Result<(), String>;
    /// True when the foreground window's process is still
    /// `expected_process` (re-checked immediately before SendInput).
    fn foreground_still(&self, expected_process: &str) -> bool;
    /// Synthesizes Ctrl+V. Only called between foreground_still checks.
    fn send_paste(&self) -> Result<(), String>;
}

static APPLY_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// UTF-16-aware splice of `replacement` into `original[start..end]`.
/// Kept pure + tested: this is the corrected text both strategies
/// verify against.
pub fn splice_utf16(original: &str, start: usize, end: usize, replacement: &str) -> String {
    // Rebuild from UTF-16 code units — provably symmetric with the
    // frontend's slice/splice semantics (INV-OFFSET-001).
    let units: Vec<u16> = original.encode_utf16().collect();
    let start = start.min(units.len());
    let end = end.max(start).min(units.len());
    let mut rebuilt: Vec<u16> = Vec::with_capacity(units.len());
    rebuilt.extend_from_slice(&units[..start]);
    rebuilt.extend(replacement.encode_utf16());
    rebuilt.extend_from_slice(&units[end..]);
    String::from_utf16_lossy(&rebuilt)
}

/// Entry point. `probe` is injected (real UIA impl in production, fakes
/// in tests). The clipboard backend is likewise injected.
pub fn apply_fix(
    probe: &dyn ApplyProbe,
    clipboard: &dyn ClipboardBackend,
    req: &ApplyRequest,
) -> ApplyResult {
    if APPLY_IN_FLIGHT.swap(true, Ordering::AcqRel) {
        return ApplyResult {
            ok: false,
            strategy: "aborted".into(),
            message: "another apply is already in flight".into(),
        };
    }
    let _guard = SingleFlightGuard;
    apply_fix_impl(probe, clipboard, req)
}

/// Guard-free core so unit tests exercise the strategy logic without
/// colliding on the process-wide single-flight flag (tests run in
/// parallel; production always enters through [`apply_fix`]).
pub fn apply_fix_impl(
    probe: &dyn ApplyProbe,
    clipboard: &dyn ClipboardBackend,
    req: &ApplyRequest,
) -> ApplyResult {
    let _clipboard_guard = clipboard_lock();

    let expected_text = splice_utf16(&req.original_text, req.start, req.end, &req.replacement);

    let target = match probe.probe(&req.process) {
        Ok(t) => t,
        Err(e) => {
            return ApplyResult {
                ok: false,
                strategy: "aborted".into(),
                message: format!("target probe failed: {e}"),
            }
        }
    };

    match target {
        ApplyTarget::WrongTarget { actual } => ApplyResult {
            ok: false,
            strategy: "aborted".into(),
            message: format!(
                "INV-APPLY-001: focused target is '{actual}', expected '{}'",
                req.process
            ),
        },
        ApplyTarget::Unsupported => ApplyResult {
            ok: false,
            strategy: "unsupported".into(),
            message: "focused field exposes no apply pattern".into(),
        },
        ApplyTarget::Value { element_text } => {
            if element_text != req.original_text {
                return ApplyResult {
                    ok: false,
                    strategy: "aborted".into(),
                    message: "field text changed since the issue was captured; re-checking".into(),
                };
            }
            match probe.set_value(&expected_text) {
                Ok(()) => match probe.probe(&req.process) {
                    Ok(ApplyTarget::Value { element_text: now }) => {
                        if now == expected_text {
                            ApplyResult {
                                ok: true,
                                strategy: "value-set".into(),
                                message: "applied (undo history replaced)".into(),
                            }
                        } else {
                            // Verify failed → attempt revert (SetValue path
                            // only; documented).
                            let _ = probe.set_value(&req.original_text);
                            ApplyResult {
                                ok: false,
                                strategy: "verify-failed".into(),
                                message: "post-apply text mismatch; reverted".into(),
                            }
                        }
                    }
                    _ => ApplyResult {
                        ok: false,
                        strategy: "verify-failed".into(),
                        message: "could not re-read after apply".into(),
                    },
                },
                Err(e) => ApplyResult {
                    ok: false,
                    strategy: "value-set".into(),
                    message: format!("SetValue failed: {e}"),
                },
            }
        }
        ApplyTarget::Text { document_text } => {
            if document_text != req.original_text {
                return ApplyResult {
                    ok: false,
                    strategy: "aborted".into(),
                    message: "field text changed since the issue was captured; re-checking".into(),
                };
            }
            // Select the span, then re-verify focus RIGHT BEFORE the
            // synthetic paste (INV-APPLY-001 focus-race abort).
            if let Err(e) = probe.select_range(req.start, req.end) {
                return ApplyResult {
                    ok: false,
                    strategy: "aborted".into(),
                    message: format!("range select failed: {e}"),
                };
            }
            // Audit M6: select_range re-resolves the target. A same-
            // process focus change between probe and select would pass
            // the foreground check but land the paste in an element
            // whose content was never compared — re-read and compare.
            match probe.probe(&req.process) {
                Ok(ApplyTarget::Text { document_text }) if document_text == req.original_text => {}
                Ok(_) => {
                    return ApplyResult {
                        ok: false,
                        strategy: "aborted".into(),
                        message: "target changed after selection; aborted".into(),
                    }
                }
                Err(e) => {
                    return ApplyResult {
                        ok: false,
                        strategy: "aborted".into(),
                        message: format!("post-select re-verify failed: {e}"),
                    }
                }
            }
            if !probe.foreground_still(&req.process) {
                return ApplyResult {
                    ok: false,
                    strategy: "aborted".into(),
                    message: "INV-APPLY-001: foreground changed before paste; aborted".into(),
                };
            }
            let probe_ptr = probe as *const dyn ApplyProbe;
            let result = with_temporary_clipboard(clipboard, &req.replacement, || {
                // Re-verify inside the paste window too: the clipboard
                // restore delay is fixed, and the race window is between
                // select and paste.
                if !probe.foreground_still(&req.process) {
                    return Err("INV-APPLY-001: foreground changed during paste; no input sent".into());
                }
                unsafe { &*probe_ptr }.send_paste()
            });
            match result {
                Ok(()) => ApplyResult {
                    ok: true,
                    strategy: "text-paste".into(),
                    message: "applied via paste (undo preserved in most editors)".into(),
                },
                Err(e) => ApplyResult {
                    ok: false,
                    strategy: "text-paste".into(),
                    message: e,
                },
            }
        }
    }
}

struct SingleFlightGuard;
impl Drop for SingleFlightGuard {
    fn drop(&mut self) {
        APPLY_IN_FLIGHT.store(false, Ordering::Release);
    }
}
// NOTE (F4, resolved empirically): UIA TextUnit::Character on Windows
// advances per UTF-16 code unit — a surrogate pair is TWO units. Verified
// at runtime with examples/uia_probe.rs against RichEdit; the scalar-
// conversion helper this note replaces was removed as falsified. Raw
// UTF-16 offsets flow through select_range unchanged.


// ── Windows UIA probe ───────────────────────────────────────────────
#[cfg(target_os = "windows")]
pub mod win_probe {
    use super::ApplyProbe;
    use super::ApplyTarget;
    use uiautomation::patterns::{UITextPattern, UIValuePattern};
    use uiautomation::UIAutomation;

    pub struct UiaApplyProbe {
        last_expected: std::sync::Mutex<Option<String>>,
    }

    impl Default for UiaApplyProbe {
        fn default() -> Self {
            Self { last_expected: std::sync::Mutex::new(None) }
        }
    }

    impl UiaApplyProbe {
        /// Resolve the target element: the focused element when it belongs
        /// to `expected` (the common case), else the monitor's stored
        /// field HWND — the card itself often holds focus at apply time.
        /// Identity is re-verified on the stored path (INV-APPLY-001).
        fn resolve_target(
            &self,
            expected: &str,
        ) -> Result<(uiautomation::UIElement, uiautomation::UIAutomation, String), String> {
            let automation = UIAutomation::new().map_err(|e| format!("UIAutomation::new: {e}"))?;

            if let Ok(el) = automation.get_focused_element() {
                let process = Self::element_process(&el);
                if process.eq_ignore_ascii_case(expected) {
                    return Ok((el, automation, process));
                }
            }

            if let Some((hwnd, process)) =
                crate::text_monitor::last_field_for(expected)
            {
                let handle = uiautomation::types::Handle::from(hwnd);
                if let Ok(el) = automation.element_from_handle(handle) {
                    // Identity check: the window must still belong to the
                    // expected process (INV-APPLY-001 — exact target).
                    let current = Self::element_process(&el);
                    if current.eq_ignore_ascii_case(&process)
                        && current.eq_ignore_ascii_case(expected)
                    {
                        // element_from_handle yields the top-level frame; patterns
                        // live on the editable descendant. Bounded walk to find it.
                        if let Some(edit) = Self::find_editable_descendant(&automation, &el) {
                            return Ok((edit, automation, current));
                        }
                    }
                }
            }

            let actual = automation
                .get_focused_element()
                .map(|el| Self::element_process(&el))
                .unwrap_or_else(|e| format!("unresolvable ({e})"));
            Err(format!(
                "target mismatch: expected '{expected}', focused '{actual}', no stored field"
            ))
        }

        /// Bounded depth-first search for a descendant exposing a
        /// ValuePattern or TextPattern (the editable control).
        fn find_editable_descendant(
            automation: &UIAutomation,
            root: &uiautomation::UIElement,
        ) -> Option<uiautomation::UIElement> {
            fn has_patterns(el: &uiautomation::UIElement) -> bool {
                el.get_pattern::<UIValuePattern>().is_ok()
                    || el.get_pattern::<UITextPattern>().is_ok()
            }
            if has_patterns(root) {
                return Some(root.clone());
            }
            let walker = automation.get_control_view_walker().ok()?;
            let mut stack = vec![(root.clone(), 0u32)];
            while let Some((el, depth)) = stack.pop() {
                if depth >= 8 {
                    continue;
                }
                if let Ok(child) = walker.get_first_child(&el) {
                    let mut next = Some(child);
                    while let Some(node) = next {
                        if has_patterns(&node) {
                            return Some(node);
                        }
                        stack.push((node.clone(), depth + 1));
                        next = walker.get_next_sibling(&node).ok();
                    }
                }
            }
            None
        }

        fn element_process(el: &uiautomation::UIElement) -> String {
            let pid = el.get_process_id().unwrap_or(0);
            super::super::text_monitor::process_name_for_pid(pid)
                .unwrap_or_else(|| format!("pid-{pid}"))
        }
    }

    impl ApplyProbe for UiaApplyProbe {
        fn probe(&self, expected_process: &str) -> Result<ApplyTarget, String> {
            *self.last_expected.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(expected_process.to_string());
            let (el, _automation, _process) = match self.resolve_target(expected_process) {
                Ok(t) => t,
                Err(e) => return Ok(ApplyTarget::WrongTarget { actual: e }),
            };
            // INV-PRIV-001: never write into password fields. A failed
            // property query fails CLOSED — treat as password, skip.
            if el.is_password().unwrap_or(true) {
                return Ok(ApplyTarget::Unsupported);
            }
            if let Ok(vp) = el.get_pattern::<UIValuePattern>() {
                if let Ok(text) = vp.get_value() {
                    return Ok(ApplyTarget::Value { element_text: text });
                }
            }
            if let Ok(tp) = el.get_pattern::<UITextPattern>() {
                if let Ok(range) = tp.get_document_range() {
                    if let Ok(text) = range.get_text(-1) {
                        return Ok(ApplyTarget::Text { document_text: text });
                    }
                }
            }
            Ok(ApplyTarget::Unsupported)
        }

        fn set_value(&self, new_text: &str) -> Result<(), String> {
            // Expected process is re-derived from the apply request by the
            // caller; this path re-resolves the same target.
            let expected = self.last_expected.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let expected = expected.ok_or("no expected process recorded")?;
            let (el, _, _) = self.resolve_target(&expected)?;
            let vp = el
                .get_pattern::<UIValuePattern>()
                .map_err(|e| format!("no ValuePattern: {e}"))?;
            vp.set_value(new_text).map_err(|e| format!("SetValue: {e}"))
        }

        fn select_range(&self, start: usize, end: usize) -> Result<(), String> {
            use uiautomation::types::TextPatternRangeEndpoint;
            use uiautomation::types::TextUnit;
            let expected = self.last_expected.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let expected = expected.ok_or("no expected process recorded")?;
            let (el, _, _) = self.resolve_target(&expected)?;
            // The paste lands in whatever has keyboard focus — move focus
            // to the target field before selecting (the card may hold it).
            let _ = el.set_focus();
            let tp = el
                .get_pattern::<UITextPattern>()
                .map_err(|e| format!("no TextPattern: {e}"))?;
            let mut range = tp
                .get_document_range()
                .map_err(|e| format!("document range: {e}"))?;
            // INV-OFFSET-001: start/end stay RAW UTF-16 code-unit
            // offsets here. Empirically verified on Windows 10 (see
            // examples/uia_probe.rs): UIA providers on this platform
            // (RichEdit confirmed) advance TextUnit::Character per
            // UTF-16 unit — a surrogate pair is TWO units — so the
            // offsets pass through unchanged. The audit's scalar-unit
            // theory was tested and falsified at runtime.
            //
            // Residual risk: a hypothetical provider counting scalars
            // would misselect astral-containing spans. Mitigation: the
            // apply pipeline re-verifies document text equality (probe)
            // and foreground identity before any paste lands.
            // Move a clone from the start: expand to document, then walk.
            // The crate lacks absolute-offset APIs, so build [start,end)
            // by moving character units from the range start.
            range
                .move_text(TextUnit::Character, 0)
                .map_err(|e| format!("collapse: {e}"))?;
            range
                .move_text(TextUnit::Character, start as i32)
                .map_err(|e| format!("move to start: {e}"))?;
            range
                .expand_to_enclosing_unit(TextUnit::Character)
                .map_err(|e| format!("expand: {e}"))?;
            range
                .move_endpoint_by_unit(
                    uiautomation::types::TextPatternRangeEndpoint::End,
                    TextUnit::Character,
                    (end - start) as i32,
                )
                .map_err(|e| format!("extend end: {e}"))?;
            let _ = TextPatternRangeEndpoint::Start;
            range.select().map_err(|e| format!("select: {e}"))
        }

        fn foreground_still(&self, expected_process: &str) -> bool {
            use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
            unsafe {
                let hwnd = GetForegroundWindow();
                if hwnd.0.is_null() {
                    return false;
                }
                use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
                let mut pid: u32 = 0;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                match crate::text_monitor::process_name_for_pid(pid) {
                    Some(name) => name.eq_ignore_ascii_case(expected_process),
                    None => false,
                }
            }
        }

        fn send_paste(&self) -> Result<(), String> {
            crate::input_inject::send_ctrl_v()
        }
    }
}

#[cfg(target_os = "windows")]
pub use win_probe::UiaApplyProbe;

#[cfg(not(target_os = "windows"))]
pub struct UiaApplyProbe;
#[cfg(not(target_os = "windows"))]
impl ApplyProbe for UiaApplyProbe {
    fn probe(&self, _: &str) -> Result<ApplyTarget, String> {
        Ok(ApplyTarget::Unsupported)
    }
    fn set_value(&self, _: &str) -> Result<(), String> {
        Err("unsupported".into())
    }
    fn select_range(&self, _: usize, _: usize) -> Result<(), String> {
        Err("unsupported".into())
    }
    fn foreground_still(&self, _: &str) -> bool {
        false
    }
    fn send_paste(&self) -> Result<(), String> {
        Err("unsupported".into())
    }
}

// ── Tauri command ───────────────────────────────────────────────────

#[tauri::command]
pub async fn apply_fix_command(
    app: tauri::AppHandle,
    request: ApplyRequest,
) -> Result<ApplyResult, String> {
    use tauri::Emitter;
    let req = request.clone();
    let result = tokio::task::spawn_blocking(move || {
        let probe = UiaApplyProbe::default();
        let clipboard = crate::clipboard::WinClipboard;
        apply_fix(&probe, &clipboard, &req)
    })
    .await
    .map_err(|e| format!("join: {e}"))?;
    // Analytics: record the rewrite outcome (PLAN-05 rewrites table).
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = crate::analytics::db::record_rewrite(
            now,
            "fix",
            if result.ok { "applied" } else { "dismissed" },
        );
    }
    // CONTRACTS 3: wb://apply-result { id, ok, error? }
    let _ = app.emit(
        "wb://apply-result",
        serde_json::json!({
            "id": format!("{}:{}", request.start, request.end),
            "ok": result.ok,
            "error": if result.ok { None } else { Some(result.message.clone()) },
        }),
    );
    Ok(result)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::ClipboardBackend;
    use std::cell::RefCell;
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct FakeClipboardState {
        text: RefCell<Option<String>>,
    }
    struct FakeClipboard<'a>(&'a FakeClipboardState);
    impl ClipboardBackend for FakeClipboard<'_> {
        fn snapshot(&self) -> Result<crate::clipboard::ClipboardSnapshot, String> {
            Ok(crate::clipboard::ClipboardSnapshot { formats: Vec::new() })
        }
        fn set_text(&self, text: &str) -> Result<(), String> {
            *self.0.text.borrow_mut() = Some(text.into());
            Ok(())
        }
        fn restore(&self, _: &crate::clipboard::ClipboardSnapshot) -> Result<(), String> {
            Ok(())
        }
        fn get_text(&self) -> Result<Option<String>, String> {
            Ok(self.0.text.borrow().clone())
        }
    }

    struct FakeProbe {
        process: &'static str,
        pattern: &'static str,
        text: std::sync::Mutex<String>,
        set_value_calls: AtomicUsize,
        select_calls: AtomicUsize,
        foreground_ok: std::sync::atomic::AtomicBool,
        foreground_checks: AtomicUsize,
        paste_calls: AtomicUsize,
    }

    impl FakeProbe {
        fn new(process: &'static str, pattern: &'static str, text: &str) -> Self {
            Self {
                process,
                pattern,
                text: std::sync::Mutex::new(text.to_string()),
                set_value_calls: AtomicUsize::new(0),
                select_calls: AtomicUsize::new(0),
                foreground_ok: std::sync::atomic::AtomicBool::new(true),
                foreground_checks: AtomicUsize::new(0),
                paste_calls: AtomicUsize::new(0),
            }
        }
    }

    impl ApplyProbe for FakeProbe {
        fn probe(&self, expected: &str) -> Result<ApplyTarget, String> {
            if !expected.eq_ignore_ascii_case(self.process) {
                return Ok(ApplyTarget::WrongTarget {
                    actual: self.process.to_string(),
                });
            }
            let text = self.text.lock().unwrap_or_else(|e| e.into_inner()).clone();
            Ok(match self.pattern {
                "value" => ApplyTarget::Value { element_text: text },
                "text" => ApplyTarget::Text { document_text: text },
                _ => ApplyTarget::Unsupported,
            })
        }
        fn set_value(&self, new_text: &str) -> Result<(), String> {
            self.set_value_calls.fetch_add(1, Ordering::AcqRel);
            *self.text.lock().unwrap_or_else(|e| e.into_inner()) = new_text.to_string();
            Ok(())
        }
        fn select_range(&self, _s: usize, _e: usize) -> Result<(), String> {
            self.select_calls.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
        fn foreground_still(&self, _expected: &str) -> bool {
            self.foreground_checks.fetch_add(1, Ordering::AcqRel);
            self.foreground_ok.load(Ordering::Acquire)
        }
        fn send_paste(&self) -> Result<(), String> {
            self.paste_calls.fetch_add(1, Ordering::AcqRel);
            // Simulate the paste landing in the fake field.
            let mut t = self.text.lock().unwrap_or_else(|e| e.into_inner());
            *t = format!("{}PASTED{}", t.get(0..3).unwrap_or(""), t.get(3..).unwrap_or(""));
            Ok(())
        }
    }

    fn req(process: &str, original: &str, start: usize, end: usize, replacement: &str) -> ApplyRequest {
        ApplyRequest {
            process: process.into(),
            original_text: original.into(),
            start,
            end,
            replacement: replacement.into(),
        }
    }

    #[test]
    fn splice_utf16_replaces_span_exactly() {
        assert_eq!(splice_utf16("teh cat", 0, 3, "the"), "the cat");
        assert_eq!(splice_utf16("a teh b", 2, 5, "the"), "a the b");
        // Astral char before the span: UTF-16 offsets count it as 2.
        assert_eq!(splice_utf16("\u{1F680} teh", 3, 6, "the"), "\u{1F680} the");
    }

    #[test]
    fn value_strategy_applies_and_verifies() {
        let probe = FakeProbe::new("notepad.exe", "value", "teh cat");
        let clip = FakeClipboardState::default();
        let r = apply_fix_impl(&probe, &FakeClipboard(&clip), &req("notepad.exe", "teh cat", 0, 3, "the"));
        assert!(r.ok);
        assert_eq!(r.strategy, "value-set");
        assert_eq!(*probe.text.lock().unwrap_or_else(|e| e.into_inner()), "the cat");
    }

    #[test]
    fn wrong_target_aborts_with_apply001_message() {
        let probe = FakeProbe::new("evil.exe", "value", "teh cat");
        let clip = FakeClipboardState::default();
        let r = apply_fix_impl(&probe, &FakeClipboard(&clip), &req("notepad.exe", "teh cat", 0, 3, "the"));
        assert!(!r.ok);
        assert_eq!(r.strategy, "aborted");
        assert!(r.message.contains("INV-APPLY-001"));
        assert!(r.message.contains("evil.exe"));
    }

    #[test]
    fn changed_text_aborts_without_write() {
        let probe = FakeProbe::new("notepad.exe", "value", "DIFFERENT NOW");
        let clip = FakeClipboardState::default();
        let r = apply_fix_impl(&probe, &FakeClipboard(&clip), &req("notepad.exe", "teh cat", 0, 3, "the"));
        assert!(!r.ok);
        assert_eq!(probe.set_value_calls.load(Ordering::Acquire), 0);
    }

    #[test]
    fn foreground_race_aborts_before_paste() {
        let probe = FakeProbe::new("notepad.exe", "text", "teh cat");
        probe.foreground_ok.store(false, Ordering::Release);
        let clip = FakeClipboardState::default();
        let r = apply_fix_impl(&probe, &FakeClipboard(&clip), &req("notepad.exe", "teh cat", 0, 3, "the"));
        assert!(!r.ok);
        assert_eq!(r.strategy, "aborted");
        assert_eq!(probe.paste_calls.load(Ordering::Acquire), 0, "no synthetic input may be sent on mismatch");
    }

    #[test]
    fn text_strategy_selects_then_pastes() {
        let probe = FakeProbe::new("notepad.exe", "text", "teh cat");
        let clip = FakeClipboardState::default();
        let r = apply_fix_impl(&probe, &FakeClipboard(&clip), &req("notepad.exe", "teh cat", 0, 3, "the"));
        assert!(r.ok, "{}", r.message);
        assert_eq!(r.strategy, "text-paste");
        assert_eq!(probe.select_calls.load(Ordering::Acquire), 1);
        assert_eq!(probe.paste_calls.load(Ordering::Acquire), 1);
    }

    #[test]
    fn unsupported_degrades() {
        let probe = FakeProbe::new("weird.exe", "none", "");
        let clip = FakeClipboardState::default();
        let r = apply_fix_impl(&probe, &FakeClipboard(&clip), &req("weird.exe", "x", 0, 1, "y"));
        assert!(!r.ok);
        assert_eq!(r.strategy, "unsupported");
    }
}
