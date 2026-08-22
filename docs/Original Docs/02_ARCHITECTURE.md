> STALE — describes the WorkBuddy base; authoritative specs live in docs/plans/

# WorkBuddy — Target Architecture

## System Overview

```
┌───────────────────────────────────────────────────────────────────┐
│                     Student's Desktop                             │
│                                                                   │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐  │
│  │ Browser  │  │   IDE    │  │ Terminal │  │    Limitless     │  │
│  │ (Academy │  │ (VS Code │  │ (bot     │  │    Exchange      │  │
│  │ modules) │  │  etc.)   │  │  logs)   │  │    (trading)     │  │
│  └──────────┘  └──────────┘  └──────────┘  └──────────────────┘  │
│       ▲              ▲             ▲              ▲                │
│       │              │             │              │                │
│       └──────────────┴─────────────┴──────────────┘                │
│                              │                                     │
│                     ┌────────▼────────┐                            │
│                     │   WorkBuddy    │ ◄── Floating overlay bar   │
│                     │   (Tauri App)   │     Always-on-top          │
│                     └────────┬────────┘     Cross-platform         │
│                              │                                     │
└──────────────────────────────┼─────────────────────────────────────┘
                               │
                    ┌──────────▼──────────┐
                    │   External APIs      │
                    │  ┌────────────────┐  │
                    │  │ Claude API     │  │  Vision + Chat
                    │  │ (Anthropic)    │  │
                    │  ├────────────────┤  │
                    │  │ Whisper /      │  │  Speech-to-Text
                    │  │ AssemblyAI     │  │
                    │  ├────────────────┤  │
                    │  │ ElevenLabs     │  │  Text-to-Speech
                    │  │ (optional)     │  │
                    │  └────────────────┘  │
                    └─────────────────────┘
```

---

## Application Layers

### Layer 1: Rust Backend (Tauri Commands)

The Rust layer handles everything that needs native OS access.

```
src-tauri/src/
├── main.rs                 # Entry point (suppress Windows console)
├── lib.rs                  # Tauri builder, plugin registration, state
│
├── capture.rs              # Screen capture (KEEP from pluely)
│   ├── capture_to_base64()     — Full-screen capture of active monitor
│   ├── start_screen_capture()  — Multi-monitor overlay for region select
│   └── capture_selected_area() — Crop + encode selected region
│
├── speaker/                # Audio capture (KEEP from pluely)
│   ├── mod.rs                  — AudioDevice abstraction, SpeakerStream
│   ├── commands.rs             — VAD, recording, WAV encoding
│   ├── macos.rs                — CoreAudio aggregate device + tap
│   ├── windows.rs              — WASAPI loopback
│   └── linux.rs                — PulseAudio monitor
│
├── microphone/             # NEW — Push-to-talk mic input
│   ├── mod.rs                  — Mic device enumeration
│   ├── commands.rs             — Start/stop recording, VAD, WAV encoding
│   └── platform.rs             — Platform-specific mic access
│
├── claude.rs               # NEW — Direct Anthropic API client
│   ├── chat_with_vision()      — Send screenshot + transcript to Claude
│   ├── stream_response()       — SSE streaming with Tauri events
│   └── build_system_prompt()   — Curriculum-aware prompt construction
│
├── tts.rs                  # NEW — Text-to-speech (ElevenLabs)
│   ├── synthesize()            — Text → audio bytes
│   └── play_audio()            — Stream audio to speakers via rodio
│
├── pointer.rs              # NEW — Cursor pointing overlay
│   ├── parse_point_tags()      — Extract [POINT:x,y:label:screenN]
│   ├── animate_to_point()      — Bezier arc animation
│   └── create_overlay()        — Full-screen transparent panel
│
├── context.rs              # NEW — Screen context detection
│   ├── detect_active_window()  — Get focused window title/URL
│   ├── match_academy_module()  — Map title → module ID
│   └── get_prompt_profile()    — Return curriculum-specific prompt
│
├── window.rs               # Window management (KEEP, modified)
│   ├── setup_main_window()     — Position, NSPanel init
│   ├── set_window_height()     — Toggle collapsed/expanded
│   └── position_window_top_center()
│
├── shortcuts.rs            # Global hotkeys (KEEP, modified)
│   ├── setup_global_shortcuts()
│   ├── handle_shortcut_action()
│   └── update_shortcuts()
│
├── config.rs               # NEW — API key management
│   ├── get_api_keys()          — Read from keychain
│   ├── set_api_keys()          — Store in keychain
│   └── validate_keys()         — Test API connectivity
│
└── db/                     # SQLite (KEEP from pluely)
    └── migrations/
```

