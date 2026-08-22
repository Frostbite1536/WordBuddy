> STALE — describes the WorkBuddy base; authoritative specs live in docs/plans/

# Plan: OmniParser V2 Integration for Pixel-Precise UI Element Detection

## Problem

Claude's vision API provides inaccurate pixel coordinates when pointing at
UI elements. Corners work (trivially derived from image dimensions), but
interior elements like taskbar clocks, buttons, and menus are consistently
"up and to the left" of their actual position. This is a known spatial
estimation limitation of vision LLMs.

## Solution

Integrate Microsoft OmniParser V2's finetuned YOLOv8 model to detect all
interactive UI elements locally in ~150–400ms on CPU (single-digit ms on
GPU), providing pixel-precise bounding boxes. Claude then references
elements by index instead of guessing coordinates.

**Inspiration:** The Clicky project replaced Claude vision coordinate
detection with OmniParser V2 and reported large accuracy and latency
improvements for small UI targets. See the OmniParser references at the
bottom of this document.

## Architecture

```
Screenshot (JPEG)
    |
    ├──► OmniParser YOLO (local, ~150–400ms on CPU)
    |     └── Returns: [{bbox: [x1,y1,x2,y2], confidence: 0.92}, ...]
    |
    ├──► Format as text context:
    |     "Detected UI elements:
    |      [0] element at (412,520)-(580,560)
    |      [1] element at (910,15)-(980,45)
    |      [2] element at (50,1050)-(200,1075)"
    |
    └──► Send to Claude:
          - Screenshot image (existing)
          - Element list text (NEW)
          - User's question
          - Claude sees image + precise coordinates
          - References elements by index: "Click element [0]"
          - Coordinates are pixel-precise from YOLO, not LLM guesses
```

## Approach: YOLO-Only in Rust via ONNX

Use only the YOLOv8 detection model (not the full OmniParser pipeline).
Skip Florence-2 captioning — Claude already understands what elements
ARE from the screenshot; it just needs accurate coordinates.

### Why This Approach

| Factor | Full OmniParser | YOLO-Only (chosen) |
|--------|----------------|-------------------|
| Latency (CPU) | ~5-10s | ~150–400ms |
| Model size | ~1.1 GB | ~40–50 MB |
| Dependencies | Python 3.12 + PyTorch | None (pure Rust) |
| Semantic labels | Yes (Florence-2) | No (Claude does this) |
| RAM usage | 8-16 GB+ | ~200 MB |

### Key Dependencies

- `ort` crate (v2) — Rust bindings for ONNX Runtime
- OmniParser's finetuned YOLOv8s model exported to ONNX (~40–50 MB)
- License: AGPL-3.0 — **requires relicensing this project from
  GPL-3.0 → AGPL-3.0** (see "License Migration" below)

### License Migration (GPL-3.0 → AGPL-3.0)

OmniParser V2's icon detector is a finetune of Ultralytics YOLOv8 and
inherits Ultralytics' AGPL-3.0 license. Linking AGPL-3.0 code into a
GPL-3.0 project is only compatible if the combined work is distributed
under AGPL-3.0. Files that must change as part of this feature:

| File | Change |
|------|--------|
| `LICENSE` | Replace GPL-3.0 text with AGPL-3.0 text |
| `src-tauri/Cargo.toml` | `license = "AGPL-3.0"` |
| `package.json` | `"license": "AGPL-3.0"` (if present) |
| `README.md` | Update license badge + "License" section |
| `docs/ARCHITECTURE.md` | Update the "License: GPL-3.0" reference |
| `CLAUDE.md` | Update the "License: GPL-3.0" line in tech stack |

AGPL-3.0 adds a network-use source-disclosure requirement. For a desktop
app that makes outbound API calls but does not itself serve users over a
network, the practical obligation is the same as GPL-3.0 (source on
request to distributees).

## Implementation Steps

### Phase 1: Model Setup

1. Export OmniParser's `icon_detect/model.pt` (YOLOv8s) to ONNX format
   - Use ultralytics: `model.export(format="onnx", imgsz=640)`
   - Output: `icon_detect.onnx` (~40–50 MB)
   - Store in OS app data dir (downloaded on first use — see Phase 5)

2. Add `ort` crate to `Cargo.toml`:
   ```toml
   ort = { version = "2", features = ["download-binaries"] }
   ```
   `download-binaries` ships a prebuilt ONNX Runtime so users do not need
   to install it separately.

### Phase 2: Detection Module (`src-tauri/src/ui_detect.rs`)

New Rust module:
```rust
/// Detect UI elements in a screenshot using YOLOv8 ONNX model.
/// Returns bounding boxes with confidence scores.
pub struct UIDetector {
    session: ort::Session,
}

impl UIDetector {
    pub fn new(model_path: &Path) -> Result<Self, String>
    pub fn detect(&self, image: &RgbImage) -> Vec<DetectedElement>
}

pub struct DetectedElement {
    pub bbox: [f32; 4],      // [x1, y1, x2, y2] in pixels
    pub confidence: f32,
    pub class: String,        // "icon", "button", "text", etc.
}

/// Format detections as text context for the LLM
pub fn format_detections_for_llm(elements: &[DetectedElement]) -> String
```

