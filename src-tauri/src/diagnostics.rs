// Diagnostics — local-only crash + activity logging (O1).
//
// PRINCIPLES.md §97 forbids error-reporting / crash-report SDKs. We
// instead write to a rotating local log file inside Tauri's
// `app_log_dir()` and expose two Settings buttons:
//
//   - `open_log_dir`        opens the directory in the OS file manager
//   - `copy_last_log_tail`  returns the last 5 MB of the most-recent
//                           log file as a String (frontend writes it
//                           to the clipboard).
//
// The plugin (`tauri-plugin-log`, registered in `lib.rs`) handles the
// rotation and per-target writes; this module is the user-facing
// surface and the panic-hook installer.

use std::fs;
use std::path::PathBuf;

use tauri::Manager;

/// Number of bytes returned by `copy_last_log_tail`. Matches the bound
/// promised in the Settings UI ("Copy last 5 MB"). The rotated log
/// files are 5 MB each, so the tail will normally be the entire most
/// recent file; if the active file is mid-rotation we return its
/// in-progress contents.
pub const TAIL_BYTES: u64 = 5 * 1024 * 1024;

/// Install the global panic hook. Once set, any Rust panic — wherever
/// it originates — is captured to the log file before the process
/// terminates. Tauri commands return `Result<_, String>` so panics in
/// command bodies are rare, but background tasks (extension server,
/// audio capture) can panic and used to vanish silently. With this
/// hook in place the panic shows up in the log directory and the user
/// can copy it via the Diagnostics buttons.
///
/// Idempotent: calling more than once replaces the previous hook.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".to_string());

        // Panic payload is one of: &'static str, String, or other.
        let payload = info
            .payload()
            .downcast_ref::<&'static str>()
            .copied()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".to_string());

        // Force a backtrace even when RUST_BACKTRACE is unset — operators
        // are unlikely to set the env var on student machines, and
        // without a trace the log line is near-useless.
        let backtrace = std::backtrace::Backtrace::force_capture();

        // log::error! is captured by tauri-plugin-log and routed to the
        // log file. eprintln! is the belt-and-suspenders fallback in
        // case the logger never initialized (e.g. the panic happened
        // pre-builder).
        log::error!("PANIC at {location}: {payload}\n{backtrace}");
        eprintln!("PANIC at {location}: {payload}\n{backtrace}");
    }));
}

/// Resolve the app log directory (where tauri-plugin-log writes).
///
/// On every supported platform this lives under the OS conventional
/// "logs" location for the bundle identifier — e.g. on Windows
/// `%LOCALAPPDATA%\<identifier>\logs`. The path is created lazily by
/// the plugin on first write; if the user opens it before any log
/// activity happens it may not exist yet — in that case we create it
/// here so the OS file manager opens cleanly instead of erroring.
fn resolve_log_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("could not resolve app_log_dir: {e}"))?;
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("could not create log dir {}: {e}", dir.display()))?;
    }
    Ok(dir)
}

/// Tail of `path` capped at `max_bytes`. Returns the entire file if
/// it's smaller. We deliberately read the trailing window rather than
/// the whole file because logs can grow up to 5 MB before rotation
/// and the IPC payload back to the frontend is unnecessarily large
/// otherwise.
///
/// `SeekFrom::End(-max_bytes)` lands at an arbitrary byte offset that
/// can split a multi-byte UTF-8 codepoint when the log contains
/// non-ASCII text — e.g. a Windows backtrace frame referencing a path
/// like `C:\Users\李华\AppData\…`. `read_to_string` would then return
/// `InvalidData` and "Copy last 5 MB" would surface a useless error
/// exactly when the user most needs the log. Read raw bytes and
/// convert lossily so any half-codepoint at the head becomes one
/// `U+FFFD` glyph instead of breaking the whole result.
///
/// Pure helper, exposed for tests.
pub fn read_tail(path: &std::path::Path, max_bytes: u64) -> Result<String, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let len = f
        .metadata()
        .map_err(|e| format!("metadata {}: {e}", path.display()))?
        .len();
    if len > max_bytes {
        f.seek(SeekFrom::End(-(max_bytes as i64)))
            .map_err(|e| format!("seek {}: {e}", path.display()))?;
    }
    let mut raw = Vec::new();
    f.read_to_end(&mut raw)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(String::from_utf8_lossy(&raw).into_owned())
}

/// Return the most-recent regular file under `dir`. Used to find the
/// "active" log when there are several rotated files.
///
/// Entries whose `metadata()` or `modified()` calls fail are SKIPPED
/// rather than treated as if they have a UNIX-epoch timestamp. The
/// epoch fallback (used in earlier versions) silently demoted bad
/// entries below every comparison, so a log file with unreadable
/// metadata could quietly mask a newer good log file from being
/// selected. Skipping lets `copy_last_log_tail` fall through to a
/// real candidate; if every candidate fails metadata access the
/// caller observes the same "no log yet" state as an empty dir.
fn newest_file(dir: &std::path::Path) -> Result<Option<PathBuf>, String> {
    let mut newest: Option<(PathBuf, std::time::SystemTime)> = None;
    let entries = fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Skip entries we can't time-stamp instead of substituting
        // UNIX_EPOCH — see the doc comment above.
        let Some(mtime) = entry.metadata().ok().and_then(|m| m.modified().ok()) else {
            continue;
        };
        match &newest {
            Some((_, prev)) if *prev >= mtime => {}
            _ => newest = Some((path, mtime)),
        }
    }
    Ok(newest.map(|(p, _)| p))
}

