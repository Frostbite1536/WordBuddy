# WorkBuddy — System Invariants

Rules that must always hold. Every code change must preserve these.

---

## Security Invariants

### INV-SEC-001: API keys never leave the local machine except to their target API
- **Rule:** Each API key goes only to its designated endpoint. Anthropic keys
  go to `api.anthropic.com`. OpenAI keys go to `api.openai.com` (for GPT chat,
  Whisper STT, and text-embedding-3-small). ElevenLabs keys go to
  `api.elevenlabs.io` (for TTS and Scribe STT). Google API keys go to
  `generativelanguage.googleapis.com` (for Gemini LLM, Gemini TTS, and
  Gemini STT — all three reuse the same key). Keys are never logged, sent to
  telemetry, or included in error reports.
- **Rationale:** Keys are user-owned credentials. Leaking them creates
  financial liability.
- **Valid:** `client.post(API_URL).header("x-goog-api-key", &api_key)` (Gemini)
  or `.header("Authorization", format!("Bearer {}", key))` (others)
- **Invalid:** `log::info!("Using key: {api_key}")` or sending keys to any
  analytics endpoint
- **Enforcement:** Code review. Grep for `api_key` in log/print statements.

### INV-SEC-002: Config file has restrictive permissions on Unix
- **Rule:** `config.json` (which contains API keys) must be created with
  `mode(0o600)` on Unix systems. Only the owning user can read it.
- **Rationale:** Prevents other users on shared systems from reading keys.
- **Valid:** `OpenOptions::new().mode(0o600).open(&path)`
- **Invalid:** `fs::write(path, data)` (creates with 0644 default)
- **Enforcement:** `config.rs` uses `#[cfg(unix)]` block with explicit mode.

### INV-SEC-003: No third-party analytics; cohort telemetry is opt-in
- **Rule:** WorkBuddy must never send usage data, crash reports, or any
  information to servers other than:
  1. The user-configured LLM / TTS / STT APIs.
  2. HuggingFace and GitHub for model downloads and auto-update checks.
  3. **A user-configured cohort instructor endpoint, gated by per-tier
     opt-in** (see `docs/PLAN-cohort-telemetry/`).
- **Rationale:** AGPL-3.0 educational tool. Students trust it with their
  screen. The cohort feature exists only because instructors need
  curriculum signal; it ships off-by-default, requires the student to
  enroll with an explicit cohort code, and uploads only after explicit
  per-tier consent. The eight `INV-TEL-*` invariants are the
  architectural teeth.
- **Valid:** Direct API calls to configured LLM providers, ElevenLabs,
  OpenAI Whisper, HuggingFace (model download), GitHub (for auto-update
  checks), and a student-configured cohort endpoint after consent.
- **Invalid:** PostHog, Sentry, Google Analytics, any tracking pixel,
  or any cohort upload outside the documented `INV-TEL-*` flow.
- **Enforcement:**
  - Code review. Check Cargo.toml and package.json for analytics deps.
  - INV-TEL-001 / 002 / 003 / 006 / 007 / 008 / 009 / 010 / 011 / 012
    / 013 in `docs/PLAN-cohort-telemetry/INVARIANTS.md`.
  - INV-TEL-012 grep over `src/lib/telemetry/`: vendor names
    (`anthropic|openai|groq|ollama|openrouter`) must not appear there.

### INV-SEC-004: Screenshots never persist beyond the API call
- **Rule:** Captured screenshots exist only as in-memory base64 JPEG strings for
  the duration of the LLM API request. They are not written to disk, not
  stored in SQLite, and not cached between requests. UI detection bounding
  boxes, accessibility element data, and browser extension element data are
  also ephemeral — they exist only as formatted text in the system prompt
  for a single request.
- **Rationale:** Screenshots may contain sensitive information (passwords,
  private messages, financial data). Accessibility and extension element
  names may contain window titles, email subjects, or chat contents.
