//! Synthetic input injection (PLAN-04 paste path).
//!
//! The ONLY synthesized input in the product: Ctrl+V for fix
//! application and Ctrl+C for selection capture, both called exclusively
//! from `apply.rs` / `selection.rs` paths that have just re-verified the
//! foreground process (INV-APPLY-001). No arbitrary key sequences.

use std::sync::Mutex;

// Serialization guard: interleaved SendInput bursts from two threads
// would produce garbage keystrokes.
static INPUT_GUARD: Mutex<()> = Mutex::new(());

#[cfg(target_os = "windows")]
pub fn send_ctrl_v() -> Result<(), String> {
    send_combo(windows::Win32::UI::Input::KeyboardAndMouse::VK_CONTROL, 'V')
}

#[cfg(target_os = "windows")]
pub fn send_ctrl_c() -> Result<(), String> {
    send_combo(windows::Win32::UI::Input::KeyboardAndMouse::VK_CONTROL, 'C')
}

#[cfg(target_os = "windows")]
fn send_combo(
    vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY,
    ch: char,
) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY,
    };

    let _guard = INPUT_GUARD.lock().unwrap_or_else(|e| e.into_inner());

    let ctrl: u16 = vk.0;
    let key: u16 = ch.to_ascii_uppercase() as u16;

    let mk = |vk: u16, up: bool| -> INPUT {
        let mut input = INPUT {
            r#type: INPUT_KEYBOARD,
            ..Default::default()
        };
        input.Anonymous.ki = KEYBDINPUT {
            wVk: VIRTUAL_KEY(vk),
            wScan: 0,
            dwFlags: if up {
                KEYEVENTF_KEYUP
            } else {
                KEYBD_EVENT_FLAGS(0)
            },
            time: 0,
            dwExtraInfo: 0,
        };
        input
    };

    // Down Ctrl, down key, up key, up Ctrl — one atomic burst.
    let burst = [
        mk(ctrl, false),
        mk(key, false),
        mk(key, true),
        mk(ctrl, true),
    ];
    let sent = unsafe { SendInput(&burst, std::mem::size_of::<INPUT>() as i32) };
    if sent == burst.len() as u32 {
        Ok(())
    } else {
        Err(format!("SendInput sent {sent}/{} events", burst.len()))
    }
}

// ── macOS (Quartz CGEvent) ──────────────────────────────────────────

/// Synthetic Cmd-combos via CGEvent. The Accessibility/Input-Monitoring
/// permission governs posting events to other apps; without it the
/// posted events silently do nothing, mirroring how the AX reader
/// degrades when unpermitted.
///
/// Keycodes are Quartz virtual codes (kVK_ANSI positions): C=0x08,
/// V=0x09. Layout-dependent remapping is out of scope for v1 — these
/// two shortcuts sit on identical physical positions on the layouts
/// WordBuddy targets.
#[cfg(target_os = "macos")]
fn send_cmd_combo(keycode: u16) -> Result<(), String> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let _guard = INPUT_GUARD.lock().unwrap_or_else(|e| e.into_inner());

    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|()| "CGEventSource::new failed".to_string())?;
    // Down Cmd → down key → up key → up Cmd, matching the Win32 burst
    // ordering so the target app sees an atomic chord.
    for (state, flags) in [
        (true, Some(CGEventFlags::CGEventFlagCommand)),
        (true, None),
        (false, None),
        (false, Some(CGEventFlags::CGEventFlagCommand)),
    ] {
        let event = CGEvent::new_keyboard_event(source.clone(), keycode, state)
            .map_err(|()| format!("CGEvent create failed (down={state})"))?;
        if let Some(f) = flags {
            event.set_flags(f);
        }
        event.post(CGEventTapLocation::Session);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(dead_code)] // native apply is deliberately Windows-only in v1
pub fn send_ctrl_v() -> Result<(), String> {
    send_cmd_combo(0x09) // kVK_ANSI_V
}

