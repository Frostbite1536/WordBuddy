# WorkBuddy — Agent Instructions

WorkBuddy is a cross-platform desktop AI assistant built with Tauri 2 (Rust
backend + React/TypeScript frontend). It floats as a thin always-on-top bar,
captures the screen, and streams answers from the user's choice of LLM
provider. The product direction is a Dayflow-style automatic work journal
(recorder → LLM analysis → timeline); see docs/DECISIONS.md ADR-041 for the
Phase 0 strip that produced this tree.

## Tech Stack

- **Backend:** Rust (Tauri 2 commands) — screen capture, multi-LLM, TTS, STT, config, audio, accessibility detection
- **Frontend:** React 19 + TypeScript + Tailwind CSS + Vite
- **Screen capture:** `xcap` crate (cross-platform), JPEG encoding, multi-monitor support
- **Browser extension:** Chrome/Edge MV3 extension for instant DOM-based element detection (<10ms), `workbuddy-extension/`
- **Accessibility detection:** Windows `uiautomation` / macOS + Linux stubs — element names + bounding rects from the focused window
- **Detection stack (priority order):** extension (DOM, <10ms) → a11y (OS tree, 20-200ms) → LLM vision estimation
- **AI:** 6 LLM providers (Anthropic, OpenAI, Google, Groq, Ollama, OpenRouter) with provider-specific SSE parsing + stream cancellation + Anthropic tool_use for cursor pointing
- **TTS:** ElevenLabs (MP3) or Gemini Flash (PCM→WAV). **STT:** Whisper, ElevenLabs, or Gemini Flash
- **Audio capture:** `cpal` + `hound` (microphone with tuned VAD)
- **Database:** SQLite via `tauri-plugin-sql` (conversations, `sqlite:workbuddy.db`) + `rusqlite` (RAG vectors)
- **RAG:** OpenAI text-embedding-3-small, cosine similarity in Rust
- **License:** Proprietary (all rights reserved) — see LICENSE. Do NOT add copyleft-licensed dependencies or models (the AGPL YOLO stack was deliberately removed).

## Directory Structure

```
src-tauri/src/
  lib.rs          Tauri builder, plugin registration, shared HttpClient state
  main.rs         Entry point
  llm.rs          Multi-provider LLM client (vision + streaming SSE + cancellation + tool_use)
  capture.rs      Screen capture via xcap (JPEG, multi-monitor) + extension/a11y detection
  extension.rs    Browser extension HTTP server (127.0.0.1:19521) + token auth + highlight queue
  a11y.rs         Cross-platform accessibility-tree reader — dispatches to a11y/
  config.rs       API keys + settings (JSON in OS config dir `workbuddy/`)
  context.rs      Active window title detection (Win32 / osascript / xdotool)
  shortcuts.rs    Global keyboard shortcuts
  microphone.rs   Audio capture with tuned VAD (cpal + hound)
  stt.rs          Speech-to-text    tts.rs  Text-to-speech
  pointer.rs      [POINT:x,y:label] tag parsing + overlay window
  rag.rs          RAG document indexing + vector search
  journal/        Work journal (ADR-042): recorder.rs (10s capture loop,
                  idle detection, retention), db.rs (journal.sqlite,
                  rusqlite WAL), analyzer.rs (batch assembly + two-stage
                  LLM pipeline + scheduler), prompts.rs, export.rs
                  (markdown), standup.rs (standup LLM + weekly aggregation)
  wotch.rs        Wotch (floating terminal) launch integration
  diagnostics.rs  Local-only rotating log + panic hook
  window.rs       Window positioning and resize
  capabilities/   Tauri 2 IPC permissions

src/
  App.tsx / main.tsx / contexts/app.context.tsx   Shell + global state
  components/ChatBar.tsx        Input bar (screenshot/mic/tutor/send)
  components/ResponsePanel.tsx  Streaming markdown + TTS + point tags
  components/CursorOverlayWindow.tsx  Spring-physics cursor + SVG spotlight
  hooks/useMicrophone.ts        Push-to-talk hook
  pages/ Settings.tsx | Onboarding.tsx | History.tsx | Journal.tsx (timeline/standup/week tabs)
  lib/prompts.ts                System prompt builder (tutor mode + pointing rules + RAG + journal context)
  lib/journal.ts                Journal UI helpers (day math, formatting, standup markdown)
  lib/ springPhysics | sentenceBuffer | ttsQueue | pointParser | db | safeOpen | friendlyError

workbuddy-extension/   Chrome/Edge MV3 extension (DOM scanner + highlight relay)
tests/                 vitest (pointParser, friendlyError, safeOpen)
```

