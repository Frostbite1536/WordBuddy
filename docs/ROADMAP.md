# WorkBuddy — Roadmap

## Phase 0: Foundation (Complete)

**Goal:** Working Tauri 2 project that compiles on all platforms.

**Included:**
- Tauri 2 project scaffold (Rust + React + TypeScript + Tailwind)
- All source files compile (`cargo check`, `tsc --noEmit`, `vite build`)
- Tauri 2 capabilities file for IPC permissions
- Placeholder icons for all platforms
- .gitignore, CLAUDE.md, project configuration

**Success criteria:** `cargo tauri build` produces a binary.

---

## Phase 1: Core Teaching Loop (Complete)

**Goal:** Type a question → screenshot captured → LLM responds.

**Included:**
- Multi-LLM integration (Anthropic, OpenAI, Google, Groq, Ollama, OpenRouter)
- Provider-specific SSE parsing (Anthropic format vs OpenAI-compatible)
- Screen capture via `xcap` crate (full-screen, cross-platform)
- ChatBar with text input, screenshot button, send button
- ResponsePanel with streaming markdown, code copy buttons
- Settings page with provider selection, API key management, model selector
- Onboarding wizard (5 steps: welcome, API key, program, shortcuts, ready)
- Session-based conversation history
- Shared HTTP client via Tauri managed state

**Success criteria:** Student types a question, sees streaming response with
their screen as context.

---

## Phase 2: Voice I/O (Complete)

**Goal:** Push-to-talk input + spoken responses.

**Included:**
- ElevenLabs TTS integration (Rust backend + frontend Web Audio playback)
- Listen/Stop button on assistant messages
- Push-to-talk microphone capture via cpal crate with VAD
- Voice Activity Detection (RMS energy + peak amplitude thresholds)
- Whisper API speech-to-text for transcription
- Auto-submit transcribed text to chat
- STT settings section (API key, provider info)

**Success criteria:** Student holds mic button → speaks → text appears and
auto-submits. Student clicks Listen → hears response.

---

## Phase 3: Curriculum Awareness (Complete)

**Goal:** Auto-detect which module the student is viewing.

**Included:**
- Active window title detection (Linux/macOS/Windows)
- Curriculum matching for 50+ modules across 3 academies
- 7 system prompt profiles (PM/API/Agents/Lab/Exchange/IDE/Terminal)
- Context badge in ChatBar showing detected module
- PM Academy gate check to prevent false positives

**Success criteria:** ChatBar shows "API Academy — Orders" when Module 03
is open in the browser.

---

## Phase 4: Cursor Pointing (Complete)

**Goal:** Claude points at things on the student's screen.

**Included:**
- `[POINT:x,y:label:screen]` tag parser (Rust with unit tests + TS port)
- Tauri events for show/hide pointer
- CursorOverlay component with animated blue cursor and label pills
- Coordinate mapping from screenshot dimensions to window coordinates
- Auto-dismiss after 3s, Escape to dismiss, sequential point queue
- ResponsePanel strips tags from display and emits pointer events

**Success criteria:** Claude says "Click here [POINT:450,320:Place Order:0]"
and a blue cursor animates to that spot on screen.

---

## Phase 5: Polish & Ship (Complete)

**Goal:** Persistent data, CI/CD, branding, release builds.

**Included:**
- Global keyboard shortcuts (Ctrl+Shift+S/X/F, Ctrl+Space)
- SQLite persistent conversation history (survives app restarts)
- History page with expandable conversations and per-conversation delete
- GitHub Actions CI (3-platform matrix: tsc, vite build, cargo check/test)
- GitHub Actions release workflow (tauri-action, all 3 platforms)
- Auto-updater via tauri-plugin-updater (checks GitHub Releases)
- About section in Settings (version, credits, license, links)
- 5-step onboarding wizard with accessible toggles

**Success criteria:** v0.1.0 release with .dmg, .msi, .AppImage binaries.

---

## Phase 6: Per-Module Context + RAG (Complete)

**Goal:** Every module gets tailored context; dynamic doc retrieval for any question.

