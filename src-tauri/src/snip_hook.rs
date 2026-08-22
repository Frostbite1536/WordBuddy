//! Text-expansion keyboard hook (PLAN-06 task 4; ledger W6).
//!
//! **INV-HOOK-001** — admission-blocking rules for the callback:
//! - O(1) work only: append one char to a fixed 32-char ring, bounded
//!   scan of the trigger list, never I/O, never block, ALWAYS
//!   `CallNextHookEx` — even during expansion.
//! - Expansion runs on a worker thread via the PLAN-04 input utilities.
//! - Watchdog: callback overruns (> 2 ms) trip self-disable; the hook is
//!   removed and the feature flags itself off (surfaced via Settings +
//!   log). FRICTION entry records the incident class.
//! - Buffer privacy: the ring holds the last 32 chars transiently for
//!   matching only — never persisted, never logged.
//!
//! Kill switches: `snippets_enabled` config (default OFF), per-snippet
//! disable (omit from config), global pause flag (`snippets_paused`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

pub const RING_LEN: usize = 32;
/// Callback budget in nanoseconds; overruns trip the watchdog.
pub const CALLBACK_BUDGET_NS: u64 = 2_000_000;

static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);
static PAUSED: AtomicBool = AtomicBool::new(false);
static WATCHDOG_TRIPPED: AtomicBool = AtomicBool::new(false);
static LAST_CB_NS: AtomicU64 = AtomicU64::new(0);

/// Conservative process deny-list: text expansion never fires in
/// terminals or IDEs (trigger chars are shell/IDE syntax there).
pub const DEFAULT_EXCLUDED_PROCESSES: &[&str] = &[
    "windowsterminal.exe", "cmd.exe", "powershell.exe", "pwsh.exe",
    "conhost.exe", "code.exe", "devenv.exe", "idea64.exe", "goland64.exe",
    "pycharm64.exe", "rider64.exe", "clion64.exe", "vim.exe", "nvim.exe",
    "notepad.exe", "notepad++.exe",
];

#[derive(Debug, Clone)]
pub struct HookConfig {
    pub triggers: Vec<String>,
    pub excluded_processes: Vec<String>,
}

static CONFIG: Mutex<Option<HookConfig>> = Mutex::new(None);

fn with_config<T>(f: impl FnOnce(&HookConfig) -> T) -> Option<T> {
    let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().map(f)
}

pub fn set_config(cfg: Option<HookConfig>) {
    let mut guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
    *guard = cfg;
}

pub fn set_paused(paused: bool) {
    PAUSED.store(paused, Ordering::Release);
}

pub fn is_watchdog_tripped() -> bool {
    WATCHDOG_TRIPPED.load(Ordering::Acquire)
}

pub fn is_active() -> bool {
    HOOK_ACTIVE.load(Ordering::Acquire) && !WATCHDOG_TRIPPED.load(Ordering::Acquire)
}