## Build / Test / Lint Commands

```bash
npm install                      # Install frontend deps
npx tsc --noEmit                 # TypeScript type check (zero errors expected)
npx vite build                   # Frontend production build
npm test                         # Vitest unit tests
cd src-tauri && cargo check      # Rust compilation check
cd src-tauri && cargo test       # Rust unit tests
npx tauri dev                    # Run full app in dev mode
npx tauri build                  # Build release binary
```

## Coding Conventions

- **Rust:** Use `Result<T, String>` for Tauri commands. Recover from poisoned
  mutexes with `unwrap_or_else(|e| e.into_inner())`. Use the shared
  `HttpClient` from Tauri state — never create `reqwest::Client` per request.
- **TypeScript:** Use `useCallback` for handlers passed to children. Use refs
  (not state) for values that change during async operations. Navigate via
  `setCurrentPage` — never `window.location.href` (destroys React state).
- **Event listeners:** All `listen()` calls use the cancelled-flag pattern
  (see ChatBar.tsx `safeListen`). Store unlisteners and call them on cleanup.
- **API keys:** Stored in OS config dir with 0600 permissions (Unix). Merge
  keys with `set_api_key` individually — `set_settings` excludes `api_keys`.
- **SSE parsing:** Anthropic uses `event:`/`data:` + `message_stop`;
  OpenAI-compatible providers use `data: [DONE]`. Both live in `llm.rs` —
  do not mix protocols.
- **Stream cancellation:** `STREAM_GENERATION` atomic counter in `llm.rs`.
- **Screenshots:** JPEG quality 85 via `image` crate; captures the monitor
  selected in Settings. Detection runs before JPEG encoding: extension →
  a11y (800ms timeout, ≥5 element gate).
- **TTS MIME type:** Gemini returns WAV (`pcm_to_wav`), ElevenLabs MP3 —
  frontend must pick `audio/wav` vs `audio/mpeg` by `tts_provider`.
- **Audio:** stop and drop mic streams when recording ends (`STREAM_HANDLE`).

## Never Do

1. Never create a `reqwest::Client` per request — use `app.state::<HttpClient>()`
2. Never use `window.location.href` for navigation — use `setCurrentPage()`
3. Never use `.unwrap()` on `Mutex::lock()` — use `.unwrap_or_else(|e| e.into_inner())`
4. Never store API keys in frontend state longer than needed for display
5. Never use `[DONE]` for Anthropic SSE stream termination — use `message_stop`
6. Never add dependencies without verifying they're actually used
7. Never skip the Tauri capabilities file — commands fail silently without IPC permissions
8. Never commit `.env` files or plaintext API keys
9. Never use `target="_blank"` in Tauri — use `open()` from `@tauri-apps/plugin-shell` (via `lib/safeOpen.ts`)
10. Never use `h-screen` in components — the Tauri window is 600px, not viewport height
11. Never register a new Tauri command in only one of `lib.rs` invoke_handler / `capabilities/default.json`
12. Never gate TTS on the ElevenLabs key alone — the gate depends on `tts_provider` (Gemini uses the `google` key)
13. Never call UIA from an async context without `spawn_blocking` — COM MTA must be isolated
14. Never add GPL/AGPL-licensed code, models, or dependencies — the proprietary license depends on staying copyleft-free (ADR-041)

## Multi-Agent Git Safety

1. Work on separate files or clearly separated modules
2. Never force-push or rebase shared branches
3. Commit frequently with descriptive messages; pull before pushing
4. Use feature branches — never commit directly to `main`

## Known staleness

`docs/` still contains StudyBuddy-era documents (ARCHITECTURE, INVARIANTS,
ROADMAP, BUILDOUT, threat model, plans) that describe removed subsystems
(curriculum, telemetry, YOLO). Treat docs/DECISIONS.md ADR-041 + this file
as authoritative until the docs cleanup pass happens.
