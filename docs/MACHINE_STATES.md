> STALE — describes the WorkBuddy base; authoritative specs live in docs/plans/

# WorkBuddy — Machine States

State machines for all stateful subsystems. Each diagram shows valid states,
transitions, and the events/actions that trigger them.

---

## 1. App Navigation

```
                    ┌──────────────┐
        first launch│              │
     ┌──────────────► onboarding   │
     │              │              │
     │              └──────┬───────┘
     │           "Start Learning"
     │                     │
     │              ┌──────▼───────┐
     │              │              │◄──────────── setCurrentPage("chat")
     │              │     chat     │
     │              │              │──────┐──────────────────────┐
     │              └──────────────┘      │                      │
     │                                    │                      │
     │              ┌──────────────┐      │    ┌──────────────┐  │
     │              │              │◄─────┘    │              │◄─┘
     │              │   settings   │           │   history    │
     │              │              │           │              │
     │              └──────────────┘           └──────────────┘
     │              setCurrentPage           setCurrentPage
     │              ("settings")             ("history")
     │
     ├── State: currentPage ∈ { "onboarding", "chat", "settings", "history",
     │                          "cohort_enroll", "cohort_reconsent" }
     └── Stored in: app.context.tsx (React state, not URL)
```

**Rules:**
- Navigation MUST use `setCurrentPage()` — never `window.location.href` (INV-ARCH-002)
- Onboarding shown only when `!isOnboarded` (persisted in localStorage)
- All pages render inside the same Tauri webview — no routing library
- `cohort_enroll` is reached from Settings → "Enroll in a cohort"
  (M1; see `src/pages/CohortEnroll.tsx`)
- `cohort_reconsent` is reached from the Settings stale-policy banner
  (M4; see `src/pages/CohortReConsent.tsx`)

---

## 2. Chat Streaming

```
     ┌─────────┐   handleSubmit()   ┌────────────┐
     │         ├───────────────────►│            │
     │  idle   │                    │ submitting │
     │         │◄──┐                │            │
     └─────────┘   │                └─────┬──────┘
         ▲         │                      │
         │         │          invoke("stream_response")
         │         │                      │
         │         │                ┌─────▼──────┐
         │         │                │            │
         │         │   error ──────►│ streaming  │
         │         │   (catch)      │            │
         │         │                └─────┬──────┘
         │         │                      │
         │         │           chat_stream_complete
         │         │                      │
         │         │                ┌─────▼──────┐
         │         │                │            │
         │         └────────────────┤ finalizing │
         │                          │            │
         │                          └─────┬──────┘
         │                                │
         └────────────────────────────────┘
                    persist to SQLite
```

**State variables:**
- `isStreaming: boolean` — true during streaming + finalizing
- `submittingRef: React.MutableRefObject<boolean>` — guards double-submit
- `streamBufferRef: string` — accumulates chunks

**Key transitions:**
| From | Event | To | Side Effects |
|------|-------|----|-------------|
| idle | handleSubmit() | submitting | Set `isStreaming=true`, add user msg, capture screenshot |
| submitting | invoke succeeds | streaming | Stream chunks arrive via `chat_stream_chunk` events |
| submitting | invoke throws | idle | Set `isStreaming=false`, add error message |
| streaming | `chat_stream_chunk` | streaming | Append to buffer, update "streaming" message in-place. If TTS enabled, pipe through SentenceBuffer |
| streaming | `tool_use_complete` | streaming | Parse tool input, call `show_pointer()` for `point_at`/`highlight` |
| streaming | `chat_stream_complete` | finalizing | Finalize message ID (INV-DATA-003), persist to SQLite, flush SentenceBuffer |
| finalizing | SQLite write done | idle | Set `isStreaming=false`, clear refs |

---

## 3. RAG Document Index

