//! Background screen recorder for the work journal (ADR-042).
//!
//! Captures the selected monitor every N seconds (default 10) as a ~1080p
//! JPEG q85 into `<app_data_dir>/recordings/`, with idle seconds + the
//! foreground window title stored per shot in journal.sqlite. Opt-in
//! (default OFF), local-only, user-purgeable; retention deletes raw frames
//! after a configurable number of days.

use image::codecs::jpeg::JpegEncoder;
use serde::Serialize;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Mutex;
use xcap::Monitor;

use super::db;

/// Frames taller than this are downscaled before JPEG encoding (Dayflow
/// parity: ~1080p keeps text legible for vision models at ~5-10x less disk
/// than native 4K).
const TARGET_MAX_HEIGHT: u32 = 1080;
/// Skip capturing entirely once the user has been idle this long — nothing
/// new is on screen and an idle span would only burn disk overnight. The
/// analyzer additionally skips fully-idle batches (Phase 2).
const IDLE_SKIP_SECS: i64 = 180;
/// Retention purge cadence while the recorder runs.
const PURGE_INTERVAL_SECS: u64 = 3600;

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Start/stop generation — a stale loop notices it was superseded and exits.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static SHOTS_TAKEN: AtomicU64 = AtomicU64::new(0);
static SHOTS_SKIPPED_IDLE: AtomicU64 = AtomicU64::new(0);
static LAST_CAPTURE_AT: AtomicI64 = AtomicI64::new(0);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn set_last_error(err: Option<String>) {
    let mut lock = LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner());
    *lock = err;
}

#[derive(Serialize, Clone, Debug)]
pub struct RecorderStatus {
    pub running: bool,
    pub interval_secs: u32,
    pub retention_days: u32,
    pub shots_taken: u64,
    pub shots_skipped_idle: u64,
    pub last_capture_at: i64,
    pub last_error: Option<String>,
    pub recordings_dir: String,
}

fn status_snapshot(app: &tauri::AppHandle) -> RecorderStatus {
    let (interval, retention) = crate::config::with_config_pub(|c| {
        (c.recorder_interval_secs, c.recorder_retention_days)
    });
    RecorderStatus {
        running: RUNNING.load(Ordering::SeqCst),
        interval_secs: interval,
        retention_days: retention,
        shots_taken: SHOTS_TAKEN.load(Ordering::SeqCst),
        shots_skipped_idle: SHOTS_SKIPPED_IDLE.load(Ordering::SeqCst),
        last_capture_at: LAST_CAPTURE_AT.load(Ordering::SeqCst),
        last_error: LAST_ERROR
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone(),
        recordings_dir: db::recordings_dir(app)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Idle detection
// ---------------------------------------------------------------------------

/// Seconds since the last user input. Best-effort: 0 on any failure so a
/// broken idle probe degrades to "always capture", never to data loss.
pub async fn idle_seconds() -> i64 {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::SystemInformation::GetTickCount;
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        unsafe {
            let mut info = LASTINPUTINFO {
                cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
                dwTime: 0,
            };
            if GetLastInputInfo(&mut info).as_bool() {
                // Both are u32 tick counts that wrap every ~49.7 days;
                // wrapping_sub gives the correct delta across the wrap.
                let idle_ms = GetTickCount().wrapping_sub(info.dwTime);
                return (idle_ms / 1000) as i64;
            }
            0
        }
    }

    #[cfg(target_os = "macos")]
    {
        // HIDIdleTime is reported in nanoseconds.
        let out = tokio::process::Command::new("sh")
            .args(["-c", "ioreg -c IOHIDSystem | awk '/HIDIdleTime/ {print $NF; exit}'"])
            .output()
            .await;
        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<i64>()
                .map(|ns| ns / 1_000_000_000)
                .unwrap_or(0),
            _ => 0,
        }
    }

    #[cfg(target_os = "linux")]
    {
        // xprintidle prints idle milliseconds (X11 only; 0 on Wayland/missing).
        let out = tokio::process::Command::new("xprintidle").output().await;
        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<i64>()
                .map(|ms| ms / 1000)
                .unwrap_or(0),
            _ => 0,
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        0
    }
}

