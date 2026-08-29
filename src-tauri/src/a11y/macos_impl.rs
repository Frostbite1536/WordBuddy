//! macOS AXUIElement backend for accessibility-powered element detection.
//!
//! Uses the `accessibility` crate to walk the AX tree of the frontmost app.
//! Requires the Accessibility permission to be granted in
//! System Settings > Privacy & Security. Returns an empty vector (no error)
//! if the permission isn't granted — callers fall back to YOLO+OCR.
//!
//! Note: Chromium-based apps (VS Code, Claude Desktop) lazy-activate their
//! AX tree. The first query can take 100–500ms while the tree builds;
//! subsequent queries on the same window are fast. Element enumeration is
//! capped like the Windows walker (400 elements / 2000 siblings) so a huge
//! browser DOM cannot stall the caller either way.

use std::ffi::c_void;

use accessibility::AXUIElementAttributes as _;
use accessibility::{AXAttribute, AXUIElement};
use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::string::CFString;
use core_graphics::geometry::{CGPoint, CGSize};

use super::{FieldRead, Rect, UIElement};

/// Total-element cap, mirroring `windows_impl.rs` (runaway browser DOM guard).
const MAX_ELEMENTS: usize = 400;
/// Per-level sibling cap, mirroring `windows_impl.rs`.
const MAX_SIBLINGS: usize = 2000;

/// Enumerate accessibility elements in the frontmost app's focused window.
pub async fn get_foreground_elements(max_depth: u32) -> Result<Vec<UIElement>, String> {
    tokio::task::spawn_blocking(move || collect_elements(max_depth))
        .await
        .map_err(|e| format!("a11y task join failed: {e}"))?
}

fn collect_elements(max_depth: u32) -> Result<Vec<UIElement>, String> {
    // Gracefully no-op when the user hasn't granted Accessibility permission.
    if !is_process_trusted() {
        eprintln!("[a11y] macOS Accessibility permission not granted — falling back to YOLO+OCR");
        return Ok(Vec::new());
    }

    let app = focused_application()?;
    let window: AXUIElement = app
        .focused_window()
        .map_err(|e| format!("no focused window: {e}"))?;

    let mut elements = Vec::new();
    walk_element(&window, 1, max_depth, &mut elements);
    Ok(elements)
}

/// The frontmost app's AX element via the system-wide element's
/// `AXFocusedApplication` attribute (avoids needing NSWorkspace/pid).
fn focused_application() -> Result<AXUIElement, String> {
    let system_wide = AXUIElement::system_wide();
    let value: CFType = system_wide
        .attribute(&custom_attribute(
            accessibility_sys::kAXFocusedApplicationAttribute,
        ))
        .map_err(|e| format!("AXFocusedApplication query failed: {e}"))?;
    value
        .downcast_into::<AXUIElement>()
        .ok_or_else(|| "AXFocusedApplication is not an AXUIElement".to_string())
}

fn custom_attribute(name: &str) -> AXAttribute<CFType> {
    AXAttribute::<CFType>::new(&CFString::new(name))
}

/// Depth-first walk of the AX tree. Any single-node failure skips that node
/// but continues traversal (same degrade posture as the Windows walker).
fn walk_element(element: &AXUIElement, depth: u32, max_depth: u32, out: &mut Vec<UIElement>) {
    if out.len() >= MAX_ELEMENTS {
        return;
    }

    // Extract properties — any failure skips this element (continues traversal).
    let ax_role = element.role().map(|s| s.to_string()).ok();
    let title = element.title().map(|s| s.to_string()).unwrap_or_default();

    if let Some(display_role) = ax_role.as_deref().and_then(classify_role) {
        if !title.trim().is_empty() {
            if let Some(rect) = frame(element) {
                if rect.width > 0 && rect.height > 0 {
                    out.push(UIElement {
                        name: title,
                        role: display_role.to_string(),
                        bounding_rect: rect,
                        automation_id: element
                            .identifier()
                            .map(|s| s.to_string())
                            .unwrap_or_default(),
                        depth,
                    });
                }
            }
        }
    }

    if depth >= max_depth || out.len() >= MAX_ELEMENTS {
        return;
    }

    let children: CFArray<AXUIElement> = match element.children() {
        Ok(c) => c,
        Err(_) => return,
    };
    for (sibling_count, child) in children.iter().enumerate() {
        if out.len() >= MAX_ELEMENTS || sibling_count >= MAX_SIBLINGS {
            break;
        }
        walk_element(&child, depth + 1, max_depth, out);
    }
}

