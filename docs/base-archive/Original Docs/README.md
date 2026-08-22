> STALE — describes the WorkBuddy base; authoritative specs live in docs/plans/

# WorkBuddy — Cross-Platform AI Teaching Assistant

A cross-platform desktop AI companion that guides students through
PM_Academy, API_Academy, Agents_Academy, and Limitless_Trader_Lab.

## What Is This?

WorkBuddy is a screen-aware AI teaching assistant that lives as a floating
overlay on the student's desktop. It can see their screen, listen to voice
questions, and respond with contextual guidance — whether they're reading an
academy module in the browser, writing code in their IDE, running a bot in the
terminal, or placing a trade on Limitless Exchange.

Think of it as **Clicky** (our macOS-only prototype) rebuilt for
**Windows, macOS, and Linux**.

## Why?

Our four programs span different activities:

| Program              | Where students work             | What they need help with        |
|----------------------|---------------------------------|---------------------------------|
| PM_Academy           | Browser (modules + Limitless)   | Concepts, trade guidance        |
| API_Academy          | Browser + IDE + Terminal        | Code debugging, API usage       |
| Agents_Academy       | Browser + IDE + Terminal        | Agent architecture, tool wiring |
| Limitless_Trader_Lab | All of the above + Discord      | End-to-end coaching             |

No single in-browser tool covers all of these contexts. Students need a
desktop-level companion that follows them across programs.

## Base: Pluely Fork

After evaluating options, we're forking
[pluely](https://github.com/iamsrikanthnani/pluely) — a Tauri 2 desktop app
with cross-platform screen capture, system audio, and LLM streaming. See
`01_PLUELY_EVALUATION.md` for the full analysis.

**Key trade-off:** Pluely is GPL-3.0 (copyleft). WorkBuddy must remain
open-source under GPL-3.0. This aligns with our educational mission.

## Planning Documents

| Document                         | Contents                                         |
|----------------------------------|--------------------------------------------------|
| `01_PLUELY_EVALUATION.md`        | Pluely deep dive — what to keep, strip, and add  |
| `02_ARCHITECTURE.md`             | Target architecture after modifications           |
| `03_FEATURE_SPEC.md`             | Feature specification for PM Education            |
| `04_PROGRAM_INTEGRATION.md`      | Per-program integration details                   |
| `05_ROADMAP.md`                  | Phased implementation plan with milestones         |

## Relationship to Clicky and Wotch

WorkBuddy draws from two existing Frostbite1536 projects:

- **[Clicky](https://github.com/Frostbite1536/clicky)** — macOS-only AI
  teaching buddy (Swift). WorkBuddy ports its TTS (ElevenLabs), cursor
  pointing, and teaching-focused UX to a cross-platform Tauri app.
- **[Wotch](https://github.com/Frostbite1536/Wotch)** — Cross-platform
  floating terminal for Claude Code (Electron). A future integration
  (post v0.1) will let WorkBuddy and Wotch share curriculum context,
  so students get screen-aware tutoring (WorkBuddy) alongside a
  Claude Code terminal (Wotch) — two tools that complement rather than
  overlap.

WorkBuddy is built first as a standalone pluely fork. Wotch integration
comes in v0.3 as a lightweight bridge (local IPC), not a merge.

## Tech Stack (Target)

- **Desktop framework:** Tauri 2 (Rust backend + web frontend)
- **Frontend:** React + TypeScript + Tailwind CSS
- **Screen capture:** `xcap` crate (cross-platform)
- **Audio capture:** CoreAudio (macOS) / WASAPI (Windows) / PulseAudio (Linux)
- **AI:** Claude API (Anthropic) via direct API calls
- **STT:** Whisper API or AssemblyAI
- **TTS:** ElevenLabs (ported from Clicky)
- **Database:** SQLite (local conversation history)
- **Binary size:** ~10MB