/// Likely-locked heuristic (ADR-042): on Windows the lock screen leaves the
/// foreground window NULL/titleless while input is idle. Cheaper than
/// WTSRegisterSessionNotification (which needs a window message pump we
/// don't have on this task) and fails toward capturing, not toward gaps.
fn likely_locked(window_title: &str, idle_secs: i64) -> bool {
    window_title.is_empty() && idle_secs >= 60
}

// ---------------------------------------------------------------------------
// Filenames
// ---------------------------------------------------------------------------

/// `YYYYMMDD_HHmmssSSS.jpg` for a local timestamp.
pub fn filename_for(dt: &chrono::DateTime<chrono::Local>) -> String {
    dt.format("%Y%m%d_%H%M%S%3f.jpg").to_string()
}

// ---------------------------------------------------------------------------
// Frame capture
// ---------------------------------------------------------------------------

/// Side-by-side composite of every monitor, ordered left-to-right by
/// desktop x position, tops aligned, black fill where heights differ.
/// Used when the journal monitor setting is "all" — the journal wants
/// the whole workday, unlike assistant screenshots which need a single
/// monitor for pointing coordinates.
fn composite_all_monitors(monitors: &[Monitor]) -> Result<image::RgbImage, String> {
    let mut shots: Vec<(i32, image::RgbImage)> = Vec::with_capacity(monitors.len());
    for m in monitors {
        let img = m
            .capture_image()
            .map_err(|e| format!("Failed to capture monitor: {e}"))?;
        shots.push((
            m.x().unwrap_or(0),
            image::DynamicImage::ImageRgba8(img).to_rgb8(),
        ));
    }
    shots.sort_by_key(|(x, _)| *x);
    let total_w: u32 = shots.iter().map(|(_, i)| i.width()).sum();
    let max_h: u32 = shots.iter().map(|(_, i)| i.height()).max().unwrap_or(1);
    let mut canvas = image::RgbImage::new(total_w.max(1), max_h.max(1));
    let mut x_off: i64 = 0;
    for (_, img) in &shots {
        image::imageops::replace(&mut canvas, img, x_off, 0);
        x_off += img.width() as i64;
    }
    Ok(canvas)
}

/// Capture the configured monitor(s), downscale to ≤1080p, return JPEG
/// bytes. Runs on a blocking thread (xcap is synchronous).
/// `monitor_setting`: "all" = composite every monitor, numeric index =
/// that monitor, anything else = primary.
fn capture_jpeg_blocking(monitor_setting: &str) -> Result<Vec<u8>, String> {
    let monitors = Monitor::all().map_err(|e| format!("Failed to list monitors: {e}"))?;

    let mut rgb = if monitor_setting == "all" {
        if monitors.is_empty() {
            return Err("No monitors found".to_string());
        }
        composite_all_monitors(&monitors)?
    } else {
        let monitor_index: Option<usize> = if monitor_setting != "auto" {
            monitor_setting.parse().ok()
        } else {
            None
        };
        let monitor = if let Some(idx) = monitor_index {
            monitors
                .get(idx)
                .or_else(|| monitors.first())
                .ok_or_else(|| "No monitors found".to_string())?
        } else {
            let mut iter = monitors.iter();
            let first = iter.next().ok_or_else(|| "No monitors found".to_string())?;
            iter.find(|m| m.is_primary().unwrap_or(false)).unwrap_or(first)
        };

        let img = monitor
            .capture_image()
            .map_err(|e| format!("Failed to capture screen: {e}"))?;
        image::DynamicImage::ImageRgba8(img).to_rgb8()
    };

    let (w, h) = (rgb.width(), rgb.height());
    if h > TARGET_MAX_HEIGHT {
        let new_w = ((w as u64 * TARGET_MAX_HEIGHT as u64) / h as u64).max(1) as u32;
        rgb = image::imageops::resize(
            &rgb,
            new_w,
            TARGET_MAX_HEIGHT,
            image::imageops::FilterType::Triangle,
        );
    }

    let mut jpeg: Vec<u8> = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(Cursor::new(&mut jpeg), 85);
    encoder
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("Failed to encode JPEG: {e}"))?;
    Ok(jpeg)
}

