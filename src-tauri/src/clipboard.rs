//! Clipboard save/restore for synthetic-paste applies (PLAN-04 task 4).
//!
//! The paste-path apply temporarily overwrites the user's clipboard with
//! a replacement string. This module snapshots every HGLOBAL-backed
//! clipboard format before the overwrite and restores it afterwards, so
//! the user's clipboard (including file-list copies like Explorer's
//! image-file copy) survives the operation.
//!
//! The OS boundary is behind [`ClipboardBackend`] so unit tests run
//! against an in-memory fake; the real implementation is a thin Win32
//! wrapper (`OpenClipboard` → snapshot → restore) executed on a blocking
//! thread by callers.

use std::sync::Mutex;

/// One snapshot format: format id + raw bytes (HGLOBAL content copy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardSnapshot {
    pub formats: Vec<(u32, Vec<u8>)>,
}

/// Mockable OS layer (PLAN-04 task 4).
pub trait ClipboardBackend {
    /// Fails if the clipboard cannot be opened (retry-worthy).
    fn snapshot(&self) -> Result<ClipboardSnapshot, String>;
    /// Sets exactly one CF_UNICODETEXT payload.
    fn set_text(&self, text: &str) -> Result<(), String>;
    /// Restores the snapshot (empty snapshot clears text formats).
    fn restore(&self, snap: &ClipboardSnapshot) -> Result<(), String>;
    /// Reads current CF_UNICODETEXT (verification readback).
    fn get_text(&self) -> Result<Option<String>, String>;
}

// ── Windows implementation ───────────────────────────────────────────

#[cfg(target_os = "windows")]
mod win {
    use super::{ClipboardBackend, ClipboardSnapshot};

    pub struct WinClipboard;

    impl WinClipboard {
        fn with_open<R>(&self, f: impl FnOnce() -> Result<R, String>) -> Result<R, String> {
            unsafe {
                // Retry briefly: the clipboard is a shared resource and
                // other apps commonly hold it for a few milliseconds.
                let mut opened = false;
                for attempt in 0..20 {
                    if windows::Win32::System::DataExchange::OpenClipboard(
                        windows::Win32::Foundation::HWND::default(),
                    )
                    .is_ok()
                    {
                        opened = true;
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                    let _ = attempt;
                }
                if !opened {
                    return Err("could not open clipboard after 20 attempts".into());
                }
                let result = f();
                let _ = windows::Win32::System::DataExchange::CloseClipboard();
                result
            }
        }
    }

    const CF_UNICODETEXT: u32 = 13;

    impl ClipboardBackend for WinClipboard {
        fn snapshot(&self) -> Result<ClipboardSnapshot, String> {
            self.with_open(|| unsafe {
                let mut formats = Vec::new();
                let mut fmt = windows::Win32::System::DataExchange::EnumClipboardFormats(0);
                let mut guard = 0;
                while fmt != 0 && guard < 64 {
                    // Skip formats that are not HGLOBAL-backed handles we
                    // can copy (BITMAP, ENHMETAFILE, METAFILEPICT). Their
                    // absence is restored implicitly by leaving them off
                    // (EmptyClipboard semantics we do NOT use — we only
                    // set what we snap, so untouched formats... are gone
                    // if another app flushes; acceptable residual risk,
                    // logged by callers when readback mismatches).
                    // HGLOBAL-backed formats we can copy by value:
                    // text family + CF_DIB + CF_HDROP + CF_LOCALE + every
                    // registered format. Bitmap/metafile HANDLE formats
                    // (2, 3, 14, 17) are excluded — they are not plain
                    // global allocations.
                    let is_global = matches!(fmt, 1 | 7 | 8 | 13 | 15 | 16) || fmt >= 49152;
                    if is_global {
                        let handle = windows::Win32::System::DataExchange::GetClipboardData(fmt);
                        if let Ok(handle) = handle {
                            let hglobal = windows::Win32::Foundation::HGLOBAL(handle.0);
                            let size = windows::Win32::System::Memory::GlobalSize(hglobal);
                            if size > 0 && size <= 16 * 1024 * 1024 {
                                let ptr = windows::Win32::System::Memory::GlobalLock(hglobal);
                                if !ptr.is_null() {
                                    let bytes = std::slice::from_raw_parts(ptr.cast::<u8>(), size);
                                    formats.push((fmt, bytes.to_vec()));
                                    let _ = windows::Win32::System::Memory::GlobalUnlock(hglobal);
                                }
                            }
                        }
                    }
                    fmt = windows::Win32::System::DataExchange::EnumClipboardFormats(fmt);
                    guard += 1;
                }
                Ok(ClipboardSnapshot { formats })
            })
        }