### Layer 2: React Frontend

```
src/
├── main.tsx                    # React entry
├── routes/index.tsx            # Route definitions
│
├── contexts/
│   ├── app.context.tsx         # Global state (KEEP, strip license)
│   ├── theme.context.tsx       # Theme (KEEP)
│   └── curriculum.context.tsx  # NEW — Active program/module state
│
├── hooks/
│   ├── useSystemAudio.ts       # System audio + VAD (KEEP)
│   ├── useMicrophone.ts        # NEW — Push-to-talk mic recording
│   ├── useChatCompletion.ts    # Chat + screenshots (KEEP, modify)
│   ├── useGlobalShortcuts.ts   # Shortcut bridge (KEEP)
│   ├── useWindow.ts            # Resize + focus (KEEP)
│   ├── useCurriculum.ts        # NEW — Module detection + prompt
│   ├── useTTS.ts               # NEW — Text-to-speech playback
│   └── usePointer.ts           # NEW — Cursor pointing events
│
├── components/
│   ├── ChatBar.tsx             # NEW — Main input bar (replaces pluely)
│   ├── ResponsePanel.tsx       # NEW — Expandable response display
│   ├── Overlay.tsx             # Region capture (KEEP)
│   ├── CursorOverlay.tsx       # NEW — Pointing cursor animation
│   ├── OnboardingFlow.tsx      # NEW — First-launch setup
│   ├── ModuleBadge.tsx         # NEW — Shows detected module context
│   └── VoiceIndicator.tsx      # NEW — Push-to-talk visual feedback
│
├── lib/
│   ├── claude.ts               # NEW — Anthropic API types + helpers
│   ├── curriculum/
│   │   ├── prompts.ts          # System prompts per program
│   │   ├── modules.ts          # Module titles + objectives map
│   │   └── detection.ts        # Window title → module matching
│   └── functions/
│       ├── ai-response.ts      # SSE streaming (KEEP, modify)
│       └── stt.ts              # Speech-to-text (KEEP, modify)
│
└── pages/
    ├── Chat.tsx                # Main chat interface
    ├── Settings.tsx            # API keys, voice, shortcuts
    ├── History.tsx             # Past conversations
    └── Dashboard.tsx           # Study progress (future)
```

---

## Data Flow: Student Asks a Question

```
1. Student presses Ctrl+Space (push-to-talk hotkey)
   │
   ├── shortcuts.rs receives key press
   ├── Emits "start-mic-recording" event to frontend
   └── useMicrophone.ts begins recording from mic
       │
2. Student speaks: "How do I place a limit order on Limitless?"
   │
   ├── VAD detects end of speech (1s silence)
   ├── Audio encoded to WAV, base64-encoded
   └── Sent to Whisper/AssemblyAI for transcription
       │
3. Transcription returns: "How do I place a limit order on Limitless?"
   │
   ├── capture.rs takes screenshot of active monitor
   ├── context.rs detects active window:
   │   └── Title: "Module 03 — Orders | API Academy"
   │   └── Matched: API_Academy, Module 03
   │
4. System prompt constructed:
   │
   │   "You are WorkBuddy, an AI teaching assistant for the
   │    Limitless Exchange education platform. The student is
   │    currently in API Academy, Module 03: Orders.
   │
   │    Module objectives:
   │    - Understand order types (market, limit, stop)
   │    - Place orders via the Limitless SDK
   │    - Handle order lifecycle events
   │
   │    The student can see: [screenshot attached]
   │    The student asked: 'How do I place a limit order?'
   │
   │    Guide them using the code examples visible on screen.
   │    If you need to point at a UI element, use [POINT:x,y:label]."
   │
5. Claude API call (vision + text):
   │
   │   POST https://api.anthropic.com/v1/messages
   │   ├── model: claude-sonnet-4-20250514
   │   ├── system: [curriculum-aware prompt]
   │   ├── messages: [
   │   │   { role: "user", content: [
   │   │     { type: "image", source: { data: screenshot_base64 } },
   │   │     { type: "text", text: "How do I place a limit order?" }
   │   │   ]}
   │   │ ]
   │   └── stream: true
   │
6. Response streams back via SSE:
   │
   ├── Text displayed in ResponsePanel.tsx (expanding window)
   ├── [POINT:x,y:label] tags parsed → CursorOverlay animates
   └── Full response sent to ElevenLabs TTS → audio plays back
       │
7. Conversation stored in SQLite for history
```