/// Read the element's AXPosition/AXSize pair into screen-space pixels.
///
/// These attributes carry `AXValue` payloads (CGPoint/CGSize), which the
/// high-level `accessibility` crate does not type — so the copied CFType is
/// unwrapped through its raw ref and decoded with `AXValueGetValue`.
/// Coordinates come back in the global display space (primary-display origin),
/// matching what `format_elements` expects after monitor-offset subtraction.
fn frame(element: &AXUIElement) -> Option<Rect> {
    let position: CFType = element
        .attribute(&custom_attribute(accessibility_sys::kAXPositionAttribute))
        .ok()?;
    let size: CFType = element
        .attribute(&custom_attribute(accessibility_sys::kAXSizeAttribute))
        .ok()?;

    let mut point = CGPoint { x: 0.0, y: 0.0 };
    let mut cg_size = CGSize {
        width: 0.0,
        height: 0.0,
    };
    // SAFETY: both values were just produced by AXUIElementCopyAttributeValue
    // (via the safe wrapper); if they are not AXValues of the expected type
    // AXValueGetValue returns false and we propagate None. The pointers stay
    // borrowed for the duration of the calls only.
    unsafe {
        let ok_pos = accessibility_sys::AXValueGetValue(
            position.as_concrete_TypeRef() as accessibility_sys::AXValueRef,
            accessibility_sys::kAXValueTypeCGPoint,
            &mut point as *mut CGPoint as *mut c_void,
        );
        let ok_size = accessibility_sys::AXValueGetValue(
            size.as_concrete_TypeRef() as accessibility_sys::AXValueRef,
            accessibility_sys::kAXValueTypeCGSize,
            &mut cg_size as *mut CGSize as *mut c_void,
        );
        if !(ok_pos && ok_size) {
            return None;
        }
    }
    Some(Rect {
        x: point.x as i32,
        y: point.y as i32,
        width: cg_size.width as i32,
        height: cg_size.height as i32,
    })
}

/// Map an AX role string to the short role vocabulary used in the LLM prompt
/// format (same shape as Windows `control_type_to_string`). Returns None for
/// layout/decoration roles (`AXPane`, `AXGroup`, `AXImage`, `AXSplitGroup`,
/// `AXScrollBar`, …) which are traversed but never emitted as targets.
fn classify_role(ax_role: &str) -> Option<&'static str> {
    Some(match ax_role {
        "AXButton" | "AXToolbarButton" | "AXPopUpButton" => "Button",
        "AXCheckBox" => "CheckBox",
        "AXRadioButton" => "Radio",
        "AXComboBox" => "ComboBox",
        "AXTextField" | "AXTextArea" | "AXSecureTextField" => "Edit",
        "AXLink" => "Link",
        "AXMenuItem" => "MenuItem",
        "AXSlider" => "Slider",
        "AXStaticText" => "Text",
        "AXRow" | "AXListRow" => "ListItem",
        "AXTabGroup" => "TabGroup",
        "AXToolbar" => "Toolbar",
        _ => return None,
    })
}

/// Whether the current process has been granted Accessibility permission.
///
/// Exposed to the frontend via the `check_a11y_permission` Tauri command
/// (registered in lib.rs); Settings shows a permission prompt when false.
pub fn is_process_trusted() -> bool {
    unsafe { accessibility_sys::AXIsProcessTrusted() }
}

// ── Focused-field reading (text_monitor) + selection capture ───────

