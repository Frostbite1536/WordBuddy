# WorkBuddy — Architectural Decision Log

## ADR-001: Fork Pluely as Base, Not Build from Scratch

**Date:** 2026-04-13
**Decision:** Fork pluely (Tauri 2 desktop app) rather than building from scratch.
**Context:** Need cross-platform screen capture, audio, and floating window.
Building these from scratch on 3 platforms would take months.
**Alternatives:**
- Build from scratch with Tauri 2 (3-4x longer)
- Use Electron (200MB binary vs 10MB)
- Extend Wotch (wrong framework for screen capture)
**Rationale:** Pluely has working xcap, audio, window management, and
shortcut systems. Saves ~3 months of platform-specific work.
**Consequence:** GPL-3.0 license (copyleft). Acceptable for educational tool.

## ADR-002: Direct API Calls, No Proxy Server

**Date:** 2026-04-13
**Decision:** Call Anthropic and ElevenLabs APIs directly from the desktop app.
No Cloudflare Worker or proxy server.
**Context:** Clicky uses a Cloudflare Worker to hide API keys. Pluely uses
a server proxy for LLM routing.
**Alternatives:**
- Cloudflare Worker proxy (like Clicky)
- Pluely-style server proxy
**Rationale:** Simpler architecture. No server to maintain. Keys stay on the
student's machine. Students already need their own API keys for the academies.
**Consequence:** Students must manage their own API keys. Keys stored locally.

## ADR-003: React State Navigation, Not URL Routing

**Date:** 2026-04-13
**Decision:** Use `setCurrentPage()` context state for page transitions.
No React Router or URL-based navigation.
**Context:** Tauri renders a single webview. URL changes (window.location.href)
destroy all React state including messages, settings, and streaming buffer.
**Alternatives:**
- React Router (standard for web apps)
- window.location.href (simplest)
**Rationale:** State preservation is critical during streaming. The app is
small enough (4 pages) that a router adds complexity without benefit.
**Consequence:** No deep linking, no browser back button. Acceptable for a
desktop app.

## ADR-004: Shared reqwest::Client via Tauri Managed State

**Date:** 2026-04-13
**Decision:** Create one `reqwest::Client` in `lib.rs` setup and share it
via `app.manage(HttpClient(...))`. All commands access it via
`app.state::<HttpClient>()`.
**Context:** Initial implementation created a new Client per request,
preventing connection pooling and TLS session reuse.
**Alternatives:**
- Client per request (original, simpler but wasteful)
- lazy_static global (works but harder to configure timeouts)
**Rationale:** Connection pooling reduces latency. Consistent timeouts.
Single point of configuration.
**Consequence:** Commands need `AppHandle` parameter to access state.

## ADR-005: Curriculum Detection via Window Title, Not Browser Extension

**Date:** 2026-04-13
**Decision:** Detect which academy module is active by reading the focused
window's title using OS-level APIs (xdotool, osascript, PowerShell).
**Context:** Need to know which module the student is viewing to tailor
the system prompt.
**Alternatives:**
- Browser extension that reports the URL
- Meta tags in academy HTML read via accessibility API
- Manual selection by the student
**Rationale:** No extension install required. Works across all browsers and
non-browser contexts (IDE, terminal). Title matching is good enough for v1.
**Consequence:** Detection depends on window title format. PM Academy needs
a gate check to prevent false positives on generic terms. Wayland on Linux
may not support window title detection.

## ADR-006: Anthropic SSE Format, Strip OpenAI Conventions

**Date:** 2026-04-13
**Decision:** Parse Anthropic's SSE format exclusively. Use `message_stop`
for stream termination. Never check for `[DONE]`.
**Context:** Initial implementation used OpenAI's `[DONE]` sentinel, which
Anthropic never sends. This created dead code and masked the actual stream
termination event.
**Alternatives:**
- Support both formats for flexibility
- Keep [DONE] as a harmless no-op
**Rationale:** Dead protocol code suggests misunderstanding and will confuse
future contributors. The app only supports Claude.
**Consequence:** If Anthropic changes their SSE format, only one place to update.

## ADR-007: AGPL-3.0 License

**Date:** 2026-04-13 (updated 2026-04-14)
**Decision:** License WorkBuddy under AGPL-3.0.
**Context:** WorkBuddy draws architecture from pluely (GPL-3.0) and
incorporates OmniParser V2's YOLOv8s model (AGPL-3.0 via Ultralytics).
AGPL-3.0 is required when linking AGPL-licensed code.
**Alternatives:**
- Build entirely from scratch to use MIT (3-4x longer)
- Skip OmniParser and keep GPL-3.0 (loses pixel-precise pointing)
**Rationale:** AGPL-3.0 adds network-use source-disclosure, but for a
desktop app that doesn't serve users over a network, the practical
obligation is the same as GPL-3.0 (source on request to distributees).
**Consequence:** All contributions must be AGPL-3.0 compatible.

## ADR-008: Multi-LLM Provider Abstraction

**Date:** 2026-04-13
**Decision:** Support 6 LLM providers (Anthropic, OpenAI, Google, Groq,
Ollama, OpenRouter) via a provider config abstraction in `llm.rs`.
**Context:** Students have different API access and budgets. Some have free
Groq keys or local Ollama installs. Locking to Anthropic-only limits adoption.
**Alternatives:**
- Anthropic-only (original design)
- Frontend-side provider routing
**Rationale:** Provider abstraction is minimal (URL, auth header, format flag).
Two SSE parsers cover all providers. Students get free options (Ollama, Groq).
**Consequence:** Must maintain two SSE parsers. Cannot rely on Anthropic-
specific features (like tool use) without provider guards.

## ADR-009: cpal for Cross-Platform Audio Capture

**Date:** 2026-04-13
**Decision:** Use the `cpal` crate for microphone input with custom VAD
(voice activity detection) based on RMS energy and peak amplitude thresholds.
**Context:** Push-to-talk needs platform audio capture. cpal abstracts
WASAPI (Windows), CoreAudio (macOS), and ALSA/PulseAudio (Linux).
**Alternatives:**
- Web Speech API from the frontend (free, no key, but lower quality)
- Platform-specific implementations per OS
- webrtcvad crate (more accurate but heavier dependency)
**Rationale:** cpal is the standard Rust audio crate. Custom VAD is simpler
than adding a C dependency. Thresholds matched from pluely's working values.
**Consequence:** Adds ~2 crate dependencies (cpal, hound). Audio quality
depends on system microphone. VAD thresholds may need tuning per environment.

## ADR-010: Client-Side Point Tag Parsing

**Date:** 2026-04-13
**Decision:** Parse `[POINT:x,y:label]` tags in TypeScript (`pointParser.ts`)
rather than calling the Rust `parse_point_tags` function via IPC.
**Context:** The point tag regex is trivial. Calling Rust via IPC adds
latency and complexity for a simple string operation.
**Alternatives:**
- Invoke Rust parser via Tauri command (consistent, but unnecessary IPC)
- Parse during streaming (complex, tags may be split across chunks)
**Rationale:** Avoids an IPC round-trip. The regex is identical in both
implementations. Parsing happens once after stream completes.
**Consequence:** Two implementations of the same regex must stay in sync.
The Rust version is the canonical source with unit tests.

