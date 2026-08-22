# WorkBuddy Screen Reader — Browser Extension

A Chrome/Edge extension that reads the current page's DOM and provides
element positions to WorkBuddy. Replaces YOLO+OCR detection for web-based
content with instant, pixel-precise element data.

## Why

OCR-based text detection takes 5-8 seconds per capture and can miss small
buttons. For web content (Limitless Academy, Limitless Exchange, API docs), the
extension reads the DOM directly — element detection drops from 5-8 seconds
to <10ms with perfect accuracy.

## Installation

1. Open `chrome://extensions` in Chrome or Edge
2. Enable **Developer mode** (toggle in top-right)
3. Click **Load unpacked**
4. Select the `workbuddy-extension/` folder
5. Open WorkBuddy Settings → copy the **Auth Token**
6. Click the extension icon in the toolbar → paste the token → click **Save**

The status dot turns green when connected to WorkBuddy.

## How It Works

```
Extension (content.js)         Background (background.js)      WorkBuddy (Rust)
┌────────────────────┐         ┌──────────────────────┐        ┌──────────────────┐
│ Scans DOM every 3s │         │ Relays HTTP requests │        │ HTTP server on   │
│                    ├────────►│                      ├───────►│ 127.0.0.1:19521  │
│ scanVisibleElements│  msg    │ POST /scan           │  HTTP  │                  │
│   → buttons, links │         │ GET /highlight       │        │ Stores elements  │
│   → headings, forms│◄────────┤                      │◄───────┤ Returns highlights│
│                    │  resp   │ Bearer token auth    │  JSON  │                  │
│ highlightElement() │         │                      │        │ Injects into LLM │
│   → CSS overlay    │         │                      │        │ prompt on capture │
└────────────────────┘         └──────────────────────┘        └──────────────────┘
```

### Data Flow: Element Detection

1. Content script scans the DOM for visible interactive elements every 3 seconds
2. Sends the element list to the background service worker via `chrome.runtime.sendMessage`
3. Background worker POSTs to `http://127.0.0.1:19521/scan` with Bearer token auth
4. WorkBuddy stores the elements in `ExtensionState`
5. On next screenshot capture, WorkBuddy uses extension elements instead of YOLO+OCR
6. Element list is injected into the LLM system prompt with viewport coordinates

### Data Flow: Highlighting

1. LLM responds with `point_at` tool call referencing an element index
2. WorkBuddy queues a `HighlightCommand` with the element's viewport rect
3. Extension polls `GET /highlight` every 300ms (via background worker)
4. Content script injects a CSS overlay directly into the page DOM
5. Overlay auto-removes after 3 seconds with fade-out animation

### Why In-Page Highlighting

Converting browser viewport coordinates to screen coordinates is an
**unsolved W3C problem** — `window.screenX/Y` may or may not include the
toolbar, `devicePixelRatio` complicates CSS→physical pixel conversion,
multi-monitor setups can have negative coordinates, and browser zoom changes
the ratio unpredictably.

The extension sidesteps this entirely: it injects a `<div>` with
`position: fixed` at the element's `getBoundingClientRect()` position,
which is guaranteed correct because it's in the same coordinate space.

## Architecture

### Files

| File | Lines | Purpose |
|------|-------|---------|
| `manifest.json` | 29 | MV3 manifest — scoped permissions, no `<all_urls>` |
| `content.js` | 132 | DOM scanner + CSS highlight injection |
| `background.js` | 67 | Service worker — HTTP relay between content script and WorkBuddy |
| `popup.html` | 71 | Connection status + config UI (dark theme) |
| `popup.js` | 57 | Popup controller — save/copy token, check status |

### Rust Server (`src-tauri/src/extension.rs`)

| Component | Purpose |
|-----------|---------|
| `ExtensionState` | Shared state: elements, page info, token, pending highlights |
| `start_extension_server()` | TCP listener on `127.0.0.1:19521` (falls back to 19522, 19523) |
| `read_request()` / `write_response()` | Minimal HTTP/1.1 parser (no framework dependency) |
| `handle_connection()` | Routes: `GET /status`, `POST /scan`, `GET /highlight` |
| `get_extension_status` | Tauri command — exposes status to Settings page |
| `extension_highlight` | Tauri command — queues highlight for extension |
| `regenerate_extension_token` | Tauri command — rotates auth token |

## Security

### Threat: Malicious Page Connects to Localhost

Any website can reach `http://127.0.0.1:19521`. Without auth, a malicious
page could inject fake element data into the LLM prompt.

### Mitigations

| Layer | Mechanism |
|-------|-----------|
| **Authentication** | 256-bit random token in `Authorization: Bearer <token>` header. Token generated on first launch, stored at `%APPDATA%/workbuddy/extension-token`. |
| **Network binding** | Server binds to `127.0.0.1` only — not accessible from the network. |
| **Domain scoping** | Content scripts only run on matched domains (Limitless Exchange, GitHub, localhost). No `<all_urls>`. |
| **CORS preflight** | Server handles OPTIONS and returns `Access-Control-Allow-*` headers. |
| **Token rotation** | Regenerate button in Settings creates a new token instantly. |
| **Data direction** | Extension *pushes* data to WorkBuddy. WorkBuddy never pushes executable content to the extension. |