/// Pure matcher: does the ring (last `RING_LEN` chars, oldest first)
/// end with any trigger? Returns the matched trigger + its length.
/// Bounded: triggers are capped at 64 entries × 32 chars by the setter.
pub fn match_trigger(ring: &[u8; RING_LEN], ring_len: usize, triggers: &[String]) -> Option<(String, usize)> {
    if triggers.len() > 64 {
        return None;
    }
    for trig in triggers {
        let t = trig.as_bytes();
        if t.is_empty() || t.len() > RING_LEN || t.len() > ring_len {
            continue;
        }
        // Bound BOTH ends by ring_len: an open-ended slice runs to the
        // end of the fixed 32-byte array and drags in stale zero bytes
        // whenever the ring isn't full — the trigger then never matches
        // (caught by the new unit tests; expansion was dead below 32
        // buffered chars).
        let suffix = &ring[ring_len - t.len()..ring_len];
        if suffix == t {
            return Some((trig.clone(), t.len()));
        }
    }
    None
}

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use std::sync::atomic::AtomicIsize;

    static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);
    static EXPAND_TX: std::sync::Mutex<Option<std::sync::mpsc::Sender<String>>> =
        std::sync::Mutex::new(None);

    /// Ring: printable ASCII chars only (triggers are ASCII by config).
    static RING: Mutex<( [u8; 32], usize )> = Mutex::new(([0u8; 32], 0));

    fn ring_push(c: u8) -> Option<(String, usize)> {
        let (matched_trigger, matched_len) = {
            let mut guard = RING.lock().unwrap_or_else(|e| e.into_inner());
            let (buf, len) = &mut *guard;
            if *len < RING_LEN {
                buf[*len] = c;
                *len += 1;
            } else {
                buf.copy_within(1.., 0);
                buf[RING_LEN - 1] = c;
            }
            let triggers = with_config(|cfg| cfg.triggers.clone())?;
            match match_trigger(buf, *len, &triggers) {
                Some((t, l)) => (t, l),
                None => return None,
            }
        };
        // Clear the ring after a match so the expansion isn't re-matched.
        {
            let mut guard = RING.lock().unwrap_or_else(|e| e.into_inner());
            guard.1 = 0;
        }
        Some((matched_trigger, matched_len))
    }

    fn ring_clear() {
        let mut guard = RING.lock().unwrap_or_else(|e| e.into_inner());
        guard.1 = 0;
    }

    unsafe extern "system" fn hook_proc(
        code: i32,
        wparam: windows::Win32::Foundation::WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        let t0 = std::time::Instant::now();
        // O(1) body — bounded work only. Any early return still calls
        // CallNextHookEx at the end (single exit point).
        if code >= 0 && !WATCHDOG_TRIPPED.load(Ordering::Relaxed) {
            let kbd = &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT);
            let wm_keydown = 0x0100u32; // wparam: WM_KEYDOWN
            let is_keydown = wparam.0 == wm_keydown as usize;
            if is_keydown && !PAUSED.load(Ordering::Relaxed) {
                // Printable ASCII range via vkCode (0x41..=0x5A map to
                // 'A'..'Z'; 0x30..=0x39 to digits; punctuation via the
                // shifted set is approximated by the base char — trigger
                // definitions should use unshifted ASCII).
                let vk = kbd.vkCode;
                let ch = if (0x41..=0x5A).contains(&vk) {
                    Some((vk + 32) as u8) // lowercase
                } else if (0x30..=0x39).contains(&vk) {
                    Some(vk as u8)
                } else {
                    match vk {
                        0xBA => Some(b';'), 0xBB => Some(b'='), 0xBC => Some(b','),
                        0xBD => Some(b'-'), 0xBE => Some(b'.'), 0xBF => Some(b'/'),
                        0xC0 => Some(b'`'), 0xDB => Some(b'['), 0xDC => Some(b'\\'),
                        0xDD => Some(b']'), 0xDE => Some(b'\''),
                        _ => None,
                    }
                };
                if let Some(c) = ch {
                    if let Some((trigger, _tlen)) = ring_push(c) {
                        // Post to the worker; NEVER expand in-callback.
                        let guard = EXPAND_TX.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(tx) = guard.as_ref() {
                            let _ = tx.send(trigger.clone());
                            ring_clear();
                        }
                    }
                }
            }
        }
        let elapsed = t0.elapsed().as_nanos() as u64;
        LAST_CB_NS.store(elapsed, Ordering::Relaxed);
        if elapsed > CALLBACK_BUDGET_NS {
            WATCHDOG_TRIPPED.store(true, Ordering::Release);
        }
        windows::Win32::UI::WindowsAndMessaging::CallNextHookEx(
            None, code, wparam, lparam,
        )
    }

    pub fn start(cfg: HookConfig, app: tauri::AppHandle) -> Result<(), String> {
        if HOOK_ACTIVE.load(Ordering::Acquire) {
            // Already running — refresh triggers/deny-list in place
            // (Settings edits must take effect without off/on toggle;
            // verifier finding F3, entry 0017). The expansion worker
            // re-reads snippet bodies from global config per event, so
            // refreshing this HookConfig is sufficient.
            set_config(Some(cfg));
            return Ok(());
        }
        WATCHDOG_TRIPPED.store(false, Ordering::Release);
        set_config(Some(cfg));

        let (tx, rx) = std::sync::mpsc::channel::<String>();
        *EXPAND_TX.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);

        // Expansion worker: performs the synthetic input OUTSIDE the hook.
        std::thread::spawn(move || {
            while let Ok(trigger) = rx.recv() {
                let Some(cfg) = with_config(|c| HookConfig {
                    triggers: c.triggers.clone(),
                    excluded_processes: c.excluded_processes.clone(),
                }) else { continue };
                let Some((body, cursor_offset)) = crate::config::with_config_pub(|cc| {
                    cc.snippets
                        .iter()
                        .find(|s| s.trigger == trigger)
                        .map(|s| (s.body.clone(), s.cursor_offset))
                }) else { continue };

                // INV-APPLY-001 scope check: never expand in excluded
                // foreground processes.
                unsafe {
                    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
                    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
                    let hwnd = GetForegroundWindow();
                    if hwnd.0.is_null() { continue; }
                    let mut pid: u32 = 0;
                    GetWindowThreadProcessId(hwnd, Some(&mut pid));
                    let Some(name) = crate::text_monitor::process_name_for_pid(pid) else { continue };
                    let lname = name.to_ascii_lowercase();
                    let excluded = DEFAULT_EXCLUDED_PROCESSES.iter().any(|d| lname == *d)
                        || cfg.excluded_processes.iter().any(|u| {
                            let u = u.trim().to_ascii_lowercase();
                            !u.is_empty() && (lname == u || lname == format!("{u}.exe"))
                        });
                    if excluded { continue; }
                }

                // Expansion: backspace the trigger chars, then type the
                // body via unicode SendInput (the PLAN-04 input path).
                if crate::input_inject::send_backspaces(trigger.chars().count()).is_err() {
                    continue;
                }
                let rendered = body.replace("$CURSOR$", "");
                if crate::input_inject::send_unicode_text(&rendered).is_err() {
                    continue;
                }
                // $CURSOR$ marker: move caret left by the tail length.
                if let Some(pos) = body.find("$CURSOR$") {
                    let tail_chars =
                        body[pos + "$CURSOR$".len()..].chars().count();
                    let _ = crate::input_inject::send_left_arrows(tail_chars);
                }
                let _ = cursor_offset; // reserved for finer caret placement
            }
        });

        // Hook thread: install + message pump (WH_KEYBOARD_LL requires it).
        std::thread::spawn(move || unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
            };
            use windows::Win32::Foundation::HMODULE;
            let hhook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), HMODULE::default(), 0);
            match hhook {
                Ok(h) => {
                    HOOK_HANDLE.store(h.0 as isize, Ordering::Release);
                    HOOK_ACTIVE.store(true, Ordering::Release);
                    let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
                    while windows::Win32::UI::WindowsAndMessaging::GetMessageW(
                        &mut msg, None, 0, 0,
                    )
                    .as_bool()
                    {
                        windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                        windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
                        if WATCHDOG_TRIPPED.load(Ordering::Acquire) {
                            break;
                        }
                    }
                    let h = HOOK_HANDLE.swap(0, Ordering::AcqRel);
                    if h != 0 {
                        // isize -> HHOOK round-trip (same ABI pin note as
                        // a11y/windows_impl.rs).
                        let hook = windows::Win32::UI::WindowsAndMessaging::HHOOK(
                            h as *mut core::ffi::c_void,
                        );
                        let _ = UnhookWindowsHookEx(hook);
                    }
                    HOOK_ACTIVE.store(false, Ordering::Release);
                    eprintln!("[snippets] hook removed (watchdog or shutdown)");
                }
                Err(e) => {
                    eprintln!("[snippets] SetWindowsHookExW failed: {e}");
                    WATCHDOG_TRIPPED.store(true, Ordering::Release);
                }
            }
        });
        Ok(())
    }

    pub fn stop() {
        WATCHDOG_TRIPPED.store(true, Ordering::Release);
        HOOK_ACTIVE.store(false, Ordering::Release);
        // Drop the expansion channel: queued-but-unprocessed triggers
        // must not expand after stop, and dropping the sender ends the
        // worker thread (verifier finding F3).
        *EXPAND_TX.lock().unwrap_or_else(|e| e.into_inner()) = None;
        // The pump loop observes the watchdog flag and unhooks itself.
    }
}