**Included:**
- 23 curated topic snippet files from 36 Limitless source documents
- Module map routing all 52 modules across 3 academies to relevant snippets
- Context router: per-module lookup → tier-level fallback
- RAG system: OpenAI text-embedding-3-small for document indexing
- Markdown chunking with header-aware splitting (~500 tokens/chunk)
- In-memory cosine similarity search in Rust (<10ms for ~300 chunks)
- Separate rag_vectors.db via rusqlite (avoids main DB conflicts)
- ChatBar calls search_docs before building system prompt
- Settings page: Document Knowledge Base section (index/clear/status)
- Graceful degradation: RAG disabled if no OpenAI key, static snippets still work

**Success criteria:** Module 03 (Orders) gets EIP-712 + delegated orders context.
Student asks "how do fees work?" and RAG surfaces fee.md chunks alongside static snippets.

---

## Phase 7: Clicky-Inspired Improvements (Complete)

**Goal:** Polished cursor pointing, streaming TTS, and richer context — inspired by Clicky and its web fork.

**Included:**
- Anthropic tool_use API for cursor pointing (`point_at`, `highlight` tools) — replaces text tag parsing for Anthropic provider, fallback kept for others
- Spring-physics cursor animation (`SpringValue` damped harmonic oscillator, stiffness=170, damping=18) — smoother than CSS transitions
- SVG mask spotlight overlay — dims entire screen, bright elliptical cutout at target with pulse ring and radial glow
- Full-screen transparent cursor overlay window (`cursor_overlay`) — separate Tauri window, click-through, covers entire monitor
- Streaming sentence TTS (`SentenceBuffer` + `TTSQueue`) — speak responses sentence-by-sentence during streaming, not after
- UI element context enrichment — per-module UI descriptions for all 52 modules so LLM can reference buttons/forms by name

**Success criteria:** Anthropic responds with tool_use → spotlight dims screen and spring-animated cursor lands on target element. Student hears response spoken sentence-by-sentence as it streams in.

---

## Phase 8: Tutor Mode (Complete)

**Goal:** Socratic teaching mode inspired by Clicky's tutor mode, adapted for structured educational content.

**Included:**
- Tutor mode toggle in ChatBar (BookOpen icon, amber highlight when active)
- Tutor mode toggle in Settings (between Active Program and Auto-Screenshot)
- `TUTOR_MODE_INSTRUCTIONS` prompt block (10-point Socratic teaching directive)
- Composes with all 7 base profiles — injected after base prompt, before reference material
- `tutor_mode: bool` persisted in AppConfig (Rust) and Settings (TypeScript)
- Dynamic placeholder text ("Ready to learn — ask a question or say what you see...")

**Key adaptation from Clicky:** No idle-triggered observation (students are idle while reading).
Instead, tutor mode is student-initiated — the LLM's response style shifts to ask questions,
guide interactive experiments, and build on prior answers.

**Success criteria:** Student toggles tutor mode → asks about a concept → LLM asks a follow-up
question to test understanding rather than giving a direct answer. LLM points aggressively at
interactive elements and creates mini-exercises.

---

## Future Phases

### v0.2 — Enhanced Integration
- Page Agent integration in academy HTML modules
- Meta tags in academy HTML for richer module detection
- Shared curriculum context via local IPC with Wotch

### v0.3 — Lab Cohort Features
- Week progress tracking
- Study statistics dashboard
- Export study summary for coach check-ins

### v0.4 — Privacy & Quality
- Migrate API keys to OS keychain (keyring crate)
- Bundle Google Fonts locally for offline/privacy
- Code signing for release binaries
- Screenshot review before sending (privacy UX)

### v0.5 — Community
- Anonymized Q&A knowledge base (opt-in)
- Proper app icon (graduation cap + cursor motif)

---

## Phase 9: OmniParser V2 + Stream Reliability (Complete)

**Date:** 2026-04-14

**What was built:**
- **OmniParser V2 UI detection** — Local YOLOv8s model via ONNX Runtime
  detects buttons, icons, menus with pixel-precise coordinates (~150-400ms CPU).
  Feature is opt-in: download model (~40-50 MB) and enable in Settings.
