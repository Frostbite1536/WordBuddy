//! Linux AT-SPI2 backend for accessibility-powered element detection and
//! focused-field reading.
//!
//! Uses the `atspi` crate (D-Bus based). Requires the `at-spi2-core`
//! daemon (default on GNOME/KDE); when the bus is absent we log once and
//! degrade to empty/Unsupported — never an error storm. GTK/Qt apps work
//! out of the box; Electron apps need `ACCESSIBILITY_ENABLED=1` at launch
//! (community trick, not officially documented).
//!
//! D-Bus round-trips cost real time (measured 100–500ms for whole-tree
//! enumeration in the ancestor repo), so both entry points are budgeted:
//! node budgets cap per-call work, and the focused-field reader caches the
//! last focused object path and revalidates it cheaply before re-walking.
//!
//! INV-EXCL-001 ordering is enforced here: the foreground process identity
//! is resolved from the focused object's bus connection BEFORE any field
//! value is read. Only state/role/name metadata is touched before that.

use std::future::Future;
use std::pin::Pin;
use std::sync::{LazyLock, Mutex};

use std::time::{Duration, Instant};

use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::component::ComponentProxy;
use atspi::proxy::text::TextProxy;
use atspi::{AccessibilityConnection, CoordType, Role, State};
use zbus::names::UniqueName;
use zbus::zvariant::ObjectPath;

use super::{FieldRead, Rect, UIElement};

/// Total-element cap (mirrors `windows_impl.rs`).
const MAX_ELEMENTS: usize = 400;
/// Per-level sibling cap (mirrors `windows_impl.rs`, tuned down for D-Bus).
const MAX_SIBLINGS: usize = 200;
/// Node budget for the focused-element search — state queries only, but
/// they still cross the bus.
const FOCUS_SEARCH_BUDGET: usize = 200;
/// Cached focused-object revalidation window. Within it we pay two cheap
/// D-Bus calls instead of a full desktop walk.
const FOCUS_CACHE_TTL_MS: u64 = 2_000;

// ── Runtime + connection plumbing ───────────────────────────────────

/// Dedicated current-thread runtime. zbus (with the tokio feature) needs a
/// reactor to drive its internal tasks; readers run inside tokio's
/// `spawn_blocking` where no reactor exists, so we own one here. Every
/// AT-SPI call must go through this same runtime instance because the
/// cached connection's executor is bound to it.
static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build AT-SPI runtime")
});

static CONNECTION: LazyLock<Option<AccessibilityConnection>> = LazyLock::new(|| {
    match RUNTIME.block_on(AccessibilityConnection::new()) {
        Ok(conn) => Some(conn),
        Err(e) => {
            // One clear log line when the daemon is missing — the
            // documented Linux prerequisite (PLAN-08 §4.2).
            eprintln!(
                "[a11y] AT-SPI2 accessibility bus unavailable ({e}); \
                 is the at-spi2-core daemon running? Native text \
                 monitoring stays off."
            );
            None
        }
    }
});

fn connection() -> Result<&'static AccessibilityConnection, String> {
    CONNECTION
        .as_ref()
        .ok_or_else(|| "no AT-SPI2 accessibility bus".into())
}

async fn accessible_proxy(
    conn: &AccessibilityConnection,
    name: Option<&UniqueName<'static>>,
    path: &ObjectPath<'static>,
) -> Result<AccessibleProxy<'static>, String> {
    let dest = name.ok_or_else(|| "null object reference".to_string())?;
    AccessibleProxy::builder(conn.connection())
        .cache_properties(zbus::proxy::CacheProperties::No)
        .destination(dest.clone())
        .map_err(|e| e.to_string())?
        .path(path.clone())
        .map_err(|e| e.to_string())?
        .build()
        .await
        .map_err(|e| e.to_string())
}

