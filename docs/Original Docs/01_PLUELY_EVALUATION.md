# Pluely Evaluation — Fork Feasibility Analysis

## Repository

- **URL:** https://github.com/iamsrikanthnani/pluely
- **Stars:** ~1,800 | **Forks:** 372
- **License:** GPL-3.0 (copyleft — any fork must remain GPL-3.0)
- **Version:** 0.1.9
- **Language split:** TypeScript 81% / Rust 19%

---

## Architecture Overview

Pluely is a Tauri 2 desktop app with a React frontend and Rust backend.
It runs as a thin floating bar (600x54px) at the top of the screen that
expands to 600px tall when showing responses.

```
┌─────────────────────────────────────────────────────────┐
│  Frontend (React + TypeScript + Tailwind)                │
│  ├── useChatCompletion    — LLM streaming + screenshots  │
│  ├── useSystemAudio       — VAD + audio orchestration    │
│  ├── useGlobalShortcuts   — Tauri event bridge           │
│  ├── useWindow            — Resize (54px ↔ 600px)        │
│  └── Overlay.tsx          — Region capture selection      │
├─────────────────────────────────────────────────────────┤
│  Rust Backend (Tauri commands)                           │
│  ├── capture.rs   — xcap screen capture                  │
│  ├── speaker/     — Platform audio capture + VAD         │
│  │   ├── macos.rs   (CoreAudio via cidre)                │
│  │   ├── windows.rs (WASAPI)                             │
│  │   └── linux.rs   (PulseAudio)                         │
│  ├── api.rs       — LLM/STT proxy with SSE streaming    │
│  ├── window.rs    — Positioning, dashboard, resize       │
│  ├── shortcuts.rs — Global hotkey registration           │
│  ├── activate.rs  — License system (REMOVE)              │
│  └── db/          — SQLite migrations                    │
└─────────────────────────────────────────────────────────┘
```

---

## What Works Well (Keep)

### 1. Cross-Platform Screen Capture
- Uses `xcap` crate — works on macOS, Windows, Linux
- Two modes: full-screen instant capture and region selection with overlay
- Multi-monitor support with DPI scaling
- Monitor detection via window overlap-area calculation

### 2. Cross-Platform System Audio Capture
- macOS: CoreAudio aggregate device with process tap (`cidre` crate)
- Windows: WASAPI loopback capture (`wasapi` crate)
- Linux: PulseAudio monitor source (`libpulse-binding`)
- All three output f32 sample streams via a unified `SpeakerStream` trait

### 3. Voice Activity Detection (VAD)
- Rust-level VAD with RMS energy + peak amplitude analysis
- Configurable sensitivity: `rms > 0.012` or `peak > 0.035`
- Pre-speech buffering (12 chunks ~0.27s) prevents clipping starts
- Silence detection (45 chunks ~1s) auto-ends utterances
- Soft-knee noise gate + normalization before encoding
- 16-bit WAV output, base64-encoded, emitted as Tauri events

### 4. Window Management
- Transparent, borderless, always-on-top, skip-taskbar
- macOS: NSPanel via `tauri-nspanel` (non-activating, joins all Spaces)
- Content-protected (invisible in screen recordings)
- Keyboard-driven repositioning (arrow keys at 60fps)
- Dynamic height toggle (collapsed bar ↔ expanded response panel)

### 5. Global Shortcut System
- `tauri-plugin-global-shortcut` with dynamic registration
- Press/release tracking for continuous actions (window movement)
- Configurable key combos stored in frontend, synced to Rust backend
- Actions: toggle visibility, screenshot, audio recording, focus input

### 6. LLM Streaming
- OpenAI-compatible SSE streaming format
- Provider-agnostic (Claude, OpenAI, Gemini, Mistral, Groq, Ollama)
- Multi-modal: images sent as base64 data URIs in messages array
- Abort support via `AbortController`
- Custom system prompts per conversation

### 7. Frontend Foundation
- React 19 + TypeScript + Tailwind CSS + Radix UI components
- Markdown rendering with syntax highlighting (Shiki) and math (KaTeX)
- Command palette (cmdk)
- SQLite local conversation history
- Theme system (light/dark)

---

## What Must Be Stripped (Remove)

### 1. License Activation System (`activate.rs`)
- Machine UID fingerprinting, remote license validation
- Feature gating based on license status
- Payment endpoint integration
- **Impact:** Remove `activate.rs`, strip all license checks from
  `shortcuts.rs` and frontend, remove `tauri-plugin-machine-uid`

### 2. Pluely Server Dependency
- LLM routing goes through `{APP_ENDPOINT}/api/response` to get config
- Model listing fetched from pluely's server
- System prompt generation via server API
- Audio transcription config from server
- User activity reporting
- **Impact:** Replace with direct API calls to Claude/Whisper.
  This is the largest refactor — touches `api.rs` and multiple frontend hooks

### 3. PostHog Analytics (`tauri-plugin-posthog`)
- Session tracking initialized despite "no telemetry" claim
- `user_activity()` reports usage metrics to pluely's server
- `report_api_error()` sends error details remotely
- **Impact:** Remove plugin from Cargo.toml + tauri.conf.json,
  strip all `posthog` calls from Rust and frontend