```
     ┌─────────────┐
     │             │
     │  no_index   │  (rag_vectors.db doesn't exist or doc_chunks empty)
     │             │
     └──────┬──────┘
            │
    "Index Documents"
    button click
            │
     ┌──────▼──────┐
     │             │
     │  indexing   │  ingest_all_documents() running
     │             │  (reads .md files, chunks, embeds, stores)
     │             │
     └──────┬──────┘
            │
       success / partial failure
            │
     ┌──────▼──────┐
     │             │
     │   indexed   │  N chunks from M source files
     │             │  search_docs() available
     │             │
     └──────┬──────┘
            │
     "Clear Index"              "Index Documents"
     button click               (re-index)
            │                         │
     ┌──────▼──────┐                  │
     │             │                  │
     │  no_index   ├──────────────────┘
     │             │
     └─────────────┘
```

**State variables:**
- `ragStatus: { total_chunks, source_files, last_ingested }` — from `get_ingestion_status()`
- `indexing: boolean` — true while `ingest_all_documents` is running

**Behavior per state:**
| State | search_docs() | Settings UI |
|-------|--------------|-------------|
| no_index | Returns `[]` (no chunks to search) | Shows "No documents indexed yet" |
| indexing | Returns `[]` (table may be mid-write) | Shows spinner + "Indexing..." |
| indexed | Embeds query, cosine search, returns top-K | Shows chunk count + last indexed date |

**Error states:**
- No OpenAI key → `get_openai_key()` returns error → indexing fails with message
- Network error during embedding → partial index (some files indexed, some not)
- Partial index is usable — search works on whatever chunks were stored

---

## 4. Context Resolution

```
     ┌─────────────────┐
     │ detect_active_   │
     │ window()         │  OS-level window title detection
     │                  │
     └────────┬─────────┘
              │
              ▼
     ┌─────────────────┐   match found    ┌──────────────────┐
     │ match_curriculum │ ───────────────► │ WindowContext     │
     │ _context()       │                  │ { program,        │
     └────────┬─────────┘                  │   module_id,      │
              │                            │   module_title,    │
         no match                          │   tier }           │
              │                            └─────────┬──────────┘
              ▼                                      │
     ┌─────────────────┐                             ▼
     │ WindowContext    │               ┌─────────────────────────┐
     │ { program: null  │               │ resolveModuleContext()  │
     │   module_id: null│               │ (module_map.ts)         │
     │ }                │               └─────────┬───────────────┘
     └────────┬─────────┘                         │
              │                          ┌────────┴────────┐
              ▼                     found│                 │not found
     ┌─────────────────┐                 ▼                 ▼
     │ Tier-level       │    ┌──────────────────┐  ┌──────────────────┐
     │ fallback context │    │ Module-level     │  │ Tier-level       │
     │ (getContext       │    │ context          │  │ fallback         │
     │  Reference)      │    │ (composed from   │  │ (switch on       │
     └─────────────────┘    │  snippet keys)   │  │  program + tier) │
                             └──────────────────┘  └──────────────────┘
```

**Resolution priority:**
1. Per-module snippets via `MODULE_CONTEXT_MAP[program][moduleId]`
2. Tier-level context via `getContextReference(program, tier)` switch
3. Default: `LIMITLESS_PLATFORM + MARKET_MECHANICS`

**RAG layer (additive):**
After static context is resolved, `search_docs()` adds query-specific chunks.
RAG never replaces static context — it supplements it.

---

## 5. Microphone / Push-to-Talk

```
     ┌─────────┐  startRecording()  ┌────────────┐
     │         ├───────────────────►│            │
     │  idle   │                    │ recording  │
     │         │◄──────────────────┤            │
     └─────────┘  stopRecording()   └─────┬──────┘
         ▲                                │
         │                      VAD detects speech
         │                      (or manual stop)
         │                                │
         │                          ┌─────▼──────┐
         │                          │            │
         │                          │transcribing│
         │                          │            │
         │                          └─────┬──────┘
         │                                │
         │                     text inserted into input
         │                     auto-submit triggered
         │                                │
         └────────────────────────────────┘
```

**State variables:**
- `isRecording: boolean` — from `useMicrophone` hook
- Rust-side: `RECORDING: Mutex<bool>`, `STREAM_HANDLE: Mutex<Option<Stream>>`

**Key rules:**
- `stop_mic_capture` MUST drop the `cpal::Stream` (INV-ARCH-007)
- Audio never persists beyond transcription (INV-SEC-005)
- Global shortcut `Ctrl+Space` toggles recording on/off