/// PID of the process owning an AT-SPI object, via the bus daemon's
/// GetConnectionUnixProcessID on the object's unique bus name.
async fn pid_of_object(conn: &AccessibilityConnection, dest: &UniqueName<'static>) -> Option<u32> {
    let dbus = zbus::fdo::DBusProxy::new(conn.connection()).await.ok()?;
    dbus.get_connection_unix_process_id(dest.clone().into())
        .await
        .ok()
}

fn process_name_for_pid(pid: u32) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let name = comm.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

// ── Element detection ───────────────────────────────────────────────

/// Enumerate accessibility elements from the focused window (the frame
/// carrying the `Active` state). Returns an empty vector when no active
/// frame is exposed or when no app publishes an accessibility tree.
pub async fn get_foreground_elements(max_depth: u32) -> Result<Vec<UIElement>, String> {
    tokio::task::spawn_blocking(move || collect_elements(max_depth))
        .await
        .map_err(|e| format!("a11y task join failed: {e}"))?
}

fn collect_elements(max_depth: u32) -> Result<Vec<UIElement>, String> {
    RUNTIME.block_on(collect_elements_async(max_depth))
}

async fn collect_elements_async(max_depth: u32) -> Result<Vec<UIElement>, String> {
    let conn = connection()?;
    let root = conn
        .root_accessible_on_registry()
        .await
        .map_err(|e| format!("registry root: {e}"))?;

    // Desktop → applications → top-level frames; emit only the frame
    // marked Active (the window manager's notion of focus). Some toolkits
    // never set Active on any frame — report empty rather than paying a
    // full-desktop walk.
    let mut elements = Vec::new();
    for app_ref in root.get_children().await.unwrap_or_default() {
        if elements.len() >= MAX_ELEMENTS {
            break;
        }
        let Ok(app) = accessible_proxy(conn, app_ref.name(), app_ref.path()).await else {
            continue;
        };
        for frame_ref in app.get_children().await.unwrap_or_default() {
            let Ok(frame) = accessible_proxy(conn, frame_ref.name(), frame_ref.path()).await else {
                continue;
            };
            let active = matches!(frame.get_state().await, Ok(s) if s.contains(State::Active));
            if !active {
                continue;
            }
            // Budget doubles the element cap: interior nodes outnumber
            // emitted ones, but this still bounds pathological trees.
            let mut budget = MAX_ELEMENTS * 2;
            walk_element(conn, &frame, 1, max_depth, &mut elements, &mut budget).await;
            return Ok(elements);
        }
    }
    Ok(elements)
}

/// Depth-first walk of one frame's tree. Any single-node failure skips
/// that node but continues traversal (same degrade posture as Windows).
///
/// Recursive async fns need boxing (`async_recursion` would pull another
/// dep for two call sites), so this returns a pinned boxed future.
fn walk_element<'a>(
    conn: &'a AccessibilityConnection,
    element: &'a AccessibleProxy<'static>,
    depth: u32,
    max_depth: u32,
    out: &'a mut Vec<UIElement>,
    budget: &'a mut usize,
) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        if out.len() >= MAX_ELEMENTS || *budget == 0 {
            return;
        }
        *budget -= 1;

        let state = match element.get_state().await {
            Ok(s) => s,
            Err(_) => return, // dead object — prune the subtree
        };
        // Prune invisible subtrees; tolerate toolkits that expose only one of
        // Visible/Showing.
        if !state.contains(State::Visible) && !state.contains(State::Showing) {
            return;
        }

        let role = match element.get_role().await {
            Ok(r) => r,
            Err(_) => return,
        };

        if let Some(display_role) = classify_role(role) {
            let name = element.name().await.unwrap_or_default();
            if !name.trim().is_empty() {
                if let Some((x, y, w, h)) = extents_of(conn, element).await {
                    if w > 0 && h > 0 {
                        out.push(UIElement {
                            name,
                            role: display_role.to_string(),
                            bounding_rect: Rect {
                                x,
                                y,
                                width: w,
                                height: h,
                            },
                            // AT-SPI exposes no stable automation-id equivalent.
                            automation_id: String::new(),
                            depth,
                        });
                    }
                }
            }
        }

        if depth >= max_depth || out.len() >= MAX_ELEMENTS {
            return;
        }

        for child_ref in element
            .get_children()
            .await
            .unwrap_or_default()
            .into_iter()
            .take(MAX_SIBLINGS)
        {
            if out.len() >= MAX_ELEMENTS {
                break;
            }
            match accessible_proxy(conn, child_ref.name(), child_ref.path()).await {
                Ok(child) => walk_element(conn, &child, depth + 1, max_depth, out, budget).await,
                Err(_) => continue,
            }
        }
    })
}
async fn extents_of(
    conn: &AccessibilityConnection,
    element: &AccessibleProxy<'static>,
) -> Option<(i32, i32, i32, i32)> {
    let component = ComponentProxy::builder(conn.connection())
        .cache_properties(zbus::proxy::CacheProperties::No)
        .destination(element.inner().destination().clone())
        .ok()?
        .path(element.inner().path().clone())
        .ok()?
        .build()
        .await
        .ok()?;
    component.get_extents(CoordType::Screen).await.ok()
}

