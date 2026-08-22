> STALE — describes the WorkBuddy base; authoritative specs live in docs/plans/

# WorkBuddy — Architecture

## High-Level Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                   Student's Desktop                         │
│                                                             │
│  ┌────────┐  ┌────────┐  ┌──────────┐  ┌───────────────┐   │
│  │Browser │  │  IDE   │  │ Terminal │  │   Limitless   │   │
│  │(Academy│  │(VS Code│  │ (bot     │  │   Exchange    │   │
│  │modules)│  │ etc.)  │  │  logs)   │  │   (trading)   │   │
│  └───┬────┘  └────────┘  └──────────┘  └───────┬───────┘   │
│      │            │           │                 │            │
│      │ ┌──────────┴───────────┴─────────────────┘            │
│      │ │                                                     │
│      │ │         ┌──────────▼──────────┐                     │
│      │ │         │    WorkBuddy       │  Floating overlay   │
│      │ │         │    (Tauri App)      │  Always-on-top      │
│      │ │         └──────────┬──────────┘  54px / 600px       │
│      │ │                    │                                 │
│      │ └────────────────────┤ Window title detection          │
│      │                      │                                 │
│      ▼                      │                                 │
│  ┌─────────────────────┐    │                                 │
│  │  Browser Extension  │    │  HTTP POST /scan                │
│  │  (content.js)       ├────┘  to 127.0.0.1:19521             │
│  │  DOM scanner +      │       (instant element detection)    │
│  │  CSS highlights     │                                      │
│  └─────────────────────┘                                      │
│                         │                                     │
│              ┌──────────▼──────────┐                         │
│              │   🎤 Microphone     │  Push-to-talk input     │
│              │   📍 Cursor Overlay │  Points at screen       │
│              │   💾 SQLite DB      │  Persistent history     │
│              └──────────┬──────────┘                         │
└─────────────────────────┼────────────────────────────────────┘
                          │
               ┌──────────▼──────────┐
               │   External APIs      │
               │  Anthropic (Claude)  │  Vision + Chat + SSE + tool_use
               │  OpenAI (GPT/Whisper)│  Chat + Whisper STT + embeddings
               │  Google (Gemini)     │  Chat + Vision + TTS + STT
               │  Groq (Llama)        │  Chat (fast inference)
               │  Ollama (local)      │  Chat (free, offline)
               │  OpenRouter (100+)   │  Chat (model routing)
               │  ElevenLabs          │  Text-to-Speech + Scribe STT
               └─────────────────────┘
