# WordBuddy

A privacy-first, system-wide **writing assistant** for Windows: real-time
correctness checking in any text field, color-coded inline suggestions in the
browser, a floating suggestion card near the caret in native apps, one-click
fix application, a selection-rewrite palette, weekly writing analytics, and
personalization.

## Platform

**Windows today; macOS/Linux not implemented.** The native capture, widget,
and apply machinery is Windows-only (UI Automation + Win32). macOS and Linux
builds compile with stub backends that detect nothing
(`src-tauri/src/text_monitor.rs` non-Windows reader returns `Unsupported`;
macOS/Linux accessibility modules are stubs). Do not expect any native
functionality off Windows. The browser extension works wherever Chrome/Edge
runs.

## Features (shipped)

- **Local correctness checking** — spelling/grammar via
  [harper-core](https://github.com/automattic/harper) running entirely on your
  machine (`src-tauri/src/engine/`). Works with no LLM key and no network.
- **Browser inline checking** — a Chrome/Edge extension underlines issues
  directly in page text fields and offers clickable fix chips. Active only on
  sites matched by the extension's manifest (see `wordbuddy-extension/manifest.json`
  `content_scripts.matches`; edit that list to add sites).
- **Native floating widget + apply** — in desktop apps (proven on Notepad),
  a suggestion card appears near the focused field with correction chips;
  applying writes the fix back through UIA with identity/foreground
  safeguards (INV-APPLY-001, `src-tauri/src/apply.rs`).
- **Selection-rewrite palette** — select text anywhere, press
  `Ctrl+Shift+W`, get an AI rewrite in the palette window to copy back.
- **Writing goals & dialect** — audience/formality/domain goals and en-US /
  en-GB / en-CA / en-AU / en-IN dialect shape the checks (`WritingGoals`,
  `Dialect` in `src-tauri/src/engine/mod.rs`).
- **Personal style rules & dictionary** — your own find→replace rules and
  vocabulary feed the check engine (`StyleRule`, `PersonalDictionary`).
- **Text-expansion snippets** — type an abbreviation, get an expansion.
  **Default OFF**: the keyboard hook only starts if you enable it in Settings
  (`snippets_enabled` defaults false in `src-tauri/src/config.rs`) — and it
  never fires inside terminals or IDEs (`DEFAULT_EXCLUDED_PROCESSES` in
  `src-tauri/src/snip_hook.rs`).
- **Weekly stats dashboard** — local-only activity analytics in a SQLite file
  (`writing.sqlite`; counts and shapes, never field text — see
  `src-tauri/src/analytics/` and Settings → retain-snippets stays OFF unless
  you opt in).
- **Multi-provider LLM config with local-only mode** — bring your own key for
  Anthropic/OpenAI/Gemini/Groq/OpenRouter, or run fully local against Ollama,
  or skip keys entirely with first-class local-only mode (onboarding → "Work
  local-only"). Correctness checking never needs an LLM. Ambient keystrokes
  are never sent to any LLM: style passes run only on browser-surface text
  (`WB_DISABLE_LLM=1` force-disables all LLM calls).

## Quick start

Prerequisites:

- Node.js (LTS) and npm
- Rust toolchain, **rustc ≥ 1.98** (older rustc miscompiles harper-core 2.8)
- Windows 10/11

```bash
npm install

# Fresh clones ONLY: build dist/ before touching cargo — tauri's
# generate_context! embeds frontendDist at compile time and fails without it.
npx vite build

# Run the desktop app (starts Vite itself on subsequent runs)
npx tauri dev
```

Browser extension (for inline checking):

1. Open `chrome://extensions` (or `edge://extensions`), enable Developer mode
2. **Load unpacked** → select the `wordbuddy-extension/` folder
3. In WordBuddy's window: Settings → copy the auth token
4. Extension toolbar icon → paste token → Save

The app serves the extension on `127.0.0.1` (ports 19521–19523) with
token authentication; nothing is exposed to the network.

## Screenshots

> **PLACEHOLDER — screenshots not yet captured.** This session had no GUI;
> real screenshots of the browser underlines, the native suggestion widget,
> and the rewrite palette belong here. Do not ship release notes referencing
> images that don't exist.

## Unsigned builds

There is no code-signing certificate yet. Windows SmartScreen will warn when
first running an unsigned WordBuddy binary ("Windows protected your PC" →
More info → Run anyway). Auto-update is disabled until signing exists.

## Documentation

- Phase plans & contracts: [docs/plans/PLAN-INDEX.md](docs/plans/PLAN-INDEX.md)
- Per-app support matrix: [docs/APPLY-COMPAT.md](docs/APPLY-COMPAT.md)
- Security model: [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md)
- Invariants: [docs/INVARIANTS.md](docs/INVARIANTS.md), decisions:
  [docs/DECISIONS.md](docs/DECISIONS.md)

Proprietary — all rights reserved.
