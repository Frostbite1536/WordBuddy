# Engineering Prompt — WorkBuddy

You are working on WorkBuddy, a cross-platform desktop AI teaching assistant
built with Tauri 2 (Rust backend + React/TypeScript frontend). It supports
6 LLM providers, screen capture, push-to-talk microphone, cursor pointing
overlay, persistent SQLite history, a Chrome/Edge MV3 browser extension,
RAG-backed curriculum retrieval, Teach Me lesson-plan mode, and a stdio
MCP server exposing curriculum context to Claude Code.

Before making changes:
1. Read `CLAUDE.md` for project conventions and "never do" rules
2. Read `docs/INVARIANTS.md` for system rules that must always hold
3. Read `docs/ARCHITECTURE.md` for component design and data flow
4. Read `docs/DECISIONS.md` for recent ADRs (most recent: ADR-030..033)

## Key conventions

- **Rust:** `Result<T, String>` for Tauri commands, shared `HttpClient` state,
  poison-recovering Mutex (`unwrap_or_else(|e| e.into_inner())`)
- **TypeScript:** `useCallback` for handlers, refs for async values,
  `setCurrentPage` for navigation (never `window.location.href`)
- **Event listeners:** cancelled-flag pattern for cleanup (canonical example
  in `ChatBar.tsx` — INV-ARCH-006)
- **SSE parsing:** Anthropic format uses `message_stop`; OpenAI-compatible
  uses `[DONE]` (handled separately in `llm.rs` — never mix protocols).
  Anthropic parser also handles `tool_use` content blocks (`point_at`,
  `highlight` emit `tool_use_complete` events).
- **Stream cancellation:** `STREAM_GENERATION: AtomicU64` in `llm.rs`; each
  new `stream_response` call increments. Old streams check on every chunk
  and abort if superseded.
- **API keys:** individual `set_api_key`, never batch via `set_settings`
  (INV-DATA-001). Frontend merges `api_keys` rather than replaces (INV-DATA-002).
- **Audio:** `microphone.rs` static Mutex for stream handle, always drop on
  stop (INV-ARCH-007). Audio never persisted (INV-SEC-005).
- **Screenshots:** ephemeral only — never stored in SQLite or disk
  (INV-SEC-004). JPEG quality 85 via `image` crate.
- **Database:** screenshots and audio never stored in SQLite; only text
  content + metadata.
- **Tauri 2 capabilities:** every new `#[tauri::command]` requires an entry
  in `src-tauri/capabilities/default.json` (INV-ARCH-003).
- **TTS/STT:** provider is selected via `tts_provider` / `stt_provider`
  config fields; each provider pulls its own API key. Frontend gates + MIME
  type must be provider-aware (INV-DATA-005, INV-ARCH-011). Gemini retry
  honors Retry-After up to 5s + exponential backoff on 5xx without hint
  (ADR-032).
- **Accessibility:** UIA calls must run via `tokio::task::spawn_blocking`
  (INV-ARCH-012). Coordinates from a11y must be reconciled to capture space
  by subtracting the captured monitor's offset (INV-ARCH-013).
- **Detection stack:** extension (DOM, <10ms) → a11y (OS tree, 20-200ms)
  → YOLO+OCR (150ms-8s) → LLM vision estimation. First source with usable
  output wins. Detected elements are ephemeral (INV-SEC-004).
- **Extension security:** HTTP server binds to `127.0.0.1` only (INV-SEC-007),
  uses bearer token in `Authorization` header (INV-SEC-006), password inputs
  are ALWAYS skipped regardless of mask_form_inputs toggle (INV-SEC-008).
  Data freshness enforced via 10-second window (INV-ARCH-010).
- **Curriculum context:** composable snippets via `module_map.ts` +
  `CONTEXT_REGISTRY` (INV-CURR-003). RAG degrades gracefully without
  OpenAI key (INV-CURR-004). PM Academy requires gate check to avoid
  false positives (INV-CURR-001).
