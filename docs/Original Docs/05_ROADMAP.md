# WorkBuddy — Implementation Roadmap

## Phase Overview

| Phase | Name                  | Duration | Outcome                              |
|-------|-----------------------|----------|--------------------------------------|
| 0     | Fork & Strip          | 1 week   | Clean pluely fork, builds on 3 OS    |
| 1     | Core Teaching Loop    | 2 weeks  | Screenshot + Claude + text response  |
| 2     | Voice I/O             | 1 week   | Push-to-talk + TTS                   |
| 3     | Curriculum Awareness  | 1 week   | Module detection + tailored prompts  |
| 4     | Cursor Pointing       | 1 week   | Screen annotation from Claude        |
| 5     | Polish & Ship         | 1 week   | Onboarding, branding, release builds |

**Total: ~7 weeks from fork to first release.**

---

## Phase 0: Fork & Strip (Week 1)

**Goal:** Clean codebase that builds and runs on macOS, Windows, and Linux
with all commercial/tracking features removed.

### Tasks

- [ ] Fork pluely repository to `Frostbite1536/WorkBuddy`
- [ ] Update license header to acknowledge GPL-3.0 fork
- [ ] Remove `activate.rs` and all license activation code
- [ ] Remove `tauri-plugin-posthog` from Cargo.toml and frontend
- [ ] Remove `tauri-plugin-machine-uid` dependency
- [ ] Remove all `user_activity()` and `report_api_error()` calls
- [ ] Remove `pluely.api.ts` (license check endpoint)
- [ ] Remove content protection flags (make window visible in recordings)
- [ ] Rename app: "Pluely" → "WorkBuddy" across codebase
  - `tauri.conf.json`: identifier, productName, title
  - Window titles in `window.rs` and `lib.rs`
  - Frontend strings and branding
  - Package name in `Cargo.toml` and `package.json`
- [ ] Replace app icon with WorkBuddy icon (placeholder OK)
- [ ] Verify build on macOS: `cargo tauri build`
- [ ] Verify build on Windows: `cargo tauri build`
- [ ] Verify build on Linux: `cargo tauri build`
- [ ] Strip "Dev Space" / custom provider UI (simplify to single provider)
- [ ] Run the app — confirm it launches, shows floating bar, can be toggled

### Deliverable
Clean fork that builds on all 3 platforms, launches as "WorkBuddy",
has no commercial/tracking code, and shows an empty floating bar.

---

## Phase 1: Core Teaching Loop (Weeks 2-3)

**Goal:** Student types a question → screenshot captured → Claude responds
with streaming text.

### Tasks

#### Week 2: Claude API Integration
- [ ] Create `claude.rs` — direct Anthropic API client
  - HTTP POST to `https://api.anthropic.com/v1/messages`
  - `anthropic-version: 2023-06-01` header
  - Streaming via SSE (`stream: true`)
  - Vision: base64 image in messages array
- [ ] Create `config.rs` — API key management via keychain
  - `get_api_key()` / `set_api_key()` Tauri commands
  - Validate key on save (test call to Claude)
- [ ] Replace pluely's `api.rs` proxy with direct Claude calls
  - Remove `fetch_api_response_config()` server dependency
  - Remove model fetching from pluely server
  - Hardcode model options: claude-sonnet-4-20250514, claude-opus-4-20250514
- [ ] Update frontend `ai-response.function.ts`
  - Parse Anthropic SSE format (`event: content_block_delta`)
  - Handle `delta.text` extraction (different from OpenAI format)
- [ ] Update `useChatCompletion.ts`
  - Wire screenshot capture to Claude vision call
  - Auto-capture screenshot on every question submission
  - Display streaming response in panel

#### Week 3: UI Polish
- [ ] Redesign `ChatBar.tsx` for teaching context
  - Placeholder text: "Ask about what's on your screen..."
  - Screenshot indicator (show thumbnail when captured)
  - Submit on Enter, Shift+Enter for newline
- [ ] Redesign `ResponsePanel.tsx`
  - Markdown with syntax highlighting (keep Shiki)
  - Copy code button on code blocks
  - Scroll to bottom on new content
  - "New question" button to collapse panel
