use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PointTarget {
    pub x: f64,
    pub y: f64,
    pub label: String,
    pub screen: u32,
}


/// Parse [POINT:x,y:label:screenN] tags from response text.
/// Used by the TypeScript pointParser.ts port; kept here for tests.
#[allow(dead_code)]
pub fn parse_point_tags(text: &str) -> (String, Vec<PointTarget>) {
    let mut clean_text = text.to_string();
    let mut points = Vec::new();

    // Find all [POINT:x,y:label:screenN] patterns
    while let Some(start) = clean_text.find("[POINT:") {
        if let Some(end) = clean_text[start..].find(']') {
            let tag = &clean_text[start + 7..start + end];
            let parts: Vec<&str> = tag.split(':').collect();

            if parts.len() >= 2 {
                let coords: Vec<&str> = parts[0].split(',').collect();
                if coords.len() == 2 {
                    if let (Ok(x), Ok(y)) = (coords[0].parse::<f64>(), coords[1].parse::<f64>()) {
                        // Reject NaN/Infinity and negative pixel coords —
                        // they're never valid screen positions and keep this
                        // parser in lockstep with the TS port, which uses a
                        // stricter regex that rejects them by construction.
                        if x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0 {
                            // Last part may be screen number; everything between is the label
                            let (label, screen) = if parts.len() >= 3 {
                                let last = parts[parts.len() - 1];
                                let screen_num = last.replace("screen", "").trim().parse::<u32>();
                                if let Ok(s) = screen_num {
                                    (parts[1..parts.len() - 1].join(":"), s)
                                } else {
                                    (parts[1..].join(":"), 0)
                                }
                            } else {
                                (parts[1].to_string(), 0)
                            };
                            points.push(PointTarget { x, y, label, screen });
                        }
                    }
                }
            }

            clean_text = format!("{}{}", &clean_text[..start], &clean_text[start + end + 1..]);
        } else {
            break;
        }
    }

    (clean_text, points)
}

/// Point at a screen element. Emits a `pointer_show` event to the
/// cursor_overlay window if the cursor overlay is enabled in settings.
/// When disabled, logs the point but doesn't show a visual overlay.
#[tauri::command]
pub async fn show_pointer(app: AppHandle, target: PointTarget) -> Result<(), String> {
    let enabled = crate::config::with_config_pub(|c| c.cursor_overlay_enabled);
    if enabled {
        eprintln!("[pointer] show_pointer: ({}, {}) label={:?}", target.x, target.y, target.label);
        let _ = app.emit("pointer_show", &target);
        // Re-apply click-through on the overlay window — Windows can lose
        // WS_EX_TRANSPARENT after hide/show cycles.
        if let Some(overlay) = app.get_webview_window("cursor_overlay") {
            let _ = overlay.show();
            let _ = overlay.set_ignore_cursor_events(true);
        }
    } else {
        eprintln!("[pointer] show_pointer: ({}, {}) label={:?} (overlay disabled)", target.x, target.y, target.label);
    }
    Ok(())
}

/// Hide the pointer overlay. Emits a `pointer_hide` event when overlay is enabled.
#[tauri::command]
pub async fn hide_pointer(app: AppHandle) -> Result<(), String> {
    let enabled = crate::config::with_config_pub(|c| c.cursor_overlay_enabled);
    if enabled {
        let _ = app.emit("pointer_hide", ());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_point_tags() {
        let text = "Click the button [POINT:450,320:Place Order:0] to submit";
        let (clean, points) = parse_point_tags(text);
        assert_eq!(clean, "Click the button  to submit");
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].x, 450.0);
        assert_eq!(points[0].y, 320.0);
        assert_eq!(points[0].label, "Place Order");
    }

    #[test]
    fn test_no_point_tags() {
        let text = "No pointing here";
        let (clean, points) = parse_point_tags(text);
        assert_eq!(clean, "No pointing here");
        assert!(points.is_empty());
    }
}