## ADR-011: SQLite for Conversation Persistence

**Date:** 2026-04-13
**Decision:** Store conversations in SQLite via `tauri-plugin-sql` with
schema: `conversations` (id, created_at, program, module_id) and `messages`
(id, conversation_id, role, content, timestamp).
**Context:** Session-only message history was lost on app restart. Students
want to review past conversations.
**Alternatives:**
- Flat file per conversation (no query support)
- IndexedDB in the webview (Tauri discourages)
- Cloud sync (violates no-telemetry invariant)
**Rationale:** `tauri-plugin-sql` was already registered. SQLite is durable,
queryable, and local-only. Screenshot content is explicitly excluded.
**Consequence:** Database file grows with usage. No cloud backup. Student
is responsible for data on their machine.

## ADR-012: Composable Topic Snippets + Module Map for Per-Module Context

**Date:** 2026-04-13
**Decision:** Use 23 curated topic snippet files composed via a module map
rather than 52 monolithic per-module context files. Each module maps to a
list of snippet keys; a resolver joins them at runtime.
**Context:** 36 Limitless source documents contain ~120k tokens of reference
material. Injecting all of it per query exceeds token limits. Per-module
monolithic files would duplicate content across modules.
**Alternatives:**
- 52 monolithic per-module files (massive duplication)
- Tier-level only (too generic for module-specific questions)
- RAG-only (misses baseline context when no OpenAI key available)
**Rationale:** Composable snippets eliminate duplication (23 snippets vs 52
files). Module map is easy to update as new modules are added. Static
snippets provide baseline context even without RAG.
**Consequence:** Adding a new module requires only updating module_map.ts,
not creating new snippet files (unless covering a new topic).

## ADR-013: RAG with Separate SQLite + In-Memory Cosine Similarity

**Date:** 2026-04-13
**Decision:** Use rusqlite with a separate `rag_vectors.db` file for the RAG
vector store, with in-memory cosine similarity computation in Rust.
**Context:** Need dynamic document retrieval for questions that static
snippets don't cover. ~300 chunks from 36 source docs.
**Alternatives:**
- sqlite-vec extension (native vector ops but adds binary dependency)
- Shared DB with tauri-plugin-sql (potential locking conflicts)
- External vector DB like Qdrant (heavy dependency for 300 chunks)
- Embedding in frontend via WebAssembly (complex, slower)
**Rationale:** 300 chunks × 1536 dims fits easily in memory. Cosine
similarity in Rust is trivial and fast (<10ms). Separate DB avoids conflicts
with the tauri-plugin-sql plugin. OpenAI embeddings are cheap ($0.003 to
index all docs).
**Consequence:** Requires OpenAI API key for indexing and search. RAG is
optional — degrades gracefully to static snippets only.

## ADR-014: Anthropic Tool_Use for Cursor Pointing

**Date:** 2026-04-13
**Decision:** Define `point_at` and `highlight` tools in the Anthropic
request body so Claude calls them natively via tool_use, replacing
`[POINT:x,y:label]` text tag parsing for Anthropic provider.
**Context:** Parsing coordinate tags from free text using regex is fragile —
tags can be split across SSE chunks, and the LLM sometimes formats them
incorrectly. Anthropic's tool_use API provides structured JSON output.
**Alternatives:**
- Keep text-only `[POINT:]` tags (fragile, requires holdback buffer)
- OpenAI function_calling (different format, would need per-provider tool defs)
- Computer Use API (more accurate but heavier, beta-only)
**Rationale:** Tool_use is natively supported in the existing SSE stream,
produces structured JSON, and is more reliable. `[POINT:]` kept as fallback
for non-Anthropic providers. No new dependencies needed.
**Consequence:** Tool definitions only sent to Anthropic. Non-Anthropic
providers use text tag fallback. Parser must handle mixed content blocks.

## ADR-015: Spring-Physics Cursor Animation

**Date:** 2026-04-13
**Decision:** Use a damped harmonic oscillator (`SpringValue` class,
stiffness=170, damping=18) driven by `requestAnimationFrame` instead of
CSS transitions for cursor movement.
**Context:** CSS `transition` produces mechanical linear/ease motion.
Clicky's native app uses spring physics for natural, playful cursor movement.
**Alternatives:**
- CSS cubic-bezier transitions (simple but rigid)
- Web Animations API (limited physics control)
- GreenSock/Framer Motion (large dependency)
**Rationale:** Pure TypeScript (~90 lines), zero dependencies, produces
natural overshoot-and-settle motion. Matched parameters from Clicky Web
(stiffness=170, damping=18). rAF loop also enables trail ghost rendering.
**Consequence:** Slightly more complex than CSS transitions. Must manage
rAF lifecycle (start/stop/cleanup).

## ADR-016: Full-Screen Transparent Overlay Window for Cursor Pointing

**Date:** 2026-04-13
**Decision:** Create a second Tauri window (`cursor_overlay`) that covers
the full primary monitor for cursor pointing, instead of rendering the
overlay inside the main 600px WorkBuddy window.
**Context:** The main WorkBuddy window is 600px tall. Point coordinates
reference the full screen (from screenshots). Mapping screen coords into
a 600px window produces incorrect positions.
**Alternatives:**
- Render in main window with screen-relative transforms (incorrect positions)
- Use Rust-side pointer with platform-specific cursor APIs (complex, fragile)
- Single full-screen window for both UI and overlay (loses the compact bar design)
**Rationale:** Separate transparent window matches Clicky's macOS NSPanel
approach. Tauri 2 supports `transparent: true`, `set_ignore_cursor_events(true)`,
and `alwaysOnTop: true`. The overlay loads the same frontend but detects
its window label and renders only `CursorOverlayWindow`.
**Consequence:** Two Tauri windows. The overlay window must be sized to
the primary monitor at startup. Multi-monitor support requires one window
per monitor (future enhancement).

## ADR-018: Tutor Mode via Prompt Injection, Not Idle Detection

**Date:** 2026-04-13
**Decision:** Implement tutor mode as a composable prompt block injected into
`buildSystemPrompt()` when the `tutor_mode` setting is enabled, rather than
Clicky's idle-triggered observation pattern.
**Context:** Clicky's native macOS app has a tutor mode that captures the screen
after 3 seconds of keyboard/mouse idle and proactively guides the user through
software step-by-step. WorkBuddy's academy pages are educational content —
students are idle *while reading*, which would trigger constant interruptions.
**Alternatives:**
- Idle-triggered observation like Clicky (3s idle → capture → guide)
- Separate tutor prompt profiles per program (7 more prompts to maintain)
- Code-level changes to response handling (unnecessary complexity)
**Rationale:** The behavioral difference (ask questions vs. give answers) is
entirely a prompting concern. A single `TUTOR_MODE_INSTRUCTIONS` block composes
with all 7 base profiles without modification. The toggle is a persisted setting
accessible from both ChatBar and Settings, requiring zero new Tauri commands.
Academy pages have interactive elements (sliders, simulators, code tabs) that
the tutor can guide students through — this works better with student-initiated
interaction than idle observation.
**Consequence:** Tutor mode effectiveness depends on prompt engineering quality.
The LLM must consistently follow the Socratic instructions across all providers.
Non-Anthropic providers without tool_use still get tutor behavior but use
`[POINT:]` text fallback for pointing.