### Phase 3: Integration into Capture Pipeline

Modify `capture_to_base64()` in `src-tauri/src/capture.rs`:
1. Capture screenshot (existing, lands as `rgb_img: RgbImage`)
2. Run `UIDetector::detect(&rgb_img)` on the **in-memory RGB buffer**
   **before** JPEG encoding — do not round-trip through JPEG
3. JPEG-encode as today for the LLM vision payload
4. Return `CaptureResult` with a new `detected_elements` field:
   ```rust
   #[derive(Serialize, Clone)]
   pub struct CaptureResult {
       pub base64: String,
       pub width: u32,
       pub height: u32,
       pub detected_elements: Option<String>, // Text-formatted element list
   }
   ```

Detection is gated by the `ui_detection_enabled` setting (see Phase 5);
when disabled, `detected_elements` is `None` and there is no latency
cost. Detection must also respect the 10s capture timeout already in
place — if it would push total time over the budget, skip it and return
`None`.

### Phase 4: LLM Prompt Integration

Add a new `detectedElements?: string` parameter to `buildSystemPrompt`
(`src/lib/curriculum/prompts.ts:122-130`) and thread the value from
`CaptureResult.detected_elements` through `ChatBar.tsx` (already builds
the prompt **after** the capture completes — see
`src/components/ChatBar.tsx:313-321`).

When present, inject:

```
--- DETECTED UI ELEMENTS (pixel-precise coordinates) ---
A local detector has identified these interactive elements in the
screenshot. These coordinates are authoritative. When directing the
student's attention, TRUST this list over your visual estimate: if
you identify an element in the image that corresponds to one of the
detected entries below, use the detected coordinates, NOT the
coordinates you would estimate from looking at the image.

[0] element at (412,520)-(580,560)  center (496,540)
[1] element at (910,15)-(980,45)    center (945,30)
...

If the element you want to point at is NOT in this list, fall back to
your own visual estimate.
```

**Provider coverage.** The existing `point_at` / `highlight` tool
definitions in `src-tauri/src/llm.rs:170-199` are attached **only** for
Anthropic providers (`uses_anthropic_format`). The other five providers
(OpenAI, Google, Groq, Ollama, OpenRouter) use the
`[POINT:x,y:label]` tag fallback emitted via
`src/lib/curriculum/prompts.ts:169`. Both paths benefit from the
detected-element list because it ships in the system prompt, which all
providers receive. Additionally update:

- `point_at` tool description in `llm.rs`: append "If a DETECTED UI
  ELEMENTS list is present in the system prompt, use its coordinates
  verbatim; reference elements by their index."
- The `[POINT:x,y:label]` fallback paragraph in `prompts.ts`: same
  instruction, applies to all non-Anthropic providers.

### Phase 5: Settings UI, Config Field, and Model Download Commands

**AppConfig field** (`src-tauri/src/config.rs`): add first-class field,
following the pattern used for `capture_monitor` in PR #4:
```rust
pub struct AppConfig {
    // ... existing fields
    pub ui_detection_enabled: bool, // default false
}
```
Make sure `set_settings` copies it through (do not route it via
`api_keys`).

**New Tauri commands** (`src-tauri/src/ui_detect.rs` or a dedicated
module):
- `get_ui_model_status() -> UIModelStatus` — reports
  `{ downloaded: bool, path: Option<String>, size_bytes: Option<u64> }`
- `download_ui_model() -> Result<(), String>` — downloads the ONNX model
  from a pinned HuggingFace URL into the OS app data dir, with SHA-256
  integrity verification against a hardcoded hash. Emits progress
  events (`ui_model_download_progress`) so the UI can show a bar.
- `delete_ui_model() -> Result<(), String>` — lets the user reclaim disk
  space.

Per CLAUDE.md rule #12, every new command must be registered in **both**:
1. `invoke_handler![...]` in `src-tauri/src/lib.rs`
2. Permissions list in `src-tauri/capabilities/default.json` (if the
   project's policy requires explicit entries for app commands; current
   custom commands are implicitly allowed, but the capabilities file
   must still be reviewed — any new `core:*` window/event permissions
   triggered by the download progress emitter belong here)

**Settings UI** (`src/pages/Settings.tsx`):
- Toggle: "Local UI detection (OmniParser)" — bound to
  `ui_detection_enabled`
- Status row: "Model: not downloaded" / "Model: 47 MB — ready"
- Button: "Download model (~40–50 MB)" — calls `download_ui_model`,
  shows progress from the emitted events, disables the toggle until the
  download completes
