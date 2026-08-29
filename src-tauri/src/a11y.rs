//! Cross-platform accessibility-powered UI element detection.
//!
//! Uses native OS accessibility APIs to enumerate visible interactive elements
//! in the foreground window with pixel-precise bounding rectangles. This is
//! dramatically more accurate than LLM vision coordinate estimation for
//! pointing at buttons/tabs/inputs in IDEs, terminals, and Electron apps.
//!
//! Coordinates are returned in physical screen pixels with the primary
//! monitor's top-left as origin. Callers must reconcile with the captured
//! monitor's offset (see `format_elements`).

use serde::Serialize;

#[derive(Serialize, Clone, Debug, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Serialize, Clone, Debug)]
pub struct UIElement {
    pub name: String,
    pub role: String,
    pub bounding_rect: Rect,
    pub automation_id: String,
    pub depth: u32,
}

impl UIElement {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn center(&self) -> (i32, i32) {
        (
            self.bounding_rect.x + self.bounding_rect.width / 2,
            self.bounding_rect.y + self.bounding_rect.height / 2,
        )
    }
}

#[cfg(target_os = "linux")]
mod linux_impl;
#[cfg(target_os = "macos")]
mod macos_impl;
#[cfg(target_os = "windows")]
mod windows_impl;

/// Get visible interactive UI elements from the foreground window.
///
/// Returns an empty vector (no error) on unsupported platforms or if the
/// accessibility API fails — callers should treat the absence of results
/// as "fall back to YOLO+OCR" rather than a hard error.
///
/// `max_depth` limits tree walk depth to avoid enormous browser DOM trees.
/// A value of 5–8 works well for IDEs and Electron apps.
pub async fn get_foreground_elements(max_depth: u32) -> Result<Vec<UIElement>, String> {
    #[cfg(target_os = "windows")]
    {
        windows_impl::get_foreground_elements(max_depth).await
    }
    #[cfg(target_os = "macos")]
    {
        macos_impl::get_foreground_elements(max_depth).await
    }
    #[cfg(target_os = "linux")]
    {
        linux_impl::get_foreground_elements(max_depth).await
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = max_depth;
        Ok(Vec::new())
    }
}

/// Tauri command: returns detected UI elements for the foreground window.
/// Frontend can call this directly to inspect what's available.
#[tauri::command]
pub async fn detect_ui_elements() -> Result<Vec<UIElement>, String> {
    // Skip if user has disabled a11y detection in settings.
    let enabled = crate::config::with_config_pub(|c| c.a11y_detection_enabled);
    if !enabled {
        return Ok(Vec::new());
    }
    get_foreground_elements(6).await
}

/// One focused-field observation, platform-neutral. Mirrors the outcome
/// shapes `text_monitor::ReadOutcome` needs without leaking OS types.
/// Each backend owns the INV-EXCL-001/INV-PRIV-001 ordering that produces
/// these variants.
#[derive(Debug)]
#[allow(dead_code)] // platform readers consume it on macOS/Linux only
pub(crate) enum FieldRead {
    /// Foreground process resolved and excluded — nothing was read.
    Excluded(String),
    /// Password detected BEFORE any value read. Role/state query errors
    /// count as password: fail closed.
    Password {
        process: String,
        rect: Option<(i32, i32, i32, i32)>,
    },
    Text {
        process: String,
        text: String,
        rect: Option<(i32, i32, i32, i32)>,
    },
    /// No focused editable element / no readable value / permission
    /// missing — callers back off, no error storm.
    NoField,
    Transient(String),
}

/// Tauri command: whether the OS-level accessibility permission is granted.
/// macOS requires the Accessibility permission for the AX tree walk; every
/// other platform returns true (no permission exists).
#[tauri::command]
pub async fn check_a11y_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        tokio::task::spawn_blocking(macos_impl::is_process_trusted)
            .await
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Tauri command: open the OS accessibility-permission pane. macOS only —
/// other platforms resolve to a clear error rather than a silent no-op.
#[tauri::command]
pub async fn open_a11y_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        tokio::task::spawn_blocking(|| {
            std::process::Command::new("open")
                .arg(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
                )
                .status()
                .map(|_| ())
                .map_err(|e| format!("open System Settings failed: {e}"))
        })
        .await
        .map_err(|e| format!("join: {e}"))?
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("accessibility permission prompt is a macOS-only flow".into())
    }
}

