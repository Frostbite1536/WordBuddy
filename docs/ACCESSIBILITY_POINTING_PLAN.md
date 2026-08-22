# Accessibility-Powered Pointing — Implementation Plan

**Goal:** Replace estimated LLM coordinates with pixel-precise element bounding
boxes from native OS accessibility APIs, dramatically improving cursor overlay
accuracy in IDEs, terminals, Claude Desktop, and web browsers.

**Status:** Planning
**Priority:** High — coordinate accuracy is the #1 user-facing quality issue
**Scope:** Cross-platform (Windows, macOS, Linux)

---

## Problem Statement

The cursor overlay points at the wrong location because the LLM estimates
pixel coordinates from a JPEG screenshot. Even with YOLO icon detection
and OCR text regions, the model's coordinate guesses are often 50-200px off
for elements without exact anchors. This is most noticeable in multi-point
sequences where each subsequent point drifts further.

The current detection stack:
1. **Browser extension** — instant DOM-based detection (<10ms), pixel-precise, but only works in Chrome/Edge on Limitless Exchange pages
2. **OmniParser YOLO** — detects buttons/icons (~150-400ms CPU), but trained on web UI, misses OS/IDE elements
3. **PaddleOCR** — detects text regions (~2-8s), good for labels but no bounding boxes for non-text elements
4. **LLM vision** — estimates coordinates from screenshot, unreliable for precise pointing

**Missing:** A way to get pixel-precise bounding boxes for any visible UI element
in any application — buttons, tabs, panels, input fields, menu items, tree items.

---

## Solution: Native Accessibility APIs

Every major OS provides accessibility APIs that expose the full UI tree of
any application, including element names, roles, and **pixel-precise bounding
rectangles**. Screen readers use these APIs. We can use them for pointing.

### Why Accessibility APIs?

| Property | YOLO/OCR | Accessibility API |
|----------|----------|-------------------|
| Accuracy | ~50-200px estimate | Pixel-precise bounding rect |
| Coverage | Web UI only (YOLO) | Every application with an a11y tree |
| Speed | 150ms-8s (measured) | **Estimated** 20-500ms — verify with real app measurements |
| Element types | Icons, text | Buttons, tabs, menus, inputs, trees, panels |
| Labels | OCR text (noisy) | Exact element names from the app |
| IDE support | Poor | Excellent (when the IDE exposes a tree) |
| Terminal support | None | Good (visible buffer) |
| Privacy | Screenshot leaves app | Metadata only, no screenshots |

**Note on speed numbers:** All performance claims in this plan are estimates
based on typical UIA/AX/AT-SPI behavior. Actual latency must be measured per
platform and per app type before committing to an SLA. Chromium's lazy
accessibility activation on macOS can add a 100-500ms penalty on the first
query (should cache the tree per-window to avoid repeated activation).

### Target Applications (Priority Order)

1. **VS Code / Cursor** — Electron, rich UIA/AX/AT-SPI tree
2. **Claude Desktop** — Electron, same as VS Code
3. **Terminals** (Windows Terminal, iTerm, GNOME Terminal) — good accessibility support
4. **JetBrains IDEs** — Java Swing apps; likely use a Java-to-native a11y bridge, but JetBrains docs don't explicitly cite JAB. Treat as "best effort, test empirically"
5. **Web browsers** (Chrome, Edge, Firefox) — massive trees, need filtering
6. **OS UI** (desktop, taskbar, system tray) — basic but useful

---

## Phase 1: Platform Abstraction Layer

### New File: `src-tauri/src/a11y.rs`

A cross-platform module that provides a unified interface:

```rust
pub struct UIElement {
    pub name: String,           // "Save" button, "Explorer" panel
    pub role: String,           // "Button", "Tab", "TreeItem", "Edit"
    pub bounding_rect: Rect,    // pixel-precise (x, y, width, height)
    pub automation_id: String,  // programmatic ID (e.g., "workbench.action.files.save")
    pub depth: u32,             // tree depth (for filtering)
}

pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Get all visible, interactive UI elements in the foreground window.
/// Filters to control-level elements (no raw text nodes or layout containers).
/// Limits tree depth to avoid enormous browser DOM trees.
pub async fn get_foreground_elements(max_depth: u32) -> Result<Vec<UIElement>, String>

/// Get the focused element only (fastest — single IPC call).
pub async fn get_focused_element() -> Result<Option<UIElement>, String>
```