- **Stream cancellation** — `STREAM_GENERATION` atomic counter prevents old
  SSE streams from piling up and freezing the UI.
- **Content-based timeout** — Detects when Anthropic sends pings but no text
  for 30s, gracefully ending the response instead of hanging.
- **JPEG screenshots** — 15-30x smaller than PNG, preventing API request bloat.
- **Multi-monitor capture** — Select which monitor to screenshot in Settings.
- **ElevenLabs STT** — Reuse TTS key for speech-to-text, no extra OpenAI key needed.
- **Anti-hallucination prompt** — Conditional vision instructions prevent the LLM
  from fabricating screen content when no screenshot is attached.
- **Voice VAD tuning** — Lowered thresholds, longer silence wait, minimum clip
  length to prevent Whisper hallucinations on short audio.
- **Close button** — `exit(0)` via tauri-plugin-process terminates all windows.
- **License** — GPL-3.0 → AGPL-3.0 (required by OmniParser YOLO weights).

---

## Phase 10: Browser Extension for Instant Element Detection (Complete)

**Date:** 2026-04-14

**Goal:** Replace 5-8s YOLO+OCR detection with instant DOM-based element
detection for web-based content.

**Included:**
- **Chrome/Edge extension** (MV3) — content script scans DOM for buttons,
  links, headings, inputs every 3s. Background service worker relays data
  to WorkBuddy via HTTP.
- **Localhost HTTP server** — Raw TCP server on `127.0.0.1:19521` with
  token-based auth (256-bit hex). Three endpoints: `/status`, `/scan`,
  `/highlight`. Port fallback (19521→19522→19523).
- **Capture pipeline integration** — `capture.rs` checks extension freshness
  (<10s) before running YOLO+OCR. Extension data: <10ms. YOLO+OCR: 5-8s.
- **In-page CSS highlighting** — Extension injects `position: fixed` overlays
  directly into the page DOM. Avoids the unsolvable viewport-to-screen
  coordinate mapping problem. Pixel-perfect, zero conversion.
- **Settings UI** — ExtensionSection shows connection status, element count,
  port, auth token (copy + regenerate).
- **Seamless fallback** — When extension is not installed, YOLO+OCR path
  is completely unchanged. Students without the extension get the same
  experience as before.

**Key design decisions:**
- HTTP request-driven (not WebSocket) — avoids MV3 service worker 30s termination
- Push model — extension pushes to WorkBuddy, not pull
- No new Cargo dependencies — raw `tokio::net::TcpListener` for HTTP server
- Scoped to relevant domains only (no `<all_urls>`)

**Success criteria:** Browser extension connected → student asks a question →
elements detected in <10ms → LLM response references pixel-precise element
positions. Without extension, YOLO+OCR fallback works identically to before.

---

## Phase 11: Gemini Audio + Accessibility Pointing (Complete)

**Date:** 2026-04-16

**Goal:** Reduce API key friction (reuse Google key across LLM/TTS/STT) and
dramatically improve pointing accuracy in IDEs, terminals, and Electron apps
by reading native OS accessibility trees.

**Part A — Gemini STT (3rd provider, ~1 hour):**
- Added `"gemini"` dispatch arm in `stt.rs` using `gemini-2.5-flash` (stable GA,
  not preview) via `generateContent` with `inlineData` (base64 WAV sent directly)
- 10 MB base64 cap (20 MB request total including prompt)
- `strip_transcript_artifacts()` removes `"Transcript:"` / `"Transcription:"`
  prefixes and wrapping double quotes that Gemini sometimes adds
- Handles `promptFeedback.blockReason` (safety block) + empty `candidates` array
  (silent audio returns empty string, not error)
- 4 unit tests covering prefix stripping, quote handling, internal quote
  preservation, empty/whitespace input
- Settings UI: 3rd pill in STT provider selector ("Gemini Flash") + Google
  key hint when selected without a key
- Cost: ~3x cheaper than Whisper for short utterances (32 tokens/sec × $1.00/1M)