- [ ] Settings page with API key input
  - Anthropic API key (required, validated)
  - Model selector dropdown
  - Test connection button
- [ ] Basic conversation history
  - Store Q&A pairs in SQLite
  - List view with search
  - Tap to view past conversation

### Deliverable
Working teaching loop: type question → see screenshot → get Claude
response with code highlighting. Settings page for API key. History.

---

## Phase 2: Voice I/O (Week 4)

**Goal:** Push-to-talk voice questions + optional spoken responses.

### Tasks

#### Push-to-Talk Input
- [ ] Create `microphone/` module in Rust backend
  - Enumerate mic devices via `cpal`
  - Record audio on command (start/stop via Tauri events)
  - Use existing VAD pipeline from `speaker/commands.rs`
  - Encode to WAV, base64, emit `"mic-speech-detected"` event
- [ ] Create `useMicrophone.ts` hook
  - Listen for `"mic-speech-detected"` events
  - Send audio to STT provider
  - Insert transcribed text into chat bar and auto-submit
- [ ] Wire push-to-talk hotkey (`Ctrl+Space`)
  - Key down → start mic recording + show visual indicator
  - Key up → stop recording → transcribe → submit
- [ ] STT provider options:
  - Primary: Whisper API (OpenAI compatible)
  - Alternative: AssemblyAI
  - Fallback: Web Speech API (browser native, no key needed)
- [ ] Add `VoiceIndicator.tsx` — pulsing mic icon during recording

#### Text-to-Speech Output
- [ ] Create `tts.rs` — ElevenLabs integration
  - POST to `https://api.elevenlabs.io/v1/text-to-speech/{voice_id}`
  - Model: `eleven_flash_v2_5` (low latency)
  - Receive audio bytes, play via `rodio` crate
- [ ] Create `useTTS.ts` hook
  - Trigger TTS after response completes
  - Playback controls (play/pause/stop)
  - Auto-play toggle in settings
- [ ] Add TTS settings
  - ElevenLabs API key (optional)
  - Voice selection dropdown
  - Enable/disable toggle
  - "Listen" button on responses (hidden when TTS not configured)
- [ ] Audio conflict handling
  - Pause TTS playback when push-to-talk activates
  - Resume or cancel after recording

### Deliverable
Hold Ctrl+Space to ask a question by voice. Hear Claude's response
spoken back (when ElevenLabs configured). Visual indicators throughout.

---

## Phase 3: Curriculum Awareness (Week 5)

**Goal:** WorkBuddy detects which module the student is viewing and
tailors Claude's responses accordingly.

### Tasks

- [ ] Create `context.rs` — active window detection
  - macOS: `NSWorkspace.shared.frontmostApplication` + accessibility API
  - Windows: `GetForegroundWindow` + `GetWindowText`
  - Linux: `xdotool getactivewindow getwindowname` or X11 API
  - Return window title string to frontend
- [ ] Create `src/lib/curriculum/modules.ts`
  - Static map of all 50 modules across 3 academies
  - Each entry: id, title, tier, objectives, hasQuest, prerequisites
- [ ] Create `src/lib/curriculum/detection.ts`
  - Pattern matching: window title → module ID
  - Fuzzy matching for partial titles
  - Special cases: Limitless Exchange, IDE, Terminal
- [ ] Create `src/lib/curriculum/prompts.ts`
  - System prompt templates per program (4 programs)
  - Template variables: {module_title}, {tier_name}, {objectives}
  - Fallback prompt for unknown context
- [ ] Create `useCurriculum.ts` hook
  - Poll active window every 2 seconds (not on every keystroke)
  - Update detected context in app state
  - Pass context to Claude API call
- [ ] Create `ModuleBadge.tsx` component
  - Small pill on the chat bar: "API Academy — Orders"
  - Updates as student switches windows
  - Clickable to override (manual program selection)
- [ ] Wire curriculum context into Claude system prompt
  - Inject detected module info into every API call
  - Include objectives, tier position, and program-specific style
- [ ] Test across all detection scenarios:
  - PM_Academy module open in browser
  - API_Academy module open + IDE open side by side
  - Terminal running code with no browser visible
  - Limitless Exchange open
  - Unknown application focused