---

## API Key Management

All API keys stored locally in the OS keychain via `tauri-plugin-keychain`.
No keys leave the student's machine. No proxy server required.

```
Keychain entries:
├── workbuddy.anthropic_api_key     — Required
├── workbuddy.elevenlabs_api_key    — Optional (TTS)
├── workbuddy.assemblyai_api_key    — Optional (STT alternative)
└── workbuddy.whisper_endpoint      — Optional (custom Whisper)
```

**Minimum viable:** Only the Anthropic API key is required.
Without ElevenLabs, responses are text-only (no voice).
Without a dedicated STT key, use browser-native Web Speech API as fallback.

---

## Window Modes

### Collapsed (Default): 54px floating bar
```
┌──────────────────────────────────────────────┐
│ 🎓 WorkBuddy  │  [Ask anything...]  │ 🎤 📸 │
└──────────────────────────────────────────────┘
```
- Text input field with mic and screenshot buttons
- Shows detected module context (e.g., "API Academy — Orders")
- Draggable, repositionable via keyboard shortcuts

### Expanded: Up to 600px response panel
```
┌──────────────────────────────────────────────┐
│ 🎓 WorkBuddy  │  [Ask anything...]  │ 🎤 📸 │
├──────────────────────────────────────────────┤
│                                              │
│  To place a limit order using the Limitless  │
│  SDK, you'll use the `createOrder` method:   │
│                                              │
│  ```typescript                               │
│  const order = await client.createOrder({    │
│    marketId: "0x...",                         │
│    side: "buy",                              │
│    type: "limit",                            │
│    price: 0.65,                              │
│    amount: 100                               │
│  });                                         │
│  ```                                         │
│                                              │
│  I can see you're looking at the TypeScript  │
│  tab in Module 03. The code example on your  │
│  screen shows a market order — change the    │
│  `type` field from "market" to "limit" and   │
│  add the `price` parameter.                  │
│                                              │
│  [▶ Listen]  [📋 Copy code]                  │
└──────────────────────────────────────────────┘
```

### Cursor Pointing Mode
When Claude's response includes `[POINT:x,y:label:screenN]`:
- A blue animated cursor appears on the full-screen transparent overlay
- Travels along a bezier arc from current position to target
- Label text appears next to the target point
- Fades after 3 seconds of inactivity

---

## Build & Distribution

| Platform | Build Command           | Output                    | Size   |
|----------|-------------------------|---------------------------|--------|
| macOS    | `cargo tauri build`     | `WorkBuddy.dmg`          | ~10MB  |
| Windows  | `cargo tauri build`     | `WorkBuddy_Setup.msi`    | ~12MB  |
| Linux    | `cargo tauri build`     | `workbuddy.AppImage`     | ~10MB  |
|          |                         | `workbuddy.deb`          |        |

Auto-updates via `tauri-plugin-updater` using GitHub Releases as the
update feed (same pattern as Clicky's `appcast.xml`).

---

## Security Model

1. **API keys stay local** — stored in OS keychain, never transmitted
   except to the respective API endpoints
2. **No proxy server** — direct HTTPS calls to Anthropic, ElevenLabs,
   AssemblyAI (unlike pluely's server proxy or Clicky's Cloudflare Worker)
3. **Screenshots never leave the machine** except in the Claude API call
   (base64 in the request body, processed and discarded)
4. **No telemetry** — PostHog removed, no usage tracking
5. **Conversations local-only** — SQLite database in app data directory
6. **GPL-3.0** — full source available to students