**Part B — Gemini 3.1 Flash TTS (2nd provider, 1 session):**
- New `tts_provider` setting (`"elevenlabs"` default, `"gemini"` opt-in)
- `synthesize_gemini()` calls `gemini-2.5-flash-preview-tts` with
  `responseModalities=["AUDIO"]`, returns raw signed 16-bit LE PCM at 24kHz mono
- `pcm_to_wav()` manually wraps raw PCM in a 44-byte WAV header (RIFF/WAVE/fmt/data)
- 3-attempt retry on documented 500-error randomness
- 30 prebuilt voices (Sulafat/Achird/Sadaltager for tutor defaults, full list
  in `list_tts_voices` Tauri command)
- Critical refactor: removed hardcoded `api_keys.elevenlabs` gate from
  `ChatBar.tsx:102` and `ResponsePanel.tsx:245` — gate now checks
  `tts_provider` (INV-DATA-005). Without this, Gemini TTS would silently fail.
- MIME type dispatch: `audio/wav` for Gemini, `audio/mpeg` for ElevenLabs
  (INV-ARCH-011). Frontend checks `tts_provider` before constructing data URI.
- `VoiceSelector` dropdown swaps options when provider changes
- 2 unit tests verifying WAV header (RIFF magic, fmt chunk, 24kHz, byte_rate,
  block_align, total length with and without PCM data)

**Part C — Accessibility-powered pointing (2-3 sessions):**
- New `a11y.rs` module + platform-gated submodules (`a11y/{windows,macos,linux}_impl.rs`)
- **Windows**: `uiautomation 0.24` via `tokio::task::spawn_blocking` (COM MTA
  isolation — INV-ARCH-012). Control view walker with depth-limited traversal,
  filters to 15 interactive roles (Button/Tab/TreeItem/MenuItem/Edit/Link/etc.),
  skips unlabelled elements. Caps at 400 traversed / 200 emitted.
- **macOS**: `accessibility 0.2` / `accessibility-sys 0.2` deps pulled in on
  macOS. Stub implementation (returns empty, gracefully falls through to
  YOLO+OCR) — full AXUIElement walk to be completed on platform-testing pass.
  Includes `is_process_trusted()` helper that checks `AXIsProcessTrusted()`.
- **Linux**: `atspi 0.29` dep pulled in on Linux. Stub implementation — full
  AT-SPI2 D-Bus walk pending platform testing.
- Detection stack ordering in `capture.rs`: extension → a11y (800ms timeout,
  ≥5 element gate) → YOLO+OCR → LLM estimation. First to produce data wins.
- Coordinate reconciliation (INV-ARCH-013): a11y returns absolute screen
  coords (primary monitor origin). `capture.rs` captures `mon_offset` from
  `xcap::Monitor.x()/y()` and passes to `format_elements()` which subtracts
  the offset + skips elements outside the captured monitor's bounds.
- New `detect_ui_elements` Tauri command gated by `a11y_detection_enabled`
  setting (default on).
- Prompt restructure in `prompts.ts`: DETECTED UI ELEMENTS block moved
  **before** vision instructions, added explicit POINTING RULES that tell
  the LLM to search the list first and use coords verbatim before estimating.
- 6 unit tests: center computation, unlabeled filtering, monitor-offset
  reconciliation, out-of-bounds skip, long-name truncation (80 chars +
  ellipsis), 200-element cap with truncated marker.

**Success criteria:**
- A student with only a Google API key can enable Gemini STT, Gemini TTS,
  and Gemini LLM — all reusing the same key, no extra sign-ups.
- Pointing at a "Save" button in VS Code lands within 5px of center
  (previously: 50-200px estimation error).
- Multi-point sequences (3+ points) all land accurately because each uses
  pixel-precise a11y coords, not LLM estimation that drifts per point.
- Detection adds <300ms to the capture pipeline (800ms budget, typically
  20-200ms on Windows UIA).
- Graceful fallback to YOLO+OCR when a11y is unavailable (macOS permission
  not granted, Linux AT-SPI2 daemon off, fullscreen game with no a11y tree).