/// Map an AT-SPI role to the short role vocabulary used in the LLM prompt
/// format (same shape as Windows `control_type_to_string`). Layout and
/// decoration roles return None: traversed, never emitted.
fn classify_role(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::Button | Role::ToggleButton => "Button",
        Role::CheckBox | Role::CheckMenuItem => "CheckBox",
        Role::RadioButton | Role::RadioMenuItem => "Radio",
        Role::ComboBox => "ComboBox",
        Role::Entry | Role::Text | Role::PasswordText | Role::SpinButton => "Edit",
        Role::Link => "Link",
        Role::MenuItem => "MenuItem",
        Role::Slider => "Slider",
        Role::Label | Role::Heading | Role::Paragraph | Role::Caption => "Text",
        Role::ListItem | Role::TableCell => "ListItem",
        Role::PageTab => "Tab",
        _ => return None,
    })
}

// ── Focused-field reading (text_monitor) ────────────────────────────

/// Cached focus: the object that most recently validated as the focused
/// editable. Revalidated with two cheap calls per tick; a failed
/// validation falls back to the full desktop search once.
#[derive(Clone)]
struct FocusHit {
    dest: UniqueName<'static>,
    path: ObjectPath<'static>,
    checked_at: Instant,
}

static FOCUS_CACHE: Mutex<Option<FocusHit>> = Mutex::new(None);

fn cached_focus() -> Option<FocusHit> {
    let guard = FOCUS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    guard.clone()
}

fn store_focus(hit: &FocusHit) {
    let mut guard = FOCUS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(FocusHit {
        dest: hit.dest.clone(),
        path: hit.path.clone(),
        checked_at: Instant::now(),
    });
}

