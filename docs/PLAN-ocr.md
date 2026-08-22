> STALE — describes the WorkBuddy base; authoritative specs live in docs/plans/

# Plan: OCR Text Detection for Accurate Text Pointing

## Problem

OmniParser's YOLO model detects **interactive elements** (buttons, icons,
menus) but not **text content** (headings, labels, paragraphs). When the
user asks WorkBuddy to point at specific text like "WorkBuddy Setup
Tutorial", the LLM falls back to visual estimation, which is consistently
~50px off vertically.

## Solution

Use the `paddle-ocr-rs` crate (v0.6.1) which wraps PaddleOCR ONNX models
via the same `ort` runtime we already use. This handles the full pipeline:
text detection (DBNet) + classification + recognition (SVTR_LCNet), including
all complex post-processing (binarization, contour extraction, polygon
unclipping, coordinate rescaling).

### Why `paddle-ocr-rs` Instead of Building from Scratch

The DBNet detection model outputs a probability heatmap, not bounding boxes.
Converting to boxes requires: thresholding, contour extraction, polygon
fitting, box scoring, Vatti clipping algorithm expansion, and coordinate
rescaling. The `paddle-ocr-rs` crate (1,432 lines of Rust) handles all of
this. Building it from scratch would be the hardest part of the
implementation for no benefit.

### License

PaddleOCR and its models are **Apache 2.0** licensed (not MIT as previously
stated). Apache 2.0 is compatible with AGPL-3.0.

## Architecture

```
Screenshot (RGB buffer)
    |
    ├──► YOLO detection (existing, ~150-400ms)
    |     └── Interactive elements: buttons, icons, menus
    |
    ├──► PaddleOCR via paddle-ocr-rs (NEW, ~100-300ms)
    |     └── Text regions with recognized content
    |
    ├──► Merge both lists, deduplicate overlaps
    |
    └──► Format for LLM:
          "[0] element at (412,520)-(580,560)  center (496,540)"
          "[1] text 'WorkBuddy Setup Tutorial' at (100,200)-(450,230)  center (275,215)"
```

## Implementation

### Cargo.toml

```toml
paddle-ocr-rs = { version = "0.6", default-features = false, features = ["download-binaries"] }
```

This crate depends on `ort ^2.0.0-rc.10` (compatible with our `2.0.0-rc.12`)
and includes all post-processing logic.

### Models

`paddle-ocr-rs` can download models automatically on first use, or we can
bundle specific models. English-optimized models for this use case:

| Model | File | Size |
|-------|------|------|
| Detection (DBNet) | `en_PP-OCRv4_det_infer.onnx` | ~4.5 MB |
| Classification | `ch_ppocr_mobile_v2.0_cls_infer.onnx` | ~0.5 MB |
| Recognition (SVTR_LCNet) | `en_PP-OCRv4_rec_infer.onnx` | ~7.3 MB |
| Dictionary | `en_dict.txt` | <1 KB |
| **Total** | | **~12.3 MB** |

### Code Changes

**`src-tauri/src/ui_detect.rs`** — Add OCR alongside YOLO:

```rust
use paddle_ocr_rs::PaddleOcr;

static OCR_ENGINE: Mutex<Option<PaddleOcr>> = Mutex::new(None);

pub struct TextRegion {
    pub bbox: [f32; 4],
    pub text: String,
    pub confidence: f32,
}

pub fn detect_text(image: &RgbImage) -> Result<Vec<TextRegion>, String> {
    // Initialize PaddleOcr on first call (loads 3 models)
    // Run detection + classification + recognition
    // Return text regions with bounding boxes and content
}
```

**`src-tauri/src/capture.rs`** — Run OCR after YOLO:

```rust
// Existing YOLO detection
let yolo_elements = ui_detect::detect_elements(&rgb_img)?;

// NEW: OCR text detection
let text_regions = ui_detect::detect_text(&rgb_img)?;

// Merge into single detected_elements string
let detected_elements = ui_detect::format_all_detections(&yolo_elements, &text_regions);
```

**`src-tauri/src/ui_detect.rs`** — Updated format function:

```rust
pub fn format_all_detections(
    elements: &[DetectedElement],
    text_regions: &[TextRegion],
) -> String {
    // YOLO elements: "[0] element at (x1,y1)-(x2,y2) center (cx,cy)"
    // OCR regions:   "[N] text 'content' at (x1,y1)-(x2,y2) center (cx,cy)"
    // Deduplicate overlapping YOLO + OCR boxes
}
```

### Settings

The OCR models can be downloaded alongside the YOLO model or separately.
The `paddle-ocr-rs` crate supports automatic model download, but for
consistency with our existing download UI, we can manage it manually.

### Files to Modify

| File | Change |
|------|--------|
| `Cargo.toml` | Add `paddle-ocr-rs` dependency |
| `src-tauri/src/ui_detect.rs` | Add `detect_text()`, `TextRegion`, merged formatting |
| `src-tauri/src/capture.rs` | Run OCR alongside YOLO, combine results |

## Performance

| Component | Latency (CPU) | Model Size |
|-----------|--------------|------------|
| YOLO detection | ~150-400ms | 12 MB |
| OCR detection (DBNet) | ~50-100ms | ~4.5 MB |
| OCR classification | ~5ms | ~0.5 MB |
| OCR recognition | ~15-50ms/crop | ~7.3 MB |
| **Total OCR pipeline** | **~100-300ms** | **~12.3 MB** |

## References

- [paddle-ocr-rs crate](https://crates.io/crates/paddle-ocr-rs) — Apache 2.0
- [PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR) — Apache 2.0
- [PP-OCRv4 models](https://github.com/PaddlePaddle/PaddleOCR/blob/release/2.7/doc/doc_en/models_list_en.md)