- **Enforcement:** Verify `capture.rs` returns base64 without file I/O.
  Verify `a11y.rs` returns elements without caching. Verify `db.ts`
  `saveMessage` stores only text content.

### INV-SEC-005: Audio recordings never persist beyond transcription
- **Rule:** Microphone audio captured by `microphone.rs` exists only as
  in-memory samples and base64 WAV strings. Audio is not written to disk,
  not stored in SQLite, and is discarded after transcription completes.
- **Rationale:** Audio may capture ambient conversations, passwords spoken
  aloud, or other sensitive content.
- **Enforcement:** Verify `microphone.rs` uses only in-memory buffers.
  Verify `stt.rs` does not persist the audio parameter.

### INV-SEC-008: Extension scrubs password fields unconditionally
- **Rule:** The browser extension's content script must skip `<input>`
  elements with `type="password"` in every scan. Password values must
  never be placed in the element list sent to WorkBuddy, regardless of
  the `mask_form_inputs` toggle state.
- **Rationale:** Passwords are unconditionally sensitive. The user-facing
  `mask_form_inputs` toggle covers other fields (email, search, generic
  text) but password fields are ALWAYS masked as a hard baseline.
- **Valid:** `if (el.tagName === 'INPUT' && el.type === 'password') return;`
  in `content.js::scanVisibleElements`.
- **Invalid:** Any code path that reads `el.value` on a password input.
- **Enforcement:** Grep `content.js` for the `type === 'password'` skip.
  Manually verify behavior with a login form.

---

## Architecture Invariants

### INV-ARCH-001: Shared HTTP client via Tauri managed state
- **Rule:** All HTTP requests must use the shared `HttpClient` from
  `app.state::<HttpClient>()`. Never create `reqwest::Client::new()` in a
  command handler.
- **Rationale:** Connection pooling, consistent timeouts, single TLS session.
- **Valid:** `let client = &app.state::<HttpClient>().0;`
- **Invalid:** `let client = reqwest::Client::new();`
- **Enforcement:** Grep for `Client::new()` or `Client::builder()` outside
  `lib.rs` setup.

### INV-ARCH-002: Navigation via React state, not URL
- **Rule:** Page transitions use `setCurrentPage()` from app context.
  Never use `window.location.href`, React Router, or URL-based navigation.
- **Rationale:** Tauri renders a single webview. URL changes destroy React
  state (messages, settings, streaming buffer).
- **Valid:** `setCurrentPage("settings")`
- **Invalid:** `window.location.href = "/settings"`
- **Enforcement:** Grep for `window.location` and `react-router`.

### INV-ARCH-003: Tauri 2 capabilities file must exist
- **Rule:** `src-tauri/capabilities/default.json` must grant IPC permissions
  for every `#[tauri::command]` registered in `lib.rs`. Without it, commands
  silently fail with permission denied.
- **Rationale:** Tauri 2 security model requires explicit capability grants.
- **Enforcement:** When adding a new command, verify the capability file
  includes its permission.

### INV-ARCH-004: Provider-specific SSE parsing
- **Rule:** SSE stream parsing must use the correct protocol per provider.
  Anthropic: `message_stop` event type terminates the stream. OpenAI-compatible
  providers: `data: [DONE]` sentinel terminates the stream. These are handled
  by `parse_anthropic_stream` and `parse_openai_stream` in `llm.rs` respectively.
  Anthropic parser also handles `tool_use` content blocks: `content_block_start`
  (type: tool_use), `content_block_delta` (type: input_json_delta), and
  `content_block_stop` → emits `tool_use_complete` event. Tool definitions
  (`point_at`, `highlight`) are only added to Anthropic request bodies.
- **Rationale:** Mixing protocols causes missed stream termination or dead code.
  Tool_use is Anthropic-only; non-Anthropic providers use `[POINT:]` text fallback.
- **Enforcement:** Code review on `llm.rs`. Anthropic path must not check for
  `[DONE]`. OpenAI path must not check for `message_stop`. Tool definitions
  must not be added to OpenAI-format request bodies.