fn clear_focus_cache() {
    let mut guard = FOCUS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

pub(crate) fn read_focused_field(excluded: &[String]) -> FieldRead {
    RUNTIME.block_on(read_focused_field_async(excluded))
}

async fn read_focused_field_async(excluded: &[String]) -> FieldRead {
    let conn = match connection() {
        Ok(c) => c,
        Err(_) => return FieldRead::NoField, // absence logged once at startup
    };

    // Fast path: validate the cached focused object (two small calls).
    // INV-EXCL-001 still holds: snapshot_if_readable resolves process +
    // exclusion BEFORE any value read, cached or not.
    if let Some(cached) = cached_focus() {
        if cached.checked_at.elapsed() < Duration::from_millis(FOCUS_CACHE_TTL_MS) {
            match accessible_proxy(conn, Some(&cached.dest), &cached.path).await {
                Ok(proxy) => {
                    if matches!(proxy.get_state().await, Ok(s) if s.contains(State::Focused)) {
                        if let Some(read) =
                            snapshot_if_readable(conn, Some(&cached.dest), &cached.path, excluded)
                                .await
                        {
                            return read;
                        }
                    } else {
                        clear_focus_cache();
                    }
                }
                Err(_) => clear_focus_cache(),
            }
        }
    }

    // Slow path: find the active frame, then the focused editable under it.
    let root = match conn.root_accessible_on_registry().await {
        Ok(r) => r,
        Err(e) => return FieldRead::Transient(format!("registry root: {e}")),
    };

    for app_ref in root.get_children().await.unwrap_or_default() {
        let Ok(app) = accessible_proxy(conn, app_ref.name(), app_ref.path()).await else {
            continue;
        };
        for frame_ref in app.get_children().await.unwrap_or_default() {
            let Ok(frame) = accessible_proxy(conn, frame_ref.name(), frame_ref.path()).await else {
                continue;
            };
            if !matches!(frame.get_state().await, Ok(s) if s.contains(State::Active)) {
                continue;
            }

            let mut budget = FOCUS_SEARCH_BUDGET;
            match find_focused(conn, &frame, &mut budget).await {
                Some(hit) => {
                    if let Some(read) =
                        snapshot_if_readable(conn, Some(&hit.dest), &hit.path, excluded).await
                    {
                        return read;
                    }
                    return FieldRead::NoField;
                }
                None => {
                    // Active window without a focused editable (browser
                    // chrome, terminal without caret).
                    return FieldRead::NoField;
                }
            }
        }
    }
    FieldRead::NoField
}

/// Build the field snapshot for a known-focused bus object. Ordering here
/// is the security boundary: process identity + exclusion check + password
/// gate (fail CLOSED on role errors) all precede the value read.
/// Returns None when the node exposes no readable text.
async fn snapshot_if_readable(
    conn: &AccessibilityConnection,
    dest: Option<&UniqueName<'static>>,
    path: &ObjectPath<'static>,
    excluded: &[String],
) -> Option<FieldRead> {
    let dest = dest?;

    // Step 1: resolve the PROCESS identity only.
    let pid = match pid_of_object(conn, dest).await {
        Some(pid) => pid,
        // Fail closed (Greptile P1): no identity, nothing may be read.
        None => return Some(FieldRead::Excluded("<unresolved-pid>".into())),
    };
    let process = match process_name_for_pid(pid) {
        Some(name) => name,
        None => return Some(FieldRead::Excluded(format!("pid-{pid}"))),
    };
    if crate::text_monitor::process_excluded(&process, excluded) {
        return Some(FieldRead::Excluded(process));
    }

    let node = accessible_proxy(conn, Some(dest), path).await.ok()?;
    let rect = extents_of(conn, &node)
        .await
        .map(|(x, y, w, h)| (x, y, x + w, y + h));

    // Step 2: password gate BEFORE the value read; errors fail closed.
    let is_password = !matches!(
        node.get_role().await,
        Ok(Role::Entry | Role::Text | Role::SpinButton)
    );
    if is_password {
        // Value intentionally NEVER read. Anything that isn't plainly an
        // editable entry (including PasswordText and unknown/error roles)
        // is treated as a password.
        return Some(FieldRead::Password { process, rect });
    }

    // Step 3: value read.
    let text = text_content(conn, &node).await?;
    let hit = FocusHit {
        dest: dest.clone(),
        path: path.clone(),
        checked_at: Instant::now(),
    };
    store_focus(&hit);
    Some(FieldRead::Text {
        process,
        text,
        rect,
    })
}

/// Depth-limited search for the bus object holding the `Focused` state.
/// Boxed for the same async-recursion reason as `walk_element`.
fn find_focused<'a>(
    conn: &'a AccessibilityConnection,
    root: &'a AccessibleProxy<'static>,
    budget: &'a mut usize,
) -> Pin<Box<dyn Future<Output = Option<FocusHit>> + Send + 'a>> {
    Box::pin(async move {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;

        if matches!(root.get_state().await, Ok(s) if s.contains(State::Focused)) {
            let dest = root.inner().destination().clone();
            let dest = UniqueName::try_from(dest).ok()?;
            return Some(FocusHit {
                dest,
                path: root.inner().path().clone(),
                checked_at: Instant::now(),
            });
        }

        for child_ref in root
            .get_children()
            .await
            .unwrap_or_default()
            .into_iter()
            .take(MAX_SIBLINGS)
        {
            if *budget == 0 {
                return None;
            }
            let Ok(child) = accessible_proxy(conn, child_ref.name(), child_ref.path()).await else {
                continue;
            };
            if let Some(found) = find_focused(conn, &child, budget).await {
                return Some(found);
            }
        }
        None
    })
}