### 4. Stealth/Undetectable Features
- Content protection (window invisible in screen recordings)
- Design philosophy around being invisible in video calls
- **Impact:** Remove content protection flags — educational tool should
  be visible and branded. Change UX from "hidden assistant" to
  "visible study companion"

### 5. Branding
- Window titles hardcoded to "Pluely"
- Icons, about dialog, app name throughout
- **Impact:** Find-and-replace across codebase, new icon/branding

---

## What Must Be Added (Build)

### 1. Direct Claude API Integration
Replace the pluely proxy with direct Anthropic API calls:
- System prompt with curriculum context
- Vision API for screenshot analysis
- Streaming responses via SSE
- API key stored locally in keychain (already has `tauri-plugin-keychain`)

### 2. Text-to-Speech (from Clicky)
Pluely has no TTS. Port from Clicky's architecture:
- ElevenLabs API integration (`eleven_flash_v2_5` model)
- Audio playback via `cpal` or `rodio` crate
- Can route through a Cloudflare Worker (like Clicky) to protect API keys

### 3. Cursor Pointing System (from Clicky)
Pluely has no screen annotation. Port from Clicky's architecture:
- Parse `[POINT:x,y:label:screenN]` tags from Claude responses
- Full-screen transparent overlay (already have overlay infrastructure)
- Animated cursor movement along bezier arcs
- Multi-monitor coordinate mapping

### 4. Curriculum-Aware System Prompts
Context-specific prompts that change based on what's on screen:
- Detect which academy module is visible (URL or page title matching)
- Inject module objectives, tier context, and learning path
- Different prompt profiles for PM/API/Agents/Lab

### 5. Microphone Input
Pluely captures system audio (speaker loopback) for meeting transcription.
WorkBuddy needs microphone input for push-to-talk questions:
- Add microphone capture path alongside system audio
- Use existing VAD pipeline with mic input
- Global hotkey for push-to-talk (like Clicky's Ctrl+Option)

### 6. Onboarding Flow
First-launch experience:
- API key configuration (Claude, optionally ElevenLabs)
- Permission grants (screen recording, microphone, accessibility)
- Program selection (which academy are you enrolled in?)
- Quick tutorial ("Press Ctrl+Space to ask a question")

---

## Platform-Specific Notes

| Feature              | macOS                          | Windows              | Linux                    |
|----------------------|--------------------------------|----------------------|--------------------------|
| Screen capture       | xcap (works)                   | xcap (works)         | xcap (works)             |
| System audio         | CoreAudio tap (cidre)          | WASAPI loopback      | PulseAudio monitor       |
| Floating window      | NSPanel (non-activating)       | Always-on-top        | Always-on-top            |
| Window behavior      | Joins all Spaces, float level  | Skip taskbar         | Skip taskbar             |
| Permissions          | System Preferences deep links  | Sound settings       | pavucontrol/GNOME        |
| Build output         | .dmg                           | .msi, .exe           | .deb, .rpm, .AppImage    |
| Min OS               | macOS 10.13                    | Windows 10+          | WebKitGTK required       |
| Microphone           | AVCaptureDevice                | WASAPI               | PulseAudio               |

**macOS has the richest implementation** (NSPanel, CoreAudio process tap).
Windows and Linux work but have simpler window management (no NSPanel
equivalent means the window steals focus on click). This is acceptable
for an educational tool — students will actively interact with it.

---

## Risk Assessment

| Risk                                    | Severity | Mitigation                                  |
|-----------------------------------------|----------|---------------------------------------------|
| GPL-3.0 forces open-source              | Low      | Aligns with educational mission             |
| Pluely server dependency deeply coupled  | High     | Phase 1 priority: replace with direct APIs  |
| NSPanel macOS-specific code              | Medium   | Already abstracted; Windows/Linux fallback  |
| `tauri-nspanel` uses custom Tauri fork   | Medium   | Check compatibility with upstream Tauri 2   |
| Pluely stops being maintained            | Low      | We fork and own the code                    |
| xcap crate breaks on OS update           | Low      | Actively maintained, 1k+ stars              |
| ElevenLabs API cost for TTS              | Medium   | Make TTS optional; offer local TTS fallback |

---

## Decision: Fork Pluely

**Recommendation: Fork and modify.**

The alternative — building from scratch with Tauri 2 using pluely as
reference only — would take 3-4x longer for the same result. Pluely's
cross-platform audio capture, screen capture, window management, and
shortcut system represent months of platform-specific work that we get
for free.

The GPL-3.0 constraint is acceptable. The server dependency is the main
engineering challenge but is well-scoped (replace `api.rs` proxy calls
with direct Anthropic SDK calls).

**What we inherit (saves ~3 months):**
- Working screen capture on 3 platforms
- Working audio capture on 3 platforms with VAD
- Working global shortcut system
- Working floating window management with NSPanel
- Working LLM streaming infrastructure
- Working SQLite conversation storage
- Working React + Tailwind UI foundation
- Working build/packaging for .dmg, .msi, .deb, .AppImage

**What we build (~4-6 weeks):**
- Direct Claude API integration (replace pluely proxy)
- Push-to-talk microphone input
- Text-to-speech (port from Clicky)
- Cursor pointing overlay (port from Clicky)
- Curriculum-aware system prompts
- Branding and onboarding
- Strip commercial features