#[tauri::command]
pub fn open_log_dir(app: tauri::AppHandle) -> Result<String, String> {
    let dir = resolve_log_dir(&app)?;
    // tauri-plugin-shell exposes `opener.open_path` but we'd need the
    // OpenerExt trait + a permission grant. Since the dir is a known
    // local user path, the platform-specific call is simpler and avoids
    // adding shell-scope:allow-open-path grants for an arbitrary
    // OS-conventional path.
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("spawn explorer: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("spawn open: {e}"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("spawn xdg-open: {e}"))?;
    }
    Ok(dir.display().to_string())
}

#[tauri::command]
pub fn copy_last_log_tail(app: tauri::AppHandle) -> Result<String, String> {
    let dir = resolve_log_dir(&app)?;
    let Some(latest) = newest_file(&dir)? else {
        return Ok(String::new());
    };
    read_tail(&latest, TAIL_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_log() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wordbuddy.log");
        (dir, path)
    }

    #[test]
    fn read_tail_returns_full_file_when_smaller_than_cap() {
        let (_dir, path) = tmp_log();
        std::fs::write(&path, b"short content").unwrap();
        let out = read_tail(&path, 1024).unwrap();
        assert_eq!(out, "short content");
    }

    #[test]
    fn read_tail_caps_at_requested_bytes_when_file_is_larger() {
        let (_dir, path) = tmp_log();
        let payload: Vec<u8> = (0..2048).map(|i| i as u8 % 26 + b'a').collect();
        std::fs::write(&path, &payload).unwrap();
        let out = read_tail(&path, 256).unwrap();
        // Tail should be exactly 256 bytes — the last 256 of the file.
        assert_eq!(out.len(), 256);
        assert_eq!(out.as_bytes(), &payload[payload.len() - 256..]);
    }

    #[test]
    fn read_tail_propagates_open_failure() {
        let path = std::path::Path::new("/nonexistent/path/to/log");
        let err = read_tail(path, 128).unwrap_err();
        assert!(err.contains("open"));
    }

    #[test]
    fn read_tail_handles_seek_into_middle_of_multibyte_codepoint() {
        // Regression for the Greptile P1 finding on PR #36: when the
        // file contains non-ASCII text (e.g. a Windows username like
        // `C:\Users\李华\…` in a backtrace frame), seeking to
        // End(-max_bytes) can land mid-codepoint. read_to_string used
        // to return InvalidData and "Copy last 5 MB" surfaced an
        // unhelpful "invalid utf-8 sequence" message exactly when the
        // user most needed the log. The function now reads raw bytes
        // and converts lossily, so a half-codepoint at the head
        // becomes one U+FFFD glyph instead of breaking the call.
        let (_dir, path) = tmp_log();
        // `李华` = E6 9D 8E  E5 8D 8E (6 bytes for two codepoints).
        // Repeat enough that the seek lands well past the start.
        let mut payload = b"prefix-prefix-prefix-".to_vec();
        for _ in 0..100 {
            payload.extend_from_slice("李华".as_bytes());
        }
        std::fs::write(&path, &payload).unwrap();
        // 17 is not on a codepoint boundary for the trailing 李华
        // sequence (each char is 3 bytes), so the seek splits a char.
        // The previous implementation would have returned
        // Err("read …: stream did not contain valid UTF-8").
        let out = read_tail(&path, 17).expect("read_tail must not error on mid-codepoint seek");
        // The string is non-empty and ends with the canonical Chinese
        // character, confirming the tail of the file is intact.
        assert!(!out.is_empty());
        assert!(out.ends_with('华'));
        // The leading bytes that fell mid-codepoint are now one or two
        // replacement chars (U+FFFD); we just confirm the call returned
        // a String whose final code unit is the expected glyph.
    }

    #[test]
    fn newest_file_picks_the_most_recently_modified_entry() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("wordbuddy.log.1");
        let b = dir.path().join("wordbuddy.log.2");
        std::fs::write(&a, b"older").unwrap();
        // Spread the mtimes so we don't depend on filesystem timestamp
        // resolution — sleep long enough for any reasonable FS to
        // record distinct stamps.
        std::thread::sleep(std::time::Duration::from_millis(40));
        let mut f = std::fs::File::create(&b).unwrap();
        f.write_all(b"newer").unwrap();
        let picked = newest_file(dir.path()).unwrap().unwrap();
        assert_eq!(picked.file_name(), b.file_name());
    }

    #[test]
    fn newest_file_returns_none_for_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let out = newest_file(dir.path()).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn install_panic_hook_does_not_panic_when_called_twice() {
        // Idempotency: setting twice must not abort. set_hook silently
        // replaces the previous hook, but we exercise the call to be
        // sure nothing else changed.
        install_panic_hook();
        install_panic_hook();
    }
}