### Platform Backends

#### Windows — `uiautomation` crate (v0.24.4)

```toml
[target.'cfg(target_os = "windows")'.dependencies]
uiautomation = "0.24"
```

- Uses COM-based UI Automation (`IUIAutomation`, `IUIAutomationElement`)
- Get foreground window via existing `GetForegroundWindow()` in `context.rs`
- `UIAutomation::element_from_handle(handle)` → element tree
- Use **control view walker** (skips raw/layout nodes) with depth limit
- Use **cache request** to batch properties (Name, BoundingRectangle, ControlType, AutomationId) in one cross-process call per element
- **No permissions required** — available to all desktop processes
- Performance: **estimated** 20-200ms for typical IDE window (500-2000 control elements). Actual numbers will vary; measure before setting SLAs.

**Gotchas:**
- `UIAutomation::new()` initializes COM as MTA. If another part of the app later tries STA COM, conflicts can occur. Always call via `tokio::task::spawn_blocking` to isolate thread state.
- `element_from_handle` takes a `uiautomation::types::Handle`, **not** a raw `HWND`. Requires conversion: `Handle::from(hwnd.0 as isize)`.
- UIA returns **physical screen coordinates** across all monitors — see "Coordinate Space Reconciliation" section below.

Key implementation details:
```rust
#[cfg(target_os = "windows")]
pub async fn get_foreground_elements(max_depth: u32) -> Result<Vec<UIElement>, String> {
    use uiautomation::UIAutomation;
    use uiautomation::types::Handle;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // Run on blocking thread to avoid blocking tokio AND to isolate COM MTA init
    tokio::task::spawn_blocking(move || {
        let automation = UIAutomation::new().map_err(|e| e.to_string())?;
        let hwnd = unsafe { GetForegroundWindow() };
        // IMPORTANT: uiautomation expects its own Handle wrapper, not raw HWND.
        // HWND is a wrapper over an isize pointer — convert via isize.
        let handle = Handle::from(hwnd.0 as isize);
        let root = automation.element_from_handle(handle).map_err(|e| e.to_string())?;
        let walker = automation.get_control_view_walker().map_err(|e| e.to_string())?;
        let cache = automation.create_cache_request().map_err(|e| e.to_string())?;
        // Configure cache to batch Name, BoundingRectangle, ControlType, AutomationId
        // (specific cache_request API calls — see uiautomation docs)
        let mut elements = Vec::new();
        walk_tree(&walker, &root, 0, max_depth, &mut elements, &cache)?;
        Ok(elements)
    }).await.map_err(|e| e.to_string())?
}
```

Filter to relevant control types:
- `Button`, `Edit`, `Tab`, `TabItem`, `MenuItem`, `TreeItem`, `ListItem`
- `Hyperlink`, `ComboBox`, `CheckBox`, `RadioButton`, `Slider`
- Skip: `Pane`, `Group`, `Separator`, `Thumb`, `ScrollBar`, `Image`

#### macOS — `accessibility` crate (v0.2.0)

```toml
[target.'cfg(target_os = "macos")'.dependencies]
accessibility = "0.2"
accessibility-sys = "0.2"
```

- Uses AXUIElement API (XPC-based, cross-process)
- `AXUIElement::application(pid)` → `.focused_window()?.children()`
- Properties: `.title()`, `.role()`, `.frame()` (CGRect: x, y, w, h)
- **Requires Accessibility permission** in System Preferences > Privacy & Security
  - `AXIsProcessTrusted()` to check, prompt user in Settings if not granted
- Chromium apps (VS Code, Claude Desktop) **lazy-activate** accessibility — first query triggers tree build (~100-500ms penalty, subsequent queries fast)
- Performance: 50-200ms for typical IDE window

Permission handling:
```rust
#[cfg(target_os = "macos")]
pub fn check_accessibility_permission() -> bool {
    unsafe { accessibility_sys::AXIsProcessTrusted() }
}
```

Add a permission check + prompt in Settings.tsx for macOS users.

#### Linux — `atspi` crate (v0.29.0)

```toml
[target.'cfg(target_os = "linux")'.dependencies]
atspi = "0.29"
```