### INV-ARCH-005: Mutex recovery, never panic
- **Rule:** `Mutex::lock()` must use `.unwrap_or_else(|e| e.into_inner())`
  to recover from poisoned mutexes. Never bare `.unwrap()`.
- **Rationale:** A single panic in any config operation permanently poisons
  the mutex, crashing all subsequent operations.
- **Enforcement:** Grep for `.lock().unwrap()`.

### INV-ARCH-006: Event listener cleanup via cancelled-flag pattern
- **Rule:** All `listen()` calls in React components must use the
  cancelled-flag pattern: track `cancelled` boolean and `unlisteners` array,
  check `cancelled` after each async `listen()` resolves, call all
  unlisteners on cleanup.
- **Rationale:** Async `listen()` can resolve after component unmount,
  creating memory leaks and stale handlers.
- **Valid:** See `ChatBar.tsx` streaming listener effect.
- **Invalid:** `listen("event", handler)` without cleanup in useEffect return.
- **Enforcement:** Code review on any file that imports `listen`.

### INV-ARCH-007: Audio resources cleaned up on stop
- **Rule:** `stop_mic_capture` must drop the `cpal::Stream` (via setting
  `STREAM_HANDLE` to `None`) and clear the `RECORDING` state. Audio
  resources must not accumulate across start/stop cycles.
- **Rationale:** Leaked audio streams consume OS resources and may prevent
  other apps from accessing the microphone.
- **Enforcement:** `microphone.rs` `stop_mic_capture_inner()` drops both
  `STREAM_HANDLE` and `RECORDING`.

---

## Data Integrity Invariants

### INV-DATA-001: set_settings excludes API keys
- **Rule:** The `set_settings` Tauri command copies model, program, and UI
  preferences. It intentionally does NOT copy `api_keys`. Keys are managed
  exclusively through `set_api_key`.
- **Rationale:** Prevents accidental key overwrite when updating preferences.
  Separates credential management from settings management.
- **Enforcement:** `config.rs` `set_settings` function comment + code review.

### INV-DATA-002: Frontend merges api_keys, never replaces
- **Rule:** `updateSettings({ api_keys: { anthropic: key } })` must merge
  with existing keys, not replace the entire `api_keys` object.
- **Rationale:** Adding an Anthropic key must not delete an existing
  ElevenLabs key.
- **Valid:** `updated.api_keys = { ...prev.api_keys, ...partial.api_keys }`
- **Invalid:** `updated.api_keys = partial.api_keys`
- **Enforcement:** `app.context.tsx` `updateSettings` implementation.

### INV-DATA-003: Streaming messages finalize with unique ID
- **Rule:** During streaming, the assistant message uses `id: "streaming"`.
  On `chat_stream_complete`, the message must be finalized by replacing the
  ID with `crypto.randomUUID()`. There must never be more than one message
  with `id: "streaming"` at any time.
- **Rationale:** Prevents unbounded array growth from appending duplicate
  streaming messages. Ensures history contains only finalized messages.
- **Enforcement:** `ChatBar.tsx` event listeners.

### INV-DATA-004: Conversations persist to SQLite on stream complete
- **Rule:** When `chat_stream_complete` fires, the current conversation's
  user and assistant messages must be saved to SQLite via `db.ts`. A new
  conversation ID is created on the first message after clearing or on
  fresh start.
- **Rationale:** Conversations must survive app restarts.
- **Valid:** `saveConversation()` + `saveMessage()` in app.context.tsx
  listener.
- **Invalid:** Relying only on in-memory `messages` state array.
- **Enforcement:** `app.context.tsx` `chat_stream_complete` listener.

---

## Curriculum Invariants

### INV-CURR-001: PM Academy requires gate check
- **Rule:** PM Academy module matching must require a gate (`"pm academy"`,
  `"pm_academy"`, or filename pattern like `"01_pm101"`) before matching
  generic module names like "Risk" or "Arbitrage".