- **Title overlap (ADR-030):** when comparing OS window title to extension
  page title, require case-insensitive exact equality OR (separator-gated
  substring with minimum length). Separators: `: — – · • » › | /`.
- **RAG chunker (ADR-031):** fence-level tracking preserves multi-backtick
  code blocks. `tokenize_blocks` walks line-by-line tracking
  `Option<usize>` fence level; close requires backtick count ≥ open count.
- **Extension highlight routing (ADR-033):** `point_at` tool always uses
  cursor overlay; `highlight` tool routes to in-page extension overlay when
  `extension_highlight_enabled` is true AND extension is connected; else
  falls back to cursor overlay.
- **Single Settings panel (INV-ARCH-014):** components inside Settings that
  cache user-entered values (e.g. `SttKeyInput`) resync from props on
  `initial` change — assumes single mounted Settings instance.
- **Persisted settings (INV-DATA-006):** every field in `AppConfig` must be
  copied in `config.rs::set_settings`, mirrored in TS `Settings` interface
  + `defaultSettings` in `app.context.tsx`. Adding a field requires all three.
- **MCP server (Wotch integration):** `workbuddy-mcp` is a separate binary
  crate (`src-tauri/workbuddy-mcp/`). Reads main app's config, RAG DB, and
  bundled curriculum read-only (INV-ARCH-016). Never writes to stdout except
  protocol messages (INV-ARCH-015). See `docs/WOTCH_INTEGRATION.md`.

## Key modules

### Rust (`src-tauri/src/`)
- `lib.rs` — Tauri builder, plugin registration, shared HttpClient state
- `llm.rs` — 6-provider LLM streaming + Anthropic tool_use + stream cancellation
- `capture.rs` — screen capture + detection stack dispatcher
- `extension.rs` — browser extension HTTP server (127.0.0.1:19521-3) + token auth
- `a11y.rs` + `a11y/{windows,macos,linux}_impl.rs` — cross-platform a11y tree
- `config.rs` — API keys + settings with manual `Default` impl (ADR-030-era refactor)
- `context.rs` — active window detection + curriculum module matching + ADR-030 title overlap
- `microphone.rs` — cpal audio capture with VAD
- `stt.rs` — 3-provider STT dispatch (Whisper / ElevenLabs / Gemini)
- `tts.rs` — 2-provider TTS dispatch (ElevenLabs MP3 / Gemini PCM→WAV) + pcm_to_wav
- `pointer.rs` — [POINT:] tag parsing + show_pointer event emission (Tauri command)
- `rag.rs` — RAG document indexing + search (OpenAI embeddings, cosine similarity, SQLite)
- `ui_detect.rs` — OmniParser YOLOv8 + PaddleOCR via ONNX Runtime
- `lesson_plans.rs` — bundled markdown lesson plan loader for Teach Me mode
- `shortcuts.rs` — global keyboard shortcuts
- `window.rs` — window positioning / resize
- `workbuddy-mcp/` — separate binary crate; stdio MCP server for Claude Code

### TypeScript (`src/`)
- `App.tsx` — shell (routing via `setCurrentPage`, not URL)
- `main.tsx` — React entry (detects `window.label` to render main OR cursor_overlay)
- `contexts/app.context.tsx` — global state, settings persistence, updateSettings
- `components/ChatBar.tsx` — input bar + streaming TTS + tool_use_complete routing
- `components/ResponsePanel.tsx` — markdown + TTS listen button + point-tag parsing
- `components/CursorOverlayWindow.tsx` — full-screen spring-physics cursor overlay
- `components/ErrorBoundary.tsx` — React error boundary
- `hooks/useMicrophone.ts` — push-to-talk hook (transcribe + auto-submit)
- `lib/curriculum/prompts.ts` — system prompts, safeSlice UTF-16 safety (ADR)
- `lib/curriculum/context/` — composable snippets + module_map + ui_elements
- `lib/curriculum/lessonProgress.ts` — session-count + checkpoint-marker extraction
- `lib/db.ts` — SQLite CRUD for conversations, messages, lesson_progress
- `lib/pointParser.ts` — [POINT:] parsing (TS port of pointer.rs)
- `lib/sentenceBuffer.ts` — streaming-TTS sentence extraction
- `lib/ttsQueue.ts` — provider-aware sequential playback queue
- `lib/springPhysics.ts` — SpringValue damped harmonic oscillator
- `pages/Settings.tsx` — all toggles (provider, keys, TTS, STT, RAG, extension, a11y, overlay)
- `pages/History.tsx` — SQLite-backed conversation history
- `pages/Onboarding.tsx` — 5-step first-launch wizard