/// One recorder tick: probe idle + title, maybe skip, else capture + persist.
async fn take_shot(app: &tauri::AppHandle) -> Result<(), String> {
    let idle = idle_seconds().await;
    let title = crate::context::active_window_title().await;

    if idle >= IDLE_SKIP_SECS || likely_locked(&title, idle) {
        SHOTS_SKIPPED_IDLE.fetch_add(1, Ordering::SeqCst);
        return Ok(());
    }

    // Journal-specific monitor setting wins; empty = follow the assistant
    // screenshot monitor (legacy behavior before the setting existed).
    let monitor_setting = crate::config::with_config_pub(|c| {
        if !c.journal_capture_monitor.is_empty() {
            c.journal_capture_monitor.clone()
        } else if !c.capture_monitor.is_empty() {
            c.capture_monitor.clone()
        } else {
            "auto".to_string()
        }
    });

    let jpeg = tokio::task::spawn_blocking(move || capture_jpeg_blocking(&monitor_setting))
        .await
        .map_err(|e| format!("Capture task failed: {e}"))??;

    let now_local = chrono::Local::now();
    let captured_at = now_local.timestamp();
    let dir = db::recordings_dir(app)?;
    let path = dir.join(filename_for(&now_local));
    std::fs::write(&path, &jpeg).map_err(|e| format!("Failed to write frame: {e}"))?;

    let conn = db::open(app)?;
    db::insert_screenshot(
        &conn,
        captured_at,
        &path.to_string_lossy(),
        jpeg.len() as i64,
        idle,
        &title,
    )?;

    SHOTS_TAKEN.fetch_add(1, Ordering::SeqCst);
    LAST_CAPTURE_AT.store(captured_at, Ordering::SeqCst);
    Ok(())
}

/// Delete frames (files + rows) older than the retention window.
fn purge_expired(app: &tauri::AppHandle) -> Result<usize, String> {
    let retention_days = crate::config::with_config_pub(|c| c.recorder_retention_days);
    let cutoff = db::now_secs() - (retention_days as i64) * 86_400;
    let conn = db::open(app)?;
    let candidates = db::select_purge_candidates(&conn, cutoff)?;
    if candidates.is_empty() {
        return Ok(0);
    }
    let mut ids = Vec::with_capacity(candidates.len());
    for (id, file_path) in &candidates {
        // A missing file is fine — the row is stale either way.
        let _ = std::fs::remove_file(file_path);
        ids.push(*id);
    }
    let n = db::delete_screenshot_rows(&conn, &ids)?;
    log::info!("[recorder] retention purged {n} frames older than {retention_days}d");
    Ok(n)
}

// ---------------------------------------------------------------------------
// Loop + commands
// ---------------------------------------------------------------------------

/// Start the capture loop. Idempotent — a second start while running is a
/// no-op returning current status.
pub fn start(app: tauri::AppHandle) -> RecorderStatus {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return status_snapshot(&app);
    }
    let my_gen = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    SHOTS_TAKEN.store(0, Ordering::SeqCst);
    SHOTS_SKIPPED_IDLE.store(0, Ordering::SeqCst);
    set_last_error(None);
    log::info!("[recorder] starting (gen {my_gen})");

    let app_for_loop = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last_purge = std::time::Instant::now();
        // Purge once at startup so a machine that sleeps through the hourly
        // cadence still expires old frames.
        if let Err(e) = purge_expired(&app_for_loop) {
            log::warn!("[recorder] startup purge failed: {e}");
        }
        loop {
            if !RUNNING.load(Ordering::SeqCst)
                || GENERATION.load(Ordering::SeqCst) != my_gen
            {
                log::info!("[recorder] loop gen {my_gen} exiting");
                break;
            }
            match take_shot(&app_for_loop).await {
                Ok(()) => set_last_error(None),
                Err(e) => {
                    log::warn!("[recorder] shot failed: {e}");
                    set_last_error(Some(e));
                }
            }
            if last_purge.elapsed().as_secs() >= PURGE_INTERVAL_SECS {
                if let Err(e) = purge_expired(&app_for_loop) {
                    log::warn!("[recorder] purge failed: {e}");
                }
                last_purge = std::time::Instant::now();
            }
            let interval =
                crate::config::with_config_pub(|c| c.recorder_interval_secs).clamp(2, 600);
            tokio::time::sleep(std::time::Duration::from_secs(interval as u64)).await;
        }
    });
    status_snapshot(&app)
}