---

## 6. Cursor Pointing (Full-Screen Overlay Window)

**Trigger sources:**
- Anthropic tool_use: `tool_use_complete` event → `show_pointer()`
- Text tag fallback: `[POINT:x,y:label]` parsed in ResponsePanel → `show_pointer()`

```
     ┌─────────┐  show_pointer()  ┌────────────┐
     │ overlay ├─────────────────►│ spotlight  │  SVG mask dims screen
     │ hidden  │                  │ + cursor   │  Spring physics animates
     │         │◄────────┐        │ showing    │  cursor to target
     └─────────┘         │        └─────┬──────┘
                         │              │
                    3s timeout     next point in queue?
                    or Escape           │
                         │        ┌─────▼──────┐
                         │   no   │  spring    │  yes
                         │◄───────┤  animate   ├──────► showing point N+1
                         │        │  to next   │        (spotlight follows)
                    hide_pointer  └────────────┘
                    (hides window)
```

**State variables:**
- `queueRef: OverlayPoint[]` — queue of points to show
- `posState: { x, y }` — current cursor position (driven by spring physics)
- `springXRef / springYRef: SpringValue` — damped harmonic oscillators
- `spotlightOpacity: number` — SVG overlay opacity (0 → 0.45 on show)
- `trail: Array<{ x, y }>` — ghost positions for motion feedback
- `timerRef: NodeJS.Timeout` — 3s auto-dismiss after last point

---

## 7. Window Visibility

```
     ┌─────────────┐
     │             │ Ctrl+Shift+S
     │  collapsed  │◄──────────────────┐
     │  (54px bar) │                   │
     │             │                   │
     └──────┬──────┘                   │
            │                          │
       user asks                  Ctrl+Shift+S
       question                   or click away
            │                          │
     ┌──────▼──────┐                   │
     │             │───────────────────┘
     │  expanded   │
     │  (600px)    │
     │             │
     └─────────────┘
```

**State:** `isExpanded: boolean` in app.context.tsx
**Window size:** Controlled by `set_window_height(54)` / `set_window_height(600)` Tauri command

---

## 8. Settings Validation

```
     ┌─────────┐  user types key  ┌────────────┐
     │         ├─────────────────►│            │
     │  empty  │                  │   dirty    │
     │         │                  │            │
     └─────────┘                  └─────┬──────┘
                                        │
                                   click "Save"
                                        │
                                  ┌─────▼──────┐
                                  │            │
                                  │ validating │ (Anthropic: API call)
                                  │            │ (Others: save directly)
                                  └──┬──────┬──┘
                                     │      │
                                valid│      │invalid
                                     │      │
                              ┌──────▼┐  ┌──▼──────┐
                              │       │  │         │
                              │ saved │  │  error  │
                              │  ✓    │  │  ✗      │
                              └───────┘  └─────────┘
```

**Applies to:** Provider API key, ElevenLabs key, STT key
**State:** `validationResult: boolean | null`

---

## 9. Streaming Sentence TTS (Provider-Aware)

```
     ┌──────────┐  chat_stream_chunk   ┌──────────────┐
     │          ├─────────────────────►│              │
     │ inactive │  (TTS enabled +      │ accumulating │
     │          │   provider key)      │ (SentenceBuffer)
     └──────────┘                      └──────┬───────┘
         ▲                                    │
         │                          sentence boundary
         │                          detected (.!?\n)
         │                                    │
         │                             ┌──────▼───────┐
         │                             │              │ synthesize_speech(
         │                             │   speaking   │   text, voiceId,
         │                             │   (queued)   │   provider: settings
         │                             │              │     .tts_provider)
         │                             └──────┬───────┘
         │                                    │
         │           chat_stream_complete     │ all sentences played
         │           + flush()                │
         │                                    │
         └────────────────────────────────────┘
                      handleSubmit → reset()
```

