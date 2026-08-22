# WorkBuddy — Remaining Buildout Plan

What's done, what's partially done, and what needs to be built.

## Status Summary

| Feature                  | Status         | Code Exists? |
|--------------------------|----------------|-------------|
| Text Q&A + streaming     | Done           | Yes         |
| Screenshot capture       | Done           | Yes         |
| Claude API (vision+SSE)  | Done           | Yes         |
| Markdown rendering       | Done           | Yes         |
| Conversation history     | Done (session) | Yes         |
| TTS (ElevenLabs)         | Done           | Yes         |
| Window detection         | Done           | Yes         |
| Curriculum matching      | Done           | Yes         |
| Context badge            | Done           | Yes         |
| System prompts (7)       | Done           | Yes         |
| Onboarding wizard        | Done           | Yes         |
| Settings page            | Done           | Yes         |
| Window management        | Done           | Yes         |
| Pointer tag parsing      | Done           | Yes         |
| Cursor overlay UI        | Done           | Yes         |
| Push-to-talk mic         | Done           | Yes         |
| Speech-to-text           | Done           | Yes         |
| Global shortcuts         | Done           | Yes         |
| Persistent history (DB)  | Done           | Yes         |
| App icon / branding      | Done (About)   | Yes         |
| CI/CD                    | Done           | Yes         |
| Auto-update              | Done           | Yes         |
| Per-module context (52)  | Done           | Yes         |
| Topic snippets (23)      | Done           | Yes         |
| RAG vector search        | Done           | Yes         |
| RAG Settings UI          | Done           | Yes         |
| Tool-use pointing        | Done           | Yes         |
| Spring-physics cursor    | Done           | Yes         |
| SVG spotlight overlay    | Done           | Yes         |
| Streaming sentence TTS   | Done           | Yes         |
| UI element context       | Done           | Yes         |
| Full-screen overlay window | Done         | Yes         |
| Tutor mode (Socratic)    | Done           | Yes         |
| JPEG screenshots         | Done           | Yes         |
| Multi-monitor selector   | Done           | Yes         |
| Stream cancellation      | Done           | Yes         |
| Content-based timeout    | Done           | Yes         |
| Anti-hallucination prompt| Done           | Yes         |
| ElevenLabs STT           | Done           | Yes         |
| Voice VAD tuning         | Done           | Yes         |
| Close button (exit)      | Done           | Yes         |
| OmniParser UI detection  | Done           | Yes         |
| License → AGPL-3.0       | Done           | Yes         |
| Browser extension        | Done           | Yes         |
| Extension HTTP server    | Done           | Yes         |
| Extension capture integ. | Done           | Yes         |
| Extension Settings UI    | Done           | Yes         |
| Gemini STT (3rd provider)| Done           | Yes         |
| Gemini 3.1 Flash TTS     | Done           | Yes         |
| 30 Gemini voices + selector | Done        | Yes         |
| TTS provider dispatch    | Done           | Yes         |
| pcm_to_wav helper        | Done           | Yes         |
| Accessibility detection (UIA) | Done      | Yes (Windows)|
| a11y macOS stub          | Done           | Yes (stub)  |
| a11y Linux stub          | Done           | Yes (stub)  |
| Detection stack ordering | Done           | Yes         |
| a11y coord reconciliation | Done          | Yes         |
| list_tts_voices command  | Done           | Yes         |
| Unit tests: a11y format  | Done           | Yes (6)     |
| Unit tests: STT artifacts| Done           | Yes (4)     |
| Unit tests: WAV header   | Done           | Yes (2)     |
| mask_form_inputs toggle  | Done           | Yes         |
| INV-SEC-008 (password scrub) | Done       | Yes         |
| Cohort enrollment UI (M1)  | Done         | Yes         |
| Consent receipts + CRUD (M1) | Done       | Yes         |
| SQLite migrations: telemetry tables (M1) | Done | Yes  |
| Tier 1 collector (M2)    | Done           | Yes         |
| Telemetry uploader (M2)  | Done           | Yes         |
| Uploader preflight INV-TEL-002/003 (M2) | Done | Yes   |
| Retention sweep + enqueue gate (M2) | Done | Yes      |
| Upload-history UI modal (M2) | Done       | Yes         |
| Tier 2 redactor (M4)     | Done           | Yes         |
| Tier 2 tagger (M4)       | Done           | Yes         |
| Tier 2 collector (M4)    | Done           | Yes         |
| Practice-mode toggle (M4) | Done          | Yes         |
| Redaction audit summary UI (M4) | Done    | Yes         |
| Re-consent diff UI (M4)  | Done           | Yes         |
| Vitest harness + redactor matrix (M4) | Done | Yes (53) |
| Live dev-endpoint E2E test (M2 exit) | Pending | No — needs running workbuddy-cohort-server |
| M3 pilot cohort           | Pending       | N/A — instructor + students activity |
| Adversarial redactor test set (M4) | Pending | No — needs M3 pilot data |
| Withdraw + delete-remote flow (M5) | Planned | No |
| Export-my-data flow (M5/M6) | Planned     | No |