### Deliverable
Chat bar shows "API Academy — Orders" badge when viewing Module 03.
Claude's responses reference the module objectives and teaching style
matches the program. Graceful fallback for unrecognized contexts.

---

## Phase 4: Cursor Pointing (Week 6)

**Goal:** Claude can point at specific locations on the student's screen
with an animated cursor overlay.

### Tasks

- [ ] Create `pointer.rs` — point tag parsing and coordination
  - Parse `[POINT:x,y:label:screenN]` from Claude response text
  - Map coordinates to absolute screen position (multi-monitor)
  - Account for DPI scaling
- [ ] Extend Claude system prompt with pointing instructions
  - Explain `[POINT:x,y:label:screenN]` syntax to Claude
  - Guidelines: use pointing for UI elements, buttons, code lines
  - Coordinate estimation based on screenshot dimensions
- [ ] Create `CursorOverlay.tsx` — full-screen transparent overlay
  - Non-interactive (clicks pass through to underlying windows)
  - Blue cursor icon (matching Clicky's visual style)
  - Bezier arc animation from current position to target
  - Label text with background pill for readability
  - Auto-dismiss after 3 seconds
  - Escape key to dismiss immediately
- [ ] Create `usePointer.ts` hook
  - Listen for point events from response parser
  - Manage overlay visibility and animation state
  - Queue multiple points (animate sequentially)
- [ ] Strip `[POINT:...]` tags from displayed response text
  - Show pointing action inline: "Click here →" with visual indicator
  - Tags should not appear as raw text in the response panel
- [ ] Test pointing accuracy
  - Point at browser UI elements
  - Point at IDE elements (line numbers, buttons)
  - Point at terminal output
  - Multi-monitor pointing

### Deliverable
When Claude says "Click the Place Order button [POINT:450,320:Place Order:0]",
a blue cursor animates to that screen location with a "Place Order" label.

---

## Phase 5: Polish & Ship (Week 7)

**Goal:** Onboarding flow, branding, documentation, and release builds.

### Tasks

#### Onboarding
- [ ] Create `OnboardingFlow.tsx` — multi-step first-launch wizard
  - Step 1: Welcome message explaining WorkBuddy
  - Step 2: Anthropic API key input + validation
  - Step 3: Optional keys (ElevenLabs, STT provider)
  - Step 4: Permission requests (screen, mic, accessibility)
  - Step 5: Program selection (PM/API/Agents/Lab)
  - Step 6: Quick tutorial (animated demo of core features)
  - Step 7: "You're ready!" with hotkey cheat sheet
- [ ] First-launch detection (flag in SQLite)
- [ ] Re-accessible from Settings → "Run Setup Again"

#### Branding
- [ ] Design WorkBuddy icon (graduation cap + cursor motif)
- [ ] Color scheme: match PM_Education dark theme
  - Background: `#09090b` / `#18181b`
  - Accent: Emerald `#10b981` (educational, fresh, distinct from
    any single academy's accent color)
- [ ] Splash/loading screen with WorkBuddy branding
- [ ] About dialog with credits (pluely, Clicky, Limitless)

#### Documentation
- [ ] Write user-facing README for WorkBuddy repo
  - What it is, screenshots, download links
  - Quick start guide
  - API key setup instructions (with screenshots)
  - Hotkey reference
  - FAQ (cost, privacy, platform support)
- [ ] Write CLAUDE.md for developer onboarding
- [ ] Write CONTRIBUTING.md

#### Release
- [ ] Set up GitHub Actions CI for all 3 platforms
  - Build .dmg (macOS), .msi (Windows), .AppImage + .deb (Linux)
  - Run on push to `main` and on tags
- [ ] Configure `tauri-plugin-updater` for GitHub Releases
- [ ] Create first GitHub Release (v0.1.0)
  - Release notes
  - Platform binaries attached
  - Download links in README
- [ ] Test installation flow on each platform
  - macOS: .dmg → drag to Applications → first launch permissions
  - Windows: .msi installer → first launch
  - Linux: .AppImage (no install) or .deb (apt install)

### Deliverable
v0.1.0 release with downloadable installers for all 3 platforms,
onboarding wizard, branded UI, and documentation.

---

## Post-Launch Roadmap

### v0.2 — Page Agent Integration (Future)
- Embed Page Agent (`alibaba/page-agent`) into academy HTML modules
- In-page interactive walkthroughs complementing desktop WorkBuddy
- Page Agent handles browser-side guidance; WorkBuddy handles desktop

### v0.3 — Wotch Integration (Future)

> **SUPERSEDED 2026-04-20:** See `docs/WOTCH_INTEGRATION.md` for the
> plan of record (ADR-034). The section below is preserved for
> historical context. Since it was written, Wotch has shipped a local
> HTTP API, its own stdio MCP server, and a Claude Code hook/MCP IPC
> stack — so the integration shape described below (inventing a custom
> IPC layer) has been replaced by "WorkBuddy becomes an MCP server
> that any Claude Code instance can call, including Claude Code running
> inside Wotch."

**Why:** [Wotch](https://github.com/Frostbite1536/Wotch) is our
cross-platform Electron floating terminal with Claude Code integration.
For API_Academy and Agents_Academy students who are writing code,
Wotch + WorkBuddy together cover the full workflow:

| Tool        | Strength                         | Student activity           |
|-------------|----------------------------------|----------------------------|
| WorkBuddy  | Sees screen, teaches concepts    | Reading modules, trading   |
| Wotch       | Terminal + Claude Code access    | Writing code, running bots |

**Why not merge them:**
- Different frameworks: Tauri (~10MB) vs Electron (~200MB)
- Different purposes: teaching (vision) vs coding (terminal)
- Merging would mean rewriting one in the other's framework,
  losing the pluely fork head start (screen capture, audio, VAD)
- Two focused tools > one bloated tool

**Integration approach (lightweight, not a merge):**

1. **Shared context via local IPC**
   - WorkBuddy detects curriculum context (which module, which program)
   - Shares context with Wotch via a local file or localhost WebSocket
   - Wotch can display the module badge in its terminal pill
   - When a student asks WorkBuddy a coding question, it can suggest
     "Try this in Wotch" with a pre-filled command

2. **Launch integration**
   - WorkBuddy "Open Terminal" button launches Wotch (if installed)
   - Wotch "Ask WorkBuddy" command sends a question to WorkBuddy
   - Both detect each other's presence via process listing

3. **Wotch as the Lab coding companion**
   - Limitless_Trader_Lab kickoff recommends both tools:
     - WorkBuddy for learning (all students)
     - Wotch for coding (API/Agents path students)
   - Week 0 pre-work includes installing both

4. **Curriculum-aware Claude Code prompts in Wotch**
   - When WorkBuddy shares context, Wotch can inject it into
     Claude Code's system prompt via hooks or MCP
   - Claude Code then knows the student is working on Module 03: Orders
     and can tailor code suggestions accordingly

**Scope:** This is a lightweight bridge, not a rewrite. Estimated
1-2 weeks of work on top of both existing tools.

### v0.4 — Lab Cohort Features (Future)
- Week progress tracking (which modules completed)
- Study statistics dashboard (questions asked, time spent per module)
- Export study summary for coach check-ins
- Pre-built "ask about this" prompts for common sticking points

### v0.5 — Community Features (Future)
- Share anonymized Q&A pairs to a community knowledge base
- "Other students asked..." suggestions based on module context
- Opt-in only, privacy-first

### v0.6 — Local LLM Support (Future)
- Ollama integration for students without API keys
- Lower quality but zero cost
- Useful for environments with restricted internet

---

## Resource Requirements

### Development
- **Primary developer:** 1 full-time engineer for 7 weeks
- **Skills needed:** Rust, TypeScript/React, Tauri 2, platform APIs
- **Testing:** Access to macOS, Windows 10+, Ubuntu 22.04+

### Ongoing Costs (Per Student)
- **Claude API:** ~$0.01-0.05 per question (Sonnet with screenshot)
- **ElevenLabs:** ~$0.01 per response (optional)
- **Whisper:** ~$0.006 per 30s of audio (optional)
- **Total per active student:** $0.50-2.00/day depending on usage

### Infrastructure
- **None for v1.** No server required — all API calls are direct.
- **GitHub Releases** for binary hosting (free)
- **GitHub Actions** for CI (free for public repos under GPL-3.0)