## ADR-017: Streaming Sentence TTS

**Date:** 2026-04-13
**Decision:** Split streamed text into sentences using `SentenceBuffer` and
speak each sentence immediately via `TTSQueue`, instead of waiting for the
full response before TTS.
**Context:** Students wait 10-30 seconds for long responses before hearing
anything. Clicky Web uses a similar `SentenceBuffer` pattern for real-time
speech synthesis.
**Alternatives:**
- Full-response TTS only (current — student waits)
- Browser SpeechSynthesis API (free but lower quality, no ElevenLabs voices)
- Word-by-word streaming (too choppy for ElevenLabs)
**Rationale:** Sentence-level granularity balances quality (complete
sentences sound natural) with latency (first sentence plays within 2-3
seconds). Rate limiting (1 req/sec) prevents API overuse. Each sentence
is a separate ElevenLabs call, so cost scales with response length.
**Consequence:** Multiple TTS API calls per response (vs. one). Rate
limiting adds slight delay between sentences. Must handle cancellation
cleanly when user sends a new message mid-playback.

## ADR-019: OmniParser V2 for Pixel-Precise UI Detection

**Date:** 2026-04-14
**Decision:** Integrate Microsoft OmniParser V2's finetuned YOLOv8s model
for local UI element detection via ONNX Runtime.
**Context:** Claude's vision API provides inaccurate pixel coordinates for
interior UI elements (corners work, but buttons/icons are consistently "up
and to the left"). A Clicky community member replaced Claude vision
coordinate detection with OmniParser V2 and reported large accuracy gains.
**Alternatives:**
- Full OmniParser pipeline with Florence-2 captioning (~5-10s CPU, 1.1GB)
- Custom-trained YOLO under MIT license (avoids AGPL, needs training data)
- Accept LLM estimation errors (no model needed)
**Rationale:** YOLO-only approach gives pixel-precise bounding boxes in
~150-400ms on CPU, ~40-50MB model. Claude already understands what elements
ARE from the screenshot — it just needs accurate coordinates. Skip Florence-2
entirely. Feature is opt-in (default off, user downloads model on demand).
**Consequence:** Adds ONNX Runtime dependency (~50MB DLL). Requires AGPL-3.0
license (from Ultralytics YOLOv8). Model downloaded from HuggingFace.
Detection output is ephemeral (in-memory only, never persisted).

## ADR-020: Stream Cancellation via Generation Counter

**Date:** 2026-04-14
**Decision:** Use an atomic `STREAM_GENERATION` counter to cancel superseded
SSE streams instead of allowing them to pile up.
**Context:** Old `stream_response` calls were never cancelled. Multiple SSE
streams accumulated, exhausted the HTTP connection pool (4 per host), and
blocked the Tauri IPC bridge — freezing the entire UI including window
dragging. Root cause confirmed during live testing.
**Alternatives:**
- Abort the reqwest request via `Client::abort()` (not available mid-stream)
- Frontend tracks and cancels old invokes (Tauri doesn't support invoke cancellation)
**Rationale:** Each new `stream_response` increments the counter. Both parsers
check it on every chunk and abort if superseded. No new dependencies. The
aborted stream's connection is dropped, freeing the pool slot.
**Consequence:** Old responses may be partially visible before being superseded.
The frontend already handles this (streaming message is replaced).

## ADR-021: JPEG Screenshots Instead of PNG

**Date:** 2026-04-14
**Decision:** Encode screenshots as JPEG quality 85 instead of PNG.
**Context:** Full-resolution PNG screenshots (1920x1080) were ~6MB as base64,
contributing to API request size issues and potential stream timeouts.
**Rationale:** JPEG at quality 85 produces ~200-400KB — 15-30x smaller — while
keeping text readable for LLM vision. LLMs handle JPEG and PNG equally well.
**Consequence:** Slight quality loss vs PNG (lossy compression), but not
noticeable for screen content at quality 85. MIME types updated in llm.rs.

## ADR-022: ElevenLabs as Alternative STT Provider

**Date:** 2026-04-14
**Decision:** Allow ElevenLabs Speech-to-Text as an alternative to OpenAI
Whisper for push-to-talk transcription.
**Context:** Users with an ElevenLabs key for TTS can reuse it for STT,
eliminating the need for a separate OpenAI key.
**Rationale:** Reduces the number of API keys users need to manage.
ElevenLabs STT quality is comparable to Whisper.
**Consequence:** New `stt_provider` config field. stt.rs routes to the
selected provider. Users need to enable "Speech to Text" permission on
their ElevenLabs API key.

## ADR-023: HTTP Request-Driven Extension, Not WebSocket

**Date:** 2026-04-14
**Decision:** Use HTTP POST/GET for browser extension ↔ WorkBuddy
communication instead of a persistent WebSocket connection.
**Context:** Chrome MV3 service workers terminate after 30 seconds of
idle. A WebSocket connection would be killed, requiring keepalive hacks
and reconnection logic.
**Alternatives:**
- WebSocket with keepalive (needs 25s ping timer, reconnection logic)
- Chrome native messaging (requires separate native host binary)
- Long polling / SSE (more complex than simple request/response)
**Rationale:** WorkBuddy only needs element data when taking a screenshot.
A request/response model fits perfectly — no persistent connection needed.
The extension pushes scan data every 3 seconds and polls for highlights
every 300ms. Both are simple HTTP round-trips.
**Consequence:** Highlight latency is up to 300ms (poll interval) instead
of instant (WebSocket push). Acceptable for pointing feedback.

## ADR-024: In-Page CSS Highlighting, Not Tauri Cursor Overlay

**Date:** 2026-04-14
**Decision:** The browser extension injects CSS overlay divs directly into
the page DOM for highlighting, instead of mapping to screen coordinates
for the Tauri cursor overlay window.
**Context:** Converting browser viewport coordinates to screen coordinates
is an unsolved W3C problem (see w3c/csswg-drafts#5814). `window.screenX/Y`
may not include the toolbar. `devicePixelRatio` complicates CSS→physical
pixel conversion. Multi-monitor setups have negative coordinates. Browser
zoom changes the ratio unpredictably.
**Alternatives:**
- Map viewport coords to screen coords for Tauri overlay (unreliable)
- Use Chrome's Accessibility API to get screen rects (not available in MV3)
- Use native messaging to get window geometry (complex, platform-specific)
**Rationale:** Injecting a `<div>` with `position: fixed` at the element's
`getBoundingClientRect()` position is guaranteed correct — same coordinate
space as the element itself. Zero coordinate conversion, pixel-perfect.
**Consequence:** Highlights only visible within the browser viewport. For
non-browser content (IDE, terminal), the Tauri cursor overlay remains the
fallback path.

## ADR-025: Raw TCP HTTP Server, No Framework Dependency

**Date:** 2026-04-14
**Decision:** Implement the extension HTTP server with raw
`tokio::net::TcpListener` and manual HTTP/1.1 parsing instead of adding
hyper, axum, or warp as dependencies.
**Context:** The server handles 3 simple endpoints (GET /status,
POST /scan, GET /highlight) on localhost only. JSON payloads are small
(<64KB). reqwest already pulls in hyper for the client side.
**Alternatives:**
- hyper 1.x (standard, but needs hyper-util + http-body-util = 3 new crates)
- axum (convenient routing, but heavy dependency for 3 endpoints)
- tiny_http (simpler, but another dependency)
**Rationale:** Zero new dependencies. The HTTP parsing is ~60 lines of
straightforward code. All requests are localhost, controlled by our
extension client. The protocol surface is minimal and well-defined.
**Consequence:** No automatic handling of edge cases like chunked transfer
encoding, HTTP/2, or keep-alive pipelining. These are not needed for our
localhost use case.

## ADR-026: Extension Push Model with Background Relay

**Date:** 2026-04-14
**Decision:** Content script scans the DOM and pushes data to WorkBuddy
via the background service worker, rather than WorkBuddy pulling from
the extension on demand.
**Context:** WorkBuddy (Rust) cannot initiate requests to the extension
— browser extensions don't expose HTTP servers. Communication must go
from extension to WorkBuddy.
**Alternatives:**
- Pull model via long-polling (extension opens persistent connection)
- Chrome native messaging (requires separate executable, platform-specific)
- Shared file system (extension writes DOM data to disk)
**Rationale:** Push model is simplest. Content script scans every 3 seconds
and sends via `chrome.runtime.sendMessage` to the background worker, which
POSTs to localhost. WorkBuddy stores the latest data. No persistent
connections, no native messaging, no file I/O.
**Consequence:** Element data may be up to 3 seconds stale. The 10-second
freshness threshold in `has_fresh_data()` ensures stale data from closed
tabs isn't used.

## ADR-027: Gemini as Third STT Provider (Reuse Google Key)

**Date:** 2026-04-16
**Decision:** Add `"gemini"` as a third STT provider alongside Whisper and
ElevenLabs. Uses `gemini-2.5-flash` (stable GA, not preview) via
`generateContent` with `inlineData` — base64 WAV sent directly.
**Context:** Users with a Google API key for the Gemini LLM provider had to
configure a *separate* OpenAI or ElevenLabs key for push-to-talk. Gemini
supports audio understanding natively — the same key works for all three
services (LLM, TTS, STT).
**Alternatives:**
- Keep Whisper + ElevenLabs only (users configure 2 keys)
- Use `gemini-3-flash-preview` (newer but preview-tier fragility)
- Use Gemini Live API (real-time streaming — complex, different protocol)
**Rationale:** Single-key convenience is the primary win. Cost is ~3x
cheaper than Whisper for short utterances (32 tokens/sec × $1.00/1M input).
Stable 2.5 Flash over preview 3 Flash eliminates "my app broke overnight"
failures. Gemini also understands emotion + non-speech sounds (future-proofs
for richer features).
**Consequence:** Three dispatch arms in `stt.rs`. Gemini path needs extra
robustness: 10MB base64 size cap, `promptFeedback.blockReason` handling,
empty `candidates` array = silent audio (returns empty string, not error),
and `strip_transcript_artifacts()` to remove Gemini's occasional
"Transcript:" prefix and wrapping quotes.

## ADR-028: Gemini 3.1 Flash TTS as Alternative Provider

**Date:** 2026-04-16
**Decision:** Add Gemini 3.1 Flash TTS as a second TTS provider alongside
ElevenLabs. New `tts_provider` setting (`"elevenlabs"` default,
`"gemini"` opt-in). 30 prebuilt voices via `list_tts_voices` Tauri command.
**Context:** ElevenLabs requires a separate subscription + API key. Users
with a Google key already configured for Gemini LLM/STT should be able to
reuse it for TTS. Gemini TTS is cheaper per-char and offers 30 curated
voices (vs ElevenLabs' 1000+ cloned voices that most users don't need).
**Alternatives:**
- ElevenLabs-only (forces separate subscription)
- Cloud TTS (Google Cloud TTS — different API, no Gemini audio-tag support)
- Browser SpeechSynthesis (free but lower quality, no voice consistency)
**Rationale:** Preview-tier `gemini-2.5-flash-preview-tts` is stable enough
for production (used by claude-tts reference impl). Sulafat (Warm) is an
excellent default tutor voice. 30 voices cover the "I want something
different" case without the complexity of voice cloning. Reusing the Google
key is the decisive UX win.
**Consequence:**
- Gemini returns raw signed 16-bit LE PCM at 24kHz mono. Must wrap in a
  44-byte WAV header manually (`pcm_to_wav()`) — browser `<audio>` can't
  play raw PCM directly.
- MIME type dispatch (INV-ARCH-011): `audio/wav` for Gemini, `audio/mpeg`
  for ElevenLabs. Mismatched MIME silently fails.
- 3-attempt retry for documented 500-error randomness in Gemini TTS.
- Hardcoded `api_keys.elevenlabs` gates in ChatBar/ResponsePanel had to be
  refactored to provider-aware gates (INV-DATA-005) — without this refactor,
  Gemini users would have silently-disabled TTS even with a valid Google key.

## ADR-029: Accessibility APIs for Pixel-Precise Pointing

**Date:** 2026-04-16
**Decision:** Add a new `a11y.rs` module that reads element names + bounding
rectangles from the OS accessibility tree of the foreground window. Platform
backends: Windows UIA (`uiautomation 0.24`, real), macOS AXUIElement
(`accessibility 0.2`, stub until tested), Linux AT-SPI2 (`atspi 0.29`,
stub until tested).
**Context:** The LLM's pixel coordinates are estimated from JPEG screenshots
and are typically 50-200px off for interior UI elements. Multi-point
sequences drift further with each point. YOLO+OCR catches buttons/icons but
misses structured hierarchy (tabs, tree items, input fields, menu items).
Extension data only works in browsers. IDEs, terminals, and Electron apps
need a different source.
**Alternatives:**
- Rely on Claude's Computer Use beta (heavier, beta-only, Anthropic-only)
- Train a larger YOLO model (more data, more inference time)
- Ship without IDE/terminal pointing (primary user pain point)
**Rationale:** Every major OS exposes a pixel-precise UI tree that screen
readers already use. No model download, no ML latency — typically 20-200ms
on Windows UIA. Runs locally, data never leaves the machine. The same names
the OS gives screen reader users are ideal labels for the LLM to reference.
**Consequence:**
- Detection stack gains a 3rd source. Order: extension → a11y → YOLO+OCR.
  a11y has 800ms timeout and ≥5-element gate (some apps have stub trees).
- Coordinate reconciliation required (INV-ARCH-013): a11y returns absolute
  screen coords with primary-monitor origin; `capture.rs` subtracts the
  captured monitor's offset before formatting for the prompt.
- Windows COM MTA state: `UIAutomation::new()` initializes COM on its calling
  thread. Must run via `tokio::task::spawn_blocking` to isolate state
  (INV-ARCH-012).
- Platform-conditional Cargo deps: `uiautomation = "0.24"` on Windows,
  `accessibility + accessibility-sys = "0.2"` on macOS, `atspi = "0.29"` on
  Linux. Each only activates on its target OS.
- macOS requires Accessibility permission. Gracefully returns empty on
  denial, falls through to YOLO+OCR.

## ADR-030: Broaden title-overlap separator set + exact-match escape hatch

**Date:** 2026-04-18
**Decision:** `title_overlaps_extension` accepts any of `: — - | / · » › • –` as
a title-separator discriminator, AND treats case-insensitive exact equality
between the OS window title and the extension's page title as an unambiguous
match regardless of length or separator content.
**Context:** The initial post-audit fix added a separator gate so generic
titles like "Dashboard" couldn't hijack unrelated app windows ("Dashboard ·
Dropbox"). The gate only recognized Latin-style separators (`: — - | /`),
which rejected Limitless/partner pages that happen to use `·` or a guillemet.
It also rejected legitimate short titles like "Kickoff" that the page
actually uses as its entire `document.title`.
**Alternatives:**
- Drop the separator gate (reintroduces the original false-positive)
- Hardcode per-domain lists (brittle, doesn't scale to partner pages)
- Match on URL host instead of title (would require exposing browser URL to
  context.rs via an OS API that doesn't exist cross-platform)
**Rationale:** The separator set is still a discriminator — short unbranded
titles like "Home" or "Dashboard" from unrelated apps still can't substring-
match, because browsers append their own brand with some kind of separator.
Exact-equality is a stronger signal than substring containment: when a full
OS window title equals the full page title, there's no ambiguity — it's the
same page. Exact-match sidesteps the separator requirement for the rare case
of an app that doesn't rewrite the title at all.
**Consequence:** Title-overlap detection works across a wider class of brand
conventions. Two new regression tests lock in behavior for `·`-separated
titles and for the "Kickoff" exact-match case.

## ADR-031: Track fence-level counts in the RAG block tokenizer

**Date:** 2026-04-18
**Decision:** `tokenize_blocks` counts the number of leading backticks at each
fence line and only closes an open fence when a subsequent fence line has a
count ≥ the opening count. A 3-backtick line inside a 4-backtick fence is
treated as content, not as a premature close.
**Context:** CommonMark allows a fenced code block to contain any number of
backticks less than the opening count, specifically so authors can embed
``` inside a `````` block when documenting Markdown itself. The initial
post-audit `tokenize_blocks` treated any 3+ backtick line as a fence toggle,
which would corrupt any doc that uses this pattern.
**Alternatives:**
- Accept the limitation (docs don't currently use nested fences; fragile)
- Replace with a CommonMark parser crate (pulldown-cmark, comrak) (adds
  dependencies; our chunker only needs a tiny subset of CommonMark)
- Support `~~~` fences in addition to `` ``` `` (yagni; not in our docs)
**Rationale:** The fix is local (track an `Option<usize>` fence level through
the walk) and matches CommonMark's specified behavior without pulling in a
full parser. Authors who write docs about Markdown itself now embed code
correctly.
**Consequence:** A malformed doc with unbalanced fences (opening count > any
later closing count) still falls through to the existing "unterminated code
block" behavior — emit what we have at end-of-input. Regression test added
for the nested-fence case.

## ADR-032: Honor Retry-After + exponential backoff on transient TTS retries

**Date:** 2026-04-18
**Decision:** When Gemini TTS returns 429 or 5xx, the retry loop sleeps before
the next attempt. The delay is `min(Retry-After header, 5s)` when the server
provides one, otherwise `250ms × 2^attempt` (capped at 2s). Total worst-case
wait for 3 attempts stays under 15 seconds.
**Context:** The initial retry-predicate fix (M8) widened the retryable set
from "500 only" to "429 + 5xx", but retried immediately, without respecting
`Retry-After` or adding any backoff. A rate-limited client that retries
instantly gets rate-limited again — wastes its 3 attempts on identical failures.
**Alternatives:**
- Respect the full `Retry-After` value with no cap (can block UI 30-60s while
  the user waits for audio — unacceptable for TTS)
- Fixed 1-second sleep between attempts (simpler but ignores server hints)
- Retry forever with exponential backoff (breaks the bounded-time contract
  the user expects from TTS — a missing voiceover is better than a stuck UI)
**Rationale:** Capping at 5s per-attempt keeps worst case under 15s for the
three-attempt budget, which is the upper bound a student will tolerate for
voice playback before abandoning. Exponential backoff for server errors
without a header hint is a well-known correct-default that gives transient
issues a moment to resolve without being slow for the common success case.
**Consequence:** TTS with a rate-limited key will still fail after 3 tries,
but won't waste the tries on instant-retry 429s. The silent fallback (TTS
just doesn't play for this sentence) is unchanged.

## ADR-033: Route `highlight` tool to browser extension, keep `point_at` on cursor overlay

**Date:** 2026-04-18
**Decision:** When the Anthropic `highlight` tool fires and the browser
extension is connected with fresh data, WorkBuddy routes the highlight to
the extension's in-page CSS overlay instead of the full-screen cursor overlay
window. `point_at` always uses the cursor overlay. A new
`extension_highlight_enabled` setting (default on) lets users force the cursor
overlay everywhere.
**Context:** The Tauri `cursor_overlay` window covers the entire primary
monitor with a transparent always-on-top WebView. On a maximized browser it
works but feels heavy — the SVG spotlight mask dims the screen, the cursor
animation runs for every highlight, and multi-monitor users see the overlay
flash across a single screen. Inside the browser, the extension already has
an in-page CSS highlight (`content.js::highlightElement`) that scrolls with
the page, respects browser chrome, and doesn't touch a second WebView. The
`extension_highlight` Tauri command already enqueued these highlights — it
just was never invoked.
**Alternatives:**
- Route BOTH point_at and highlight to the extension when available (point_at
  is the precise cursor animation — the extension's static rectangle is
  semantically different and visually worse for pinpointing a single element)
- Route by context (on-browser → extension, elsewhere → overlay) regardless
  of tool (requires a window-classifier the app doesn't have and would
  surprise users who expect consistent overlay behavior)
- Leave highlight on the overlay (keeps the command dead and forgoes the UX
  improvement on browser pages)
**Rationale:** The two tool schemas already encode different intent — point_at
for "find this specific element", highlight for "mark this region". The
cursor animation was designed for the former; the extension's labeled-rect
overlay was designed for the latter. Splitting by tool aligns the routing with
the semantic distinction the model is already making. Browser-page UX wins
without affecting IDE/terminal/other-app flows, which route to the overlay
as before because the extension won't be connected. The toggle is a safety
valve for students who prefer one consistent UI across contexts.
**Consequence:**
- `highlight` tool schema extended with optional `width`/`height` fields so
  the model can match a known rect (e.g., from the DETECTED UI ELEMENTS list).
  Defaults to a 120×40 rect centered on (x, y) when not provided.
- New persisted setting `extension_highlight_enabled` (default `true`).
  Requires update to INV-DATA-006's covered-fields list.
- Coord-space caveat: extension element coords are CSS/viewport, screenshot
  coords are physical. When the model uses an extension element's center
  coords (the dominant case when the extension is connected), the coords
  land in the right space. When the model estimates from the screenshot
  while the extension is also connected, the highlight can land off on
  high-DPR or windowed browsers. The toggle exists for students who hit
  this failure mode.
- No new Tauri commands. Reuses the existing `extension_highlight` command
  and `get_extension_status` probe.

## ADR-034: Stdio MCP server for Claude Code curriculum awareness

**Date:** 2026-04-20
**Decision:** Ship a standalone `workbuddy-mcp` binary (new workspace
crate at `src-tauri/workbuddy-mcp/`) that speaks the Anthropic Model
Context Protocol over stdio. Exposes six tools: `get_current_module`,
`get_lesson_plan`, `get_module_context`, `list_modules`,
`get_ui_elements`, `search_docs`. A Settings toggle auto-registers the
binary in `~/.claude.json` so Claude Code — running inside Wotch or
anywhere else — can query curriculum context directly.
**Context:** Wotch (the sibling project; same maintainer) ships a mature
Claude Code integration (hooks, MCP IPC, MCP stdio server). The highest-
leverage path for WorkBuddy ↔ Wotch integration is NOT to invent a new
IPC layer between the two desktop apps (as an older roadmap assumed) but
for WorkBuddy to become a curriculum-aware MCP provider that any
Claude Code instance consumes through the standard protocol. See
`docs/WOTCH_INTEGRATION.md` for the detailed plan — this ADR captures
only the top-level decision.
**Alternatives:**
- Custom local IPC between the two apps (reinvents what MCP already
  specifies).
- WorkBuddy polls Wotch's `/v1/status` (low value — the screenshot
  already captures what Claude is doing; see WOTCH_INTEGRATION §2).
- Pill badge on Wotch for current module (decorative; the student
  knows their module since they navigated to it; see §2).
- Node/Electron implementation of the MCP server (adds a runtime;
  doesn't share types with the Rust main app's config parsing).
**Rationale:** MCP is a stable protocol. Building to it rather than to
Wotch-specific glue means the same integration works in a plain
terminal, a VS Code MCP client, Claude Desktop, or any other tool that
speaks the protocol. A standalone binary lets the MCP server start when
the main Tauri app isn't open. Read-only access to the shared on-disk
state (`config.json`, `rag_vectors.db`, bundled `curriculum.json`,
lesson-plan markdown) avoids runtime IPC between the two WorkBuddy
processes.
**Consequence:**
- New workspace crate `workbuddy-mcp` (~1500 LOC Rust) with its own
  test suite.
- `curriculum.json` generated at build time from the TypeScript
  curriculum sources via `scripts/generate-curriculum.ts` (run with
  `npx tsx`). Keeps the TS side as the single source of truth for
  snippets + module map; the MCP binary embeds the JSON via
  `include_str!`.
- Four new invariants (INV-ARCH-015/016, INV-SEC-009/010) scope the
  MCP binary's capabilities: stderr-only logging, read-only access to
  main-app state, OpenAI-only network calls for embeddings, no
  arbitrary subprocess spawning.
- Two new persisted settings: `claude_code_mcp_registered` (default
  false; toggle runs `register_claude_mcp` / `unregister_claude_mcp`),
  `wotch_integration_enabled` (default true; shows "Open in Wotch"
  toolbar button).
- `~/.claude.json` writes are atomic (temp-file-then-rename) and merge
  preserving other `mcpServers` entries so Wotch's concurrent
  auto-registration isn't clobbered. The read-modify-write cycle itself
  isn't atomic at the OS level, so `merge_write` adds a post-write
  verify-and-retry loop (up to 3 attempts with 50/100/200 ms back-off)
  that detects when a concurrent writer overwrote us and re-applies the
  merge. True advisory locking (fd-lock / flock / LockFileEx) would
  eliminate the race class entirely and is flagged for v1.1 — for now
  verify-and-retry covers realistic collisions (user toggles both apps'
  settings within the same second) without a new Cargo dep.
- Sidecar bundling of the MCP binary in release builds is a v1.1
  follow-up — the registration code currently locates the binary via
  `std::env::current_exe()?.parent()?.join("workbuddy-mcp…")`, which
  works in dev (cargo workspace puts all binaries alongside) and will
  work in release once Tauri's `externalBin` is wired in
  `tauri.conf.json`.

## ADR-035: Cohort telemetry in TypeScript, not Rust

**Date:** 2026-04-24
**Decision:** Implement the entire cohort-telemetry subsystem
(collector, tagger, redactor, uploader, consent, queue CRUD) as
TypeScript modules under `src/lib/telemetry/`. No new Rust module,
no new Tauri commands.
**Context:** The plan originally targeted `src-tauri/src/telemetry/**`.
The SQLite database is accessed via `tauri-plugin-sql` from the
frontend (`src/lib/db.ts`); putting telemetry in Rust would require
opening the same DB file from a second SQLite client, adding WAL /
locking complexity for no benefit.
**Alternatives:**
- Rust crate mirroring `config.rs` — rejected: duplicates DB client
  + serializes through IPC for every enqueue call.
- Split: Rust for the uploader, TS for the collector — rejected: the
  uploader's preflight scan needs to see the same serialized bytes
  the collector produced; splitting adds a JSON-serialize round-trip.
**Rationale:** Telemetry handles no secrets (cohort token is not
sensitive like an LLM API key), so the Rust/TS trust boundary is not
the deciding factor. Direct `fetch()` from the webview is the only
HTTP client on the path (INV-TEL-012 bans third-party HTTP clients
in the telemetry directory anyway). Tests run in vitest, colocated
with the code they exercise.
**Consequence:** All INV-TEL-* enforcement lives in TypeScript.
The INV-TEL-012 grep over `src/lib/telemetry/` is the architectural
defense against quietly pulling in an LLM SDK on this path.

## ADR-036: Redactor-then-tagger pipeline order (not tagger-then-redactor)

**Date:** 2026-04-24
**Decision:** The Tier 2 collector runs the redactor on each
student message first, then passes the **redacted fragment** to the
tagger for the module-confidence score.
**Context:** `docs/PLAN-cohort-telemetry/ARCHITECTURE.md` originally
sketched Tier 2 data flow as `tagger → redactor`
(tagger picks fragment candidates, redactor sanitizes them).
`REDACTION.md` lists the tag-confidence gate as stage 7, after the
regex redactions of stage 4. The implementation follows REDACTION.md.
**Alternatives:**
- Tagger first — rejected: PII enters the tagger's keyword scan,
  and the `tag_confidence` reflects signal from strings that will
  be stripped before upload. Inflates confidence on PII-heavy
  messages whose redacted form has too little signal to defend.
- Run tagger on both and take the minimum — rejected: more complex
  for no additional safety benefit.
**Rationale:** Tagging the redacted fragment is fail-closed. A
fragment whose signal is gutted by redaction drops out of the 0.6
confidence threshold; a fragment whose signal survives redaction
is one the instructor could reasonably interpret. The tagger
never stores or transmits its input, so tagging the original
would be non-leaky in isolation — but the downstream `fragment`
field is the redacted form, so the confidence should be over the
same text.
**Consequence:** ARCHITECTURE.md §Data flow — Tier 2 was updated in
the same work to show the implemented order; the tagger formula
`conf = 0.4 + 0.6·primary − 0.2·bestOther` is calibrated for
redacted-text overlap.

## ADR-037: INV-TEL-012 vendor-name grep + prefix-shape rule IDs

**Date:** 2026-04-24
**Decision:** Files under `src/lib/telemetry/` must not contain any
of `anthropic`, `openai`, `groq`, `ollama`, `openrouter`. Rule IDs
that would otherwise carry those names (e.g. `api_key_openai`)
use prefix-shape names instead (`key_sk_prefix`).
**Context:** INV-TEL-012 bans LLM-provider SDK imports on the
telemetry path. Vendor-name grep is the CI-enforceable approximation
of that rule — no string match in the directory, no accidental
import possible. Rule-ID naming had to be reconciled with this.
**Alternatives:**
- Allow vendor names as string literals, only ban `import` — rejected:
  grep is the canonical check and it can't distinguish imports from
  string literals without a full parser. Allowing literals creates a
  future regression path the grep can't catch.
- Use hashes or opaque IDs (`KEY_01`, `KEY_02`) — rejected: audit
  rows would be unreadable to instructors.
**Rationale:** Key prefix shape is a stable, vendor-agnostic way to
describe what a regex matches (`sk-ant-...` vs `sk-...` vs `AIza...`)
and reads clearly in the `redaction_audit` rule_id column.
REDACTION.md carries the prefix→vendor mapping for the instructor-
facing side so the vendor names are documented exactly once, outside
the grep-enforced directory.
**Consequence:** Any new `key_*` rule in the redactor or uploader
preflight uses the prefix shape, not the vendor. The CI grep is
in `.github/workflows/ci.yml` alongside `npm test`.

## ADR-038: `parkPermanent` sentinel (attempt_count = 999), not row deletion

**Date:** 2026-04-24
**Decision:** Telemetry queue rows that fail permanently (preflight
hit, TLS gate, stale consent, malformed JSON, 4xx-non-401) have
their `attempt_count` set to `999` via `queue.parkPermanent()`.
The row stays in the queue until the retention sweep purges it
(uploaded rows > 30 days OR cohort past `ends_at + 90d`).
**Context:** Initial M2 implementation only incremented
`attempt_count` via `recordFailure` on all failure paths. That meant
a permanent-preflight-fail row would be retried on every app launch,
ratcheting the counter by 1 per launch — and re-running preflight +
audit writes every time. The in-memory `nextRetryAt = Infinity`
gate worked for the current session but was lost on restart.
**Alternatives:**
- Delete the row — rejected: violates INV-TEL-011 (student must be
  able to see every payload, including the rejected ones, in the
  upload-history modal).
- Add a `permanent: bool` column — rejected: schema change for what
  a sentinel value achieves. `attempt_count >= MAX_ATTEMPTS` is
  already the "don't retry this tick" gate; extending it to 999 is
  a natural generalization.
- Keep in-memory only — rejected: doesn't survive restart.
**Rationale:** A sentinel well above `MAX_ATTEMPTS = 5` makes the
"parked" state readable (`last_error` also stores the reason like
`http_400` / `consent_stale_or_withdrawn` / `_preflight_*`). The
uploader's early-exit `row.attempt_count >= MAX_ATTEMPTS` check
skips parked rows in O(1) on every launch, with no network or
audit side-effects.
**Consequence:** MACHINE_STATES.md §16 documents the state.
INVARIANTS.md §INV-TEL-013 (policy drift) and the preflight
references call out `parkPermanent` specifically.

## ADR-039: 5-minute idle gate for Tier 2 collection

**Date:** 2026-04-24
**Decision:** The Tier 2 collector only considers a conversation
eligible when its most recent message is at least `TIER2_IDLE_MS =
5 * 60 * 1000` ms old. The gate is enforced in SQL via the
`idle_conversations` CTE in `collector.ts`.
**Context:** Tier 2 emits one payload per session (per SCHEMA.md).
Without the idle gate, the 10-minute fallback collector tick would
emit a payload for the currently-active conversation whenever it
fired — and the LEFT JOIN on `telemetry_queue.session_id` would
then exclude that conversation from all future ticks, silently
dropping every subsequent message in the same session.
**Alternatives:**
- Emit multiple payloads per session (e.g., one per message batch)
  — rejected: violates SCHEMA.md one-payload-per-session, forces
  the instructor endpoint to reconstruct sessions.
- Emit on session-end only (no fallback) — rejected: a force-quit
  or crash never emits. The 10-min fallback is a durability net.
- Shorter gate (1 min) — rejected: a student pausing to read an
  answer is inside 1 minute. Premature emission remains a risk.
- Longer gate (30 min) — rejected: sessions that end cleanly via
  `clearMessages` wait too long for first emission, delaying
  instructor feedback by a full app-open window.
**Rationale:** 5 minutes is longer than typical think-time inside
an active conversation and short enough that session-end via
`clearMessages` is picked up on the next tick (the session becomes
immediately idle and the SQL LEFT JOIN ensures exactly-once
enqueue). Tracked in code as `TIER2_IDLE_MS` so the tuning is
discoverable and testable.
**Consequence:** ARCHITECTURE.md §Data flow — Tier 2 + MACHINE_STATES
§17 document the gate. A session abandoned mid-typing is collected
after ≥5 min of silence; a session ended cleanly is collected on
the next 10-min tick or 60-sec uploader prime.


## ADR-040: Mirror limitless-academy rename (PM_Academy → Limitless Academy)

**Date:** 2026-05-02
**Decision:** Treat the renamed `limitless-academy` repo as the canonical
editorial source for curriculum, mirror its new layout into the bundled
`src-tauri/lesson_plans/` tree, accept the new module numbering for API
Academy (16 → 18 modules) and Agents Academy (14 → 16 modules), accept
the new tier names ("API basics", "Data", "Production" for API Academy;
"Foundations" expanded to span 01-06 in Agents Academy), and refresh the
Trader Lab day-by-day prompt agenda to match the rewritten lesson plans.
**Context:** The PM_Academy repo was renamed to limitless-academy (the new
umbrella name) and reorganized: `academies/{pm,api,agents}_academy/` and
`programs/{limitless_trader_lab,market_maker_bootcamp}/`. API and Agents
Academy each prepended two new modules (Infrastructure, plus
TraderControlPanel / Your Dashboard) so students learn the deployment
substrate before any agent code. Trader Lab Days 2–6 swapped roles —
deployment moved to Day 2, control panel to Day 3, etc. Agents 14 was
renamed Risk & Kill Switches → Kill Switches, 16 renamed First Production
Agent → First Agent. Tier "Data & Backtesting" → "Data" and "Strategies
& Production" → "Production".
**Alternatives:**
- Pin to the pre-rename layout — rejected: the academy launches alongside
  WorkBuddy; a divergent module index would surface as wrong tier names
  in tutor responses and broken module-detection on the live academy site.
- Maintain two parallel layouts behind a feature flag — rejected: doubles
  the testing surface for a one-time migration; old cohorts won't go back.
- Rebrand "PM Academy" wholesale to "Limitless Academy" — rejected: the
  upstream brand keeps "PM Academy" as a child of the Limitless Academy
  umbrella. Renaming the inner academy would make it harder to tell
  WorkBuddy users which course they're on.
**Rationale:** The bundled lesson plans + the per-module context + the
day-by-day prompt are user-visible tutoring text. Drift from the live
academy means WorkBuddy contradicts the curriculum the student is
actually reading. Tier-name aliases were retained in `getContextReference`
(both old and new names route to the same context) so caches/settings
that snapshotted the old tier name keep working through the transition.
**Consequence:**
- Bundled lesson plan count: 60 → 64 (added Infrastructure × 2, Dashboard,
  TraderControlPanel; PM and Trader Lab counts unchanged at file level
  but Trader Lab content fully refreshed).
- `src-tauri/src/context.rs` `api_modules` and `agents_modules` arrays
  rewritten to match the upstream workbuddy-* `<meta>` tags exactly.
- `workbuddy-extension/manifest.json` keeps a single
  `*://*.limitless.exchange/*` wildcard, which covers both the exchange
  at `limitless.exchange` and the academy at
  `academy.limitless.exchange`. Speculative Railway / custom-domain
  entries were added during the sync and trimmed once the production
  domain was confirmed.
- Future re-alignments are encoded in the `sync-limitless-academy` skill
  at `.claude/skills/sync-limitless-academy/SKILL.md` (10-step playbook
  covering orphan deletion, tier-name aliasing, curriculum.json regen,
  Trader Lab day-by-day drift, and verification gates).

## ADR-041: Phase 0 — WorkBuddy strip, provenance audit, and relicense

**Date:** 2026-07-03
**Decision:** Fork StudyBuddy into WorkBuddy (new repo), strip the
education-specific subsystems, remove all copyleft-licensed components,
and relicense as proprietary (all rights reserved).
**Context:** WorkBuddy's new direction is a cross-platform automatic
work journal (Dayflow-style: periodic capture → LLM analysis →
timeline). The curriculum, cohort-telemetry, and lesson-plan subsystems
are dead weight for that product, and a possible commercial future
requires resolving the AGPL-3.0 license inherited from ADR-007/ADR-019.
**Provenance audit (2026-07-03):** StudyBuddy's git history begins at a
fresh initial commit with no shared ancestry with pluely. Line-level
comparison of every same-named file against pluely upstream (GPL-3.0,
verified live) found 1–9% overlap at both the initial commit and the
current tree, consisting of Tauri boilerplate (`Ok(())`, handler
registration), standard API calls, and two borrowed command names.
Side-by-side reads of capture.rs and window.rs confirmed independent
implementations. Conclusion: ADR-001's word "fork" was inaccurate —
no pluely code was copied, so no GPL derivative-work obligation exists.
**Copyleft removal:** The one real copyleft artifact was the OmniParser
V2 YOLOv8s model (AGPL-3.0 via Ultralytics, ADR-019). ui_detect.rs, the
ort/ndarray/paddle-ocr-rs dependencies, and the model-download UI were
deleted. Pointing survives on the extension → a11y → LLM-estimation
stack.
**Stripped:** curriculum context/prompts/module-matching, lesson plans +
loader + sync scripts, Teach Me mode, cohort telemetry (M1/M2/M4) +
telemetry proxy + Cohort pages, studybuddy-mcp server + MCP
registration, program selection in Settings/Onboarding, academy skins.
**Kept:** multi-LLM streaming, screen capture, browser-extension + a11y
detection, cursor-pointing overlay, STT/TTS, RAG, SQLite history,
Wotch launch integration, diagnostics.
**Consequence:** LICENSE is now a proprietary all-rights-reserved
notice with credits; Cargo.toml license = "LicenseRef-Proprietary".
ADR-007 and ADR-019 are superseded. Rust test count drops from 89 to
~64 (curriculum-matcher tests deleted with the feature); vitest from
126 to 37 (telemetry tests deleted). Docs under docs/ still describe
some removed subsystems — flagged for a follow-up cleanup pass.

## ADR-042: Journal recorder writes screenshots to disk (revokes INV-SEC-004)

**Date:** 2026-07-03
**Decision:** Add a background work-journal recorder
(`src-tauri/src/journal/`) that captures the selected monitor every N
seconds (default 10) as a ≤1080p JPEG q85 into
`<app_data_dir>/recordings/`, with per-shot idle seconds and foreground
window title stored in a Rust-side rusqlite database
(`<app_data_dir>/journal.sqlite`, WAL). This consciously revokes the old
INV-SEC-004 "screenshots never touch disk" invariant.
**Why it's acceptable:** the entire product direction (ADR-041 —
Dayflow-style automatic journal) requires a local frame history to
analyze. Mitigations: **opt-in** (`recorder_enabled` defaults to OFF and
only the user can flip it in Settings), **local-only** (frames are never
uploaded; the analysis pipeline sends sampled frames only to the user's
own configured LLM provider), **bounded** (hourly retention purge
deletes frames + rows older than `recorder_retention_days`, default 14;
idle spans ≥3 min are not captured at all), and **user-purgeable** (the
recordings dir is plain files the user can delete; deleting them never
breaks the timeline, which lives in separate tables).
**Idle/lock handling (honest description):** idle seconds come from
`GetLastInputInfo` on Windows, `ioreg` HIDIdleTime on macOS, and
`xprintidle` on Linux (0 on failure — degrades toward capturing).
Session lock is detected heuristically (empty foreground title + idle
≥ 60s ⇒ skip), NOT via `WTSRegisterSessionNotification` — the proper
session-change message pump is a follow-up. A locked Windows session
also can't be captured by xcap in practice, so the heuristic only
avoids writing black/garbage frames.
**Schema:** all journal tables (screenshots, analysis_batches,
batch_screenshots, observations, timeline_cards, llm_calls,
daily_standup_entries) are created in Phase 1 so Phases 2–4 are
schema-stable. Analysis products are retained past frame expiry — the
journal outlives its raw material.
**Consequence:** new config fields (recorder_enabled,
recorder_interval_secs, recorder_retention_days, analysis_provider,
analysis_model); new commands (recorder_start/stop/status,
journal_list_screenshots, journal_read_screenshot); `chrono` added
(MIT/Apache-2.0); the recorder auto-resumes on app start when enabled.
Threat-model docs still describing INV-SEC-004 are stale until the
docs cleanup pass.