### Token Management

```
WorkBuddy launch
    │
    ├── Read %APPDATA%/workbuddy/extension-token
    │     exists + ≥32 chars? → use it
    │     missing? → generate 256-bit hex token → write file
    │
    └── Token shown in Settings page → user copies to extension popup
```

The token file is in the same directory as `config.json` (API keys), which
already has restrictive permissions on Unix (`0600`).

## Scoped Domains

The extension only activates on pages matching these patterns:

| Pattern | Purpose |
|---------|---------|
| `*://*.limitless.exchange/*` | Limitless Exchange trading UI |
| `*://*.github.com/*` | API docs, SDK repos |
| `*://localhost/*` | Local development servers |
| `*://127.0.0.1/*` | Local development servers |

To add more domains, edit the `matches` array in `manifest.json`.

## Scanned Elements

The content script scans for these selectors:

```
button, a, input, select, textarea,
[role="button"], [role="link"], [role="tab"],
h1, h2, h3, h4, h5, h6,
label, [data-testid]
```

Each element is included if:
- Width and height are both > 0
- At least partially visible in the viewport
- Has non-empty text content (≤80 characters)

Elements are deduplicated and sent as JSON with tag, text, viewport rect,
type, and href.

## LLM Prompt Format

When extension data is available, the capture pipeline injects this into
the system prompt instead of YOLO+OCR detections:

```
--- DETECTED PAGE ELEMENTS (from browser extension, pixel-precise) ---
[0] button 'Login' at viewport (1768,171) size 50x27
[1] button 'Sign Up' at viewport (1827,167) size 78x33
[2] a 'Markets' at viewport (157,171) size 74x26 href=https://limitless.exchange/markets
[3] h2 'UCL Challenge' at viewport (300,400) size 200x30
[4] button 'Join' at viewport (350,450) size 60x25

When pointing at elements from this list, reference them by index.
For in-browser highlighting, the extension will highlight the element
directly in the page — no screen coordinate conversion needed.
```

## Port Management

| Port | Status |
|------|--------|
| 19521 | Default — tried first |
| 19522 | Fallback if 19521 is busy |
| 19523 | Second fallback |

The active port is written to `%APPDATA%/workbuddy/extension-port` for
extension discovery. The extension reads this file (or the user configures
the port manually in the popup).

## Manifest V3 Considerations

| Concern | How It's Handled |
|---------|-----------------|
| Service worker 30s idle timeout | No persistent connection — request/response HTTP model. Worker wakes on each content script message. |
| No persistent background page | All state in `chrome.storage.local`, not in-memory variables. |
| Content script fetch restrictions | Content scripts delegate HTTP to background worker via `chrome.runtime.sendMessage`. |
| CSP for extension pages | Popup uses inline styles only, no external scripts. |

## Fallback Behavior

When the extension is not installed or disconnected, WorkBuddy falls back
to YOLO+OCR detection seamlessly. The capture pipeline checks extension
freshness before every screenshot:

```rust
if extension.has_fresh_data() {
    // Extension: <10ms, pixel-precise
    use extension.format_elements()
} else if ui_detection_enabled {
    // YOLO + OCR: 5-8s, approximate
    detect_parallel(&rgb_img)
}
```

Students who don't install the extension get the same experience as before.

## Performance Comparison

| Component | Extension | YOLO+OCR |
|-----------|----------|----------|
| Element detection | <10ms | 5-8s |
| Highlighting | <5ms (CSS injection) | ~600ms (spring animation) |
| Total per question | <50ms overhead | 5-8s overhead |
| Model download | None | ~24 MB |
| Accuracy | Pixel-precise (DOM rects) | Approximate (bounding boxes) |

## Cross-Browser Support

| Browser | Status | Notes |
|---------|--------|-------|
| Chrome | Full support | MV3 manifest as-is |
| Edge | Full support | Same Chromium engine |
| Firefox | Not yet | Needs `"background": { "scripts": [...] }` instead of `service_worker` |
| Safari | Not planned | Different extension API entirely |

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Extension shows "Disconnected" | Make sure WorkBuddy is running. Check the port matches (default 19521). |
| "WorkBuddy not running" in popup | WorkBuddy's HTTP server didn't start. Check stderr for `[extension]` log lines. |
| "Invalid token" (401) | Copy a fresh token from WorkBuddy Settings → paste in extension popup → Save. |
| No elements detected | Make sure the current page matches a domain in `manifest.json` matches list. |
| Highlights don't appear | Check that the content script is loaded (extension icon should be active). |
| Port conflict | Another app is using 19521. WorkBuddy tries 19522/19523 automatically. Update the port in the extension popup. |

## Development

```bash
# After modifying extension files, reload in chrome://extensions
# (click the circular arrow on the extension card)

# Watch Rust server logs
# WorkBuddy stderr shows [extension] prefixed messages:
#   [extension] HTTP server listening on 127.0.0.1:19521
#   [extension] scan received — 23 elements

# Test the server directly
curl http://127.0.0.1:19521/status
# → {"connected":false,"version":"1.0.0"}
```

## License

AGPL-3.0 — same as WorkBuddy.