#[cfg(target_os = "windows")]
pub use win::{start, stop};

#[cfg(not(target_os = "windows"))]
pub fn start(_cfg: HookConfig, _app: tauri::AppHandle) -> Result<(), String> {
    Err("unsupported on this platform".into())
}

#[cfg(not(target_os = "windows"))]
pub fn stop() {}

/// Pure simulation for the Settings test box — no hook, no OS.
pub fn simulate_expansion(
    typed_text: &str,
    snippets: &[crate::engine::Snippet],
) -> Option<(String, String, usize)> {
    let triggers: Vec<String> = snippets.iter().map(|s| s.trigger.clone()).collect();
    let mut buf = [0u8; RING_LEN];
    let mut len = 0usize;
    for b in typed_text.bytes() {
        if len < RING_LEN {
            buf[len] = b;
            len += 1;
        } else {
            buf.copy_within(1.., 0);
            buf[RING_LEN - 1] = b;
        }
    }
    match match_trigger(&buf, len, &triggers) {
        Some((trigger, tlen)) => {
            let snip = snippets.iter().find(|s| s.trigger == trigger)?;
            let head = &typed_text[..typed_text.len() - tlen];
            let expanded_body = snip.body.replace("$CURSOR$", "");
            let cursor_from_end = snip
                .body
                .find("$CURSOR$")
                .map(|pos| snip.body[pos + "$CURSOR$".len()..].chars().count())
                .unwrap_or(0);
            Some((
                format!("{head}{expanded_body}"),
                trigger,
                cursor_from_end,
            ))
        }
        None => None,
    }
}