---

## Work Item 1: Cursor Pointing Overlay

**What exists:** `pointer.rs` parses `[POINT:x,y:label:screenN]` tags,
`show_pointer`/`hide_pointer` commands emit Tauri events. Unit tests pass.

**What's missing:**

### 1a. Wire parsing into streaming pipeline
- **Where:** `src/components/ChatBar.tsx` or `ResponsePanel.tsx`
- **What:** After `chat_stream_complete`, parse the finalized response
  through `parse_point_tags` (call it from frontend via a new Tauri command,
  or parse client-side with a TS port of the regex)
- **Decision:** Client-side parsing is simpler — the regex is trivial in TS.
  No need for another IPC round-trip.
- **Effort:** ~20 lines of TypeScript

### 1b. CursorOverlay component
- **Where:** New file `src/components/CursorOverlay.tsx`
- **What:**
  - Full-screen transparent overlay (position: fixed, pointer-events: none)
  - Blue cursor icon (Lucide `MousePointer2`) at the target position
  - Label text with background pill next to the cursor
  - Bezier arc animation from previous position to target
  - Auto-dismiss after 3 seconds
  - Escape key to dismiss immediately
  - Support multiple sequential points (queue and animate one by one)
- **Mount:** In `App.tsx`, always rendered alongside ChatBar/ResponsePanel
- **Listens for:** `pointer_show` and `pointer_hide` Tauri events
- **Effort:** ~150 lines of TypeScript + CSS animation

### 1c. Multi-monitor coordinate mapping
- **Where:** `CursorOverlay.tsx` or new utility
- **What:** The `[POINT:x,y]` coordinates are relative to the screenshot
  dimensions. The overlay needs to map screenshot coordinates to screen
  coordinates. This requires knowing the screenshot dimensions (width/height
  returned alongside base64 from `capture_to_base64`).
- **Change needed in capture.rs:** Return `{ base64: String, width: u32, height: u32 }`
  instead of just the base64 string
- **Effort:** ~30 lines Rust + 20 lines TypeScript

**Total effort: ~1-2 days**

---

## Work Item 2: Global Keyboard Shortcuts

**What exists:** `tauri-plugin-global-shortcut` is registered in `lib.rs`.
Onboarding displays shortcut reference. No handlers are wired.

**What's missing:**

### 2a. Register shortcuts in setup
- **Where:** `src-tauri/src/lib.rs` setup closure, or new `shortcuts.rs`
- **What:** Register global hotkeys:
  - `Ctrl+Shift+S` → toggle window visibility
  - `Ctrl+Shift+X` → take screenshot and focus input
  - `Ctrl+Shift+F` → focus text input
- **API:** `app.global_shortcut().on_shortcut("ctrl+shift+s", |app, _| { ... })`
- **Effort:** ~40 lines of Rust

### 2b. Push-to-talk shortcut (deferred)
- `Ctrl+Space` push-to-talk requires mic capture (Work Item 3)
- Register the shortcut but emit an event; mic capture handles it

**Total effort: ~0.5 day**

---

## Work Item 3: Push-to-Talk Microphone Input

**What exists:** Empty `src-tauri/src/microphone/` and `speaker/` dirs.
Disabled Mic button in ChatBar. Mentioned in onboarding shortcuts.

**What's missing:**

### 3a. Microphone capture (Rust)
- **Where:** New `src-tauri/src/microphone.rs`
- **Dependencies:** Add `cpal = "0.15"` back to Cargo.toml
- **What:**
  - Enumerate mic devices via `cpal`
  - Start/stop recording on command (`start_mic`, `stop_mic`)
  - Capture audio as f32 samples
  - Voice Activity Detection: RMS energy + peak amplitude thresholds
  - Encode to 16-bit WAV, base64-encode, emit `mic-speech-detected` event
- **Reference:** Pluely's `speaker/commands.rs` for VAD parameters
- **Effort:** ~200 lines of Rust

### 3b. Speech-to-text (Rust)
- **Where:** New `src-tauri/src/stt.rs`
- **Dependencies:** Uses shared `HttpClient`
- **What:**
  - `transcribe_audio` command: takes base64 WAV, sends to STT API
  - Support Whisper API (OpenAI-compatible): multipart POST with audio file
  - Return transcribed text
- **Fallback:** Browser Web Speech API (free, no key needed, lower quality)
- **Effort:** ~80 lines of Rust

