//! macOS AXUIElement backend for accessibility-powered element detection.
//!
//! Uses the `accessibility` crate to walk the AX tree of the frontmost app.
//! Requires the Accessibility permission to be granted in
//! System Settings > Privacy & Security. Returns an empty vector (no error)
//! if the permission isn't granted — callers fall back to YOLO+OCR.
//!
//! Note: Chromium-based apps (VS Code, Claude Desktop) lazy-activate their
//! AX tree. The first query can take 100–500ms while the tree builds.

use super::{Rect, UIElement};

/// Enumerate accessibility elements in the frontmost app's focused window.
pub async fn get_foreground_elements(max_depth: u32) -> Result<Vec<UIElement>, String> {
    tokio::task::spawn_blocking(move || collect_elements(max_depth))
        .await
        .map_err(|e| format!("a11y task join failed: {e}"))?
}

fn collect_elements(max_depth: u32) -> Result<Vec<UIElement>, String> {
    // Gracefully no-op when the user hasn't granted Accessibility permission.
    if !is_process_trusted() {
        eprintln!(
            "[a11y] macOS Accessibility permission not granted — falling back to YOLO+OCR"
        );
        return Ok(Vec::new());
    }

    // TODO: Full AXUIElement tree walk.
    // Implementation plan:
    //   1. Add `accessibility = "0.2"` to [target.'cfg(target_os = "macos")'.dependencies]
    //      in Cargo.toml (removed until needed to satisfy the unused-dep rule)
    //   2. Use `AXUIElement::application(pid)` → `.focused_window()?.children()`
    //   3. Attributes via AXAttribute: title(), role(), position(), size()
    //
    // Returning empty for now is safe — the capture pipeline falls back to
    // YOLO+OCR. Fill this in when testing on a real macOS system.
    let _ = max_depth;
    Ok(Vec::new())
}

/// Whether the current process has been granted Accessibility permission.
///
/// Not currently exposed as a Tauri command — there is no frontend UI for
/// macOS permission prompts yet. When the full macOS impl lands, register
/// a `check_a11y_permission` command in lib.rs and wire it into Settings.tsx.
#[allow(dead_code)]
pub fn is_process_trusted() -> bool {
    // SAFETY: AXIsProcessTrusted is a read-only query with no side effects
    // and is callable from any thread.
    unsafe { accessibility_sys::AXIsProcessTrusted() }
}
