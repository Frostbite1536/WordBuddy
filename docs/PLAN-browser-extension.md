# Plan: Browser Extension for Instant Element Detection

## Problem

OCR-based text detection takes 5-8 seconds per capture and can miss small
buttons. For web-based content (PM Academy, Limitless Exchange, API docs),
we can do much better by reading the DOM directly.

## Solution

A Chrome/Edge extension that reads the current page's DOM and provides
element positions to WorkBuddy. Two key design decisions based on the
audit:

1. **Request-driven HTTP** instead of persistent WebSocket — avoids MV3
   service worker lifecycle issues (30s termination kills WebSocket).
2. **In-page highlighting** instead of Tauri overlay — avoids the
   unsolvable viewport-to-screen coordinate mapping problem.

## Architecture

```
WorkBuddy (Tauri)                   Browser Extension
┌──────────────────┐                 ┌─────────────────────┐
│ Rust backend      │   HTTP POST    │ Content Script        │
│                    │◄──────────────│                       │
│ localhost:19521    │   /scan        │ - Reads DOM           │
│ /scan endpoint    │───────────────►│ - Returns element list│
│                    │   JSON result  │                       │
│ Injects elements  │                │ /highlight endpoint   │
│ into LLM prompt   │───────────────►│ - Injects CSS overlay │
│                    │   highlight    │   directly into page  │
└──────────────────┘                 └─────────────────────┘
```

### Why HTTP Instead of WebSocket

| Factor | WebSocket | HTTP Request/Response |
|--------|-----------|----------------------|
| MV3 service worker | Killed after 30s idle; needs keepalive hack | No persistent connection needed |
| Complexity | Connection management, reconnection | Simple POST/response |
| Security | Any page can connect | Same concern, but stateless |
| When data is needed | Continuously? No — only on screenshot | On-demand, matches our use case |

WorkBuddy only needs element data when taking a screenshot. A request/
response model fits perfectly — no persistent connection needed.

### Why In-Page Highlighting Instead of Tauri Overlay

The Tauri cursor overlay uses screen-pixel coordinates from the screenshot.
Converting browser viewport coordinates to screen coordinates is an
**unsolved W3C problem** — there is no reliable API to get the exact
viewport-to-screen offset:

- `window.screenX/Y` may or may not include the toolbar
- `devicePixelRatio` complicates CSS → physical pixel conversion
- Multi-monitor setups can have negative coordinates
- Browser zoom changes the ratio unpredictably

**Solution:** Instead of mapping to screen coordinates, inject the highlight
**directly into the page DOM** as a CSS overlay. The extension creates a
`<div>` with `position: fixed` at the element's `getBoundingClientRect()`
position — which is guaranteed correct because it's in the same coordinate
space as the element itself.

```javascript
// Highlight an element by injecting a CSS overlay into the page
function highlightElement(rect, label) {
  const overlay = document.createElement('div');
  overlay.style.cssText = `
    position: fixed;
    left: ${rect.x}px; top: ${rect.y}px;
    width: ${rect.w}px; height: ${rect.h}px;
    border: 3px solid #10b981;
    border-radius: 8px;
    background: rgba(16, 185, 129, 0.1);
    z-index: 999999;
    pointer-events: none;
    transition: opacity 0.3s;
  `;
  // Label pill
  const pill = document.createElement('div');
  pill.textContent = label;
  pill.style.cssText = `
    position: absolute; top: -28px; left: 0;
    background: #09090b; color: #10b981;
    padding: 2px 8px; border-radius: 4px;
    font-size: 12px; white-space: nowrap;
  `;
  overlay.appendChild(pill);
  document.body.appendChild(overlay);
  setTimeout(() => overlay.remove(), 3000);
}
```

This gives pixel-perfect highlighting with zero coordinate conversion.

## Security

### Authentication (Addresses CVE-2025-52882-style attacks)

Any website can connect to `localhost:19521`. Without authentication, a
malicious page could inject fake element data.

**Mitigation — Token-based auth:**

1. On launch, WorkBuddy generates a random 256-bit token
2. Writes it to `%APPDATA%/workbuddy/extension-token`
3. The extension reads this file via `chrome.runtime.getPackageDirectoryEntry`
   (or the native messaging host provides it)
4. Every HTTP request includes `Authorization: Bearer <token>`
5. The server rejects requests without a valid token