pub fn stop(app: &tauri::AppHandle) -> RecorderStatus {
    if RUNNING.swap(false, Ordering::SeqCst) {
        GENERATION.fetch_add(1, Ordering::SeqCst);
        log::info!("[recorder] stopped");
    }
    status_snapshot(app)
}

#[tauri::command]
pub async fn recorder_start(app: tauri::AppHandle) -> Result<RecorderStatus, String> {
    Ok(start(app))
}

#[tauri::command]
pub async fn recorder_stop(app: tauri::AppHandle) -> Result<RecorderStatus, String> {
    Ok(stop(&app))
}

#[tauri::command]
pub async fn recorder_status(app: tauri::AppHandle) -> Result<RecorderStatus, String> {
    Ok(status_snapshot(&app))
}

/// List captured frames for a local `YYYY-MM-DD` day.
#[tauri::command]
pub async fn journal_list_screenshots(
    app: tauri::AppHandle,
    day: String,
) -> Result<Vec<db::ScreenshotRow>, String> {
    let (start_ts, end_ts) = db::day_bounds_local(&day)?;
    tokio::task::spawn_blocking(move || {
        let conn = db::open(&app)?;
        db::list_screenshots_between(&conn, start_ts, end_ts)
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

/// Read one captured frame as base64 JPEG. The path is re-anchored under
/// the recordings dir so a tampered DB row can't read arbitrary files.
#[tauri::command]
pub async fn journal_read_screenshot(
    app: tauri::AppHandle,
    id: i64,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let conn = db::open(&app)?;
        let row = db::get_screenshot(&conn, id)?;
        let recordings = db::recordings_dir(&app)?;
        let path = std::path::Path::new(&row.file_path);
        let canonical = path
            .canonicalize()
            .map_err(|e| format!("Frame file missing: {e}"))?;
        let recordings_canonical = recordings
            .canonicalize()
            .map_err(|e| format!("Recordings dir missing: {e}"))?;
        if !canonical.starts_with(&recordings_canonical) {
            return Err("Frame path escapes recordings directory".to_string());
        }
        let bytes =
            std::fs::read(&canonical).map_err(|e| format!("Failed to read frame: {e}"))?;
        use base64::{engine::general_purpose::STANDARD, Engine};
        Ok(STANDARD.encode(bytes))
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_roundtrips_timestamp_fields() {
        use chrono::{Local, TimeZone};
        let dt = Local.with_ymd_and_hms(2026, 7, 3, 14, 5, 9).unwrap();
        let name = filename_for(&dt);
        assert_eq!(name, "20260703_140509000.jpg");
        // Lexicographic order == chronological order (retention + listing
        // rely on this being sortable).
        let later = Local.with_ymd_and_hms(2026, 7, 3, 14, 5, 10).unwrap();
        assert!(filename_for(&later) > name);
    }

    #[test]
    fn likely_locked_requires_both_signals() {
        assert!(likely_locked("", 60));
        assert!(likely_locked("", 3600));
        assert!(!likely_locked("", 5)); // just switching windows
        assert!(!likely_locked("Visual Studio Code", 120)); // reading docs
    }
}