- **Rationale:** Without the gate, any window titled "Risk Assessment" would
  false-positive as PM Academy Module 03.
- **Enforcement:** `context.rs` `match_curriculum_context` function.

### INV-CURR-002: Context detection returns empty, never crashes
- **Rule:** `detect_active_window` must always return a valid `WindowContext`,
  even when the underlying OS command fails (xdotool missing, AppleScript
  error, PowerShell blocked). Return empty strings and None fields.
- **Rationale:** Context detection is best-effort. The app must work without it.
- **Enforcement:** All platform branches use `.ok()` / `.unwrap_or_default()`.

### INV-ARCH-008: Streaming TTS must not block the UI
- **Rule:** The `SentenceBuffer` and `TTSQueue` must operate asynchronously.
  Sentence extraction happens synchronously in the `chat_stream_chunk` listener
  but TTS API calls and audio playback are async and queued. The TTSQueue must
  support cancellation (`cancel()`) and the SentenceBuffer must support reset.
  Both must be stored as refs (not state) to avoid stale closures.
- **Rationale:** Blocking TTS calls during streaming would freeze the UI.
  Rate limiting (1 req/sec) prevents overwhelming ElevenLabs API.
- **Enforcement:** `ChatBar.tsx` — `ttsQueueRef` and `sentenceBufferRef` are refs.
  `TTSQueue.cancel()` called on new submit. `SentenceBuffer.reset()` on new submit.

---

## Curriculum Invariants

### INV-CURR-003: Per-module context uses composable snippets
- **Rule:** Module context is resolved via `module_map.ts` → snippet keys →
  `CONTEXT_REGISTRY` → joined string. Never hardcode per-module context inline.
  All 52 modules must have entries in `MODULE_CONTEXT_MAP`.
- **Rationale:** Composable snippets eliminate duplication and are easy to update.
- **Enforcement:** `resolveModuleContext()` in `module_map.ts`. Type check
  ensures all snippet keys exist in registry.

### INV-CURR-004: RAG degrades gracefully without OpenAI key
- **Rule:** If no OpenAI API key is configured, `search_docs` must return an
  empty array (not error). ChatBar catches RAG failures silently and proceeds
  with static snippets only. The system prompt must always be valid without
  RAG context.
- **Rationale:** RAG is optional enrichment, not required for core functionality.
- **Enforcement:** `ChatBar.tsx` wraps `search_docs` in try/catch. `rag.rs`
  `get_openai_key()` returns an error before any API calls.

---

## Extension Invariants

### INV-SEC-006: Extension HTTP server requires token authentication
- **Rule:** All HTTP endpoints except `GET /status` must validate a
  `Authorization: Bearer <token>` header. Requests with missing or invalid
  tokens must receive a 401 response. The token is a 256-bit hex string
  generated on first launch and stored at `%APPDATA%/workbuddy/extension-token`.
- **Rationale:** Any website can connect to `localhost:19521`. Without auth,
  a malicious page could inject fake element data into the LLM prompt,
  causing the assistant to give incorrect guidance (CVE-2025-52882-style attack).
- **Valid:** `if provided != expected { write_response(401, "Unauthorized") }`
- **Invalid:** Accepting POST /scan without checking Authorization header
- **Enforcement:** `extension.rs` `handle_connection()` checks token before
  routing to any authenticated endpoint.

### INV-SEC-007: Extension server binds to localhost only
- **Rule:** The extension HTTP server must bind to `127.0.0.1`, never to
  `0.0.0.0` or any other interface. The server must not be accessible from
  other machines on the network.
- **Rationale:** Binding to all interfaces would expose the extension API
  to the local network, allowing any machine to push element data or
  read highlight commands.