**Additional mitigations:**
- Validate `Origin` header (must be `chrome-extension://<known-id>`)
- Rate limit: max 10 requests/second per connection
- Bind to `127.0.0.1` only (no network access)

## Implementation Steps

### Phase 1: Chrome Extension

```
workbuddy-extension/
├── manifest.json
├── content.js           DOM scanner + highlight injection
├── background.js        Service worker (minimal — just passes messages)
├── popup.html           Connection status UI
├── popup.js
└── icons/
```

**manifest.json:**
```json
{
  "manifest_version": 3,
  "name": "WorkBuddy Screen Reader",
  "version": "1.0.0",
  "description": "Provides page element positions to WorkBuddy",
  "permissions": [],
  "host_permissions": [
    "http://127.0.0.1:19521/*"
  ],
  "content_scripts": [{
    "matches": [
      "*://*.limitless.exchange/*",
      "*://*.github.com/*",
      "*://localhost/*",
      "*://127.0.0.1/*"
    ],
    "js": ["content.js"]
  }],
  "background": {
    "service_worker": "background.js"
  },
  "minimum_chrome_version": "116"
}
```

**Notes on manifest:**
- No `<all_urls>` — scoped to relevant domains only
- No `activeTab` — content scripts handle injection
- `host_permissions` for the localhost HTTP endpoint
- `minimum_chrome_version: 116` for service worker WebSocket keepalive
  (though we use HTTP, this ensures modern API support)
- Edge uses the same manifest (Chromium-based)
- Firefox would need `"background": { "scripts": ["background.js"] }`

**content.js — Core scanning logic:**
```javascript
function scanVisibleElements() {
  const elements = [];
  const selectors = [
    'button', 'a', 'input', 'select', 'textarea',
    '[role="button"]', '[role="link"]', '[role="tab"]',
    'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
    'label', 'nav a', '[data-testid]'
  ].join(', ');

  document.querySelectorAll(selectors).forEach((el) => {
    const rect = el.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return;
    if (rect.bottom < 0 || rect.top > window.innerHeight) return;

    const text = (el.textContent || el.value || el.placeholder || '')
      .trim().slice(0, 80);
    if (!text) return;

    elements.push({
      tag: el.tagName.toLowerCase(),
      text: text,
      rect: {
        x: Math.round(rect.left),
        y: Math.round(rect.top),
        w: Math.round(rect.width),
        h: Math.round(rect.height)
      },
      type: el.type || null,
      href: el.href || null,
    });
  });

  return elements;
}

// Listen for highlight requests from WorkBuddy
window.addEventListener('message', (event) => {
  if (event.data?.type === 'workbuddy-highlight') {
    highlightElement(event.data.rect, event.data.label);
  }
});
```

**Note on coordinates:** `rect.left/top` are viewport-relative CSS pixels.
These are used ONLY for in-page highlighting (`position: fixed`) — NOT
for the Tauri cursor overlay. The element list sent to the LLM uses text
content for matching, not coordinates.

### Phase 2: Localhost HTTP Server in Tauri

**`src-tauri/src/extension.rs`:**

```rust
use tokio::net::TcpListener;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ExtensionState {
    pub elements: Vec<WebElement>,
    pub token: String,
}

pub struct WebElement {
    pub tag: String,
    pub text: String,
    pub rect: ElementRect,
}

/// Start a localhost HTTP server for browser extension communication.
/// Binds to 127.0.0.1 only — not accessible from network.
pub async fn start_extension_server(state: Arc<Mutex<ExtensionState>>) {
    let listener = TcpListener::bind("127.0.0.1:19521").await.unwrap();

    loop {
        let (stream, _) = listener.accept().await.unwrap();
        let state = state.clone();
        tokio::spawn(async move {
            // Parse HTTP request
            // Validate Authorization: Bearer <token>
            // Validate Origin header
            // Handle POST /scan → store elements
            // Handle POST /highlight → send highlight command
        });
    }
}
```

**Dependencies:**
```toml
# Lightweight HTTP server (no need for full framework)
# Or use hyper directly for minimal footprint
hyper = { version = "1", features = ["server", "http1"] }
```

### Phase 3: Integration with Capture Pipeline

When the extension is connected and has fresh data, **prefer extension
elements over YOLO+OCR**:

```rust
// In capture.rs
let detected_elements = if extension_has_data() {
    // Extension data: instant, perfect accuracy
    format_extension_elements(&extension_state.elements)
} else if ui_detection_enabled {
    // YOLO + OCR fallback: 5-8s, approximate
    let (yolo, ocr) = detect_parallel(&rgb_img);
    format_all_detections(&yolo, &ocr)
} else {
    None
};
```

### Phase 4: LLM Prompt Format

Extension elements include semantic information that YOLO+OCR lack:

```
--- DETECTED PAGE ELEMENTS (from browser, pixel-precise) ---
[0] button 'Login' at viewport (1768,171) size 50x27
[1] button 'Sign Up' at viewport (1827,167) size 78x33
[2] link 'Markets' at viewport (157,171) size 74x26
[3] heading 'UCL Challenge' at viewport (300,400) size 200x30
[4] button 'Join' at viewport (350,450) size 60x25

When pointing at elements from this list, reference them by index.
For in-browser highlighting, the extension will highlight the element
directly in the page — no screen coordinate conversion needed.
```

### Phase 5: Highlighting Flow

When the LLM wants to point at a browser element:

1. LLM calls `point_at` with the element index from the extension list
2. WorkBuddy sends a highlight request to the extension via HTTP
3. Extension's content script injects a CSS overlay on the element
4. Overlay auto-removes after 3 seconds

This bypasses the Tauri cursor overlay entirely for web content —
no coordinate mapping, no DPI issues, pixel-perfect highlighting.

For non-web content (when extension is not connected), the existing
YOLO+OCR → Tauri cursor overlay path remains as fallback.

## Settings UI

Add to Settings page:
- Status: "Browser extension: connected" / "not detected"
- Link: "Install extension" → Chrome Web Store URL
- Port: configurable (default 19521)
- Token: show/regenerate button

## Files to Create/Modify

| File | Change |
|------|--------|
| `workbuddy-extension/manifest.json` | NEW — Chrome extension manifest (MV3) |
| `workbuddy-extension/content.js` | NEW — DOM scanner + highlight injection |
| `workbuddy-extension/background.js` | NEW — Minimal service worker |
| `workbuddy-extension/popup.html` | NEW — Connection status |
| `src-tauri/src/extension.rs` | NEW — Localhost HTTP server + auth |
| `src-tauri/src/lib.rs` | Start extension server on launch |
| `src-tauri/src/capture.rs` | Prefer extension data over YOLO+OCR |
| `src-tauri/Cargo.toml` | Add hyper dependency |
| `src/pages/Settings.tsx` | Extension connection status |

## Port Management

- Default: 19521
- On startup, check if port is free; if not, try 19522, 19523, etc.
- Write active port to `%APPDATA%/workbuddy/extension-port`
- Extension reads this file to know which port to connect to
- Configurable in Settings

## Cross-Browser Support

| Browser | Status | Notes |
|---------|--------|-------|
| Chrome | Full support | MV3 manifest as-is |
| Edge | Full support | Same Chromium engine, same manifest |
| Firefox | Partial | Needs `"background": { "scripts": [...] }` instead of `service_worker` |
| Safari | Not planned | Different extension API entirely |

## Performance

| Component | Extension (new) | YOLO+OCR (current) |
|-----------|----------------|-------------------|
| Element detection | <10ms | 5-8s |
| Highlighting | <5ms (CSS injection) | ~600ms (spring animation) |
| Total per question | <50ms | 5-8s |
| Model download | None | ~24 MB |

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| User doesn't install extension | Falls back to YOLO+OCR seamlessly |
| Port conflict | Try multiple ports, write lockfile |
| Malicious page connects to localhost | Auth token + origin validation |
| Extension blocked by corporate policy | YOLO+OCR fallback always available |
| Firefox compatibility | Document as Chrome/Edge only for v1 |
| Page navigates mid-scan | Request-driven; content script re-scans each request |

## References

- [Chrome MV3 content scripts](https://developer.chrome.com/docs/extensions/develop/concepts/content-scripts)
- [Chrome native messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)
- [CVE-2025-52882: localhost WebSocket auth bypass](https://securitylabs.datadoghq.com/articles/claude-mcp-cve-2025-52882/)
- [Chrome MV3 service worker lifecycle](https://developer.chrome.com/docs/extensions/develop/concepts/service-workers/lifecycle)
- [W3C viewport-to-screen coordinate gap](https://github.com/w3c/csswg-drafts/issues/5814)
- [hyper HTTP server](https://hyper.rs/)