/// Read the focused element of the frontmost app. Ordering is fixed by
/// INV-EXCL-001/INV-PRIV-001: resolve app + pid → exclusion check →
/// password check (fail-closed) → value read.
///
/// Returns `NoField` when Accessibility permission is missing (same
/// degrade-to-empty posture as element detection).
pub(crate) fn read_focused_field(excluded: &[String]) -> FieldRead {
    if !is_process_trusted() {
        return FieldRead::NoField;
    }

    let system_wide = AXUIElement::system_wide();

    // Step 1: resolve the foreground PROCESS identity only.
    let app = match system_wide
        .attribute(&custom_attribute(
            accessibility_sys::kAXFocusedApplicationAttribute,
        ))
        .map_err(|e| format!("AXFocusedApplication: {e}"))
        .and_then(|v: CFType| {
            v.downcast_into::<AXUIElement>()
                .ok_or_else(|| "AXFocusedApplication is not an AXUIElement".to_string())
        }) {
        Ok(app) => app,
        Err(e) => return FieldRead::Transient(e),
    };
    let pid = ax_pid(&app);
    let Some(process) = process_name_for_pid(pid) else {
        // Fail closed (Greptile P1): an unresolvable identity cannot be
        // proven non-excluded, so nothing may be read this tick. The
        // synthetic label only feeds diagnostics — it never matches an
        // exclusion entry, so exclusion itself must be implied.
        return FieldRead::Excluded(format!("pid-{pid}"));
    };
    if crate::text_monitor::process_excluded(&process, excluded) {
        return FieldRead::Excluded(process);
    }

    // Step 2+: only now touch the focused ELEMENT and its attributes.
    let element = match system_wide
        .attribute(&custom_attribute(
            accessibility_sys::kAXFocusedUIElementAttribute,
        ))
        .map_err(|e| format!("AXFocusedUIElement: {e}"))
        .and_then(|v: CFType| {
            v.downcast_into::<AXUIElement>()
                .ok_or_else(|| "AXFocusedUIElement is not an AXUIElement".to_string())
        }) {
        Ok(el) => el,
        Err(_) => {
            // No focused element right now — normal for desktop focus.
            return FieldRead::NoField;
        }
    };

    let rect = frame(&element).map(|r| (r.x, r.y, r.x + r.width, r.y + r.height));

    // INV-PRIV-001: password check BEFORE the value read. A failed role
    // query must fail CLOSED — treat unknown-role elements as passwords.
    let role = element.role().map(|s| s.to_string());
    let subrole = element.subrole().map(|s| s.to_string()).unwrap_or_default();
    let is_password = match &role {
        Ok(r) => r == "AXSecureTextField",
        Err(_) => true,
    } || subrole == "AXSecureTextField";
    if is_password {
        return FieldRead::Password { process, rect };
    }

    match element
        .value()
        .ok()
        .and_then(|v: CFType| v.downcast_into::<CFString>())
        .map(|s| s.to_string())
    {
        Some(text) => FieldRead::Text {
            process,
            text,
            rect,
        },
        None => FieldRead::NoField,
    }
}

fn ax_pid(app: &AXUIElement) -> i32 {
    // SAFETY: AXUIElementGetPid only writes through the out-param on success.
    let mut pid: i32 = 0;
    unsafe {
        accessibility_sys::AXUIElementGetPid(app.as_concrete_TypeRef(), &mut pid);
    }
    pid
}

/// PID → process image basename, cached per pid (`ps` spawn is once per
/// process, not once per tick).
pub(crate) fn process_name_for_pid(pid: i32) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::Mutex;

    static CACHE: Mutex<Option<HashMap<i32, String>>> = Mutex::new(None);

    fn uncached(pid: i32) -> Option<String> {
        let out = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(name) = cache.get(&pid) {
        return Some(name.clone());
    }
    let name = uncached(pid)?;
    // Bound the cache so a pathological pid churn can't grow it forever.
    if cache.len() >= 256 {
        cache.clear();
    }
    cache.insert(pid, name.clone());
    Some(name)
}

/// The currently selected text of the focused element, if any
/// (`AXSelectedText`). Password fields are never read (INV-PRIV-001):
/// a failed role check returns None like an empty selection, but the
/// caller-visible effect is identical — nothing leaves the field.
pub(crate) fn selected_text_of_focused_element() -> Result<Option<String>, String> {
    if !is_process_trusted() {
        return Ok(None);
    }
    let system_wide = AXUIElement::system_wide();
    let element = system_wide
        .attribute(&custom_attribute(
            accessibility_sys::kAXFocusedUIElementAttribute,
        ))
        .map_err(|e| format!("AXFocusedUIElement: {e}"))
        .and_then(|v: CFType| {
            v.downcast_into::<AXUIElement>()
                .ok_or_else(|| "AXFocusedUIElement is not an AXUIElement".to_string())
        })?;
    let role = element.role().map(|s| s.to_string());
    let subrole = element.subrole().map(|s| s.to_string()).unwrap_or_default();
    let is_password =
        !matches!(&role, Ok(r) if r != "AXSecureTextField") || subrole == "AXSecureTextField";
    if is_password {
        return Ok(None);
    }
    Ok(element
        .attribute(&custom_attribute(
            accessibility_sys::kAXSelectedTextAttribute,
        ))
        .ok()
        .and_then(|v: CFType| v.downcast_into::<CFString>())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty()))
}