- **Valid:** `TcpListener::bind("127.0.0.1:19521")`
- **Invalid:** `TcpListener::bind("0.0.0.0:19521")`
- **Enforcement:** `extension.rs` `start_extension_server()` hardcodes
  `127.0.0.1` in the bind address.

### INV-ARCH-009: Extension state via Arc<tokio::sync::Mutex>
- **Rule:** The extension state (`ExtensionState`) must be shared between
  the HTTP server task and Tauri commands via `Arc<tokio::sync::Mutex<_>>`.
  The state must be managed via `app.manage()` so Tauri commands can access
  it with `app.state::<T>()` or `app.try_state::<T>()`.
- **Rationale:** The HTTP server runs on a separate tokio task. Using
  `std::sync::Mutex` would block the tokio runtime. Using `tokio::sync::Mutex`
  allows async `.lock().await` without blocking.
- **Valid:** `app.try_state::<Arc<tokio::sync::Mutex<ExtensionState>>>()`
- **Invalid:** `static EXT_STATE: Mutex<Option<ExtensionState>>` (blocks tokio)
- **Enforcement:** `lib.rs` manages the state; `capture.rs` and `extension.rs`
  commands access it via app handle.

### INV-ARCH-010: Extension data freshness threshold
- **Rule:** Extension element data is considered "fresh" if the last scan
  was received within 10 seconds. The capture pipeline must check freshness
  via `has_fresh_data()` before using extension elements. Stale data must
  fall back to the next stack (a11y, then YOLO+OCR).
- **Rationale:** If the browser is closed or the extension disconnects,
  stale element data would describe a page that's no longer visible. The
  10-second threshold ensures data reflects the current screen state.
- **Valid:** `if lock.has_fresh_data() { Some(lock.format_elements()) }`
- **Invalid:** Using `lock.elements` without checking `last_scan_ms`
- **Enforcement:** `capture.rs` calls `has_fresh_data()` before using
  extension elements.

### INV-ARCH-011: TTS MIME type must match provider
- **Rule:** Playback code must select the audio MIME type based on the
  configured `tts_provider`: `audio/mpeg` for ElevenLabs (MP3), `audio/wav`
  for Gemini (raw PCM wrapped in a 44-byte WAV header by `tts.rs::pcm_to_wav`).
  Using the wrong MIME type silently fails — the `<audio>` element refuses
  to decode mismatched data.
- **Rationale:** Gemini's `generateContent` with `responseModalities=["AUDIO"]`
  returns raw signed 16-bit LE PCM at 24kHz mono. The browser cannot play
  raw PCM directly. The Rust backend wraps it in a WAV container; the
  frontend must tell the `<audio>` tag it's WAV.
- **Valid:** `const mimeType = provider === "gemini" ? "audio/wav" : "audio/mpeg";
  new Audio(`data:${mimeType};base64,${base64}`)`
- **Invalid:** `new Audio(`data:audio/mpeg;base64,${base64}`)` without
  checking provider.
- **Enforcement:** `ttsQueue.ts::processNext` and `ResponsePanel.tsx::handleListen`
  both switch on `provider` before constructing the data URI.

### INV-ARCH-012: UI Automation must run on a blocking thread
- **Rule:** All calls into the `uiautomation` crate on Windows must be
  wrapped in `tokio::task::spawn_blocking`. The `UIAutomation::new()`
  call initializes COM in MTA mode; running it directly on a tokio worker
  thread contaminates the runtime with COM state and can deadlock with
  future STA calls.
- **Rationale:** COM has strict thread affinity. The `uiautomation` crate's
  `UIElement` and `UITreeWalker` are `!Send` after COM init on a thread
  that hasn't declared apartment affinity. `spawn_blocking` gives us a
  dedicated OS thread where COM init is isolated and cleaned up cleanly
  when the task ends.
- **Valid:** `tokio::task::spawn_blocking(move || collect_elements(max_depth)).await?`
- **Invalid:** `let auto = UIAutomation::new()?;` inside an async function.
- **Enforcement:** `a11y/windows_impl.rs::get_foreground_elements` is the
  only entry point and uses `spawn_blocking`.

