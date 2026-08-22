//! Windows UI Automation backend for accessibility-powered element detection.
//!
//! Runs on a blocking tokio thread to isolate COM MTA initialization from
//! the rest of the app (see plan §1 "Gotchas"). Uses the control view walker
//! to skip raw/layout nodes, and filters to interactive control types.

use super::{Rect, UIElement};

/// Enumerate interactive UI elements in the foreground window.
pub async fn get_foreground_elements(max_depth: u32) -> Result<Vec<UIElement>, String> {
    tokio::task::spawn_blocking(move || collect_elements(max_depth))
        .await
        .map_err(|e| format!("a11y task join failed: {e}"))?
}

fn collect_elements(max_depth: u32) -> Result<Vec<UIElement>, String> {
    use uiautomation::types::Handle;
    use uiautomation::UIAutomation;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let automation = UIAutomation::new().map_err(|e| format!("UIAutomation::new failed: {e}"))?;

    // GetForegroundWindow returns an HWND in our pinned `windows` crate
    // version. The `uiautomation` crate transitively pins a *different*
    // `windows` version, so we cannot pass HWND directly — we round-trip
    // through `isize`, which is HWND's stable Win32 ABI on both x64 and
    // ARM64.
    //
    // If either crate ever changes the HWND ABI this conversion will
    // silently return garbage, so:
    //   1. The null-pointer guard above stops the obvious case (zero
    //      handle from no-foreground-window) before the cast.
    //   2. The debug-build assertion below catches any other zero-after-
    //      cast surprise during development without changing release
    //      behavior.
    //
    // Pin constraints (see Cargo.toml):
    //   uiautomation = pinned to a `windows-rs` major that uses
    //                  `*mut c_void` for HWND on x64/ARM64.
    //   direct dep   `windows` matches the same HWND representation.
    // Bumping either major version requires re-verifying this cast.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return Ok(Vec::new());
    }
    let raw = hwnd.0 as isize;
    debug_assert!(
        raw != 0,
        "non-null HWND collapsed to 0 across windows-rs version skew"
    );
    let handle = Handle::from(raw);

    let root = automation
        .element_from_handle(handle)
        .map_err(|e| format!("element_from_handle failed: {e}"))?;
    let walker = automation
        .get_control_view_walker()
        .map_err(|e| format!("get_control_view_walker failed: {e}"))?;

    let mut elements = Vec::new();
    // Root window itself is not usually a pointing target — start from its children.
    if let Ok(first) = walker.get_first_child(&root) {
        walk_tree(&walker, &first, 1, max_depth, &mut elements);
        let mut current = first;
        // Safety cap on sibling iteration — pathological windows with
        // thousands of top-level children (browser DOM flattened to root)
        // would otherwise hang the 800ms a11y budget.
        let mut sibling_count = 0usize;
        while let Ok(next) = walker.get_next_sibling(&current) {
            if elements.len() >= 400 || sibling_count >= 2000 {
                break;
            }
            walk_tree(&walker, &next, 1, max_depth, &mut elements);
            current = next;
            sibling_count += 1;
        }
    }

    Ok(elements)
}

fn walk_tree(
    walker: &uiautomation::UITreeWalker,
    element: &uiautomation::UIElement,
    depth: u32,
    max_depth: u32,
    out: &mut Vec<UIElement>,
) {
    // Cap total collection to avoid runaway browser DOM trees
    if out.len() >= 400 {
        return;
    }

    // Extract properties — any failure skips this element (continues traversal).
    let name = element.get_name().unwrap_or_default();
    let control_type = match element.get_control_type() {
        Ok(ct) => ct,
        Err(_) => return,
    };
    let rect = match element.get_bounding_rectangle() {
        Ok(r) => r,
        Err(_) => return,
    };
    let automation_id = element.get_automation_id().unwrap_or_default();

    let role = control_type_to_string(control_type);

    // Skip layout/non-interactive containers — we still traverse into them for
    // their children, but we don't emit them as targets.
    if is_interactive_role(&role) && !name.trim().is_empty() {
        let (x, y) = (rect.get_left(), rect.get_top());
        let (w, h) = (rect.get_width(), rect.get_height());
        if w > 0 && h > 0 {
            out.push(UIElement {
                name,
                role,
                bounding_rect: Rect { x, y, width: w, height: h },
                automation_id,
                depth,
            });
        }
    }

    if depth >= max_depth {
        return;
    }

    if let Ok(first) = walker.get_first_child(element) {
        walk_tree(walker, &first, depth + 1, max_depth, out);
        let mut current = first;
        let mut sibling_count = 0usize;
        while let Ok(next) = walker.get_next_sibling(&current) {
            // Safety cap: stop iterating siblings once we've hit the output cap
            // or processed an unreasonable number of peers at this level.
            if out.len() >= 400 || sibling_count >= 2000 {
                break;
            }
            walk_tree(walker, &next, depth + 1, max_depth, out);
            current = next;
            sibling_count += 1;
        }
    }
}

/// Map ControlType to a short role name used in the LLM prompt format.
fn control_type_to_string(ct: uiautomation::controls::ControlType) -> String {
    use uiautomation::controls::ControlType as C;
    match ct {
        C::Button => "Button",
        C::CheckBox => "CheckBox",
        C::ComboBox => "ComboBox",
        C::Edit => "Edit",
        C::Hyperlink => "Link",
        C::ListItem => "ListItem",
        C::List => "List",
        C::MenuItem => "MenuItem",
        C::Menu => "Menu",
        C::MenuBar => "MenuBar",
        C::ProgressBar => "ProgressBar",
        C::RadioButton => "Radio",
        C::Slider => "Slider",
        C::Tab => "TabGroup",
        C::TabItem => "Tab",
        C::Text => "Text",
        C::ToolBar => "Toolbar",
        C::TreeItem => "TreeItem",
        C::Tree => "Tree",
        C::SplitButton => "SplitButton",
        C::Header => "Header",
        C::HeaderItem => "HeaderItem",
        C::DataItem => "DataItem",
        C::Document => "Document",
        C::Window => "Window",
        C::Pane => "Pane",
        C::Group => "Group",
        C::Custom => "Custom",
        _ => "Other",
    }
    .to_string()
}

/// Control types that are worth pointing at (interactive or uniquely labelled).
/// `Document` is intentionally excluded: in Chromium it wraps the entire
/// rendered body as one giant element, and emitting it pollutes the 200-cap
/// with a low-signal target the LLM can't meaningfully click.
fn is_interactive_role(role: &str) -> bool {
    matches!(
        role,
        "Button"
            | "CheckBox"
            | "ComboBox"
            | "Edit"
            | "Link"
            | "ListItem"
            | "MenuItem"
            | "Radio"
            | "Slider"
            | "Tab"
            | "TreeItem"
            | "SplitButton"
            | "HeaderItem"
            | "DataItem"
    )
}