/// Read the node's full text content via the Text interface. Returns None
/// when the node exposes no Text interface or no characters.
async fn text_content(
    conn: &AccessibilityConnection,
    node: &AccessibleProxy<'static>,
) -> Option<String> {
    let text = TextProxy::builder(conn.connection())
        .cache_properties(zbus::proxy::CacheProperties::No)
        .destination(node.inner().destination().clone())
        .ok()?
        .path(node.inner().path().clone())
        .ok()?
        .build()
        .await
        .ok()?;
    // Documented contract: GetText must be called with explicitly known
    // offsets — never -1.
    let count = text.character_count().await.ok()?;
    if count <= 0 {
        return None;
    }
    let content = text.get_text(0, count).await.ok()?;
    if content.is_empty() {
        None
    } else {
        Some(content)
    }
}

/// Currently selected text of the focused element (`GetSelection` slot 0),
/// for selection capture. Password fields are never read (INV-PRIV-001):
/// password or unknown-role nodes yield Ok(None).
pub(crate) fn selected_text_of_focused_element() -> Result<Option<String>, String> {
    RUNTIME.block_on(selected_text_async())
}

async fn selected_text_async() -> Result<Option<String>, String> {
    let conn = connection()?;
    let mut budget = FOCUS_SEARCH_BUDGET;
    let root = conn
        .root_accessible_on_registry()
        .await
        .map_err(|e| format!("registry root: {e}"))?;

    // Selection capture is an explicit user action on whatever currently
    // has focus — search the active frames for the focused node.
    for app_ref in root.get_children().await.unwrap_or_default() {
        let Ok(app) = accessible_proxy(conn, app_ref.name(), app_ref.path()).await else {
            continue;
        };
        for frame_ref in app.get_children().await.unwrap_or_default() {
            let Ok(frame) = accessible_proxy(conn, frame_ref.name(), frame_ref.path()).await else {
                continue;
            };
            if !matches!(frame.get_state().await, Ok(s) if s.contains(State::Active)) {
                continue;
            }
            let Some(hit) = find_focused(conn, &frame, &mut budget).await else {
                continue;
            };
            let node = accessible_proxy(conn, Some(&hit.dest), &hit.path).await?;
            // INV-PRIV-001: fail closed on unknown roles.
            if !matches!(
                node.get_role().await,
                Ok(Role::Entry | Role::Text | Role::SpinButton)
            ) {
                return Ok(None);
            }
            let text = TextProxy::builder(conn.connection())
                .cache_properties(zbus::proxy::CacheProperties::No)
                .destination(node.inner().destination().clone())
                .map_err(|e| e.to_string())?
                .path(node.inner().path().clone())
                .map_err(|e| e.to_string())?
                .build()
                .await
                .map_err(|e| e.to_string())?;
            let (start, end) = match text.get_selection(0).await {
                Ok(sel) if sel.1 > sel.0 => sel,
                _ => return Ok(None), // no selection
            };
            let content = text.get_text(start, end).await.map_err(|e| e.to_string())?;
            return Ok(if content.trim().is_empty() {
                None
            } else {
                Some(content)
            });
        }
    }
    Ok(None)
}