```

## Component Architecture

### Rust Backend (Tauri Commands)

```
lib.rs
  │
  ├── HttpClient (managed state)
  │     Shared reqwest::Client with 10s connect / 60s request timeout
  │     Used by: llm.rs, tts.rs, stt.rs, config.rs (validation)
  │
  ├── capture.rs ──── xcap crate + detection stack
  │     capture_to_base64()     → CaptureResult { base64, width, height, detected_elements }
  │     start_region_capture()  → Region selection (delegates to full-screen)
  │     Detection stack (priority order):
  │       1. Extension data (instant, <10s freshness threshold)
  │       2. Accessibility tree (20-200ms, 800ms timeout, ≥5 elements required)
  │       3. YOLO+OCR (fallback, 150ms-8s)
  │     Captures monitor offset (mon_offset) for a11y coordinate reconciliation
  │     Deps: xcap, image, base64, extension state, a11y module
  │
  ├── a11y.rs ──── Cross-platform accessibility-tree reader
  │     get_foreground_elements(max_depth) → Vec<UIElement>
  │     detect_ui_elements Tauri command (gated by a11y_detection_enabled)
  │     format_elements(&elements, mon_offset, mon_size) → prompt-ready text
  │     Platform modules: a11y/windows_impl.rs (UIA, real), a11y/macos_impl.rs
  │       (AX stub), a11y/linux_impl.rs (AT-SPI2 stub)
  │     Windows: uiautomation 0.24 via spawn_blocking (COM MTA isolation)
  │       Control view walker, depth-limited, interactive-role filter,
  │       returns Button/Tab/TreeItem/MenuItem/Edit/Link/etc.
  │     Output: UIElement { name, role, bounding_rect, automation_id, depth }
  │     Coordinates: physical screen pixels (primary monitor origin)
  │     Filtering: unlabelled elements skipped, 200-element cap, 80-char name truncation
  │
  ├── ui_detect.rs ──── OmniParser V2 (final fallback)
  │     detect_elements() → YOLOv8s ONNX inference (~150-400ms CPU)
  │     detect_text() → PaddleOCR (~2-8s)
  │     format_all_detections() → prompt-ready text
  │     Deps: ort, paddle-ocr-rs, ndarray, image
  │
  ├── llm.rs ──── Multi-provider LLM client
  │     stream_response()      → SSE streaming with Tauri events
  │     chat_with_vision()     → Single request with screenshot
  │     list_providers()       → Available providers + models
  │     Providers: Anthropic, OpenAI, Google, Groq, Ollama, OpenRouter
  │     Anthropic tools: point_at, highlight (cursor pointing via tool_use API)
  │     Emits: chat_stream_chunk, chat_stream_complete, tool_use_complete
  │     Tool_use parsing: content_block_start → input_json_delta → content_block_stop
  │     Deps: HttpClient state, config.rs (api key per provider)
  │
  ├── config.rs ──── Settings persistence
  │     get/set_api_key()    → Individual key management
  │     get/set_settings()   → Preferences incl. tutor_mode (excludes api_keys)
  │     validate_api_key()   → Test Anthropic API connectivity
  │     Storage: ~/.config/workbuddy/config.json (0600 perms)
  │     State: static Mutex<Option<AppConfig>> with poison recovery
  │
  ├── context.rs ──── Window detection
  │     detect_active_window() → OS-specific title detection
  │     match_curriculum_context() → Title → program/module/tier
  │     Linux: xdotool | macOS: osascript | Windows: PowerShell
  │
  ├── shortcuts.rs ──── Global keyboard shortcuts
  │     setup_shortcuts()  → Register hotkeys via tauri-plugin-global-shortcut
  │     Ctrl+Shift+S → toggle-visibility event
  │     Ctrl+Shift+X → trigger-screenshot event
  │     Ctrl+Shift+F → focus-text-input event
  │     Ctrl+Space   → push-to-talk event
  │
  ├── microphone.rs ──── Audio capture
  │     start_mic_capture() → Open default input, capture with VAD
  │     stop_mic_capture()  → Stop recording, return remaining audio
  │     VAD: RMS threshold (0.012) + peak amplitude (0.035)
  │     Pre-speech buffer (12 chunks), silence detection (45 chunks)
  │     Emits: mic-speech-detected (base64 WAV)
  │     Deps: cpal, hound
  │
  ├── stt.rs ──── Speech-to-Text (3 providers)
  │     transcribe_audio() → dispatches on config.stt_provider
  │       "whisper"    → OpenAI Whisper API (multipart /audio/transcriptions)
  │       "elevenlabs" → ElevenLabs Scribe (multipart /v1/speech-to-text)
  │       "gemini"     → Gemini 2.5 Flash via inlineData (base64 WAV directly,
  │                      strip_transcript_artifacts() cleans prefixes/quotes)
  │     Unit tests: strips_transcript_prefix, preserves_internal_quotes, etc.
  │     Guards: 10MB base64 cap on Gemini (inline limit is 20MB request total)
  │     Deps: HttpClient state, config.rs (openai, elevenlabs, or google key)
  │
  ├── tts.rs ──── Text-to-Speech (2 providers)
  │     synthesize_speech() → dispatches on provider arg (falls back to config.tts_provider)
  │       "elevenlabs" → ElevenLabs eleven_flash_v2_5 (returns MP3)
  │       "gemini"     → Gemini 2.5 Flash Preview TTS (returns raw PCM →
  │                      pcm_to_wav() wraps in 44-byte WAV header, 24kHz/16-bit/mono)
  │     Retry: 3 attempts on Gemini 500 errors (documented randomness)
  │     list_tts_voices(provider) → 30 Gemini voices + 10 ElevenLabs voices
  │     Unit tests: test_pcm_to_wav_header, test_pcm_to_wav_empty
  │     Deps: HttpClient state, config.rs (elevenlabs or google key)
  │
  ├── pointer.rs ──── Cursor annotation + overlay window
  │     parse_point_tags()   → Extract [POINT:x,y:label:screen]
  │     show_pointer()       → Show cursor_overlay window + emit pointer_show event
  │     hide_pointer()       → Hide cursor_overlay window + emit pointer_hide event
  │
  ├── rag.rs ──── Document RAG (Retrieval-Augmented Generation)
  │     ingest_document()       → Chunk markdown, embed via OpenAI, store in SQLite
  │     ingest_all_documents()  → Batch-ingest all .md files in a directory
  │     search_docs()           → Embed query, cosine similarity search, return top-K
  │     get_ingestion_status()  → Chunk count, source files, last indexed timestamp
  │     clear_doc_index()       → Delete all indexed chunks
  │     Storage: rag_vectors.db (separate from main app DB, in OS config dir)
  │     Embedding: OpenAI text-embedding-3-small (1536 dims)
  │     Search: In-memory cosine similarity (~300 chunks = <10ms)
  │     Deps: rusqlite (bundled), HttpClient state, config.rs (OpenAI key)
  │
  ├── extension.rs ──── Browser extension HTTP server
  │     start_extension_server() → TCP listener on 127.0.0.1:19521
  │     handle_connection()      → Routes: /status, /scan, /highlight
  │     get_extension_status()   → Tauri cmd: connection info for Settings
  │     extension_highlight()    → Tauri cmd: queue highlight for extension
  │     regenerate_extension_token() → Tauri cmd: rotate auth token
  │     State: Arc<tokio::sync::Mutex<ExtensionState>> shared with capture.rs
  │     Auth: 256-bit hex token in %APPDATA%/workbuddy/extension-token
  │     No framework — raw tokio::net::TcpListener with manual HTTP parsing
  │
  └── window.rs ──── Window management
        setup_main_window()  → Center at top of primary monitor
        set_window_height()  → Toggle 54px ↔ 600px
        toggle_visibility()  → Show/hide