/// Format elements for inclusion in the LLM prompt.
///
/// `monitor_offset` is the (x, y) of the captured monitor's top-left in
/// screen space. a11y reports absolute screen coords; the LLM expects
/// coordinates relative to the screenshot's top-left.
#[cfg_attr(not(test), allow(dead_code))]
pub fn format_elements(
    elements: &[UIElement],
    monitor_offset: (i32, i32),
    monitor_size: (i32, i32),
) -> String {
    if elements.is_empty() {
        return String::new();
    }
    let mut out =
        String::from("--- DETECTED UI ELEMENTS (pixel-precise, from accessibility API) ---\n");
    let mut count = 0usize;
    for el in elements {
        // Skip unlabelled elements — the LLM can't match them to a user request.
        if el.name.trim().is_empty() {
            continue;
        }
        // Skip elements with zero/negative area
        if el.bounding_rect.width <= 0 || el.bounding_rect.height <= 0 {
            continue;
        }
        // Reconcile to captured monitor's coordinate space.
        let rx = el.bounding_rect.x - monitor_offset.0;
        let ry = el.bounding_rect.y - monitor_offset.1;
        let (cx, cy) = el.center();
        let cx_rel = cx - monitor_offset.0;
        let cy_rel = cy - monitor_offset.1;

        // Skip elements that fall outside the captured monitor.
        if cx_rel < 0 || cy_rel < 0 || cx_rel > monitor_size.0 || cy_rel > monitor_size.1 {
            continue;
        }

        // Clean the name so it survives our format — replace quotes + newlines.
        // Truncate to 80 CHARS (not bytes!) to avoid panicking on mid-UTF-8
        // byte boundaries when element names contain non-ASCII characters
        // (German umlauts, CJK, emoji, etc.).
        let cleaned: String = el
            .name
            .chars()
            .map(|c| match c {
                '"' => '\'',
                '\n' | '\r' | '\t' => ' ',
                c => c,
            })
            .collect();
        let name = if cleaned.chars().count() > 80 {
            let mut truncated: String = cleaned.chars().take(80).collect();
            truncated.push_str("...");
            truncated
        } else {
            cleaned
        };

        out.push_str(&format!(
            "[{}] \"{}\" center=({},{}) rect=({},{},{},{})\n",
            el.role,
            name.trim(),
            cx_rel,
            cy_rel,
            rx,
            ry,
            el.bounding_rect.width,
            el.bounding_rect.height,
        ));
        count += 1;
        // Cap at 200 to keep prompt size under control.
        if count >= 200 {
            out.push_str("... (truncated — 200 element limit reached)\n");
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(name: &str, role: &str, x: i32, y: i32, w: i32, h: i32) -> UIElement {
        UIElement {
            name: name.to_string(),
            role: role.to_string(),
            bounding_rect: Rect {
                x,
                y,
                width: w,
                height: h,
            },
            automation_id: String::new(),
            depth: 0,
        }
    }

    #[test]
    fn center_computes_midpoint() {
        let el = mk("Save", "Button", 100, 200, 80, 40);
        assert_eq!(el.center(), (140, 220));
    }

    #[test]
    fn format_skips_unlabeled_elements() {
        let els = vec![
            mk("", "Pane", 0, 0, 100, 100),
            mk("Save", "Button", 100, 200, 80, 40),
        ];
        let out = format_elements(&els, (0, 0), (1920, 1080));
        assert!(out.contains("Save"));
        assert!(!out.contains("Pane"));
    }

    #[test]
    fn format_reconciles_monitor_offset() {
        let els = vec![mk("Save", "Button", 2000, 200, 80, 40)];
        // Primary monitor is 1920 wide; capture is secondary monitor at x=1920
        let out = format_elements(&els, (1920, 0), (1920, 1080));
        assert!(out.contains("center=(120,220)")); // 2000-1920 + 40 = 120
        assert!(out.contains("rect=(80,200,80,40)"));
    }

    #[test]
    fn format_skips_out_of_bounds() {
        let els = vec![mk("OffScreen", "Button", -500, -500, 80, 40)];
        let out = format_elements(&els, (0, 0), (1920, 1080));
        assert!(!out.contains("OffScreen"));
    }

    #[test]
    fn format_truncates_long_names() {
        let long = "A".repeat(120);
        let els = vec![mk(&long, "Button", 100, 100, 80, 40)];
        let out = format_elements(&els, (0, 0), (1920, 1080));
        assert!(out.contains("AAA...")); // truncated indicator
        assert!(!out.contains(&"A".repeat(120)));
    }

    #[test]
    fn format_does_not_panic_on_utf8_boundary_in_long_names() {
        // Regression: String::truncate(80) panics if byte 80 lands mid-UTF-8.
        // 1 ASCII + 90 é chars (each 2 bytes) → 91 chars, 181 bytes.
        // byte 80 falls mid-é. Old code: panic. Fixed code: truncate at 80 chars.
        let s = "S".to_string() + &"\u{00e9}".repeat(90);
        let els = vec![mk(&s, "Button", 100, 100, 80, 40)];
        // Previously this panicked; now it truncates at 80 chars cleanly.
        let out = format_elements(&els, (0, 0), (1920, 1080));
        assert!(
            out.contains("..."),
            "output should contain truncation marker"
        );
        // Should keep exactly 80 chars of the original: 1 S + 79 é chars
        let name_chars: usize = out.chars().filter(|&c| c == '\u{00e9}').count();
        assert!(name_chars <= 90, "should not exceed original char count");
        assert!(
            (78..=80).contains(&name_chars),
            "should truncate near 80 chars, got {}",
            name_chars
        );
    }

    #[test]
    fn format_handles_emoji_without_panicking() {
        // Emojis are 4 bytes; 100 of them = 400 bytes. Previous code would
        // panic when trying to truncate(80) at a mid-emoji byte.
        let name = "\u{1F600}".repeat(100); // 100 emojis
        let els = vec![mk(&name, "Button", 100, 100, 80, 40)];
        let out = format_elements(&els, (0, 0), (1920, 1080));
        assert!(out.contains("..."));
    }

    #[test]
    fn format_caps_at_200_elements() {
        let els: Vec<UIElement> = (0..300)
            .map(|i| mk(&format!("el{i}"), "Button", 100, 100, 10, 10))
            .collect();
        let out = format_elements(&els, (0, 0), (1920, 1080));
        assert!(out.contains("truncated"));
        // Count "[Button]" occurrences — should be 200
        assert_eq!(out.matches("[Button]").count(), 200);
    }
}
