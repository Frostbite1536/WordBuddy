//! Floating suggestion-card window (PLAN-04 tasks 1, 5).
//!
//! The `widget` webview (CONTRACTS §4): ~340×240, undecorated,
//! transparent, always-on-top, skip-taskbar, hidden by default, NOT
//! click-through, created lazily on first show. The same window serves
//! two modes — suggestion card and selection-rewrite palette — selected
//! by the frontend on a `widget-mode` payload.

use serde::{Deserialize, Serialize};
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetAnchor {
    /// Field rect [left, top, right, bottom] in physical screen coords.
    pub rect: [i32; 4],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionCapture {
    pub ok: bool,
    pub text: Option<String>,
    /// text-pattern | clipboard | failed
    pub method: String,
    pub error: Option<String>,
}

const WIDGET_W: i32 = 340;
const WIDGET_H: i32 = 240;

/// Show the widget near the anchor rect (field rect; caret rect when P3
/// later provides one). Creates the window lazily on first call.
/// Repeats the WebView2 transparency pattern (FRICTION 2026-08-21).
#[tauri::command]
pub async fn widget_show_for(
    app: tauri::AppHandle,
    anchor: WidgetAnchor,
) -> Result<(), String> {
    let existing = app.get_webview_window("widget");
    let win = match existing {
        Some(w) => w,
        None => {
            let builder = tauri::WebviewWindowBuilder::new(
                &app,
                "widget",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("WordBuddy suggestions")
            .inner_size(f64::from(WIDGET_W), f64::from(WIDGET_H))
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .visible(false)
            .focused(false); // never steal focus on show
            // NOTE: no additional_browser_args — a custom flag made
            // WebView2 controller creation fail with 0x8007139F
            // (ERROR_INVALID_STATE) on this runtime.
            builder
                .build()
                .map_err(|e| format!("widget window build failed: {e}"))?
        }
    };

    // Transparency pattern (base window.rs:63-76): best-effort, log on
    // failure, never fail the show over a cosmetic tweak.
    if let Err(e) =
        win.set_background_color(Some(tauri::webview::Color(0, 0, 0, 0)))
    {
        eprintln!("[widget] set_background_color failed: {e}");
    }

    // Position: below-left of the anchor, clamped to the anchor's
    // monitor work area.
    let [l, t, r, b] = anchor.rect;
    let anchor_center_x = (l + r) / 2;
    let desired_x = anchor_center_x - WIDGET_W / 2;
    let desired_y = b + 8; // below the field
    let (x, y) = clamp_to_monitor(&win, desired_x, desired_y, l, t);

    win.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
        x,
        y,
    }))
    .map_err(|e| format!("set_position: {e}"))?;
    win.show().map_err(|e| format!("show: {e}"))?;
    win.set_always_on_top(true).map_err(|e| format!("always_on_top: {e}"))?;
    Ok(())
}

fn clamp_to_monitor(
    win: &tauri::WebviewWindow,
    desired_x: i32,
    desired_y: i32,
    anchor_x: i32,
    anchor_y: i32,
) -> (i32, i32) {
    // Prefer the monitor containing the anchor; fall back to primary.
    let monitor = win
        .monitor_from_point(f64::from(anchor_x), f64::from(anchor_y))
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten());
    let (mut x, mut y) = (desired_x, desired_y);
    if let Some(m) = monitor {
        let pos = m.position();
        let size = m.size();
        let wa_top = pos.y; // work-area approximation: full monitor minus
        let wa_bottom = pos.y + size.height as i32;
        let wa_left = pos.x;
        let wa_right = pos.x + size.width as i32;
        if x < wa_left {
            x = wa_left + 4;
        }
        if x + WIDGET_W > wa_right {
            x = wa_right - WIDGET_W - 4;
        }
        if y + WIDGET_H > wa_bottom {
            // Flip above the field instead.
            y = (anchor_y - WIDGET_H - 8).max(wa_top + 4);
        }
        if y < wa_top {
            y = wa_top + 4;
        }
    }
    (x, y)
}

#[tauri::command]
pub async fn widget_hide(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("widget") {
        win.hide().map_err(|e| format!("hide: {e}"))?;
    }
    Ok(())
}

/// Capture the current selection for the rewrite palette (PLAN-04
/// task 5). TextPattern selection first; synthetic Ctrl+C with
/// clipboard save/restore as fallback (explicit user action —
/// INV-PRIV-003 sanctioned surface).
#[tauri::command]
pub async fn selection_capture() -> Result<SelectionCapture, String> {
    tokio::task::spawn_blocking(capture_selection_impl)
        .await
        .map_err(|e| format!("join: {e}"))
}