**State variables:**
- `sentenceBufferRef: SentenceBuffer` — accumulates text, emits sentences
- `ttsQueueRef: TTSQueue` — manages sequential playback
- `ttsQueueRef.active: boolean` — true when playing or has pending items
- `settings.tts_provider: "elevenlabs" | "gemini"` — determines provider + MIME type
- `settings.tts_voice: string` — voice ID, meaning depends on provider

**Key gate (INV-DATA-005):**
- `hasKey = tts_provider === "gemini" ? !!api_keys?.google : !!api_keys?.elevenlabs`
- Chunks are only pushed to SentenceBuffer when `tts_enabled && hasKey`

**Key transitions:**
| From | Event | To | Side Effects |
|------|-------|----|-------------|
| inactive | chat_stream_chunk (TTS enabled + hasKey) | accumulating | Push chunk to SentenceBuffer |
| accumulating | sentence boundary found | speaking | SentenceBuffer calls onSentence → TTSQueue.enqueue |
| speaking | invoke("synthesize_speech") returns | playing | Audio plays via data:{mimeType};base64,… where mimeType is audio/wav (Gemini) or audio/mpeg (ElevenLabs) |
| speaking | sentence audio ends | accumulating/speaking | TTSQueue processes next, or waits for more text |
| speaking | Gemini returns 500 (attempt N<3) | speaking | Retry after backoff; after 3 failures, silently skip sentence |
| any | handleSubmit (new question) | inactive | TTSQueue.reset(), SentenceBuffer.reset() |
| any | chat_stream_complete | flush | SentenceBuffer.flush() emits remaining text |

**Rate limiting:** Max 1 TTS API call per second (applies to both providers).

**MIME type selection (INV-ARCH-011):**
| Provider | Response | MIME type used in `<audio>` |
|----------|----------|------------------------------|
| ElevenLabs | MP3 bytes | `audio/mpeg` |
| Gemini | Raw PCM wrapped in WAV via `pcm_to_wav` | `audio/wav` |

---

## 10. Tutor Mode

```
     ┌─────────────┐  toggle (ChatBar or Settings)  ┌─────────────┐
     │             ├───────────────────────────────►│             │
     │  normal     │                                │   tutor     │
     │  (Q&A mode) │◄───────────────────────────────┤  (Socratic) │
     │             │  toggle (ChatBar or Settings)  │             │
     └──────┬──────┘                                └──────┬──────┘
            │                                              │
     handleSubmit()                                 handleSubmit()
            │                                              │
     buildSystemPrompt(                             buildSystemPrompt(
       ..., tutorMode=false)                          ..., tutorMode=true)
            │                                              │
            ▼                                              ▼
     Base profile only                              Base profile +
     (direct answers)                               TUTOR_MODE_INSTRUCTIONS
                                                    (Socratic questions,
                                                     interactive guidance,
                                                     aggressive pointing)
```

**State:** `settings.tutor_mode: boolean` — persisted in AppConfig (Rust) and Settings (TypeScript)

**Toggle sources:**
- ChatBar: BookOpen icon button (amber when active)
- Settings: Toggle switch in Tutor Mode section

**Effect on prompt assembly:**
| tutorMode | Prompt Structure |
|-----------|-----------------|
| false | Base profile → reference material → RAG → vision instructions |
| true | Base profile → **TUTOR_MODE_INSTRUCTIONS** → reference material → RAG → vision instructions |

**Key behaviors when tutor mode is active:**
- Never gives direct answers unprompted — asks questions first
- Always ends with a question or action for the student
- Points aggressively at interactive UI elements
- Builds on conversation history
- For code modules: asks student to predict output before revealing
- Progressive difficulty: recall → comprehension → application

---

## 11. Browser Extension Connection

```
     ┌──────────────┐
     │              │  extension installed + token configured
     │ disconnected │
     │              │
     └──────┬───────┘
            │
     POST /scan succeeds (200)
            │
     ┌──────▼───────┐
     │              │  scan received within last 10s
     │  connected   │
     │ (fresh data) │
     └──────┬───────┘
            │
     ┌──────┴──────────────────────────────────┐
     │                                          │
  no scan for 10s                    POST /scan succeeds
     │                                          │
     ┌──────▼───────┐                           │
     │              │                           │
     │    stale     ├───────────────────────────┘
     │ (fallback to │  next scan arrives
     │  YOLO+OCR)   │
     └──────┬───────┘
            │
     extension unloaded / browser closed
     no scans for >30s
            │
     ┌──────▼───────┐
     │              │
     │ disconnected │
     │              │
     └──────────────┘
```