### INV-ARCH-013: Accessibility coordinates must be reconciled to capture space
- **Rule:** OS accessibility APIs return element bounding rectangles in
  physical screen pixels with the primary monitor's top-left as origin
  (all monitors share one coordinate space on Windows/macOS). Screenshots
  from `xcap` are cropped to a single monitor's pixels. Before passing
  accessibility elements to the LLM, the captured monitor's `(x, y)` offset
  must be subtracted so element coords match the screenshot's top-left=(0,0).
- **Rationale:** On a multi-monitor setup where monitor 2 starts at
  `x=1920`, an a11y element at `screen(2000, 100)` is at `capture(80, 100)`
  relative to that monitor's screenshot. Without reconciliation, the LLM
  would produce point coordinates that land off-screen.
- **Valid:** `a11y::format_elements(&elements, (monitor.x(), monitor.y()), (w, h))`
- **Invalid:** Passing raw screen-space coords directly into the prompt.
- **Enforcement:** `capture.rs` captures `mon_offset` from `xcap::Monitor`
  and passes it into `format_elements`. `format_elements` also skips
  elements that fall outside the captured monitor's bounds.

### INV-ARCH-014: Single Settings panel at a time
- **Rule:** The React `Settings` page is rendered at most once at any given
  time (single-webview Tauri app, single-page navigation via
  `setCurrentPage`). Components inside Settings that cache user-entered
  values (e.g. `SttKeyInput`) are allowed to resync their local state from
  props on `initial` changes without guarding against concurrent edits.
- **Rationale:** A second open Settings panel would cause a feedback loop
  where external saves overwrite an in-progress typed value via a
  props-driven `useEffect(…, [initial])`. The single-webview architecture
  makes this impossible in practice, but the invariant protects the
  assumption so future multi-window refactors don't quietly break it.
- **Enforcement:** Only one `Settings` component instance is mounted —
  `App.tsx` renders it or something else, never both. Adding a second
  Settings window would require revisiting every resync-on-prop-change
  pattern inside Settings and switching to a merge strategy.

### INV-ARCH-015: MCP server uses stderr only for logging
- **Rule:** `workbuddy-mcp` must never write to stdout except as part of
  the MCP JSON-RPC protocol stream. All diagnostics go to stderr via
  `tracing_subscriber::fmt().with_writer(std::io::stderr)`.
- **Rationale:** Stdio is the MCP protocol channel. Any stray stdout
  output corrupts the JSON-RPC framing and hangs the connected Claude
  Code client.
- **Valid:** `tracing::info!("…")` with the stderr writer configured in
  `main.rs`.
- **Invalid:** `println!`, `print!`, or any direct write to
  `io::stdout()` from `workbuddy-mcp`.
- **Enforcement:** Code review on `workbuddy-mcp/src/`. Grep for
  `println!` or `io::stdout`. The `main.rs` module-level comment flags
  this rule.

### INV-ARCH-016: MCP server has read-only access to main-app state
- **Rule:** `workbuddy-mcp` reads the main Tauri app's `config.json`,
  `rag_vectors.db`, and the bundled curriculum resources but never
  writes to any of them. The main Tauri app is the sole writer.
- **Rationale:** The MCP binary and the main app run in separate
  processes that may be active concurrently. Treating the MCP side as
  read-only eliminates the need for file locking or atomic writes across
  process boundaries for shared config.
- **Enforcement:** `workbuddy-mcp/src/config.rs` exposes only `load()`
  and `read_api_key()` — no setters. Review on any new access to
  `~/.config/workbuddy/**` from the MCP crate.

---

## Data Integrity Invariants (continued)

### INV-DATA-005: TTS key gate depends on selected provider
- **Rule:** The gate deciding whether TTS is usable must check the key for
  the **currently selected** `tts_provider`, not always ElevenLabs. Gemini
  uses the `google` key; ElevenLabs uses the `elevenlabs` key. Gating on
  `api_keys.elevenlabs` alone would silently disable TTS for Gemini users
  even when their Google key is configured.
