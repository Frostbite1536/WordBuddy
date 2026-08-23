//! F4 empirical probe (run manually: `cargo run --example uia_probe`).
//!
//! Answers with runtime evidence whether UIA `TextUnit::Character`
//! counts a surrogate pair as ONE unit (Unicode scalar) or TWO units
//! (UTF-16 code unit) — the assumption the apply-path offset
//! conversion in `apply.rs::utf16_offset_to_scalars` rests on.
//!
//! Method: put "a 😀 b" (6 UTF-16 units, 5 scalars) into a real
//! Notepad edit control via WM_SETTEXT, then walk the document range
//! with `move_text(Character, N)` for N = 0..=5 and read back which
//! character each landing position selects.
//!
//! Interpretation: if Character == scalar, the selected char sequence
//! over N=0..5 is a, ' ', 😀, ' ', b. If Character == UTF-16 unit,
//! positions inside the emoji produce replacement/garbled output.
// Windows-only probe, but `cargo check --all-targets` (CI) compiles
// examples on every platform — the real code lives in a cfg-gated
// module and a no-op main keeps other platforms valid.
#[cfg(target_os = "windows")]
mod probe {
use std::process::Command;
use uiautomation::patterns::UITextPattern;
use uiautomation::types::TextUnit;
use uiautomation::UIAutomation;

pub(super) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let text = "a \u{1F600} b";
    println!("probe text: {text:?}");
    println!(
        "utf-16 units: {}, scalars: {}",
        text.encode_utf16().count(),
        text.chars().count()
    );

    // Classic Notepad's EDIT control has no UIA TextPattern; WordPad's
    // RICHEDIT50W does.
    let mut child =
        Command::new(r"C:\Program Files\Windows NT\Accessories\wordpad.exe").spawn()?;
    std::thread::sleep(std::time::Duration::from_millis(1500));

    let result = probe(&automation_setup(), child.id(), text);
    child.kill().ok();

    println!("\n=== VERDICT ===");
    match result {
        Ok(units_are_scalars) => {
            if units_are_scalars {
                println!("UIA TextUnit::Character counts a surrogate pair as ONE unit.");
                println!("apply.rs utf16_offset_to_scalars conversion is CORRECT.");
            } else {
                println!("UIA TextUnit::Character counts UTF-16 code units!");
                println!("apply.rs conversion would MISSELECT — needs revert to raw offsets.");
            }
        }
        Err(e) => {
            println!("probe failed: {e}");
            std::process::exit(1);
        }
    }
    Ok(())
}

fn automation_setup() -> uiautomation::UIAutomation {
    UIAutomation::new().expect("UIAutomation::new")
}

/// Returns Ok(true) when Character units == Unicode scalars.
fn probe(
    automation: &UIAutomation,
    pid: u32,
    text: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let el = automation
        .create_matcher()
        .filter_fn(Box::new(move |e: &uiautomation::UIElement| {
            Ok(e.get_process_id()? == pid)
        }))
        .classname("RICHEDIT50W")
        .timeout(5000)
        .find_first()
        .map_err(|e| format!("matcher failed: {e}"))?;
    println!("stage 1: edit control found");

    // Set the text via WM_SETTEXT (no clipboard, no focus games).
    {
        use windows::Win32::Foundation::{HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SETTEXT};
        let hwnd = el.get_native_window_handle().map_err(|e| format!("native handle: {e}"))?;
        println!("stage 2: hwnd acquired");
        // Handle's inner field is private; the crate itself transmutes
        // it for Debug — same trick, HANDLE is a pointer wrapper.
        let hwnd_isize: isize = unsafe { std::mem::transmute(hwnd) };
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.push(0);
        unsafe {
            SendMessageW(
                HWND(hwnd_isize as _),
                WM_SETTEXT,
                windows::Win32::Foundation::WPARAM(0),
                LPARAM(wide.as_ptr() as isize),
            );
        }
        println!("stage 3: WM_SETTEXT sent");
    }
    std::thread::sleep(std::time::Duration::from_millis(300));

    let tp = el.get_pattern::<UITextPattern>().map_err(|e| format!("TextPattern: {e}"))?;
    let doc_text = tp
        .get_document_range()
        .map_err(|e| format!("document range: {e}"))?
        .get_text(-1)
        .map_err(|e| format!("get_text: {e}"))?;
    println!("document text via UIA: {doc_text:?}");
    assert_eq!(
        doc_text.encode_utf16().collect::<Vec<_>>(),
        text.encode_utf16().collect::<Vec<_>>(),
        "UIA document text must match what we set"
    );

    // Walk: collapse to start, move N Character units, expand to the
    // enclosing character, read back the selected text.
    println!("\nmove_text(Character, N) → selected char:");
    let mut selected: Vec<String> = Vec::new();
    for n in 0..=text.chars().count() {
        let range = tp.get_document_range()?;
        range.move_text(TextUnit::Character, 0)?;
        let moved = range.move_text(TextUnit::Character, n as i32)?;
        range.expand_to_enclosing_unit(TextUnit::Character)?;
        let sel = range.get_text(-1)?;
        println!("  N={n} (moved={moved}): {:?}", sel);
        selected.push(sel);
    }

    let expected: Vec<String> = text.chars().map(|c| c.to_string()).collect();
    Ok(selected == expected)
}
}

fn main() {
    #[cfg(target_os = "windows")]
    if let Err(e) = probe::run() {
        eprintln!("probe failed: {e}");
        std::process::exit(1);
    }
}

