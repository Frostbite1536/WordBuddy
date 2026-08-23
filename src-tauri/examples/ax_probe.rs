//! PLAN-08 §5 offset-semantics probe (macOS / AX).
//!
//! Never assume text-unit semantics on a new platform — measure them. This
//! binary drives a REAL focused editable field (TextEdit by default) and
//! answers: does `AXSelectedTextRange` count UTF-16 code units (like UIA
//! proved to on Windows RichEdit — see AUDIT F4/uia_probe) or Unicode
//! scalars?
//!
//! Procedure:
//!   1. Open TextEdit, focus an empty document.
//!   2. Run `cargo run --example ax_probe`.
//!   3. Paste/type the marker string when prompted, press Enter HERE.
//!   4. The probe selects progressively longer ranges via
//!      AXSelectedTextRange and reads back AXSelectedText, printing which
//!      unit interpretation matches.
//!
//! Record the observed result next to `select_range`'s comment block in
//! apply.rs before wiring any span selection on this platform (INV-OFFSET-
//! 001 caveat).

// The whole probe is macOS-only, but `cargo test` compiles examples on
// every platform — so the real code lives in a cfg-gated module and a
// no-op main keeps non-macOS builds valid.
#[cfg(target_os = "macos")]
mod probe {
use std::io::{BufRead, Write};

use accessibility::{AXAttribute, AXUIElement};
use accessibility::AXUIElementAttributes as _;
use core_foundation::base::{CFType, TCFType};
use core_foundation::string::CFString;

/// Emoji-bearing marker: 'a' (1 unit), U+1F600 (surrogate pair = 2 UTF-16
/// units / 1 scalar), 'b', 'e' + combining acute (2 scalars / 3 UTF-16).
const MARKER: &str = "a\u{1F600}b e\u{301}x";

fn custom_attribute(name: &str) -> AXAttribute<CFType> {
    AXAttribute::<CFType>::new(&CFString::new(name))
}

fn focused_element() -> Result<AXUIElement, String> {
    let syswide = AXUIElement::system_wide();
    let v: CFType = syswide
        .attribute(&custom_attribute(accessibility_sys::kAXFocusedUIElementAttribute))
        .map_err(|e| format!("no focused element ({e}) — focus TextEdit"))?;
    v.downcast_into::<AXUIElement>()
        .ok_or_else(|| "focused object is not an AXUIElement".to_string())
}

fn set_selected_range(el: &AXUIElement, location: usize, length: usize) -> Result<(), String> {
    let range = core_foundation::base::CFRange {
        location: location as _,
        length: length as _,
    };
    // SAFETY: range outlives the call; AXValueCreate copies it.
    let axvalue = unsafe {
        accessibility_sys::AXValueCreate(
            accessibility_sys::kAXValueTypeCFRange,
            &range as *const _ as *const std::ffi::c_void,
        )
    };
    let value = unsafe { CFType::wrap_under_create_rule(axvalue as *const _) };
    el.set_attribute(
        &custom_attribute(accessibility_sys::kAXSelectedTextRangeAttribute),
        value,
    )
    .map(|_| ())
    .map_err(|e| format!("set AXSelectedTextRange failed: {e}"))
}

fn selected_text(el: &AXUIElement) -> String {
    el.attribute(&custom_attribute(
        accessibility_sys::kAXSelectedTextAttribute,
    ))
    .ok()
    .and_then(|v: CFType| v.downcast_into::<CFString>())
    .map(|s| s.to_string())
    .unwrap_or_default()
}


pub(super) fn run() {
    if !unsafe { accessibility_sys::AXIsProcessTrusted() } {
        eprintln!("Accessibility permission not granted — grant it and retry.");
        std::process::exit(1);
    }
    println!("Marker: {MARKER:?}");
    println!(
        "UTF-16 length = {}, char (scalar) length = {}",
        MARKER.encode_utf16().count(),
        MARKER.chars().count()
    );
    print!("Type/paste the marker into the focused TextEdit document, then press Enter here... ");
    std::io::stdout().flush().ok();
    std::io::stdin().lock().lines().next();

    let el = match focused_element() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let utf16_len = MARKER.encode_utf16().count();
    let scalar_len = MARKER.chars().count();
    println!("\n-- selecting (0, n) for n = 1..=max, reporting matched prefix --");
    let mut utf16_matches = 0usize;
    let mut scalar_matches = 0usize;
    for n in 1..=utf16_len.max(scalar_len) {
        if set_selected_range(&el, 0, n).is_err() {
            continue;
        }
        let got = selected_text(&el);
        let got_u16 = got.encode_utf16().count();
        let got_chars = got.chars().count();
        let verdict = if got == MARKER.get(..n).unwrap_or("") {
            "?"
        } else if got_u16 == n {
            utf16_matches += 1;
            "UTF-16"
        } else if got_chars == n {
            scalar_matches += 1;
            "SCALAR"
        } else {
            "-"
        };
        println!("n={n:2} -> {got:?} (u16={got_u16}, chars={got_chars}) [{verdict}]");
    }
    println!(
        "\nVerdict: UTF-16 hits = {utf16_matches}, scalar hits = {scalar_matches}. \
         The dominant interpretation governs span math for macOS."
    );
}
}

// Unconditional crate-level entry: dispatches to the macOS-only probe.
fn main() {
    #[cfg(target_os = "macos")]
    probe::run();
}