- **Rationale:** `tts_provider` defaults to `"elevenlabs"` for backward
  compatibility, but a user with only a Google key should be able to switch
  to Gemini and have streaming TTS + the Listen button both work.
- **Valid:** `const hasKey = tts_provider === "gemini" ? !!api_keys?.google
  : !!api_keys?.elevenlabs;`
- **Invalid:** `const hasKey = !!api_keys?.elevenlabs;` (hardcoded)
- **Enforcement:** `ChatBar.tsx` streaming TTS gate, `ResponsePanel.tsx`
  `ttsAvailable` computation, `Settings.tsx::TTSSection` toggle disabled state.

### INV-SEC-009: MCP embedding calls honor INV-SEC-001
- **Rule:** `workbuddy-mcp::search_docs` sends query text only to
  `api.openai.com/v1/embeddings` using the user's OpenAI key read from
  the main app's `config.json`. No other outbound traffic from the MCP
  binary. No telemetry, logging of the key, or sending to any analytics
  endpoint.
- **Rationale:** API keys are user-owned credentials (INV-SEC-001);
  leaking them creates financial liability. The MCP binary shares the
  main app's key; it must obey the same endpoint restriction.
- **Enforcement:** Grep `workbuddy-mcp/src/rag.rs` for hardcoded URLs.
  Only `api.openai.com` appears.

### INV-SEC-010: MCP server spawns no arbitrary subprocesses
- **Rule:** `workbuddy-mcp` may only spawn the platform window-title
  detectors (`xdotool`, `osascript`) inherited from `context.rs`. No
  shell execution derived from tool-call inputs. No user-controlled
  arguments passed to `std::process::Command::new`.
- **Rationale:** The MCP server receives JSON-RPC requests from Claude
  Code. A malicious prompt could try to inject commands. Restricting
  subprocess spawning to a fixed allow-list of hardcoded commands
  eliminates the attack surface.
- **Enforcement:** Grep `workbuddy-mcp/src/` for `Command::new` or
  `process::Command`. Only appearances must be the three OS-specific
  window-title calls in `context.rs`.

### INV-DATA-006: set_settings must copy all persisted fields
- **Rule:** Every persisted field in `AppConfig` must be copied inside
  `config::set_settings`. Adding a field to the struct without adding a
  copy line causes the field to update in frontend state but never
  persist to disk — the next app launch reads the stale value.
- **Rationale:** The `set_settings` command intentionally does NOT use
  wholesale assignment because it must exclude `api_keys` (INV-DATA-001).
  Every other field must be copied individually.
- **Enforcement:** `config.rs::set_settings` — copy each field explicitly.
  Currently persists: `provider`, `model`, `program`, `auto_screenshot`,
  `tts_enabled`, `tts_voice`, `tts_provider`, `stt_provider`, `theme`,
  `tutor_mode`, `teach_mode`, `capture_monitor`, `ui_detection_enabled`,
  `ocr_quality`, `cursor_overlay_enabled`, `a11y_detection_enabled`,
  `mask_form_inputs`, `extension_highlight_enabled`,
  `claude_code_mcp_registered`, `wotch_integration_enabled`.

---

## Changelog