- Uses D-Bus AT-SPI2 protocol (async, requires tokio)
- `AccessibilityConnection::new()` → registry → enumerate apps
- Properties via `AccessibleProxy`: `.name()`, `.get_role()`, `.get_children()`
- Bounding rects via `ComponentProxy`: `.get_extents(CoordType::Screen)`
- **No permissions required** (any process on session bus)
- Requires `at-spi2-core` daemon running (default on GNOME/KDE)
- GTK/Qt apps work out of the box; Electron needs `ACCESSIBILITY_ENABLED=1`
- Performance: 100-500ms (D-Bus overhead is higher than COM/XPC)

---

## Phase 1.5: Critical Cross-Cutting Concerns

### Coordinate Space Reconciliation

Three existing detection sources emit coordinates in **different spaces**:

| Source | Coordinate Space | Origin |
|--------|------------------|--------|
| Browser extension | Viewport-relative | Top-left of browser viewport |
| YOLO + OCR (capture.rs) | Screenshot pixels | Top-left of captured monitor |
| OS Accessibility APIs | **Physical screen pixels** | Top-left of **primary monitor** (all monitors in one coordinate space on Windows/macOS; per-screen on Linux) |

Adding a11y without normalization will produce wrong coordinates when:
- User captures a **non-primary monitor** (common, tested in `capture.rs:56-84`)
- User has **DPI scaling** ≠ 100% (common on Windows 10+)
- Browser extension data is mixed with a11y data in the same prompt

**Normalization rules (must implement):**

1. **a11y coords → capture-relative:** Subtract the captured monitor's top-left offset.
   - Get monitor offset via `xcap::Monitor::x()`, `y()`
   - a11y element at screen `(1920, 100)` on a 2-monitor setup where monitor 2 starts at x=1920 → capture-relative `(0, 100)`

2. **DPI normalization:** On Windows, UIA returns physical pixels. Screenshots from `xcap` are also physical pixels at device resolution. Verify alignment (should match on a single-monitor 100% DPI system). On mixed-DPI multi-monitor, each monitor's scaling must be applied separately.

3. **Browser extension data must stay separate.** The extension emits viewport coords — never mix with a11y screen coords in the same element list. Pass them as two distinct sections to the LLM.

### Detection Stack Ordering

Current `capture.rs` flow:
```
if browser extension has fresh data → use extension elements (fastest, most accurate)
else → run YOLO + OCR on screenshot
```

New flow with a11y:
```
if browser extension has fresh data → use extension
else if a11y detection enabled AND foreground app is a known-supported target → use a11y
     (VS Code, Claude Desktop, terminals, IDEs — NOT fullscreen games or non-a11y apps)
else → run YOLO + OCR on screenshot
```

**Critical:** a11y is NOT always better than YOLO+OCR. Some apps have stubbed-out a11y trees. Use `context.rs` window title detection to decide whether a11y is worth trying. If a11y returns < 5 elements, fall through to YOLO+OCR.

### Unified Detected Elements Format

Currently **three different formats** compete for the LLM's attention:
- `ui_detect.rs::format_all_detections` — `"--- DETECTED UI ELEMENTS AND TEXT (pixel-precise coordinates) ---"`
- `extension.rs::format_elements` — `"--- DETECTED PAGE ELEMENTS (from browser extension, pixel-precise) ---"`
- This plan's proposed format — `[Button] "Save" center=(480,44) rect=(...)`

**Resolution:** Adopt a single canonical format across all three sources. Each source prepends a header noting its type:

```
--- DETECTED ELEMENTS (pixel-precise, from accessibility API) ---
[Button] "Save" center=(480,44) rect=(450,32,60,24)
[Tab] "Explorer" center=(90,69) rect=(0,52,180,35)
...

--- DETECTED TEXT (pixel-precise, from OCR) ---
[Text] "WorkBuddy" center=(350,14)
...
```

Migrate `ui_detect.rs::format_all_detections` and `extension.rs::format_elements`
to use the same `[Role] "label" center=(x,y)` format at the same time as adding a11y.

### Platform Fallback Compilation

```rust
// src-tauri/src/a11y.rs
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod linux;

pub async fn get_foreground_elements(max_depth: u32) -> Result<Vec<UIElement>, String> {
    #[cfg(target_os = "windows")]
    { windows::get_foreground_elements(max_depth).await }
    #[cfg(target_os = "macos")]
    { macos::get_foreground_elements(max_depth).await }
    #[cfg(target_os = "linux")]
    { linux::get_foreground_elements(max_depth).await }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    { Ok(Vec::new()) }  // graceful no-op on unsupported platforms (BSD, etc.)
}
```

