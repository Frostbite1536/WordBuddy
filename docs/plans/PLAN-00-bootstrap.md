# PLAN-00 — Bootstrap WordBuddy from WorkBuddy

Goal: turn the empty `C:/Users/LCM/Github/WordBuddy` git repo into a
building, tested copy of WorkBuddy with every subsystem WordBuddy does not
need removed, rebranded, and with **literal baseline gate outputs recorded**
before any feature work starts.

Preconditions:
- Coordination repo initialized (`WordBuddy-coordination`, protocol v1.0).
- This repo has one commit: the plan docs. Product code arrives in this phase.
- Reference tree read-only: `C:/Users/LCM/Github/WorkBuddy`.

Scope fence: this repo only + coordination STATE/status/channel writes.
**Never modify the WorkBuddy or studybuddy-followup trees.**

Agent budget: 1 builder session + 1 verifier session (protocol rule 15).

---

## Task 1 — Import

Copy WorkBuddy tree contents (excluding `.git/`, `node_modules/`, `target/`,
`dist/`) into this repo root. Commit as "import workbuddy base <its HEAD
short-sha>" so provenance is permanent. Record the source SHA in the commit
message body.

## Task 2 — Strip pass 1: Rust backend

Delete modules with no WordBuddy role (product scope decision D-series,
coordination STATE):

| Remove | Why |
|---|---|
| `src-tauri/src/journal/` (whole module) | screen-recording journal is replaced by writing analytics built fresh in PLAN-05 on its patterns |
| `src-tauri/src/wotch.rs` | floating-terminal integration, unrelated |
| `src-tauri/src/rag.rs` | document RAG unrelated |
| `src-tauri/src/microphone.rs`, `stt.rs`, `tts.rs` | voice stack unrelated to v1 |
| `src-tauri/src/capture.rs` | screenshots served the ask-about-screen flow; WordBuddy assistant is text-only |
| `src-tauri/src/pointer.rs` | pointing feature cut (CONTRACTS §3) |

Then fix fallout mechanically:
1. `lib.rs`: remove module declarations, removed commands from
   `invoke_handler`, plugin registrations that lose their last consumer.
2. `Cargo.toml`: drop now-unused deps (`xcap`, `image`, `cpal`, `hound`,
   `paddle-ocr-rs`, `ort`, `ndarray`; keep `rusqlite` — PLAN-05 uses it).
   Do NOT touch the `sqlx = "=0.8.2"` pin or the windows/uiautomation pins
   (FRICTION).
3. `capabilities/default.json`: remove permissions whose only consumers were
   removed commands. Every kept command keeps both registrations
   (lib.rs + capabilities) — silent-failure rule.
4. `llm.rs` stays **API-intact**: `use_pointing_tools` and tool_use parsing
   remain (optional params, unused by new callers). Ledger W7 schedules the
   pruning for PLAN-07 cleanup; do not destabilize the LLM engine here.

Gate: `cd src-tauri && cargo check && cargo test` — both must run green
against the reduced tree before touching frontend.

## Task 3 — Strip pass 2: Frontend + extension

Remove: `CursorOverlayWindow.tsx`, `CursorOverlay.tsx`, `useMicrophone.ts`,
`ttsQueue.ts`, `sentenceBuffer.ts`, `springPhysics.ts`, `pointParser.ts`,
`tests/pointParser.test.ts`, `tests/journal.test.ts`, `pages/Journal.tsx`,
`scripts/generate-curriculum.ts`.

Adapt: `ChatBar.tsx` (remove mic/screenshot affordances; keep text input +
send with the existing `safeListen` cancelled-flag pattern),
`ResponsePanel.tsx` (remove point-tag rendering + TTS controls),
`App.tsx` / `app.context.tsx` (drop journal/mic state slices),
`pages/Settings.tsx` (drop monitor-select, UI-detection, RAG, STT/TTS
sections; keep provider/keys/model/about).