#[cfg(target_os = "windows")]
fn capture_selection_impl() -> SelectionCapture {
    use uiautomation::patterns::UITextPattern;

    let automation = match uiautomation::UIAutomation::new() {
        Ok(a) => a,
        Err(e) => {
            return SelectionCapture {
                ok: false,
                text: None,
                method: "failed".into(),
                error: Some(format!("UIAutomation::new: {e}")),
            }
        }
    };
    let el = match automation.get_focused_element() {
        Ok(el) => el,
        Err(e) => {
            return SelectionCapture {
                ok: false,
                text: None,
                method: "failed".into(),
                error: Some(format!("get_focused_element: {e}")),
            }
        }
    };
    if el.is_password().unwrap_or(false) {
        // INV-PRIV-001: password selections are never captured.
        return SelectionCapture {
            ok: false,
            text: None,
            method: "failed".into(),
            error: Some("password field".into()),
        };
    }
    if let Ok(tp) = el.get_pattern::<UITextPattern>() {
        if let Ok(sel) = tp.get_selection() {
            if let Some(range) = sel.into_iter().next() {
                if let Ok(text) = range.get_text(-1) {
                    if !text.trim().is_empty() {
                        return SelectionCapture {
                            ok: true,
                            text: Some(text),
                            method: "text-pattern".into(),
                            error: None,
                        };
                    }
                }
            }
        }
    }

    // Fallback: synthetic Ctrl+C with clipboard save/restore
    // (snapshot → copy → read → restore).
    match capture_selection_via_clipboard() {
        Ok(text) => SelectionCapture {
            ok: true,
            text: Some(text),
            method: "clipboard".into(),
            error: None,
        },
        Err(e) => SelectionCapture {
            ok: false,
            text: None,
            method: "failed".into(),
            error: Some(e),
        },
    }
}

/// Clipboard-fallback selection capture shared by every platform with a
/// working synthetic-copy path: snapshot → Ctrl/Cmd+C → read → restore.
fn capture_selection_via_clipboard() -> Result<String, String> {
    use crate::clipboard::ClipboardBackend as _;
    let clipboard = crate::clipboard::WinClipboard;
    let guard = crate::clipboard::clipboard_lock();
    let outcome = (|| -> Result<String, String> {
        let snap = clipboard.snapshot()?;
        crate::input_inject::send_ctrl_c()?;
        std::thread::sleep(std::time::Duration::from_millis(120));
        let text = clipboard.get_text()?.unwrap_or_default();
        std::thread::sleep(std::time::Duration::from_millis(380));
        clipboard.restore(&snap)?;
        if text.trim().is_empty() {
            Err("no selection text".into())
        } else {
            Ok(text)
        }
    })();
    drop(guard);
    outcome
}

/// macOS: AXSelectedText on the focused element first (INV-PRIV-001
/// fail-closed inside the AX backend); synthetic Cmd+C clipboard
/// fallback for apps that don't expose a selection attribute.
#[cfg(target_os = "macos")]
fn capture_selection_impl() -> SelectionCapture {
    if let Ok(Some(text)) = crate::a11y::macos_impl::selected_text_of_focused_element() {
        return SelectionCapture {
            ok: true,
            text: Some(text),
            method: "ax-selected-text".into(),
            error: None,
        };
    }
    match capture_selection_via_clipboard() {
        Ok(text) => SelectionCapture {
            ok: true,
            text: Some(text),
            method: "clipboard".into(),
            error: None,
        },
        Err(e) => SelectionCapture {
            ok: false,
            text: None,
            method: "failed".into(),
            error: Some(e),
        },
    }
}

/// Linux: AT-SPI Text::GetSelection on the focused element first
/// (INV-PRIV-001 fail-closed inside the AT-SPI backend); synthetic
/// Ctrl+C clipboard fallback for apps that don't expose a selection.
#[cfg(target_os = "linux")]
fn capture_selection_impl() -> SelectionCapture {
    if let Ok(Some(text)) = crate::a11y::linux_impl::selected_text_of_focused_element() {
        return SelectionCapture {
            ok: true,
            text: Some(text),
            method: "atspi-selection".into(),
            error: None,
        };
    }
    match capture_selection_via_clipboard() {
        Ok(text) => SelectionCapture {
            ok: true,
            text: Some(text),
            method: "clipboard".into(),
            error: None,
        },
        Err(e) => SelectionCapture {
            ok: false,
            text: None,
            method: "failed".into(),
            error: Some(e),
        },
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn capture_selection_impl() -> SelectionCapture {
    SelectionCapture {
        ok: false,
        text: None,
        method: "failed".into(),
        error: Some("unsupported on this platform".into()),
    }
}