**State variables:**
- `connected: bool` — set true on first successful scan
- `last_scan_ms: u64` — epoch millis of last received scan
- `has_fresh_data()` — `connected && (now - last_scan_ms < 10_000)`

**Key transitions:**
| From | Event | To | Side Effects |
|------|-------|----|-------------|
| disconnected | POST /scan with valid token | connected | Store elements, set last_scan_ms |
| connected | POST /scan | connected | Update elements and timestamp |
| connected | 10s without scan | stale | `has_fresh_data()` returns false → capture falls back to YOLO+OCR |
| stale | POST /scan | connected | Refresh elements, update timestamp |
| any | Extension unloaded | disconnected | No more scans arrive, connected remains true but data goes stale |

**Capture pipeline behavior per state:**
| State | Element Source | Latency |
|-------|---------------|---------|
| connected (fresh) | Extension DOM elements | <10ms |
| stale | YOLO+OCR (if enabled) | 5-8s |
| disconnected | YOLO+OCR (if enabled) | 5-8s |

---

## 12. Extension Element Scanning

```
     ┌─────────┐  page load     ┌────────────┐
     │         ├───────────────►│            │
     │  idle   │                │  scanning  │
     │         │◄──┐            │  (DOM walk)│
     └─────────┘   │            └─────┬──────┘
         ▲         │                  │
         │    3s timer          querySelectorAll()
         │    fires             getBoundingClientRect()
         │         │                  │
         │         │            ┌─────▼──────┐
         │         │            │            │
         │         │            │  sending   │
         │         │            │  (to bg)   │
         │         │            └─────┬──────┘
         │         │                  │
         │         │           sendMessage response
         │         │           (may include highlights)
         │         │                  │
         │         └──────────────────┘
         │              process highlights
         │              if any returned
         │
    page unload → clearInterval
```

**Scanned selectors:**
`button, a, input, select, textarea, [role="button"], [role="link"],
[role="tab"], h1-h6, label, [data-testid]`

**Filter rules:**
- width > 0 AND height > 0
- Partially visible in viewport (not above or below fold)
- Non-empty text content (≤80 chars, whitespace normalized)
- Deduplicated via Set

**Timing:**
- Initial scan: on content script load (page load / SPA navigation)
- Periodic scan: every 3 seconds via `setInterval`
- Guard: `scanActive` flag prevents overlapping scans

---

## 13. Extension Highlight Lifecycle

```
     ┌─────────────┐  extension_highlight()  ┌──────────────┐
     │             ├────────────────────────►│              │
     │  no pending │  (Tauri command)        │   pending    │
     │  highlights │                         │   (queued)   │
     │             │◄──┐                     │              │
     └─────────────┘   │                     └──────┬───────┘
                       │                            │
                  no highlights             GET /highlight
                  in response               or POST /scan response
                       │                            │
                       │                     ┌──────▼───────┐
                       │                     │              │
                       └─────────────────────┤  delivered   │
                                             │  (to ext)    │
                                             └──────┬───────┘
                                                    │
                                          content script injects
                                          CSS overlay into DOM
                                                    │
                                             ┌──────▼───────┐
                                             │              │
                                             │  showing     │
                                             │  (3s timer)  │
                                             │              │
                                             └──────┬───────┘
                                                    │
                                              fade-out (0.3s)
                                              overlay.remove()
                                                    │
                                             ┌──────▼───────┐
                                             │              │
                                             │  dismissed   │
                                             │              │
                                             └──────────────┘
```

**State variables:**
- `pending_highlights: Vec<HighlightCommand>` — server-side queue
- Drained on `GET /highlight` or as piggyback on `POST /scan` response

**Highlight CSS:**
```css
position: fixed; border: 3px solid #10b981; border-radius: 8px;
background: rgba(16, 185, 129, 0.1); z-index: 999999;
pointer-events: none; /* click-through */
```

