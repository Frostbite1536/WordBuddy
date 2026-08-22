use tauri::{Manager, WebviewWindow};

/// Toggle the main window from a tray interaction. Mirrors the
/// `toggle_visibility` command but takes an AppHandle because tray
/// callbacks don't carry a WebviewWindow.
fn tray_toggle_main(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let visible = win.is_visible().unwrap_or(false);
        if visible {
            let _ = win.hide();
        } else {
            let _ = win.unminimize();
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}

/// System tray icon: the visible affordance for a hidden bar. The main
/// window is skipTaskbar + undecorated, so without this, hiding it leaves
/// no mouse path back (only the Ctrl+Shift+S global shortcut).
/// Left-click toggles the bar; right-click menu has Show/Hide + Quit.
pub fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let toggle_item = MenuItem::with_id(app, "toggle", "Show / Hide", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit WorkBuddy", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle_item, &quit_item])?;

    let icon = app
        .default_window_icon()
        .ok_or("no default window icon for tray")?
        .clone();

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("WorkBuddy — click to show/hide")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => tray_toggle_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                tray_toggle_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Position the main window at top-center of the primary monitor.
/// Also set up the cursor overlay window to cover the full primary monitor.
pub fn setup_main_window(window: &WebviewWindow) -> Result<(), Box<dyn std::error::Error>> {
    // Force the WebView2 default background to fully transparent. Without
    // this, `transparent: true` in tauri.conf.json gives a transparent
    // window frame but the WebView2 control inside still paints its own
    // (white-ish) default. The `#root { border-radius: 12px }` clip in
    // index.css exposes that default at the rounded corners as little
    // light-colored bars — particularly visible at the top corners over
    // a dark desktop. Tauri 2's set_background_color drives WebView2's
    // ICoreWebView2Controller2::DefaultBackgroundColor under the hood.
    // Best-effort: don't fail window setup over a cosmetic tweak (this
    // area has a history of transparency-induced freezes), but log so a
    // silent regression of the corner artifact still leaves a signal.
    if let Err(e) = window.set_background_color(Some(tauri::webview::Color(0, 0, 0, 0))) {
        eprintln!("setup_main_window: set_background_color failed: {e}");
    }

    if let Ok(Some(monitor)) = window.primary_monitor() {
        let monitor_size = monitor.size();
        let window_size = window.outer_size()?;
        let center_x =
            ((monitor_size.width as i32 - window_size.width as i32) / 2).max(0);
        // Use y=0 for cross-platform compatibility — Linux and Windows
        // don't have a macOS-style menu bar at 54px.
        window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
            x: center_x,
            y: 0,
        }))?;

        // Set up the cursor overlay window — full screen, click-through
        if let Some(overlay) = window.app_handle().get_webview_window("cursor_overlay") {
            overlay.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                x: 0,
                y: 0,
            }))?;
            overlay.set_size(tauri::Size::Physical(tauri::PhysicalSize {
                width: monitor_size.width,
                height: monitor_size.height,
            }))?;
            // Make overlay click-through so it doesn't intercept mouse events
            overlay.set_ignore_cursor_events(true)?;
        }
    }
    Ok(())
}

/// Toggle the window height between collapsed (54px) and expanded.
#[tauri::command]
pub async fn set_window_height(window: tauri::WebviewWindow, height: u32) -> Result<(), String> {
    let current_size = window.outer_size().map_err(|e| e.to_string())?;
    window
        .set_size(tauri::Size::Physical(tauri::PhysicalSize {
            width: current_size.width,
            height,
        }))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Toggle main window visibility.
#[tauri::command]
pub async fn toggle_visibility(window: tauri::WebviewWindow) -> Result<bool, String> {
    let visible = window.is_visible().map_err(|e| e.to_string())?;
    if visible {
        window.hide().map_err(|e| e.to_string())?;
    } else {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(!visible)
}

/// Idempotent-show: make the main window visible and focused regardless of
/// current state. Used by the `external-question` handler so that a question
/// posted to `/ask` always surfaces the UI, even if WorkBuddy was hidden.
#[tauri::command]
pub async fn show_main_window(window: tauri::WebviewWindow) -> Result<(), String> {
    // unminimize first — show() alone doesn't always restore a taskbar-docked
    // window on Windows / some Linux WMs. Best-effort: if the platform doesn't
    // support it, fall through to show().
    let _ = window.unminimize();
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}