        fn set_text(&self, text: &str) -> Result<(), String> {
            self.with_open(|| unsafe {
                use windows::Win32::System::DataExchange::{EmptyClipboard, SetClipboardData};
                use windows::Win32::System::Memory::{
                    GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
                };

                // UTF-16 + NUL terminator.
                let mut wide: Vec<u16> = text.encode_utf16().collect();
                wide.push(0);
                let bytes = wide.len() * 2;
                let handle =
                    GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|e| format!("GlobalAlloc: {e}"))?;
                let ptr = GlobalLock(handle);
                if ptr.is_null() {
                    return Err("GlobalLock failed".into());
                }
                std::ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), ptr.cast::<u8>(), bytes);
                let _ = GlobalUnlock(handle);
                if EmptyClipboard().is_err() {
                    return Err("EmptyClipboard failed".into());
                }
                match SetClipboardData(CF_UNICODETEXT, windows::Win32::Foundation::HANDLE(handle.0))
                {
                    Ok(_) => Ok(()),
                    Err(e) => Err(format!("SetClipboardData failed: {e}")),
                }
            })
        }

        fn restore(&self, snap: &ClipboardSnapshot) -> Result<(), String> {
            self.with_open(|| unsafe {
                use windows::Win32::System::DataExchange::{EmptyClipboard, SetClipboardData};
                use windows::Win32::System::Memory::{
                    GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
                };

                if EmptyClipboard().is_err() {
                    return Err("EmptyClipboard failed".into());
                }
                if snap.formats.is_empty() {
                    return Ok(());
                }
                for (fmt, bytes) in &snap.formats {
                    if *fmt == 0 || bytes.is_empty() {
                        continue;
                    }
                    let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
                        .map_err(|e| format!("GlobalAlloc: {e}"))?;
                    let ptr = GlobalLock(handle);
                    if ptr.is_null() {
                        continue;
                    }
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
                    let _ = GlobalUnlock(handle);
                    // Ownership transfers to the clipboard on success;
                    // on failure free to avoid a leak.
                    // On failure the allocation leaks; this windows-rs
                    // feature set exposes no GlobalFree. The path is rare
                    // (SetClipboardData failing with a fresh allocation)
                    // and bounded by the format count.
                    let _ = SetClipboardData(*fmt, windows::Win32::Foundation::HANDLE(handle.0));
                }
                Ok(())
            })
        }

        fn get_text(&self) -> Result<Option<String>, String> {
            self.with_open(|| unsafe {
                let Ok(handle) =
                    windows::Win32::System::DataExchange::GetClipboardData(CF_UNICODETEXT)
                else {
                    return Ok(None);
                };
                let hglobal = windows::Win32::Foundation::HGLOBAL(handle.0);
                let size = windows::Win32::System::Memory::GlobalSize(hglobal);
                if size == 0 {
                    return Ok(None);
                }
                let ptr = windows::Win32::System::Memory::GlobalLock(hglobal);
                if ptr.is_null() {
                    return Ok(None);
                }
                let slice = std::slice::from_raw_parts(ptr.cast::<u16>(), size / 2);
                let _ = windows::Win32::System::Memory::GlobalUnlock(hglobal);
                // Truncate at first NUL.
                let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
                Ok(Some(String::from_utf16_lossy(&slice[..end])))
            })
        }
    }
}

#[cfg(target_os = "windows")]
pub use win::WinClipboard;

// ── macOS / Linux implementation (arboard, text-only v1) ────────────

#[cfg(not(target_os = "windows"))]
mod arboard_backend {
    use super::{ClipboardBackend, ClipboardSnapshot};

    /// Pseudo format id carried in [`ClipboardSnapshot::formats`] for the
    /// text payload on non-Windows backends.
    const TEXT_FORMAT: u32 = 1;