#[tauri::command]
pub fn snippet_test(typed: String) -> Result<Option<serde_json::Value>, String> {
    let snippets = crate::config::with_config_pub(|c| c.snippets.clone());
    Ok(simulate_expansion(&typed, &snippets).map(|(expanded, trigger, cursor)| {
        serde_json::json!({ "expanded": expanded, "trigger": trigger, "cursorOffset": cursor })
    }))
}

#[tauri::command]
pub fn snippet_hook_start(app: tauri::AppHandle) -> Result<(), String> {
    let (triggers, excluded) = crate::config::with_config_pub(|c| {
        (
            // match_trigger disables ALL snippets above 64 triggers —
            // cap deterministically instead of losing the feature
            // (verifier residual (e)).
            c.snippets.iter().map(|s| s.trigger.clone()).take(64).collect(),
            c.excluded_processes.clone(),
        )
    });
    start(HookConfig { triggers, excluded_processes: excluded }, app)
}

#[tauri::command]
pub fn snippet_hook_stop() -> Result<(), String> {
    stop();
    Ok(())
}

#[tauri::command]
pub fn snippet_hook_status() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "active": is_active(),
        "watchdogTripped": is_watchdog_tripped(),
        "paused": PAUSED.load(Ordering::Acquire),
    }))
}

#[tauri::command]
pub fn snippet_set_paused(paused: bool) -> Result<(), String> {
    set_paused(paused);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snip(trigger: &str, body: &str) -> crate::engine::Snippet {
        crate::engine::Snippet {
            trigger: trigger.into(),
            body: body.into(),
            cursor_offset: 0,
        }
    }

    #[test]
    fn match_trigger_hits_ring_suffix() {
        let mut ring = [0u8; RING_LEN];
        ring[..4].copy_from_slice(b"jira");
        let triggers = vec!["jb".to_string(), "jira".to_string()];
        let (t, len) = match_trigger(&ring, 4, &triggers).unwrap();
        assert_eq!((t.as_str(), len), ("jira", 4));
    }

    #[test]
    fn match_trigger_no_match_partial() {
        let mut ring = [0u8; RING_LEN];
        ring[..3].copy_from_slice(b"jir");
        let triggers = vec!["jira".to_string()];
        assert!(match_trigger(&ring, 3, &triggers).is_none());
    }

    #[test]
    fn match_trigger_over_cap_disables_all() {
        let ring = [0u8; RING_LEN];
        let triggers: Vec<String> = (0..65).map(|i| format!("t{i}")).collect();
        assert!(match_trigger(&ring, RING_LEN, &triggers).is_none());
    }

    #[test]
    fn simulate_expansion_cursor_counts_chars_not_marker_bytes() {
        // "Hi$CURSOR$!": caret lands before "!" → 1 char from the end.
        // Byte math (marker len included) reported 9 — verifier residual (c).
        let (_, _, cursor) =
            simulate_expansion("x hi", &[snip("hi", "Hi$CURSOR$!")]).unwrap();
        assert_eq!(cursor, 1);
    }

    #[test]
    fn simulate_expansion_no_marker_means_caret_at_end() {
        let (_, _, cursor) =
            simulate_expansion("x brb", &[snip("brb", "be right back")]).unwrap();
        assert_eq!(cursor, 0);
    }

    #[test]
    fn simulate_expansion_prefix_survives() {
        let (expanded, trigger, _) =
            simulate_expansion("say brb", &[snip("brb", "be right back")]).unwrap();
        assert_eq!(trigger, "brb");
        assert_eq!(expanded, "say be right back");
    }
}
