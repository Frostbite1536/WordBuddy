> STALE — describes the WorkBuddy base; authoritative specs live in docs/plans/

# WorkBuddy — Feature Specification

## Core Features

### F1: Screen-Aware Q&A

**What:** Student asks a question (text or voice), WorkBuddy captures a
screenshot of the active monitor, sends both to Claude with curriculum
context, and streams back a response.

**How it works:**
1. Student types in the chat bar or presses push-to-talk hotkey
2. Screenshot captured via `xcap` (same monitor as WorkBuddy window)
3. Active window title detected to identify which program/module
4. System prompt constructed with curriculum context
5. Claude vision API processes screenshot + question
6. Response streamed to the expanded panel with markdown rendering

**Why it matters:** Students don't need to describe what they're looking at.
Claude sees the same screen they do — the code example, the error message,
the exchange order form, the terminal output.

**Acceptance criteria:**
- [ ] Screenshot captured in < 200ms
- [ ] Response begins streaming within 2 seconds
- [ ] Markdown rendered with syntax highlighting and math
- [ ] Code blocks have copy-to-clipboard button
- [ ] Works across browser, IDE, and terminal contexts

---

### F2: Push-to-Talk Voice Input

**What:** Student holds a hotkey to speak a question. Audio is transcribed
and submitted as the query text.

**How it works:**
1. Student holds `Ctrl+Space` (configurable)
2. Microphone recording starts, visual indicator shown in chat bar
3. On release (or after VAD silence detection), audio stops
4. WAV audio sent to Whisper/AssemblyAI for transcription
5. Transcribed text appears in chat bar and is auto-submitted

**Fallback:** If no STT API key is configured, use the browser's native
Web Speech API (free, lower quality, English-only for most browsers).

**Acceptance criteria:**
- [ ] Push-to-talk activates within 100ms of hotkey press
- [ ] Visual indicator (pulsing mic icon) during recording
- [ ] Transcription completes within 1-2 seconds
- [ ] Supports English; other languages best-effort via Whisper
- [ ] Graceful fallback to Web Speech API when no key configured

---

### F3: Text-to-Speech Response

**What:** Claude's response is spoken aloud via ElevenLabs TTS,
so students can listen while keeping their eyes on their work.

**How it works:**
1. Full response text sent to ElevenLabs `eleven_flash_v2_5`
2. Audio streams back and plays through system speakers
3. Student can pause/stop playback via UI button or hotkey

**Optional:** This feature requires an ElevenLabs API key. Without it,
responses are text-only. The UI shows a "Listen" button only when TTS
is configured.

**Acceptance criteria:**
- [ ] Audio begins playing within 1 second of response completion
- [ ] Playback can be paused/stopped via button or hotkey
- [ ] Voice selection configurable in settings
- [ ] Feature gracefully hidden when no ElevenLabs key
- [ ] Audio does not overlap with push-to-talk recording

---

### F4: Cursor Pointing

**What:** Claude can direct the student's attention to specific UI elements
by animating a blue cursor to a location on screen.

**How it works:**
1. Claude includes `[POINT:x,y:label:screenN]` in its response
2. WorkBuddy parses these tags before rendering
3. A full-screen transparent overlay appears
4. An animated blue cursor travels along a bezier arc to (x,y)
5. A label appears next to the target point
6. Overlay fades after 3 seconds of inactivity

**When Claude uses it:**
- "Click this button [POINT:450,320:Place Order:0]"
- "Your error is on this line [POINT:200,580:TypeError:0]"
- "This field needs your API key [POINT:700,200:API Key input:0]"

**Acceptance criteria:**
- [ ] Overlay is transparent and non-interactive (clicks pass through)
- [ ] Animation is smooth (60fps bezier arc)
- [ ] Works across multiple monitors
- [ ] Label text is readable against any background (shadow/outline)
- [ ] Overlay auto-dismisses after 3 seconds
- [ ] Can be dismissed early with Escape key

---

### F5: Curriculum Context Detection

**What:** WorkBuddy automatically detects which academy module the student
is viewing and tailors Claude's system prompt accordingly.

**How it works:**
1. On each query, detect the active/focused window title
2. Match against known patterns:
   - `"Module 03 — Orders | API Academy"` → API_Academy, Module 03
   - `"01_pm101.html"` → PM_Academy, Module 01
   - `"Limitless Exchange"` → Trading context
   - `"VS Code"` / `"Terminal"` → Coding context
3. Construct system prompt with matched context:
   - Module title and objectives
   - Tier position in curriculum
   - Prerequisites and next steps
   - Program-specific guidance style

**Module database:** A static JSON/TS map of all modules:

```typescript
const MODULES = {
  pm_academy: {
    "01": { title: "PM 101", tier: "Fundamentals", objectives: [...] },
    "04": { title: "Hedging", tier: "Fundamentals", hasQuest: true },
    // ...22 modules
  },
  api_academy: {
    "01": { title: "API 101", tier: "Foundations", objectives: [...] },
    // ...16 modules
  },
  agents_academy: {
    "01": { title: "Crash Course", tier: "Foundations", objectives: [...] },
    // ...12 modules
  }
};
```