### Browser extension (`workbuddy-extension/`)
- `manifest.json` — MV3, scoped to `*.limitless.exchange` + localhost
- `content.js` — DOM scanner + CSS highlight overlay injection
- `background.js` — HTTP relay (MV3 service worker) to WorkBuddy localhost
- `popup.{html,js}` — connection status + token/port configuration

## Workflow

After making changes, verify:
1. `npx tsc --noEmit` — zero TypeScript errors
2. `cd src-tauri && cargo check` — zero Rust errors (workspace-wide)
3. `cd src-tauri && cargo test` — all unit tests pass (context, rag, pointer,
   stt, tts, ui_detect, extension, a11y)
4. `npx vite build` — frontend builds successfully
5. Review `docs/INVARIANTS.md` — confirm no invariant violations introduced
6. If you added/removed a persisted setting field: confirm `AppConfig`,
   `set_settings` copy, TS `Settings`, and `defaultSettings` all mirror
   (INV-DATA-006).
7. If you added a Tauri command: register in `lib.rs` invoke_handler AND
   `capabilities/default.json` if it needs window-level permissions
   (most commands don't — `core:default` covers them).

## Coordination with Wotch

WorkBuddy and Wotch (github.com/Frostbite1536/Wotch) share a maintainer.
Integration plan: `docs/WOTCH_INTEGRATION.md`. Key contract points:

- WorkBuddy exposes a stdio MCP server (`workbuddy-mcp`) for Claude Code.
- Both apps write to `~/.claude.json` — use atomic temp-file-then-rename
  and preserve other `mcpServers` entries (§4.6.1 of the integration doc).
- Wotch's HTTP API base port is `19519` (fallback through `19528`).
  WorkBuddy's extension range is `19521-19523`. Overlap is real —
  always read port-discovery files (`~/.wotch/api-port`,
  `~/.config/workbuddy/extension-port`), never hardcode.
- Wotch response envelope is always `{ok: bool, data?, error?, code?}`.
  When calling Wotch from WorkBuddy, check `resp["ok"]` and access
  `resp["data"]["<field>"]`.
- Wotch's `POST /v1/tabs/:id/input` takes `{data: "..."}` (NOT `{text: ...}`).

## Do not

- Create `reqwest::Client` per request (INV-ARCH-001)
- Use `window.location.href` (INV-ARCH-002)
- Use `[DONE]` for Anthropic SSE termination (INV-ARCH-004)
- Bare `.unwrap()` on `Mutex::lock()` (INV-ARCH-005)
- Write API keys to logs, error messages, or any endpoint other than the
  designated API (INV-SEC-001)
- Store screenshots, a11y element data, or extension element data in
  SQLite or on disk (INV-SEC-004)
- Hardcode `audio/mpeg` for TTS playback — must check `tts_provider`
  (INV-ARCH-011)
- Call UIA from a plain async context without `spawn_blocking` (INV-ARCH-012)
- Write to stdout in `workbuddy-mcp` except protocol messages (INV-ARCH-015)
- Skip hooks (`--no-verify`) or bypass signing when committing
