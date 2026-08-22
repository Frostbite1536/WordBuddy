use base64::{engine::general_purpose::STANDARD, Engine};
use image::codecs::jpeg::JpegEncoder;
use serde::Serialize;
use std::io::Cursor;
use xcap::Monitor;

#[derive(Serialize, Clone)]
pub struct CaptureResult {
    pub base64: String,
    pub width: u32,
    pub height: u32,
    /// Text-formatted list of detected UI elements (pixel-precise coordinates).
    /// None if UI detection is disabled or model not downloaded.
    pub detected_elements: Option<String>,
}

/// Capture a monitor's screen. Reads capture_monitor setting from config.
/// Detection stack: browser extension (instant, DOM) → a11y tree (OS API).
/// `skip_ocr` is accepted for IPC compatibility but no longer used — the
/// YOLO+OCR fallback was removed with the OmniParser model (licensing).
#[tauri::command]
pub async fn capture_to_base64(app: tauri::AppHandle, skip_ocr: Option<bool>) -> Result<CaptureResult, String> {
    let _ = skip_ocr;
    eprintln!("[capture] capture_to_base64 called");

    // Check browser extension for fresh element data (instant, pixel-precise)
    let mask_inputs = crate::config::with_config_pub(|c| c.mask_form_inputs);
    let extension_elements: Option<String> = {
        use tauri::Manager;
        use std::sync::Arc;
        if let Some(ext_state) = app.try_state::<Arc<tokio::sync::Mutex<crate::extension::ExtensionState>>>() {
            let lock = ext_state.lock().await;
            if lock.has_fresh_data() {
                eprintln!("[capture] Using browser extension data ({} elements from {})",
                    lock.elements.len(), lock.page_url);
                Some(lock.format_elements(mask_inputs))
            } else {
                None
            }
        } else {
            None
        }
    };

    // Accessibility tree: pixel-precise for IDEs, terminals, Electron apps.
    // Only queried when extension didn't provide data AND user has it enabled.
    // Cheap enough (~20-200ms on Windows UIA) that we query speculatively —
    // <5 elements is treated as no usable data (LLM estimation takes over).
    let a11y_elements: Option<Vec<crate::a11y::UIElement>> = if extension_elements.is_none()
        && crate::config::with_config_pub(|c| c.a11y_detection_enabled)
    {
        match tokio::time::timeout(
            std::time::Duration::from_millis(800),
            crate::a11y::get_foreground_elements(6),
        )
        .await
        {
            Ok(Ok(els)) if els.len() >= 5 => {
                eprintln!("[capture] a11y found {} elements", els.len());
                Some(els)
            }
            Ok(Ok(els)) => {
                eprintln!("[capture] a11y only found {} elements — skipping", els.len());
                None
            }
            Ok(Err(e)) => {
                eprintln!("[capture] a11y query failed: {e}");
                None
            }
            Err(_) => {
                eprintln!("[capture] a11y query timed out (>800ms)");
                None
            }
        }
    } else {
        None
    };

    // Read monitor setting from config. Falls back to the legacy api_keys
    // slot so existing users' saved preference still works after upgrade.
    let capture_setting = crate::config::with_config_pub(|c| {
        if !c.capture_monitor.is_empty() {
            c.capture_monitor.clone()
        } else {
            c.api_keys
                .get("capture_monitor")
                .cloned()
                .unwrap_or_else(|| "auto".to_string())
        }
    });
    eprintln!("[capture] setting={:?}", capture_setting);

    let monitor_index: Option<usize> = if capture_setting != "auto" {
        capture_setting.parse().ok()
    } else {
        None
    };

    let capture_future = tokio::task::spawn_blocking(move || {
        let monitors = Monitor::all().map_err(|e| format!("Failed to list monitors: {e}"))?;

        for (i, m) in monitors.iter().enumerate() {
            let name = m.name().unwrap_or_else(|_| "?".into());
            let primary = m.is_primary().unwrap_or(false);
            eprintln!("[capture] monitor {}: {:?} primary={}", i, name, primary);
        }

        let monitor = if let Some(idx) = monitor_index {
            eprintln!("[capture] using explicit index {}", idx);
            monitors.get(idx)
                .or_else(|| monitors.first())
                .ok_or_else(|| "No monitors found".to_string())?
        } else {
            let mut iter = monitors.iter();
            let first = iter.next().ok_or_else(|| "No monitors found".to_string())?;
            iter.find(|m| m.is_primary().unwrap_or(false)).unwrap_or(first)
        };

        let name = monitor.name().unwrap_or_else(|_| "?".into());
        eprintln!("[capture] capturing: {:?}", name);

        // Capture monitor offset for a11y coordinate reconciliation (a11y
        // coords are absolute screen space; prompt needs capture-relative).
        let mon_offset = (monitor.x().unwrap_or(0), monitor.y().unwrap_or(0));

        let img = monitor
            .capture_image()
            .map_err(|e| format!("Failed to capture screen: {e}"))?;

        let width = img.width();
        let height = img.height();
        eprintln!("[capture] captured {}x{}", width, height);

        // Convert RGBA to RGB (JPEG doesn't support alpha)
        let rgb_img = image::DynamicImage::ImageRgba8(img).to_rgb8();

        // Detection stack: extension -> a11y. First source with usable
        // output wins; when both come up empty the LLM falls back to
        // estimating coordinates from the screenshot.
        let detected_elements = if let Some(ext_els) = extension_elements {
            // Extension data: instant, pixel-precise — skip other detection entirely
            Some(ext_els)
        } else if let Some(a11y_els) = a11y_elements {
            let formatted = crate::a11y::format_elements(
                &a11y_els,
                mon_offset,
                (width as i32, height as i32),
            );
            if !formatted.is_empty() {
                Some(formatted)
            } else {
                // a11y returned elements but they all fell outside the
                // captured monitor (multi-monitor with foreground window
                // on a different screen).
                eprintln!("[capture] a11y filtered all elements off-monitor");
                None
            }
        } else {
            None
        };

        // Encode as JPEG quality 85 — much smaller than PNG (~200KB vs ~6MB)
        // while keeping text readable for LLM vision
        let mut jpeg_bytes: Vec<u8> = Vec::new();
        let mut encoder = JpegEncoder::new_with_quality(Cursor::new(&mut jpeg_bytes), 85);
        encoder
            .encode(
                rgb_img.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| format!("Failed to encode JPEG: {e}"))?;

        let size_kb = jpeg_bytes.len() / 1024;
        eprintln!("[capture] encoded JPEG: {}KB", size_kb);

        Ok(CaptureResult {
            base64: STANDARD.encode(&jpeg_bytes),
            width,
            height,
            detected_elements,
        })
    });

    let timeout_secs = 10;
    match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), capture_future).await {
        Ok(result) => result.map_err(|e| format!("Task failed: {e}"))?,
        Err(_) => {
            eprintln!("[capture] TIMEOUT — capture took >{}s, aborting", timeout_secs);
            Err("Screenshot capture timed out".to_string())
        }
    }
}

/// List available monitors with their names and positions.
#[tauri::command]
pub async fn list_monitors() -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(|| {
        let monitors = Monitor::all().map_err(|e| format!("Failed to list monitors: {e}"))?;
        let mut result = Vec::new();
        for (i, m) in monitors.iter().enumerate() {
            let name = m.name().unwrap_or_else(|_| format!("Monitor {}", i));
            let w = m.width().unwrap_or(0);
            let h = m.height().unwrap_or(0);
            let primary = m.is_primary().unwrap_or(false);
            result.push(serde_json::json!({
                "index": i,
                "name": name,
                "width": w,
                "height": h,
                "primary": primary,
            }));
        }
        Ok(result)
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

/// Region selection capture. Currently delegates to full-screen.
#[tauri::command]
pub async fn start_region_capture(app: tauri::AppHandle) -> Result<CaptureResult, String> {
    capture_to_base64(app, None).await
}