#[cfg(target_os = "macos")]
pub fn send_ctrl_c() -> Result<(), String> {
    send_cmd_combo(0x08) // kVK_ANSI_C
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn send_ctrl_v() -> Result<(), String> {
    Err("unsupported on this platform".into())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn send_ctrl_c() -> Result<(), String> {
    Err("unsupported on this platform".into())
}

// ── Linux / X11 (XTEST via x11rb) ───────────────────────────────────

/// Synthetic Ctrl+chords through the XTEST extension.
///
/// Wayland is UNSUPPORTED by design: compositors intentionally provide no
/// global-input-injection path for plain clients (same posture as
/// `context.rs`'s Wayland note). We return a precise error instead of a
/// generic failure so Settings can explain why.
///
/// Keycodes are the default evdev mapping (Control_L=37, C=54, V=55).
/// Layouts that move C/V will paste/copy wrong keys — accepted v1
/// tradeoff, documented here rather than hidden; the Windows/macOS paths
/// use layout-stable mechanisms.
#[cfg(target_os = "linux")]
fn session_is_wayland_only() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_none()
}

#[cfg(target_os = "linux")]
fn send_xtest_chord(keycode: u8) -> Result<(), String> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xtest::ConnectionExt as _;

    if session_is_wayland_only() {
        return Err(
            "synthetic input is not possible on Wayland by design (X11/XWayland required)".into(),
        );
    }

    let _guard = INPUT_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let (conn, _) = x11rb::connect(None).map_err(|e| format!("X11 connect failed: {e}"))?;

    const KEY_PRESS: u8 = 2;
    const KEY_RELEASE: u8 = 3;
    const CONTROL_L: u8 = 37;
    const CURRENT_TIME: u32 = 0;
    const NO_WINDOW: x11rb::protocol::xproto::Window = 0;
    const FAKE_DEVICE: u8 = 3; // XTEST virtual device

    let press = |key: u8| {
        conn.xtest_fake_input(KEY_PRESS, key, CURRENT_TIME, NO_WINDOW, 0, 0, FAKE_DEVICE)
            .map_err(|e| e.to_string())
    };
    let release = |key: u8| {
        conn.xtest_fake_input(KEY_RELEASE, key, CURRENT_TIME, NO_WINDOW, 0, 0, FAKE_DEVICE)
            .map_err(|e| e.to_string())
    };

    press(CONTROL_L)?;
    press(keycode)?;
    release(keycode)?;
    release(CONTROL_L)?;
    conn.flush().map_err(|e| format!("X11 flush failed: {e}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(dead_code)] // native apply is deliberately Windows-only in v1
pub fn send_ctrl_v() -> Result<(), String> {
    send_xtest_chord(55)
}

#[cfg(target_os = "linux")]
pub fn send_ctrl_c() -> Result<(), String> {
    send_xtest_chord(54)
}

#[cfg(target_os = "windows")]
pub fn send_backspaces(count: usize) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY,
    };
    let _g = INPUT_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let mut burst = Vec::with_capacity(count * 2);
    for _ in 0..count {
        let mut down = INPUT {
            r#type: INPUT_KEYBOARD,
            ..Default::default()
        };
        down.Anonymous.ki = KEYBDINPUT {
            wVk: VIRTUAL_KEY(0x08),
            dwFlags: KEYBD_EVENT_FLAGS(0),
            ..Default::default()
        };
        burst.push(down);
        let mut up = INPUT {
            r#type: INPUT_KEYBOARD,
            ..Default::default()
        };
        up.Anonymous.ki = KEYBDINPUT {
            wVk: VIRTUAL_KEY(0x08),
            dwFlags: KEYEVENTF_KEYUP,
            ..Default::default()
        };
        burst.push(up);
    }
    let sent = unsafe { SendInput(&burst, std::mem::size_of::<INPUT>() as i32) };
    if sent == burst.len() as u32 {
        Ok(())
    } else {
        Err(format!("SendInput sent {sent}/{}", burst.len()))
    }
}

#[cfg(target_os = "windows")]
pub fn send_left_arrows(count: usize) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY,
    };
    let _g = INPUT_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let mut burst = Vec::with_capacity(count * 2);
    for _ in 0..count {
        let mut down = INPUT {
            r#type: INPUT_KEYBOARD,
            ..Default::default()
        };
        down.Anonymous.ki = KEYBDINPUT {
            wVk: VIRTUAL_KEY(0x25),
            dwFlags: KEYBD_EVENT_FLAGS(0),
            ..Default::default()
        };
        burst.push(down);
        let mut up = INPUT {
            r#type: INPUT_KEYBOARD,
            ..Default::default()
        };
        up.Anonymous.ki = KEYBDINPUT {
            wVk: VIRTUAL_KEY(0x25),
            dwFlags: KEYEVENTF_KEYUP,
            ..Default::default()
        };
        burst.push(up);
    }
    let sent = unsafe { SendInput(&burst, std::mem::size_of::<INPUT>() as i32) };
    if sent == burst.len() as u32 {
        Ok(())
    } else {
        Err(format!("SendInput sent {sent}/{}", burst.len()))
    }
}

/// Types arbitrary text via KEYEVENTF_UNICODE (chained SendInput bursts
/// of 32 chars to stay within the API's limit).
#[cfg(target_os = "windows")]
pub fn send_unicode_text(text: &str) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
        VIRTUAL_KEY,
    };
    let _g = INPUT_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    // UTF-16 units, not chars: astral-plane chars must arrive as
    // surrogate pairs (verifier residual (d), entry 0017).
    for chunk in text.encode_utf16().collect::<Vec<_>>().chunks(30) {
        let mut burst = Vec::with_capacity(chunk.len() * 2);
        for &cp in chunk {
            let mut down = INPUT {
                r#type: INPUT_KEYBOARD,
                ..Default::default()
            };
            down.Anonymous.ki = KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: cp,
                dwFlags: KEYEVENTF_UNICODE,
                ..Default::default()
            };
            burst.push(down);
            let mut up = INPUT {
                r#type: INPUT_KEYBOARD,
                ..Default::default()
            };
            up.Anonymous.ki = KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: cp,
                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                ..Default::default()
            };
            burst.push(up);
        }
        let sent = unsafe { SendInput(&burst, std::mem::size_of::<INPUT>() as i32) };
        if sent != burst.len() as u32 {
            return Err(format!("unicode SendInput sent {sent}/{}", burst.len()));
        }
    }
    Ok(())
}