### 3c. Frontend mic UI
- **Where:** `src/components/ChatBar.tsx`, new `src/hooks/useMicrophone.ts`
- **What:**
  - Listen for `mic-speech-detected` events
  - Send audio to STT, insert transcribed text into input, auto-submit
  - Visual indicator (pulsing mic icon) during recording
  - Press-and-hold `Ctrl+Space`: start on keydown, stop on keyup
- **Effort:** ~100 lines of TypeScript

### 3d. Settings additions
- **Where:** `src/pages/Settings.tsx`
- **What:** STT provider dropdown (Whisper / Web Speech API), API key field
- **Effort:** ~30 lines

**Total effort: ~3-4 days**

---

## Work Item 4: Persistent History (SQLite)

**What exists:** `tauri-plugin-sql` is registered. `workbuddy.db` is
configured in `tauri.conf.json`. Empty `db/` directory.

**What's missing:**

### 4a. Schema and migrations
- **Where:** `src-tauri/src/db/`
- **Tables:**
  - `conversations`: id, created_at, program, module_id
  - `messages`: id, conversation_id, role, content, timestamp
- **Effort:** ~30 lines SQL

### 4b. Save/load from frontend
- **Where:** `src/contexts/app.context.tsx`
- **What:**
  - On `chat_stream_complete`: save conversation to SQLite
  - On app launch: load recent conversations
  - History page: load from DB instead of in-memory state
- **API:** Use `@tauri-apps/plugin-sql` JS bindings
- **Effort:** ~80 lines TypeScript

**Total effort: ~1 day**

---

## Work Item 5: CI/CD & Release

**What's missing:**

### 5a. GitHub Actions workflow
- **Where:** `.github/workflows/ci.yml`
- **What:**
  - Trigger on push to main and PRs
  - Matrix: ubuntu-latest, macos-latest, windows-latest
  - Steps: install deps, `npx tsc --noEmit`, `npx vite build`,
    `cd src-tauri && cargo check`, `cargo test`
- **Effort:** ~50 lines YAML

### 5b. Release workflow
- **Where:** `.github/workflows/release.yml`
- **What:**
  - Trigger on tag push (`v*`)
  - Build `cargo tauri build` on all 3 platforms
  - Upload .dmg, .msi, .AppImage to GitHub Releases
- **Effort:** ~80 lines YAML

### 5c. Auto-update
- **Where:** `src-tauri/Cargo.toml`, `lib.rs`
- **What:** Add `tauri-plugin-updater`, configure update endpoint
  to check GitHub Releases
- **Effort:** ~20 lines

**Total effort: ~1 day**

---

## Work Item 6: Branding

**What's missing:**

### 6a. App icon
- Design: Graduation cap + cursor motif, emerald (#10b981) accent
- Sizes needed: 32x32, 128x128, 256x256, 512x512 (PNG), .ico, .icns
- **Effort:** Design task (not code)

### 6b. Splash / about dialog
- **Where:** New component or Settings section
- **What:** Credits (pluely, Clicky, Limitless), version, links
- **Effort:** ~30 lines

**Total effort: ~0.5 day (code), design time for icon**

---

## Priority Order

| Priority | Work Item                | Effort   | Impact                              |
|----------|--------------------------|----------|-------------------------------------|
| 1        | Global shortcuts         | 0.5 day  | Core UX — students need hotkeys     |
| 2        | Cursor pointing overlay  | 1-2 days | Key differentiator from plain chat  |
| 3        | Persistent history (DB)  | 1 day    | Data survives restarts              |
| 4        | Push-to-talk + STT       | 3-4 days | Hands-free learning                 |
| 5        | CI/CD + releases         | 1 day    | Ship to students                    |
| 6        | Branding                 | 0.5 day  | Professional appearance             |

**Total remaining: ~7-9 days of development**

---

## Dead Code Audit

Code that exists but is not called from any production path:

| Code | File | Status | Action |
|------|------|--------|--------|
| `parse_point_tags()` | pointer.rs:13 | Has unit tests, not called | Wire into Work Item 1 |
| `show_pointer` command | pointer.rs:54 | Registered, never invoked | Wire into Work Item 1 |
| `hide_pointer` command | pointer.rs:61 | Registered, never invoked | Wire into Work Item 1 |
| `tauri-plugin-sql` | lib.rs:25 | Plugin loaded, no queries | Wire into Work Item 4 |
| `tauri-plugin-global-shortcut` | lib.rs:23 | Plugin loaded, no shortcuts | Wire into Work Item 2 |
| `db/` directory | src-tauri/src/db/ | Empty directory | Populate in Work Item 4 |
| `microphone/` directory | src-tauri/src/microphone/ | Empty directory | Populate in Work Item 3 |
| `speaker/` directory | src-tauri/src/speaker/ | Empty directory | Remove (use microphone/ instead) |