| Date       | Change                                    | Invariant      |
|------------|-------------------------------------------|----------------|
| 2026-04-13 | Initial invariants created                | All            |
| 2026-04-13 | Add INV-SEC-005 (audio ephemeral)         | INV-SEC-005    |
| 2026-04-13 | Add INV-ARCH-006 (listener cleanup)       | INV-ARCH-006   |
| 2026-04-13 | Add INV-ARCH-007 (audio resource cleanup) | INV-ARCH-007   |
| 2026-04-13 | Add INV-DATA-004 (SQLite persistence)     | INV-DATA-004   |
| 2026-04-13 | Update INV-SEC-001 for multi-provider keys| INV-SEC-001    |
| 2026-04-13 | Update INV-SEC-003 for STT/updater APIs   | INV-SEC-003    |
| 2026-04-13 | Update INV-ARCH-004 for multi-provider SSE| INV-ARCH-004   |
| 2026-04-13 | Add INV-CURR-003 (composable snippets)    | INV-CURR-003   |
| 2026-04-13 | Add INV-CURR-004 (RAG graceful degrade)   | INV-CURR-004   |
| 2026-04-13 | Update INV-ARCH-004 for tool_use parsing  | INV-ARCH-004   |
| 2026-04-13 | Add INV-ARCH-008 (streaming TTS non-block) | INV-ARCH-008   |
| 2026-04-13 | Tutor mode: prompt-level, no new invariants | N/A (ADR-018)  |
| 2026-04-14 | Update INV-SEC-003 for HuggingFace model DL | INV-SEC-003    |
| 2026-04-14 | Update INV-SEC-004 for JPEG + UI detection  | INV-SEC-004    |
| 2026-04-14 | License GPL-3.0 → AGPL-3.0                 | INV-SEC-003    |
| 2026-04-14 | OmniParser V2 integration (ui_detect.rs)   | INV-SEC-004    |
| 2026-04-14 | Stream cancellation (STREAM_GENERATION)    | INV-ARCH-004   |
| 2026-04-14 | JPEG screenshots, multi-monitor capture    | INV-SEC-004    |
| 2026-04-14 | ElevenLabs STT alternative                 | INV-SEC-001    |
| 2026-04-14 | Anti-hallucination conditional prompt      | INV-CURR-004   |
| 2026-04-14 | Add INV-SEC-006 (extension token auth)      | INV-SEC-006    |
| 2026-04-14 | Add INV-SEC-007 (localhost-only binding)    | INV-SEC-007    |
| 2026-04-14 | Add INV-ARCH-009 (extension async state)    | INV-ARCH-009   |
| 2026-04-14 | Add INV-ARCH-010 (data freshness threshold) | INV-ARCH-010   |
| 2026-04-16 | Gemini STT added — update INV-SEC-001       | INV-SEC-001    |
| 2026-04-16 | Gemini TTS added — update INV-SEC-001       | INV-SEC-001    |
| 2026-04-16 | Add INV-ARCH-011 (TTS MIME per provider)    | INV-ARCH-011   |
| 2026-04-16 | Add INV-DATA-005 (TTS key gate)             | INV-DATA-005   |
| 2026-04-16 | Add INV-DATA-006 (set_settings completeness)| INV-DATA-006   |
| 2026-04-16 | A11y detection added (UIA/AX/AT-SPI2)       | INV-SEC-004    |
| 2026-04-16 | Add INV-ARCH-012 (UIA on spawn_blocking)    | INV-ARCH-012   |
| 2026-04-16 | Add INV-ARCH-013 (a11y coord reconciliation)| INV-ARCH-013   |
| 2026-04-16 | Add INV-SEC-008 (extension password scrub)  | INV-SEC-008    |
| 2026-04-18 | Add INV-ARCH-014 (single Settings panel)    | INV-ARCH-014   |
| 2026-04-18 | Update INV-DATA-006 for new persisted fields| INV-DATA-006   |
| 2026-04-20 | Add INV-ARCH-015 (MCP stderr-only logging)  | INV-ARCH-015   |
| 2026-04-20 | Add INV-ARCH-016 (MCP read-only config)     | INV-ARCH-016   |
| 2026-04-20 | Add INV-SEC-009 (MCP embed → OpenAI only)   | INV-SEC-009    |
| 2026-04-20 | Add INV-SEC-010 (MCP no arbitrary spawning) | INV-SEC-010    |
| 2026-04-20 | Update INV-DATA-006 for Wotch + MCP fields  | INV-DATA-006   |
