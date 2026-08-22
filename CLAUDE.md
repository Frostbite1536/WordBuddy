# WordBuddy — Agent Instructions

WordBuddy is a privacy-first, system-wide writing assistant built with Tauri 2
(Rust backend + React 19/TypeScript frontend). It floats as a thin
always-on-top bar, checks writing in any text field (harper-core correctness
locally; optional LLM clarity/engagement/delivery passes), shows inline
color-coded suggestions in browsers and a floating suggestion card near the
caret in native apps. Authoritative specs: `docs/plans/PLAN-INDEX.md` +
`docs/plans/CONTRACTS.md`. Runtime coordination protocol lives in
`C:/Users/LCM/Github/WordBuddy-coordination/` (read its README + STATE before
any phase work; builder/verifier lanes never self-certify).

## Tech Stack

- **Backend:** Rust (Tauri 2 commands) — check engine, LLM pipeline, config,
  extension relay, accessibility detection
- **Frontend:** React 19 + TypeScript + Tailwind CSS + Vite
- **Correctness engine:** `harper-core` (Apache-2.0) — local, zero network,
  zero cost (STATE decision D2). Never runs per-keystroke on an LLM path.
- **Style passes:** existing multi-provider LLM pipeline (`llm.rs`) — opt-in
  surfaces + debounce only; `WB_DISABLE_LLM=1` forces correctness-only
- **Browser extension:** Chrome/Edge MV3 (`wordbuddy-extension/`) — field
  watching + inline underlines (browser-only, D4) via the localhost relay
- **Native capture:** Windows UIA (`uiautomation` crate) for focused-field
  text/caret reads; macOS/Linux impls are stubs (ledger W1, out of scope v1)
- **AI:** 6 LLM providers (Anthropic, OpenAI, Google, Groq, Ollama,
  OpenRouter) with provider-specific SSE parsing + stream cancellation
- **Database:** SQLite via `tauri-plugin-sql` (`sqlite:wordbuddy.db`,
  conversations) + `rusqlite` (writing analytics DB, PLAN-05)
- **License:** Proprietary (all rights reserved) — see LICENSE. Do NOT add
  copyleft-licensed dependencies or models.

## Directory Structure

```
src-tauri/src/
  lib.rs          Tauri builder, plugin registration, shared HttpClient state
  main.rs         Entry point
  engine.rs       The Check contract (PLAN-01): harper correctness + LLM
                  style passes behind one pipeline (CONTRACTS §1)
  llm.rs          Multi-provider LLM client (streaming SSE + cancellation;
                  pointing/tool_use plumbing kept API-intact until PLAN-07,
                  ledger W7)
  extension.rs    Browser extension HTTP server (127.0.0.1:19521-19523) +
                  token auth + highlight queue
  a11y.rs         Cross-platform accessibility-tree reader — dispatches to a11y/
  config.rs       API keys + settings (JSON in OS config dir `wordbuddy/`)
  context.rs      Active window title detection (Win32 / osascript / xdotool)
  shortcuts.rs    Global keyboard shortcuts
  diagnostics.rs  Local-only rotating log + panic hook
  window.rs       Window positioning, resize, tray
  capabilities/   Tauri 2 IPC permissions

src/
  App.tsx / main.tsx / contexts/app.context.tsx   Shell + global state
  components/ChatBar.tsx        Input bar (text + send; safeListen pattern)
  components/ResponsePanel.tsx  Streaming markdown + external-question banner
  pages/ Settings.tsx | Onboarding.tsx | History.tsx
  lib/ prompts | db | safeOpen | friendlyError

wordbuddy-extension/   Chrome/Edge MV3 extension (field watching + relay)
tests/                 vitest (safeOpen, friendlyError)
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

Gate order per protocol: fresh `cargo check`/`cargo test` BEFORE
`npx tsc --noEmit` — a stale-artifact typecheck proves nothing.

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
- **UIA (Windows):** never call UIA from an async context without
  `spawn_blocking` — COM MTA must be isolated (a11y/windows_impl.rs).
- **Offsets:** every text-span crossing the Rust→JS boundary is UTF-16 code
  units (INV-OFFSET-001); tests must include an astral-plane char + a
  combining sequence.

## Privacy invariants (product-defining)

- **INV-PRIV-001:** password fields are never read, checked, or logged —
  checked BEFORE any value read.
- **INV-PRIV-002:** raw field text is never persisted; aggregates only.
- **INV-PRIV-003:** ambient keystrokes never reach an LLM; LLM calls carry
  text only from explicit style passes / selection rewrites / snippet
  previews. `WB_DISABLE_LLM=1` is the kill switch.
- **INV-EXCL-001:** excluded targets get no checks, no telemetry, no widget;
  the exclusion list is checked before any field-text read.

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
12. Never call UIA from an async context without `spawn_blocking` — COM MTA must be isolated
13. Never add GPL/AGPL-licensed code, models, or dependencies — the proprietary license depends on staying copyleft-free
14. Never read a password field's value for any purpose (INV-PRIV-001)
15. Never persist raw field text — aggregates only (INV-PRIV-002)
16. Never send ambient keystrokes to an LLM (INV-PRIV-003)

## Multi-Agent Git Safety

1. Work on separate files or clearly separated modules
2. Never force-push or rebase shared branches
3. Commit frequently with descriptive messages
4. Phase work happens on `main` in small committed steps; the coordination
   protocol's channel + verifier admission — not branch machinery — is the
   review gate

## Known staleness

`docs/` (except `docs/plans/`) still contains WorkBuddy-base documents that
describe removed subsystems. Each carries a STALE banner; `docs/plans/` +
this file are authoritative.
