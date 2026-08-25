# WordBuddy

A privacy-first, system-wide **writing assistant** for Windows, macOS, and
Linux: real-time correctness checking in browser and native text fields,
color-coded browser suggestions, a floating suggestion card near the caret,
one-click fix application where supported, a selection-rewrite palette, weekly
writing analytics, and personalization.

## Platform support

| Platform        |         Native field detection | Native suggestions |       Native apply | Notes                                                                 |
| --------------- | -----------------------------: | -----------------: | -----------------: | --------------------------------------------------------------------- |
| Windows 10/11   |                            Yes |                Yes |                Yes | UI Automation; the most thoroughly tested platform                    |
| macOS           |                            Yes |                Yes |            Not yet | Requires Accessibility permission in System Settings                  |
| Linux (X11)     |                            Yes |                Yes |            Not yet | Requires an AT-SPI2 session; X11 support is used by selection capture |
| Linux (Wayland) | Yes, where AT-SPI is available |                Yes | No synthetic input | Wayland intentionally does not permit global input injection          |

macOS and Linux native detection uses the platform accessibility APIs (AX on
macOS and AT-SPI2 on Linux). The app fails closed when permission, a focused
field, or the accessibility service cannot be resolved. Text-expansion snippets
are currently Windows-only, and native fix application is currently Windows-only.
Browser inline checking works wherever Chrome or Edge runs.

## Features (shipped)

- **Local correctness checking** — spelling/grammar via
  [harper-core](https://github.com/automattic/harper) running entirely on your
  machine (`src-tauri/src/engine/`). Works with no LLM key and no network.
- **Browser inline checking** — a Chrome/Edge extension underlines issues
  directly in page text fields and offers clickable fix chips. Active only on
  sites matched by the extension's manifest (see `wordbuddy-extension/manifest.json`
  `content_scripts.matches`; edit that list to add sites).
- **Native floating widget** — in supported desktop apps, a suggestion card
  appears near the focused field with correction chips. Windows uses UI
  Automation; macOS uses the Accessibility API; Linux uses AT-SPI2. Native
  fix application is currently available on Windows; macOS/Linux report apply
  as unsupported rather than silently failing (INV-APPLY-001,
  `src-tauri/src/apply.rs`).
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
- Windows 10/11, macOS 12+, or a current Linux distribution with a desktop
  accessibility stack (AT-SPI2; X11 recommended for the fullest Linux
  experience)
- macOS: grant WordBuddy access under **System Settings → Privacy & Security
  → Accessibility**
- Linux: install/run `at-spi2-core`; Wayland sessions have no global input
  injection, and native apply is currently unavailable on Linux

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
- Per-app/platform support matrix: [docs/APPLY-COMPAT.md](docs/APPLY-COMPAT.md)
- Linux/macOS implementation and limitations: [docs/plans/PLAN-08-linux-macos.md](docs/plans/PLAN-08-linux-macos.md)
- Security model: [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md)
- Invariants: [docs/INVARIANTS.md](docs/INVARIANTS.md), decisions:
  [docs/DECISIONS.md](docs/DECISIONS.md)

Proprietary — all rights reserved.