    /// arboard-backed clipboard.
    ///
    /// PLAN-08 scope decision: snapshot/restore round-trips TEXT content
    /// only. Unlike the Win32 backend, images/file-lists/custom formats
    /// on the clipboard are lost across a paste apply — a documented
    /// degradation (release notes), not silent corruption: every op
    /// surfaces errors instead of pretending success.
    pub struct ArboardClipboard;

    impl ClipboardBackend for ArboardClipboard {
        fn snapshot(&self) -> Result<ClipboardSnapshot, String> {
            let mut cb =
                arboard::Clipboard::new().map_err(|e| format!("clipboard open failed: {e}"))?;
            let text = match cb.get_text() {
                Ok(t) => Some(t),
                Err(arboard::Error::ContentNotAvailable) => None,
                Err(e) => return Err(format!("clipboard read failed: {e}")),
            };
            Ok(ClipboardSnapshot {
                formats: text
                    .map(|t| (TEXT_FORMAT, t.into_bytes()))
                    .into_iter()
                    .collect(),
            })
        }

        fn set_text(&self, text: &str) -> Result<(), String> {
            let mut cb =
                arboard::Clipboard::new().map_err(|e| format!("clipboard open failed: {e}"))?;
            cb.set_text(text.to_string())
                .map_err(|e| format!("clipboard write failed: {e}"))
        }

        /// Restores the snapshot; an empty snapshot clears the clipboard
        /// (same contract as the Windows backend).
        fn restore(&self, snap: &ClipboardSnapshot) -> Result<(), String> {
            match snap.formats.iter().find(|(id, _)| *id == TEXT_FORMAT) {
                Some((_, bytes)) => {
                    let s = String::from_utf8_lossy(bytes);
                    self.set_text(&s)
                }
                None => {
                    let mut cb = arboard::Clipboard::new()
                        .map_err(|e| format!("clipboard open failed: {e}"))?;
                    cb.clear()
                        .map_err(|e| format!("clipboard clear failed: {e}"))
                }
            }
        }