**Label pill:** Absolute-positioned div above the highlight, dark background
(`#09090b`) with accent text (`#10b981`).

**Timing:**
- Fade-in: 0.2s ease-out animation
- Visible: 3 seconds
- Fade-out: 0.3s opacity transition
- Total lifecycle: ~3.5 seconds

---

## 14. Detection Stack (Capture Pipeline)

WorkBuddy has four possible sources of pixel-precise element data. The
capture pipeline runs them in a strict priority order — the first to produce
usable data wins, and the rest are skipped.

```
     ┌─────────────────┐
     │ capture_to_     │
     │ base64()        │
     └────────┬────────┘
              │
              ▼
   ┌──────────────────────┐
   │ (1) Extension fresh? │  ExtensionState.has_fresh_data()  (last scan <10s)
   │     (127.0.0.1:19521)│
   └──────┬───────────┬───┘
          │           │
       yes│           │no
          │           ▼
          │  ┌──────────────────────┐
          │  │ (2) a11y enabled +   │  config.a11y_detection_enabled
          │  │     returns ≥5 els   │  within 800ms timeout?
          │  └──────┬───────────┬───┘
          │         │           │
          │      yes│           │no
          │         │           ▼
          │         │  ┌──────────────────────┐
          │         │  │ (3) ui_detection_    │  config.ui_detection_enabled
          │         │  │     enabled?         │  YOLO+OCR (150-400ms / 2-8s)
          │         │  └──────┬───────────┬───┘
          │         │         │           │
          │         │      yes│           │no
          │         │         │           ▼
          │         │         │    ┌─────────────┐
          │         │         │    │ (4) No      │  detected_elements = None
          │         │         │    │ detection   │  LLM estimates from screenshot
          │         │         │    └─────────────┘
          │         │         │
          ▼         ▼         ▼
   ┌──────────────────────────────┐
   │ Format as prompt text,       │  Each source uses a distinct header:
   │ inject into system prompt    │    "DETECTED PAGE ELEMENTS" (extension)
   └──────────────────────────────┘    "DETECTED UI ELEMENTS (accessibility)"
                                       "DETECTED UI ELEMENTS AND TEXT" (YOLO+OCR)
```

**State sources:**
- (1) Extension: `ExtensionState` mutex populated by the HTTP server
- (2) Accessibility: `a11y::get_foreground_elements()` dispatched to platform impl
- (3) OmniParser: `ui_detect::detect_elements()` + `detect_text()`

**Gating rules:**
| Source | Gate | Timeout | Min result size |
|--------|------|---------|-----------------|
| Extension | `has_fresh_data()` (scan within 10s) | N/A (cached) | — |
| Accessibility | `a11y_detection_enabled` config flag | 800ms | ≥5 elements |
| YOLO+OCR | `ui_detection_enabled` config flag, model downloaded | 30s | — |

**Why the min-size gate on a11y:** Some apps have stubbed-out accessibility
trees (fullscreen games, non-a11y apps). If UIA returns <5 elements, the
pipeline falls through to YOLO+OCR rather than feeding sparse data to the LLM.

**Coordinate reconciliation (INV-ARCH-013):**
Accessibility returns absolute screen coords (primary monitor origin).
Extension returns viewport coords. YOLO+OCR returns screenshot-relative coords.
All are normalized to screenshot-relative coords before prompt injection:
- a11y: subtract `(monitor.x(), monitor.y())` from element bounding rects
- extension: coords are already in the page's viewport (no conversion; data
  is labelled as "page elements" so the LLM treats them as relative to the
  browser viewport that's visible in the screenshot)
- YOLO+OCR: already screenshot-relative by construction

---

## 15. Speech-to-Text Dispatch