```

### React Frontend

```
App.tsx
  │
  ├── AppProvider (contexts/app.context.tsx)
  │     Global state via React Context
  │     ├── settings: AppConfig (provider, model, program, api_keys, tutor_mode, toggles)
  │     ├── messages: Message[] (conversation history)
  │     ├── isStreaming: boolean
  │     ├── currentContext: WindowContext (detected module)
  │     ├── currentPage: "chat" | "settings" | "history" | "onboarding"
  │     ├── isOnboarded: boolean
  │     ├── screenshotDims: { width, height } | null
  │     ├── currentConversationId: string | null
  │     └── SQLite persistence: saves messages on chat_stream_complete
  │
  ├── ChatBar.tsx ──── Input + actions
  │     Text input → invoke("stream_response")
  │     Auto-screenshot on submit → invoke("capture_to_base64")
  │     Push-to-talk mic button → useMicrophone hook
  │     Tutor mode toggle (BookOpen icon) → Socratic system prompt injection
  │     Streaming: listen("chat_stream_chunk") → update messages in-place
  │     Tool-use: listen("tool_use_complete") → invoke("show_pointer") for pointing
  │     Streaming TTS: pipes chunks through SentenceBuffer → TTSQueue
  │     Global shortcut listeners: trigger-screenshot, focus-text-input
  │     Context badge shows detected module
  │
  ├── ResponsePanel.tsx ──── Output display
  │     ReactMarkdown + remarkGfm rendering
  │     Code blocks with CopyButton
  │     Links open in system browser via plugin-shell open()
  │     TTS Listen button → invoke("synthesize_speech") → Web Audio API
  │     Point tag parsing: strips [POINT:x,y:label] and emits pointer_show
  │
  ├── CursorOverlayWindow.tsx ──── Screen pointing (separate Tauri window)
  │     Full-screen transparent window (cursor_overlay), click-through
  │     Spring-physics animation (SpringValue, stiffness=170, damping=18)
  │     SVG mask spotlight: dark overlay (45%) with bright elliptical cutout
  │     Pulse ring animation on spotlight edge + radial glow gradient
  │     Trail ghosts (3 previous positions at decreasing opacity)
  │     Auto-dismiss after 3s, Escape to dismiss, sequential point queue
  │     Coordinate mapping: (pointX / screenshotWidth) * fullScreenWidth
  │
  ├── hooks/useMicrophone.ts ──── Push-to-talk
  │     Listens for mic-speech-detected events
  │     Calls transcribe_audio, inserts text, auto-submits
  │     Exposes: isRecording, startRecording(), stopRecording()
  │
  ├── lib/springPhysics.ts ──── Spring animation
  │     SpringValue: damped harmonic oscillator (stiffness=170, damping=18)
  │     flyTo(): quadratic Bezier with easeOutExpo timing
  │
  ├── lib/sentenceBuffer.ts ──── Streaming TTS sentence extraction
  │     SentenceBuffer: splits streamed text on .!?\n boundaries
  │     Handles abbreviations (Dr., e.g.), decimals (3.14), min 10 chars
  │
  ├── lib/ttsQueue.ts ──── Sequential TTS playback queue
  │     TTSQueue: enqueue sentences, play sequentially, rate limit (1 req/sec)
  │     Provider-aware: setProviderGetter() wires tts_provider, MIME type
  │       switches between audio/wav (Gemini) and audio/mpeg (ElevenLabs)
  │     Cancel support for mid-stream interruption
  │
  ├── lib/curriculum/ ──── Context injection system
  │     prompts.ts: 7 profiles with {module_context} template + tutor mode + RAG + tool-use instructions
  │     context/index.ts: Router (module-level → tier-level fallback)
  │     context/module_map.ts: 52 module → snippet keys mapping + resolver
  │     context/topics/: 23 curated topic snippet files from Limitless docs
  │       (what_is_limitless, clob_orderbook, order_types, fees_detailed,
  │        market_resolution, merge_split, negrisk, wallet_types,
  │        making_first_trade, managing_orders, lp_rewards, venue_and_signing,
  │        developers_intro, eip712_signing, smart_contracts, api_tokens,
  │        delegated_orders, partner_accounts, market_pages,
  │        migrate_polymarket, changelog, quickstart_patterns, websocket_events)
  │
  ├── lib/pointParser.ts ──── Point tag parsing
  │     parsePointTags(text) → { cleanText, points: PointTarget[] }
  │     TypeScript port of pointer.rs regex (avoids IPC round-trip)
  │
  ├── lib/db.ts ──── SQLite persistence
  │     Schema: conversations (id, created_at, program, module_id)
  │             messages (id, conversation_id, role, content, timestamp)
  │     Functions: saveConversation, saveMessage, loadConversations,
  │                loadMessages, deleteConversation
  │     Screenshots never stored (INV-SEC-004)
  │
  └── pages/
        Settings.tsx   ── Provider, API keys, model, program, tutor mode, extension, STT, RAG indexing, about
        Onboarding.tsx ── 5-step wizard (welcome, key, program, shortcuts, ready)
        History.tsx    ── SQLite-backed conversations (expandable, deletable)