- Button: "Remove model" — calls `delete_ui_model`
- Small help text: "Runs locally, no data leaves your machine. Requires
  ~40–50 MB disk and adds ~150–400ms to each screenshot on CPU."

## Files to Modify

| File | Change |
|------|--------|
| `LICENSE` | Replace GPL-3.0 with AGPL-3.0 text |
| `README.md` | Update license badge + section |
| `CLAUDE.md` | Flip "License: GPL-3.0" → "License: AGPL-3.0" |
| `docs/ARCHITECTURE.md` | Update license reference |
| `src-tauri/Cargo.toml` | `license = "AGPL-3.0"`, add `ort = "2"` dep |
| `src-tauri/src/ui_detect.rs` | NEW — YOLO ONNX inference + element formatting + download commands |
| `src-tauri/src/capture.rs` | Run detection on `rgb_img` before JPEG encode; add `detected_elements` to `CaptureResult` |
| `src-tauri/src/config.rs` | Add `ui_detection_enabled: bool` field |
| `src-tauri/src/lib.rs` | `mod ui_detect;` + register new Tauri commands in `invoke_handler` |
| `src-tauri/capabilities/default.json` | Audit/add entries for any new event permissions (progress emitter) |
| `src/components/ChatBar.tsx` | Pass `detected_elements` into `buildSystemPrompt` |
| `src/lib/curriculum/prompts.ts` | New `detectedElements?: string` param, injection block, fallback-tag instruction update |
| `src/pages/Settings.tsx` | UI detection toggle + model download/remove/status UI |

## Model Distribution

Options:
1. **Download on first use** — App checks for model at startup, downloads
   from HuggingFace if missing. Best for keeping app size small.
2. **Bundle with release** — Include ONNX model in the installer. Adds
   ~40–50 MB to download but works offline immediately.
3. **Hybrid** — Ship without model, offer "Download" button in Settings.

Recommendation: Option 3 (hybrid) — keeps initial download small, user
opts in to the feature. Pin the exact HuggingFace revision SHA and
verify a hardcoded SHA-256 of the ONNX file before writing it to disk,
so a compromised mirror can't swap in arbitrary model weights.

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| ONNX Runtime DLL size (~50 MB) | Use `download-binaries` feature, ships pre-built |
| Model not detecting PM-specific UI elements | Test with Limitless Exchange screenshots; YOLO was trained on general web UI which covers most elements |
| CPU inference too slow on old hardware (500ms+ on low-end laptops) | Make it optional (Settings toggle, default off), skip if detection would blow the existing 10s capture budget |
| AGPL-3.0 is more restrictive than GPL-3.0 | Relicense the project to AGPL-3.0 (see "License Migration"); user has accepted |
| Claude ignores detected list and still guesses coordinates | Prompt explicitly says the detected list is authoritative; add a regression test that feeds a screenshot + known element list and asserts the returned `point_at` coordinates match an entry in the list |
| YOLO detects hundreds of elements on a busy screen, blowing prompt tokens | Cap at top-N by confidence (e.g., top 40) and sort by confidence descending |

## Success Criteria

- UI element detection completes in <500ms on a modern CPU (typical
  150–400ms), <50ms on GPU
- Claude references detected element coordinates instead of guessing
  when the target is in the list
- Clock, taskbar icons, and small UI elements are pointed to accurately
  (center of YOLO bbox lands on the clickable target)
- No freeze or noticeable UI regression while detection runs (it is on
  a blocking task, but still within the existing 10s capture timeout)
- Feature is optional (default off, user must download the model and
  enable the toggle)

## Testing

Follow the pattern of `src-tauri/src/pointer.rs` (which has cargo unit
tests for `[POINT:x,y:label]` parsing):

- **`ui_detect::format_detections_for_llm`** — pure function, trivially
  testable. Assert the exact text block produced for a fixed set of
  `DetectedElement` values, including the top-N cap and confidence
  ordering.
- **NMS / bbox filtering** — if implemented, unit-test against a
  fixture of overlapping boxes.
- **`download_ui_model` SHA verification** — test that a corrupted
  download (wrong hash) is rejected and the partial file is cleaned up.
- **Manual regression**: capture a Limitless Exchange screenshot, turn
  detection on vs off, and compare `point_at` tool_use coordinates
  emitted by Claude for the same question. The on-version should land
  inside the target element's bbox; the off-version is the baseline.

## References

- [Microsoft OmniParser V2](https://github.com/microsoft/OmniParser)
- [OmniParser models on HuggingFace](https://huggingface.co/microsoft/OmniParser-v2.0)
- [ort crate (Rust ONNX Runtime)](https://github.com/pykeio/ort)
- [YOLOv8 ONNX Rust example](https://github.com/AndreyGermanov/yolov8_onnx_rust)
- [AGPL license discussion](https://github.com/microsoft/OmniParser/issues/96)