```
     ┌─────────┐  transcribe_audio(base64_wav)  ┌─────────────┐
     │ stop_mic├───────────────────────────────►│  dispatch   │
     │ _capture│                                │  on config. │
     └─────────┘                                │stt_provider │
                                                └──────┬──────┘
                                                       │
                      ┌────────────┬───────────────────┼────────────────┐
                      │            │                   │                │
                   "whisper"   "elevenlabs"       "gemini"         (default)
                      │            │                   │                │
                      ▼            ▼                   ▼                ▼
              ┌────────────┐┌────────────┐   ┌────────────────┐ (treated as whisper)
              │ OpenAI     ││ElevenLabs  │   │ Gemini 2.5     │
              │ /audio/    ││/speech-to- │   │ Flash          │
              │ transcrip- ││text        │   │ generateContent│
              │ tions      ││(scribe_v1) │   │ inlineData WAV │
              └──────┬─────┘└──────┬─────┘   └──────┬─────────┘
                     │             │                │
                     │             │          strip_transcript
                     │             │          _artifacts()
                     │             │                │
                     └─────────────┴────────────────┘
                                   │
                                   ▼
                      returns transcribed text string
```

**State:**
- `config.stt_provider: "whisper" | "elevenlabs" | "gemini"` — default `"whisper"`
- Each provider pulls its own API key:
  - whisper → `openai` key (or legacy `stt` key)
  - elevenlabs → `elevenlabs` key (needs "Speech to Text" permission)
  - gemini → `google` key (reused from Gemini LLM/TTS)

**Provider-specific behaviors:**
| Provider | Format sent | On silence / blocked | Size limit |
|----------|-------------|----------------------|------------|
| Whisper | multipart file upload | Returns empty text | 25 MB |
| ElevenLabs | multipart file upload | Returns empty text | Varies |
| Gemini | inlineData (base64 WAV in JSON) | Returns empty string | 10 MB base64 cap |

**Artifact stripping (Gemini only):**
Despite strict prompting, Gemini sometimes prefixes responses with
"Transcript:" or wraps in quotes. `strip_transcript_artifacts()` removes
`"Transcript:"` / `"Transcription:"` prefixes and wrapping double quotes.
Internal quotes (e.g., `she said "hi"`) are preserved.

---

## 16. Cohort Telemetry Queue Row

```
                ┌─────────────────────────────────────┐
                │                                      │
                │       (collector.enqueue)            │
                │       INV-TEL-001 + 006 gates        │
                ▼                                      │
         ┌─────────────┐                              │
         │             │   uploader.tick()            │
         │   pending   │  +backoff schedule           │
         │             │  (in-memory nextRetryAt)     │
         └──────┬──────┘                              │
                │                                      │
       success  │  fail (network / 401 / 5xx / 429)   │
                │  recordFailure → attempt_count++    │
                │  scheduleBackoff (2s,8s,32s,2m,10m) │
                ▼                                      │
         ┌─────────────┐    attempt_count >= 5        │
         │  uploaded   │   ┌──────────────────────┐  │
         │             │   │  paused-till-relaunch│  │
         └──────┬──────┘   │  (in-memory only;    │  │
                │           │  next launch retries │  │
        sweepRetention()    │  with attempt_count  │  │
        uploaded_at         │  unchanged)          │  │
        < now - 30d          └──────────────────────┘  │
                ▼                                      │
         ┌─────────────┐                              │
         │   purged    │                              │
         │  (deleted)  │                              │
         └─────────────┘                              │
                                                       │
       fail (preflight INV-TEL-002/003,               │
             TLS gate INV-TEL-007,                    │
             stale consent INV-TEL-013,               │
             4xx-non-401, malformed JSON)             │
                ▼                                      │
         ┌──────────────────┐                         │
         │ parked-permanent │  attempt_count = 999    │
         │                  │  (DB persisted; will be │
         │                  │  short-circuited on     │
         │                  │  every future tick)     │
         └──────────────────┘                         │
                ▼                                      │
         retention sweep eventually purges             │
         (cohort ends_at + 90d)                        │
```

**Rules (M2 / M4):**
- A row enters `pending` only via `queue.enqueue()`, which checks
  `requireActiveConsent` (INV-TEL-001) and `hasSweepSucceeded`
  (INV-TEL-006). No code path bypasses this.
- `nextRetryAt` is in-memory per-row state; cleared on every app
  launch (which is the "paused till next launch" semantics).