**Acceptance criteria:**
- [ ] Detects PM_Academy modules by page title or filename
- [ ] Detects API_Academy and Agents_Academy modules
- [ ] Detects Limitless Exchange by domain/title
- [ ] Detects IDE and terminal contexts
- [ ] Falls back to generic prompt when context unknown
- [ ] Context shown in UI as a small badge on the chat bar

---

### F6: Global Hotkeys

**What:** Keyboard shortcuts that work regardless of which application
has focus.

**Default hotkey map:**

| Action              | Default Shortcut     | Configurable? |
|---------------------|----------------------|---------------|
| Toggle visibility   | `Ctrl+Shift+S`       | Yes           |
| Push-to-talk        | `Ctrl+Space`         | Yes           |
| Take screenshot     | `Ctrl+Shift+X`       | Yes           |
| Focus text input    | `Ctrl+Shift+F`       | Yes           |
| Move window         | `Ctrl+Alt+Arrows`    | Yes           |

**Acceptance criteria:**
- [ ] Hotkeys work when any application has focus
- [ ] No conflicts with common IDE shortcuts
- [ ] Fully configurable in Settings page
- [ ] Press-and-hold support for push-to-talk
- [ ] Visual feedback when hotkey is activated

---

### F7: Conversation History

**What:** All Q&A conversations are stored locally and searchable.

**How it works:**
- SQLite database in app data directory (from pluely)
- Each conversation tagged with detected module context
- Searchable by keyword, module, or date
- Conversations can be resumed or referenced

**Acceptance criteria:**
- [ ] Conversations persist across app restarts
- [ ] Search by keyword returns matching conversations
- [ ] Each conversation shows module context badge
- [ ] Can delete individual conversations
- [ ] Export conversations as markdown (for study notes)

---

### F8: Onboarding Flow

**What:** First-launch experience that guides students through setup.

**Steps:**
1. **Welcome** — "WorkBuddy helps you learn prediction market trading"
2. **API Key** — Enter Anthropic API key (required). Link to
   https://console.anthropic.com/ with instructions
3. **Optional Keys** — ElevenLabs (voice responses), AssemblyAI (voice input)
4. **Permissions** — Request screen recording, microphone, accessibility
   (platform-specific guidance)
5. **Program Selection** — Which program are you enrolled in?
   - PM Academy (trading fundamentals)
   - API Academy (SDK/API development)
   - Agents Academy (LLM agent building)
   - Limitless Trader Lab (cohort program)
6. **Quick Tutorial** — Interactive demo of core features
7. **Ready** — "Press Ctrl+Shift+S to show/hide WorkBuddy"

**Acceptance criteria:**
- [ ] Shown only on first launch (flag in localStorage/SQLite)
- [ ] Can be re-accessed from Settings
- [ ] API key validated before proceeding
- [ ] Platform-specific permission instructions
- [ ] Program selection affects default system prompt
- [ ] Skippable for returning users (e.g., reinstall)

---

## Settings

| Setting                  | Type       | Default                  |
|--------------------------|------------|--------------------------|
| Anthropic API Key        | Secret     | (required)               |
| ElevenLabs API Key       | Secret     | (optional)               |
| STT Provider             | Dropdown   | Web Speech API           |
| AssemblyAI API Key       | Secret     | (optional)               |
| Claude Model             | Dropdown   | claude-sonnet-4-20250514 |
| TTS Voice                | Dropdown   | (ElevenLabs default)     |
| TTS Enabled              | Toggle     | Off (until key provided) |
| Active Program           | Dropdown   | PM Academy               |
| Push-to-Talk Hotkey      | Shortcut   | Ctrl+Space               |
| Toggle Hotkey            | Shortcut   | Ctrl+Shift+S             |
| Screenshot Hotkey        | Shortcut   | Ctrl+Shift+X             |
| Auto-screenshot          | Toggle     | On (capture on every Q)  |
| Theme                    | Dropdown   | Dark                     |
| Window Position          | Dropdown   | Top Center               |
| Cursor Pointing          | Toggle     | On                       |

---

## Non-Goals (Explicitly Out of Scope)

1. **Not a code editor** — WorkBuddy doesn't edit files. It explains
   and guides. Students use their own IDE.
2. **Not a trading bot** — It doesn't place trades. It teaches students
   how to place trades.
3. **Not a proctoring tool** — No monitoring, no reporting to instructors.
   All data stays local.
4. **Not a browser extension** — It's a desktop app. Browser-level
   guidance is handled by Page Agent (separate project).
5. **Not multiplayer** — No shared sessions, no screen sharing.
   Each student has their own local instance.
6. **No cloud sync** — Conversations and settings are local-only.
   No accounts, no server-side state.