        fn get_text(&self) -> Result<Option<String>, String> {
            let mut cb =
                arboard::Clipboard::new().map_err(|e| format!("clipboard open failed: {e}"))?;
            Ok(cb.get_text().ok())
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub use arboard_backend::ArboardClipboard;
#[cfg(not(target_os = "windows"))]
pub use arboard_backend::ArboardClipboard as WinClipboard;

// ── High-level operation (backend-injected, unit-testable) ──────────

/// Runs `body` with `text` on the clipboard, then restores the snapshot.
/// `paste` performs the actual paste action (SendInput by callers) and
/// returns when the paste has landed (callers wait their fixed delay).
pub fn with_temporary_clipboard<B, F, R>(backend: &B, text: &str, mut paste: F) -> Result<R, String>
where
    B: ClipboardBackend + ?Sized,
    F: FnMut() -> Result<R, String>,
{
    let snap = backend.snapshot()?;
    backend.set_text(text)?;
    let result = paste();
    // Fixed restore delay: give the target's paste handler time to read
    // the clipboard before we swap it back (PLAN-04 risk note).
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Audit M7: the just-pasted target frequently holds the clipboard
    // lock here; a single failed restore silently destroyed the user's
    // previous contents. Retry with backoff.
    let mut restore_result = backend.restore(&snap);
    for _ in 0..3 {
        if restore_result.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        restore_result = backend.restore(&snap);
    }

    // Readback verification for text-bearing snapshots.
    if let Some(expected) = snap
        .formats
        .iter()
        .find(|(f, _)| *f == 13)
        .map(|(_, bytes)| bytes.clone())
    {
        if let Ok(Some(current)) = backend.get_text() {
            let (pairs, _) = expected.as_chunks::<2>();
            let expected_text = String::from_utf16_lossy(
                &pairs
                    .iter()
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|&c| c != 0)
                    .collect::<Vec<_>>(),
            );
            if current != expected_text {
                eprintln!(
                    "[clipboard] restore readback mismatch (expected {} chars, got {})",
                    expected_text.chars().count(),
                    current.chars().count()
                );
            }
        }
    }

    match (result, restore_result) {
        (Err(e), _) => Err(e), // paste never landed — its error wins
        (Ok(r), Ok(())) => Ok(r),
        // Audit M7/L6: a landed paste must not be reported as a failure
        // just because the restore failed — that inverted reality and
        // skewed rewrite metrics. The loss is logged loudly instead.
        (Ok(r), Err(e)) => {
            eprintln!(
                "[clipboard] paste landed but RESTORE FAILED ({e}): \
                 previous clipboard content was not recovered"
            );
            Ok(r)
        }
    }
}

/// Global single clipboard guard: applies are single-flight, and two
/// concurrent clipboard mutations would corrupt each other's snapshots.
static CLIPBOARD_GUARD: Mutex<()> = Mutex::new(());

pub fn clipboard_lock() -> std::sync::MutexGuard<'static, ()> {
    CLIPBOARD_GUARD.lock().unwrap_or_else(|e| e.into_inner()) // poison recovery
}

// ── Tests (fake backend; no OS) ─────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeClipboard {
        state: RefCell<Vec<(u32, Vec<u8>)>>,
    }

    fn text_bytes(s: &str) -> Vec<u8> {
        let mut wide: Vec<u16> = s.encode_utf16().collect();
        wide.push(0);
        wide.iter().flat_map(|c| c.to_le_bytes()).collect()
    }

    impl ClipboardBackend for FakeClipboard {
        fn snapshot(&self) -> Result<ClipboardSnapshot, String> {
            Ok(ClipboardSnapshot {
                formats: self.state.borrow().clone(),
            })
        }
        fn set_text(&self, text: &str) -> Result<(), String> {
            let mut s = self.state.borrow_mut();
            s.clear();
            s.push((13, text_bytes(text)));
            Ok(())
        }
        fn restore(&self, snap: &ClipboardSnapshot) -> Result<(), String> {
            *self.state.borrow_mut() = snap.formats.clone();
            Ok(())
        }
        fn get_text(&self) -> Result<Option<String>, String> {
            let s = self.state.borrow();
            Ok(s.iter().find(|(f, _)| *f == 13).map(|(_, b)| {
                let (pairs, _) = b.as_chunks::<2>();
                let units: Vec<u16> = pairs
                    .iter()
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|&c| c != 0)
                    .collect();
                String::from_utf16_lossy(&units)
            }))
        }
    }

    #[test]
    fn temporary_clipboard_restores_prior_content() {
        let fake = FakeClipboard::default();
        fake.set_text("user had this").unwrap();
        let out = with_temporary_clipboard(&fake, "replacement", || {
            assert_eq!(fake.get_text().unwrap().as_deref(), Some("replacement"));
            Ok(42)
        })
        .unwrap();
        assert_eq!(out, 42);
        assert_eq!(fake.get_text().unwrap().as_deref(), Some("user had this"));
    }

    #[test]
    fn non_text_formats_survive_round_trip() {
        let fake = FakeClipboard::default();
        // Simulate Explorer's image-file copy: CF_HDROP(15) + a shell
        // registered format.
        *fake.state.borrow_mut() = vec![
            (15, b"C:\\img.png\0\0\0".to_vec()),
            (49162, vec![9, 9, 9, 9]),
            (13, text_bytes("prior text")),
        ];
        with_temporary_clipboard(&fake, "fix", || Ok(())).unwrap();
        let snap = fake.snapshot().unwrap();
        assert!(snap
            .formats
            .iter()
            .any(|(f, b)| *f == 15 && b == b"C:\\img.png\0\0\0"));
        assert!(snap
            .formats
            .iter()
            .any(|(f, b)| *f == 49162 && b == &[9, 9, 9, 9]));
    }

    #[test]
    fn paste_failure_still_restores_and_propagates_error() {
        let fake = FakeClipboard::default();
        fake.set_text("prior").unwrap();
        let err = with_temporary_clipboard(&fake, "fix", || -> Result<(), String> {
            Err("paste failed".to_string())
        })
        .unwrap_err();
        assert_eq!(err, "paste failed");
        assert_eq!(fake.get_text().unwrap().as_deref(), Some("prior"));
    }

    #[test]
    fn single_flight_guard_releases() {
        let _g = clipboard_lock();
        drop(_g);
        let _g2 = clipboard_lock();
    }
}