```

## Data Flow: Browser Extension Scan

```
1. Content script scans DOM for visible interactive elements
   → querySelectorAll(buttons, links, headings, inputs, roles)
   → Filter: non-zero size, in viewport, non-empty text
   → Deduplicate via Set
2. Content script sends element array to background service worker
   → chrome.runtime.sendMessage({ type: 'scan', data: { url, title, elements } })
3. Background worker POSTs to http://127.0.0.1:19521/scan
   → Includes Authorization: Bearer <token>
   → WorkBuddy validates token, stores elements in ExtensionState
4. Response may include pending highlights
   → Background relays to content script
   → Content script injects CSS overlays into the page DOM
5. Cycle repeats every 3 seconds
   → Highlights polled separately every 300ms via GET /highlight
```

## Data Flow: Student Asks a Question

```
1. Student types in ChatBar and presses Enter
2. ChatBar calls invoke("capture_to_base64")
   → capture.rs runs the detection stack in priority order:
     (a) Extension state: has_fresh_data()? (<10s since last scan)
         → If YES: use extension DOM elements (<10ms, pixel-precise) — skip rest
     (b) Accessibility tree (if a11y_detection_enabled + no extension data)
         → Query UIA/AX/AT-SPI2 with 800ms timeout, require ≥5 elements
         → If YES: reconcile screen coords to capture space via mon_offset,
                   format as "[Role] \"name\" center=(x,y) rect=..." list
     (c) YOLO+OCR (if ui_detection_enabled + nothing above produced data)
         → Runs OmniParser YOLOv8s (150-400ms) + PaddleOCR (2-8s)
   → Capture primary (or selected) monitor via xcap, encode JPEG q=85
   → Returns CaptureResult { base64, width, height, detected_elements }
   → Dimensions stored in context for CursorOverlay coordinate mapping
3. ChatBar calls invoke("search_docs", { query: text, topK: 5 })
   → rag.rs embeds query via OpenAI text-embedding-3-small
   → Cosine similarity search against ~300 indexed doc chunks
   → Returns top 5 relevant chunks (or empty if no index / no OpenAI key)
   → RAG context injected into system prompt alongside static snippets
