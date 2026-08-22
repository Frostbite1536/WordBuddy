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
    send_combo(
        windows::Win32::UI::Input::KeyboardAndMouse::VK_CONTROL,
        'V',
    )
}

#[cfg(target_os = "windows")]
pub fn send_ctrl_c() -> Result<(), String> {
    send_combo(
        windows::Win32::UI::Input::KeyboardAndMouse::VK_CONTROL,
        'C',
    )
}

#[cfg(target_os = "windows")]
fn send_combo(
    vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY,
    ch: char,
) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
        KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    let _guard = INPUT_GUARD
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let ctrl: u16 = vk.0;
    let key: u16 = ch.to_ascii_uppercase() as u16;

    let mk = |vk: u16, up: bool| -> INPUT {
        let mut input = INPUT::default();
        input.r#type = INPUT_KEYBOARD;
        input.Anonymous.ki = KEYBDINPUT {
            wVk: VIRTUAL_KEY(vk),
            wScan: 0,
            dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
            time: 0,
            dwExtraInfo: 0,
        };
        input
    };

    // Down Ctrl, down key, up key, up Ctrl — one atomic burst.
    let burst = [mk(ctrl, false), mk(key, false), mk(key, true), mk(ctrl, true)];
    let sent = unsafe {
        SendInput(
            &burst,
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent == burst.len() as u32 {
        Ok(())
    } else {
        Err(format!("SendInput sent {sent}/{} events", burst.len()))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn send_ctrl_v() -> Result<(), String> {
    Err("unsupported on this platform".into())
}

#[cfg(not(target_os = "windows"))]
pub fn send_ctrl_c() -> Result<(), String> {
    Err("unsupported on this platform".into())
}

#[cfg(target_os = "windows")]
pub fn send_backspaces(count: usize) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
        KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    let _g = INPUT_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let mut burst = Vec::with_capacity(count * 2);
    for _ in 0..count {
        let mut down = INPUT::default();
        down.r#type = INPUT_KEYBOARD;
        down.Anonymous.ki = KEYBDINPUT { wVk: VIRTUAL_KEY(0x08), dwFlags: KEYBD_EVENT_FLAGS(0), ..Default::default() };
        burst.push(down);
        let mut up = INPUT::default();
        up.r#type = INPUT_KEYBOARD;
        up.Anonymous.ki = KEYBDINPUT { wVk: VIRTUAL_KEY(0x08), dwFlags: KEYEVENTF_KEYUP, ..Default::default() };
        burst.push(up);
    }
    let sent = unsafe { SendInput(&burst, std::mem::size_of::<INPUT>() as i32) };
    if sent == burst.len() as u32 { Ok(()) } else { Err(format!("SendInput sent {sent}/{}", burst.len())) }
}

#[cfg(target_os = "windows")]
pub fn send_left_arrows(count: usize) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    let _g = INPUT_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let mut burst = Vec::with_capacity(count * 2);
    for _ in 0..count {
        let mut down = INPUT::default();
        down.r#type = INPUT_KEYBOARD;
        down.Anonymous.ki = KEYBDINPUT { wVk: VIRTUAL_KEY(0x25), dwFlags: KEYBD_EVENT_FLAGS(0), ..Default::default() };
        burst.push(down);
        let mut up = INPUT::default();
        up.r#type = INPUT_KEYBOARD;
        up.Anonymous.ki = KEYBDINPUT { wVk: VIRTUAL_KEY(0x25), dwFlags: KEYEVENTF_KEYUP, ..Default::default() };
        burst.push(up);
    }
    let sent = unsafe { SendInput(&burst, std::mem::size_of::<INPUT>() as i32) };
    if sent == burst.len() as u32 { Ok(()) } else { Err(format!("SendInput sent {sent}/{}", burst.len())) }
}

/// Types arbitrary text via KEYEVENTF_UNICODE (chained SendInput bursts
/// of 32 chars to stay within the API's limit).
#[cfg(target_os = "windows")]
pub fn send_unicode_text(text: &str) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VIRTUAL_KEY,
    };
    let _g = INPUT_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    for chunk in text.chars().collect::<Vec<_>>().chunks(30) {
        let mut burst = Vec::with_capacity(chunk.len() * 2);
        for c in chunk {
            let cp = *c as u16;
            let mut down = INPUT::default();
            down.r#type = INPUT_KEYBOARD;
            down.Anonymous.ki = KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: cp, dwFlags: KEYEVENTF_UNICODE, ..Default::default() };
            burst.push(down);
            let mut up = INPUT::default();
            up.r#type = INPUT_KEYBOARD;
            up.Anonymous.ki = KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: cp, dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, ..Default::default() };
            burst.push(up);
        }
        let sent = unsafe { SendInput(&burst, std::mem::size_of::<INPUT>() as i32) };
        if sent != burst.len() as u32 {
            return Err(format!("unicode SendInput sent {sent}/{}", burst.len()));
        }
    }
    Ok(())
}
