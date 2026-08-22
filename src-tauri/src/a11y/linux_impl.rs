//! Linux AT-SPI2 backend for accessibility-powered element detection.
//!
//! Uses the `atspi` crate (D-Bus based) to enumerate accessible elements in
//! the focused app. Requires the `at-spi2-core` daemon (default on GNOME/KDE).
//! GTK/Qt apps work out of the box; Electron apps need `ACCESSIBILITY_ENABLED=1`
//! at launch (community trick, not officially documented).

use super::UIElement;

/// Enumerate accessibility elements from the focused window.
pub async fn get_foreground_elements(max_depth: u32) -> Result<Vec<UIElement>, String> {
    // TODO: Full AT-SPI2 implementation.
    //
    // The `atspi` crate's async API roughly looks like:
    //   let conn = atspi::AccessibilityConnection::open().await?;
    //   let registry = conn.registry_root();
    //   for app in registry.get_children().await? {
    //       for frame in app.get_children().await? { ... }
    //   }
    //   For each AccessibleProxy: get_name(), get_role(), get_children()
    //   For bounding rects: ComponentProxy::get_extents(CoordType::Screen)
    //
    // Returning empty is safe — the capture pipeline falls back to YOLO+OCR.
    // Fill this in when testing on a real Linux desktop.
    let _ = max_depth;
    Ok(Vec::new())
}
