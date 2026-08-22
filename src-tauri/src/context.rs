use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct WindowContext {
    pub title: String,
}

/// Detect the currently active/focused window title. The frontend polls this
/// to show a context badge and to skip self-captures.
#[tauri::command]
pub async fn detect_active_window() -> Result<WindowContext, String> {
    let title = get_active_window_title().await;
    Ok(WindowContext { title })
}

/// Foreground window title, used by the context badge command. (The base
/// repo's journal recorder also consumed this; that consumer is removed.)
pub async fn active_window_title() -> String {
    get_active_window_title().await
}

async fn get_active_window_title() -> String {
    #[cfg(target_os = "linux")]
    {
        // Try xdotool first (X11), fall back to empty string on Wayland or if missing.
        // Future: add Wayland support via dbus portals.
        let output = tokio::process::Command::new("xdotool")
            .args(["getactivewindow", "getwindowname"])
            .output()
            .await;
        match output {
            Ok(o) if o.status.success() => {
                String::from_utf8(o.stdout)
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            }
            _ => String::new(),
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Get the window title (not just the app name).
        // Falls back gracefully if the frontmost app has no windows (e.g., Finder desktop).
        tokio::process::Command::new("osascript")
            .args([
                "-e",
                r#"try
    tell application "System Events" to get name of first window of (first application process whose frontmost is true)
on error
    tell application "System Events" to get name of first application process whose frontmost is true
end try"#,
            ])
            .output()
            .await
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    #[cfg(target_os = "windows")]
    {
        // Direct Win32 API call — no PowerShell spawn, no C# compilation.
        // GetForegroundWindow + GetWindowTextW completes in microseconds
        // vs. the old approach of spawning powershell.exe + Add-Type + csc.exe
        // which took 1-4 seconds per call and caused process accumulation.
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

        unsafe {
            let hwnd = GetForegroundWindow();
            // NULL foreground window: shell focus, window being destroyed,
            // or no active app. GetWindowTextW would return 0 anyway, but
            // avoid passing a null handle to it.
            if hwnd.0.is_null() {
                return String::new();
            }
            let mut buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut buf);
            if len > 0 {
                String::from_utf16_lossy(&buf[..len as usize])
            } else {
                String::new()
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        String::new()
    }
}