- `attempt_count >= MAX_ATTEMPTS` (5) is the persistent gate so
  permanently-rejected rows do not re-trigger work each launch.
- `parkPermanent()` sets `attempt_count = 999` so even after the
  in-memory state is reset on launch, the DB-side cap holds.
- The retention sweep purges `uploaded` rows older than 30 days,
  `redaction_audit` rows older than 30 days, and EVERY telemetry
  row (queue + audit) for cohorts past `ends_at + 90d`.

---

## 17. Cohort Telemetry Conversation Eligibility

```
       ┌────────────────────────────────────────┐
       │                                         │
       │   conversation row + ≥1 lesson_progress │
       │   OR ≥1 student message                 │
       │                                         │
       └─────────┬──────────────────────────────┘
                 │
                 │  practice_mode_flag.practice = 1?
                 ├─── yes ──► [ineligible — INV-TEL-009]
                 │
                 │  no
                 ▼
        ┌──────────────────────────────────┐
        │ Tier 1 path (collectPending…):   │
        │  one payload per                 │
        │  (session, module) pair          │
        │  not yet in queue                │
        └──────────────┬───────────────────┘
                       │
                       │  filter: cohorts with active Tier 1 consent
                       ▼
                  enqueue Tier1Payload

        ┌──────────────────────────────────┐
        │ Tier 2 path (collectTier2…):     │
        │  idle gate:                       │
        │   MAX(message.timestamp) <        │
        │   now - TIER2_IDLE_MS (5 min)     │
        │  + not already in queue           │
        │  + redactor + tagger gates        │
        │  one payload per session          │
        └──────────────┬───────────────────┘
                       │
                       │  filter: cohorts with active Tier 1 + Tier 2
                       ▼
                  enqueue Tier2Payload
```

**Triggers (App.tsx + app.context.tsx + ChatBar.tsx):**
- session-end (`clearMessages`)
- app-close (X button before `exit(0)`)
- 10-minute fallback (`setInterval`)
- startup pass after sweep

Tier 2 emits ONE payload per session. The 5-minute idle gate means
an active conversation is not collected mid-typing (which would
freeze out future messages from that session). A session that ends
via `clearMessages` is collected on the next tick because it
immediately becomes idle and the LEFT JOIN on
`telemetry_queue.session_id` ensures exactly-once enqueue.

---

## 18. Cohort Consent Receipt

```
                              grantConsent(cohortId, tier)
                                       │
                                       ▼
                              ┌──────────────────┐
                              │   active         │
                              │  withdrawn_at    │
                              │  IS NULL,        │
                              │  policy_version  │
                              │  = current       │
                              └────────┬─────────┘
                                       │
              ┌────────────────────────┼─────────────────────────┐
              │                        │                          │
              │ withdrawConsent()      │ POLICY_VERSION bump      │
              │   appends row with     │   constant changes;       │
              │   withdrawn_at = now;  │   no DB mutation, but     │
              │   if tier=1 cascade    │   activeReceipt() now     │
              │   to tier=2            │   returns null because    │
              │                        │   policy_version mismatch │
              ▼                        ▼                           │
       ┌──────────────┐         ┌──────────────────┐              │
       │  withdrawn   │         │  stale (paused)  │              │
       └──────────────┘         └────────┬─────────┘              │
                                          │                        │
                                          │ reConsent(cohortId,    │
                                          │   tiersToGrant)        │
                                          │  - bumps enrollment    │
                                          │    .policy_version     │
                                          │  - withdraws tiers     │
                                          │    NOT in tiersToGrant │
                                          │  - grants new receipts │
                                          │    for tiersToGrant    │
                                          ▼                        │
                                  back to active ──────────────────┘
```

**Rules:**
- INV-TEL-001: every enqueue path calls `requireActiveConsent`,
  which returns false for both `withdrawn` and `stale (paused)`.
- INV-TEL-008: Tier 2 enqueue + Tier 2 send both also require an
  active Tier 1 receipt (cascade keeps these in sync).
- INV-TEL-013: stale (paused) receipts cause `parkPermanent` at
  the uploader so re-launches don't retry the row against stale
  consent.