4. ChatBar calls invoke("stream_response", { systemPrompt, userMessage,
   screenshotBase64, conversationHistory, model, provider })
   → llm.rs selects provider config (API URL, auth, format)
   → Builds request with screenshot as image content block
   → System prompt from prompts.ts: base prompt + tutor mode (if enabled) + module snippets + RAG context
5. llm.rs sends POST to provider API with stream:true
   → Anthropic: parses event:/data: lines, message_stop terminates
   → OpenAI-compatible: parses data: lines, [DONE] terminates
   → On content delta: emits "chat_stream_chunk" with text
   → On completion: emits "chat_stream_complete"
6. ChatBar listens for chat_stream_chunk events
   → Updates streaming message in-place via setMessages
7. On chat_stream_complete:
   → Finalizes message with unique ID (replaces "streaming")
   → Sets isStreaming = false
   → app.context saves user + assistant messages to SQLite
   → ResponsePanel parses finalized message for [POINT:x,y:label] tags
   → Stripped tags → clean display; points → pointer_show events
8. If TTS enabled (and the selected provider's key is configured):
   → Streaming: each sentence boundary triggers TTSQueue.enqueue with provider
     getter returning settings.tts_provider
   → Or: User clicks Listen → invoke("synthesize_speech", { provider, ... })
   → tts.rs dispatches on provider:
     - ElevenLabs → base64 MP3 → Audio(data:audio/mpeg;base64,...)
     - Gemini    → raw PCM wrapped in WAV header → Audio(data:audio/wav;base64,...)
   → MIME type must match provider or playback silently fails (INV-ARCH-011)
```

## Data Flow: Push-to-Talk

```
1. Student holds the mic button in ChatBar
2. ChatBar calls invoke("start_mic_capture")
   → microphone.rs opens default input device via cpal
   → Captures f32 samples, runs VAD (RMS + peak threshold)
   → Pre-speech buffer catches the start of utterances
3. When student releases the mic button:
   → ChatBar calls invoke("stop_mic_capture")
   → Any remaining speech audio is emitted as mic-speech-detected
4. On speech detected (silence threshold reached, or manual stop):
   → microphone.rs encodes speech to 16-bit WAV via hound
   → Emits "mic-speech-detected" event with base64 WAV
5. useMicrophone hook receives the event:
   → Calls invoke("transcribe_audio", { audio: base64WAV })
   → stt.rs dispatches on settings.stt_provider:
     - "whisper"    → multipart POST to OpenAI /v1/audio/transcriptions
     - "elevenlabs" → multipart POST to /v1/speech-to-text with scribe_v1
     - "gemini"     → generateContent with inlineData (base64 WAV directly,
                      10MB cap, strip_transcript_artifacts cleans output)
   → Returns transcribed text
6. Hook inserts text into ChatBar input and auto-submits
   → Normal chat flow continues from step 1 of question flow
```

## Data Flow: Cursor Pointing

```
1. Claude's response contains [POINT:450,320:Place Order:0] tags
2. On chat_stream_complete, ResponsePanel:
   → Parses finalized message through parsePointTags()
   → Strips tags from displayed message content
   → Calls invoke("show_pointer", { target }) for each point
3. pointer.rs emits pointer_show event to frontend
4. CursorOverlay receives the event:
   → Maps coordinates: screenX = (pointX / screenshotWidth) * windowWidth
   → Renders blue MousePointer2 icon with label pill
   → Animates with cubic-bezier transition
   → Auto-dismisses after 3s, or on Escape key
   → Queues multiple points and shows them sequentially (1s delay)
```

## Data Models

### AppConfig (Rust + TypeScript)
```
{
  api_keys: { anthropic?: string, openai?: string, google?: string,
              groq?: string, openrouter?: string, elevenlabs?: string,
              stt?: string, ollama_url?: string },
  provider: "anthropic" | "openai" | "google" | "groq" | "ollama" | "openrouter",
  model: string,          // Provider-specific model ID
  program: "pm_academy" | "api_academy" | "agents_academy" | "limitless_trader_lab",
  auto_screenshot: boolean,
  tts_enabled: boolean,
  tts_voice: string,           // Voice ID; meaning depends on tts_provider
  tts_provider: "elevenlabs" | "gemini",   // default "elevenlabs" (backward compat)
  stt_provider: "whisper" | "elevenlabs" | "gemini",   // default "whisper"
  theme: "dark",
  tutor_mode: boolean,
  capture_monitor: string,      // "auto" or numeric monitor index
  ui_detection_enabled: boolean,
  ocr_quality: "fast" | "quality",
  cursor_overlay_enabled: boolean,
  a11y_detection_enabled: boolean   // default true
}
```

### UIElement (Rust → TypeScript, from a11y.rs)
```
{
  name: string,            // "Save" button, "Explorer" panel
  role: string,            // "Button", "Tab", "TreeItem", "Edit", etc.
  bounding_rect: { x: number, y: number, width: number, height: number },
  automation_id: string,   // Programmatic ID (e.g., "workbench.action.files.save")
  depth: number            // Tree depth (1-N from root window)
}
```
Coordinates are in physical screen pixels, primary monitor origin. `capture.rs`
reconciles them to the captured monitor's space via `format_elements`.

### CaptureResult (Rust → TypeScript)
```
{
  base64: string,    // PNG image data
  width: number,     // Screenshot width in pixels
  height: number     // Screenshot height in pixels
}
```

### Message (TypeScript)
```
{
  id: string,           // crypto.randomUUID() or "streaming" during active stream
  role: "user" | "assistant",
  content: string,      // Plain text for user, markdown for assistant
  screenshot?: string,  // base64 PNG (not persisted — INV-SEC-004)
  timestamp: number     // Date.now()
}
```

### WindowContext (Rust → TypeScript)
```
{
  title: string,              // Raw window title from OS
  program: string | null,     // "pm_academy", "api_academy", "ide", etc.
  module_id: string | null,   // "01", "03", "16", etc.
  module_title: string | null, // "PM 101", "Orders", etc.
  tier: string | null          // "Fundamentals", "Real-Time", etc.
}
```

### SQLite Schema (workbuddy.db — via tauri-plugin-sql)
```sql
conversations (id TEXT PK, created_at INTEGER, program TEXT, module_id TEXT)
messages (id TEXT PK, conversation_id TEXT FK, role TEXT, content TEXT, timestamp INTEGER)
```

### RAG Vector Store (rag_vectors.db — via rusqlite, separate file)
```sql
doc_chunks (id INTEGER PK, source_file TEXT, chunk_index INTEGER,
            content TEXT, embedding BLOB, program_hint TEXT, created_at INTEGER)
```
Embeddings are serialized f32 arrays (1536 × 4 = 6144 bytes per chunk).
Cosine similarity computed in-memory in Rust (~300 chunks = <10ms).

## External Dependencies and Failure Modes

| Dependency        | Failure Mode                           | Handling                          |
|-------------------|----------------------------------------|-----------------------------------|
| Anthropic API     | Rate limit, network error, invalid key | Error shown in chat as message    |
| OpenAI API        | Rate limit, network error              | Error shown in chat as message    |
| Google Gemini API | Rate limit, network error              | Error shown in chat as message    |
| Groq API          | Rate limit, network error              | Error shown in chat as message    |
| Ollama (local)    | Not running, wrong URL                 | Connection error in chat          |
| OpenRouter API    | Rate limit, network error              | Error shown in chat as message    |
| ElevenLabs TTS API| Network error, invalid key             | Listen button hidden or error     |
| ElevenLabs STT API| 401 (missing Scribe permission)         | Transcription fails, error shown  |
| Gemini TTS API    | 500 random failure, PROHIBITED_CONTENT  | 3-attempt retry, then error       |
| Gemini STT API    | Silent audio, blocked content           | Returns empty string (not error)  |
| Whisper API       | Network error, invalid key             | Transcription fails silently      |
| OpenAI Embeddings | Network error, invalid key             | RAG disabled, static snippets only|
| Extension server  | Port busy, extension not installed      | Falls back to a11y, then YOLO+OCR |
| Accessibility API | Permission denied (macOS), daemon off   | Returns empty, falls back to YOLO |
| Windows UIA       | Foreground window unavailable          | Returns empty, falls back to YOLO |
| xcap (screen)     | Wayland (no X11), permission denied    | Returns error string to frontend  |
| cpal (audio)      | No input device, permission denied     | Mic button shows error            |
| xdotool (Linux)   | Not installed                          | Returns empty title, app works    |
| osascript (macOS) | App has no windows                     | Falls back to app name            |
| SQLite            | Disk full, permission denied           | Tauri plugin handles gracefully   |

## Security Model

1. **No proxy server** — API calls go directly to providers
2. **Keys local-only** — Stored in OS config dir with 0600 permissions
3. **No third-party analytics** — Zero PostHog / Sentry / GA / tracking
   pixels. The optional cohort-telemetry feature
   (`docs/PLAN-cohort-telemetry/`) uploads only to a
   student-configured instructor endpoint, only after explicit
   per-tier consent, and is enforced by the `INV-TEL-*` invariants.
4. **Screenshots ephemeral** — In-memory only, never written to disk or SQLite
5. **Audio ephemeral** — Microphone audio exists only in-memory during recording
6. **AGPL-3.0** — Full source available to students
7. **CSP enforced** — connect-src whitelist for API domains only

## Key Design Decisions

| Decision                            | Rationale                                          |
|-------------------------------------|----------------------------------------------------|
| Tauri 2, not Electron               | ~10MB binary vs ~200MB. Native performance.        |
| Direct API calls, no proxy          | Simpler. No server to maintain. Keys stay local.   |
| React state for navigation          | Tauri webview = single page. URL changes lose state.|
| Shared reqwest::Client              | Connection pooling. Consistent timeouts.           |
| Curriculum detection via window title | No browser extension needed. Works across all apps.|
| Multi-LLM with provider abstraction | Students can use free (Ollama/Groq) or premium models. |
| Client-side point tag parsing       | Avoids IPC round-trip for trivial regex operation.  |
| cpal for audio capture              | Cross-platform (Windows/macOS/Linux) with VAD.     |
| SQLite for conversation history     | Tauri plugin-sql already registered. Survives restarts. |
| AGPL-3.0                            | Fork of pluely + OmniParser YOLO (AGPL-3.0). Educational mission. |
| Composable topic snippets + module map | Each of 52 modules gets tailored context from 23 curated snippets.  |
| RAG with separate SQLite + in-memory cosine | ~300 chunks, <10ms search. No external vector DB needed. |
| Separate rag_vectors.db             | Avoids locking conflicts with tauri-plugin-sql main DB. |
| Anthropic tool_use for pointing     | More reliable than regex parsing of [POINT:] text tags. Fallback kept for non-Anthropic. |
| Spring-physics cursor animation     | Damped harmonic oscillator produces natural motion. No CSS transition limitations. |
| SVG mask spotlight overlay          | Dims full screen with bright cutout — far more visible than a cursor icon alone. |
| Separate cursor_overlay window      | Full-screen transparent click-through window enables pointing anywhere on screen. |
| Streaming sentence TTS              | SentenceBuffer + TTSQueue let students hear responses during streaming. |
| Tutor mode via prompt injection     | Socratic instructions compose with any of 7 base profiles. No new APIs needed. |
| Browser extension for web content   | DOM elements are instant and pixel-precise. YOLO+OCR stays as fallback for non-web apps. |
| HTTP instead of WebSocket (ext)     | MV3 kills service workers after 30s. Request-driven HTTP avoids keepalive hacks. |
| In-page CSS highlighting (ext)      | Viewport-to-screen coordinate mapping is unsolvable (W3C issue #5814). CSS overlay is pixel-perfect. |
| Raw TCP HTTP server (ext)           | 3 endpoints, localhost only. Avoids adding hyper/axum/warp as new dependencies. |
| Accessibility APIs for pointing     | Pixel-precise element names + bounding rects in any app — no model needed, ~20-200ms on Windows UIA. Detection stack: extension → a11y → YOLO+OCR. |
| UIA on spawn_blocking thread        | `uiautomation` crate initializes COM MTA; isolating it on a dedicated thread avoids contaminating the tokio runtime (INV-ARCH-012). |
| Gemini as 3rd STT provider          | Reuses Google API key, ~3x cheaper than Whisper for short utterances, accepts WAV via inlineData. No retry needed — uses gemini-2.5-flash (stable GA). |
| Gemini 3.1 Flash TTS (preview)      | 30 curated voices, reuses Google API key, lower per-char cost than ElevenLabs. Returns raw PCM — manually wrapped in WAV header (44 bytes). |
| 3-attempt retry for Gemini TTS      | Documented limitation: Gemini TTS occasionally returns 500. Retry masks this without user-visible failure; existing `.catch` in TTSQueue silently drops if all 3 fail. |