Keep untouched: `extension.rs` transport incl. `/scan` + `/highlight`
(P2 repurposes them), `a11y.rs` + platform impls, `config.rs`, `context.rs`
(window-title detection reused by native targeting), `shortcuts.rs`,
`window.rs` tray/positioning minus the `cursor_overlay` setup block,
`History.tsx`, `db.ts`, `safeOpen.ts`, `friendlyError.ts` + their tests.

Gate: `npx tsc --noEmit && npm test && npx vite build` green.

## Task 4 — Rebrand

- `package.json` name → `wordbuddy`.
- `src-tauri/Cargo.toml`: package name `wordbuddy`, lib name `wordbuddy_lib`,
  description updated.
- `tauri.conf.json`: productName `WordBuddy`, identifier
  `com.wordbuddy.app`, window titles, updater endpoints stripped if present.
- DB filename in frontend `db.ts`: `wordbuddy.db`. No migration shim needed —
  no installed base exists.
- Extension: directory stays `workbuddy-extension/` renamed to
  `wordbuddy-extension/`; manifest `name`/`short_name` → wordbuddy, new
  extension id placeholder; update the localhost port/token docs inside it.
- Icons: replace with a text-monogram placeholder SVG (`public/` +
  `src-tauri/icons/`). ANSWERED 2026-08-21 (HUMAN-INBOX Q1): placeholder
  monograms approved; no real brand assets planned. PLAN-07 does not reopen
  this unless the owner says otherwise.
- Global search for remaining `workbuddy`/`studybuddy` strings; rename user-
  facing ones, keep history-provenance mentions only in commit messages.

## Task 5 — Docs reset

- Rewrite `CLAUDE.md` for WordBuddy: inherit WorkBuddy's coding conventions
  verbatim where still true (Result<T,String>, mutex-poison recovery, shared
  HttpClient, SSE protocol split, stream-cancellation counter, capabilities
  duality, UIA spawn_blocking), delete sections for stripped subsystems, add
  pointers to `docs/plans/` + coordination protocol.
- `README.md`: short product description + "see docs/plans/PLAN-INDEX.md".
- Every inherited `docs/*.md` gets a top banner: `> STALE — describes the
  WorkBuddy base; authoritative specs live in docs/plans/` (ledger W3).

## Task 6 — Baseline gates (BLOCKING for all phases)

Run, in order, capturing **literal final output lines**:

```
cd src-tauri && cargo test
cd src-tauri && cargo check
npx tsc --noEmit
npm test
npx vite build
```

Enter each line into coordination `STATE.md` → Baseline table with the HEAD
SHA. Any pre-existing failure is recorded, named, and either fixed in this
phase (preferred) or filed as ledger finding — never silently carried.

## Acceptance criteria

1. Repo builds and all five gates print PASS; literals recorded in STATE.md.
2. `grep -ri "journal\|wotch\|omniparser\|curriculum\|whisper\|elevenlabs"`
   over `src/ src-tauri/src/` returns only historical/provenance comments.
3. App launches via `npx tauri dev`: bar appears, settings open, an LLM chat
   round-trip works with a configured key, tray show/hide works.
4. No file outside this repo was modified (verifier checks `git -C WorkBuddy status`).
5. Fresh-session cold boot works: a new agent reading coordination README +
   STATE + PLAN-INDEX can state the current phase and next action unprompted.

## Publish

Builder: commit(s), push nothing (local repo), channel entry `0001-builder-p0-published.md`
with SHA + most-likely-wrong claim. Verifier: re-run all five gates at that
SHA, spot-check acceptance items 2–4, then ACCEPT into STATE.md or FINDING.

## Risks

- **Capabilities/lib.rs drift after removals** → commands silently fail;
  mitigated by acceptance item 3 exercising IPC end-to-end.
- **Hidden coupling to stripped modules** (e.g. `context.rs` feeding prompts)
  → compile errors surface them; fix by simplifying callers, not by
  reinstating modules.
- **Extension manifest id collision** with the old dev-installed WorkBuddy
  extension in the dev browser profile → use a distinct id from day one.