### Tauri Command Registration Checklist (per CLAUDE.md rule #12)

When adding `detect_ui_elements` command:
1. Register in `src-tauri/src/lib.rs` `invoke_handler![...]`
2. Verify `src-tauri/capabilities/default.json` allows custom commands (currently covered by `core:default` — no per-command entry needed, but verify during testing)

---

## Phase 2: Integration with Capture Pipeline

### Modified Flow

Currently in `capture.rs`:
```
screenshot → YOLO detection → OCR detection → format elements → return
```

New flow:
```
screenshot → YOLO detection → OCR detection → a11y detection → merge → return
```

### New Tauri Command: `detect_ui_elements`

Separate from screenshot capture for flexibility:

```rust
#[tauri::command]
pub async fn detect_ui_elements() -> Result<Vec<UIElement>, String> {
    a11y::get_foreground_elements(5).await  // depth limit 5
}
```

### Merging Strategy

When both YOLO/OCR and accessibility data are available, merge with
accessibility taking priority (pixel-precise overrides estimates):

```
DETECTED UI ELEMENTS (pixel-precise accessibility data):
[BUTTON] "Save" at (450, 32, 60, 24)
[TAB] "Explorer" at (0, 52, 180, 35)
[EDIT] "Search files" at (180, 52, 400, 35)
[TREEITEM] "src/App.tsx" at (20, 120, 160, 22)

DETECTED TEXT REGIONS (OCR, less precise):
"WorkBuddy" at (210, 5, 280, 18)
```

### Element Format for LLM

Format accessibility elements as a structured list with center coordinates
(matching the model's point_at schema):

```
DETECTED UI ELEMENTS (use these EXACT coordinates — do NOT estimate):
- [Button] "Save" center=(480,44) rect=(450,32,60,24)
- [Tab] "Explorer" center=(90,69) rect=(0,52,180,35)
- [Edit] "Search files" center=(380,69) rect=(180,52,400,35)
- [TreeItem] "src/App.tsx" center=(100,131) rect=(20,120,160,22)
```

Providing the center coordinates directly means the model can use them as
point_at(x, y) values without any math.

---

## Phase 3: Prompt Engineering Improvements

### Current Problem

The prompt says "use coordinates verbatim" but the model still estimates.
Three specific improvements:

### 3a: Structured Lookup Instruction

Replace the current pointing instruction block with:

```
POINTING RULES (follow in order):
1. SEARCH the DETECTED UI ELEMENTS list below for the target element by name
2. If found → use the center=(x,y) coordinates EXACTLY as given
3. If NOT found → estimate from the screenshot image (less accurate)
4. NEVER estimate coordinates for an element that appears in the detected list
5. When pointing at a detected element, cite its label: point_at(480, 44, "Save")
```

### 3b: Separate Detected Elements from Reference Material

Currently, detected elements are appended at the very end of the prompt
after reference material and RAG context. Move them to a more prominent
position — right before the vision instructions:

```
[Reference material...]
[RAG context...]

=== DETECTED UI ELEMENTS (pixel-precise, from local analysis) ===
- [Button] "Run" center=(520,32) rect=(490,20,60,24)
- [Tab] "Terminal" center=(650,52) rect=(580,40,140,24)
...

[Vision instructions with POINTING RULES...]
```

### 3c: Tool Description Enhancement

Update the point_at tool description to reference detected elements:

```json
{
    "name": "point_at",
    "description": "Point at a UI element. If a DETECTED UI ELEMENTS list exists, you MUST use its center coordinates for any matching element. Only estimate from the image for elements not in the detected list."
}
```

---

## Phase 4: Settings & UX

### New Settings

```typescript
interface Settings {
    // ... existing fields
    a11y_detection_enabled: boolean;  // default: true
}
```

### Settings UI

Add a section in Settings.tsx:

```
[Accessibility icon] UI Element Detection
  Detect interactive elements (buttons, tabs, inputs) in the active window
  for precise pointing. Uses your OS accessibility framework.
  [Toggle: ON/OFF]

  [macOS only] Accessibility Permission: [Granted / Not Granted]
  [Grant Permission button → opens System Preferences]
```

### Privacy Considerations

- Accessibility data stays local — never sent to any API
- Only element names + bounding rects are collected (no text content)
- Data is ephemeral — discarded after each LLM request
- User can disable via toggle
- Add to PRIVACY_POLICY.md: "When UI Element Detection is enabled,
  WorkBuddy reads element names and positions from the active window
  using your OS accessibility framework. This data is used locally to
  improve pointing accuracy and is never transmitted externally."

---

## Phase 5: Electron App Activation

VS Code, Claude Desktop, and other Electron apps **lazy-activate**
their accessibility trees. The tree is empty until an assistive
technology client is detected. We need to trigger activation:

### Windows
- Querying via UIA automatically activates Chromium's accessibility.
  No special handling needed.

### macOS
- Set `AXEnhancedUserInterface` attribute on the application element:
  ```rust
  app.set_attribute("AXEnhancedUserInterface", true)?;
  ```
- Or pass `--force-renderer-accessibility` flag (not practical for
  third-party apps).
- First query penalty: ~100-500ms while tree builds. Cache the tree
  and refresh incrementally.

### Linux
- `ACCESSIBILITY_ENABLED=1` environment variable is a community-reported trick
  that affects Electron apps launched after setting it — **not officially
  documented** by Electron or Chromium. Validate empirically before relying on it.
- Official Electron API: `app.setAccessibilitySupportEnabled(enabled)` (only
  macOS/Windows per Electron docs; Linux not documented).
- GTK/Qt apps activate automatically when a11y daemon is running.
- For third-party apps we don't control, we can't force activation.

---

## Implementation Order

| Step | Files | Effort | Description |
|------|-------|--------|-------------|
| 1 | `src-tauri/Cargo.toml` | S | Add platform-conditional dependencies |
| 2 | `src-tauri/src/a11y.rs` | L | Platform abstraction layer (3 backends) |
| 3 | `src-tauri/src/a11y.rs` | M | Element filtering + depth limiting |
| 4 | `src-tauri/src/lib.rs` | S | Register new Tauri command |
| 5 | `src-tauri/src/capture.rs` | M | Integrate a11y into capture pipeline |
| 6 | `src/lib/curriculum/prompts.ts` | M | Prompt restructuring (phases 3a-3c) |
| 7 | `src-tauri/src/config.rs` | S | Add `a11y_detection_enabled` setting |
| 8 | `src/contexts/app.context.tsx` | S | Add setting to TS interface |
| 9 | `src/pages/Settings.tsx` | M | UI toggle + macOS permission check |
| 10 | `docs/PRIVACY_POLICY.md` | S | Accessibility data disclosure |
| 11 | Testing | L | Test with VS Code, Claude Desktop, terminals on each OS |

**Estimated total:** 2-3 sessions of focused work.

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Chromium lazy activation delay (100-500ms first query) | Slow first pointing | Cache tree per-window, refresh on focus change |
| Enormous browser DOM trees (thousands of nodes) | Slow detection | Depth limit (5), control-view filter, 500ms timeout |
| JetBrains Java Accessibility Bridge slow | 500ms+ for large projects | Fallback to YOLO+OCR, warn in Settings |
| macOS permission not granted | No accessibility data | Graceful fallback to YOLO+OCR, prompt in Settings |
| Linux AT-SPI2 daemon not running | No data on minimal installs | Check on startup, show warning |
| `uiautomation` crate Windows-only types | Won't compile cross-platform | `#[cfg(target_os)]` everywhere |
| Element name language mismatch | Model can't match element to request | Also pass role and automation_id |

---

## Success Criteria

1. Pointing at a VS Code "Save" button lands within 5px of center
2. Pointing at a terminal tab item lands on the correct tab
3. Pointing at a Claude Desktop chat input lands in the input field
4. Multi-point sequences (3+ points) all land accurately
5. Detection adds <300ms to the capture pipeline on all platforms
6. Graceful fallback to YOLO+OCR when accessibility is unavailable

---

## Appendix: Crate Versions & Dependencies

```toml
# Windows only
[target.'cfg(target_os = "windows")'.dependencies]
uiautomation = "0.24"

# macOS only
[target.'cfg(target_os = "macos")'.dependencies]
accessibility = "0.2"
accessibility-sys = "0.2"

# Linux only
[target.'cfg(target_os = "linux")'.dependencies]
atspi = "0.29"
```

No new frontend dependencies. The `windows` crate is already a dependency.
