> STALE — WorkBuddy-base document; kept for provenance. Authoritative specs live in docs/plans/

# WorkBuddy — State after the autonomous Phases 1–4 buildout

> Read this before acting; it is the handoff from the 2026-07-03 session
> that executed the full journal buildout. Also read `CLAUDE.md`
> (conventions) and `docs/DECISIONS.md` ADR-041 (Phase 0 strip) + ADR-042
> (recorder revokes INV-SEC-004).

## What happened

All four phases of the Dayflow-style work journal were built autonomously
on branch **`journal-buildout`** (off `phase-0-workbuddy`), one commit per
phase. **On 2026-07-04 Jeremy had it pushed and merged: `main` was
fast-forwarded a96e94f → the branch tip and pushed to origin.** The
`journal-buildout` and `phase-0-workbuddy` branches are also on origin.
Phase commits:

- `d646582` phase-1: recorder + journal.sqlite storage (ADR-042)
- `729922c` phase-2: two-stage analysis pipeline
- `153c325` phase-3: journal timeline page + markdown export
- `c10e9ca` phase-4: standup, weekly view, chat-with-journal
- (final docs/verify commit follows this file's update)

`main` now contains everything above (fast-forward — the phase commits
are main's history); `phase-0-workbuddy` remains at `c96545b`.

## Gates at the end of the session (all green, read from runner output)

- `npx tsc --noEmit` → 0 errors
- `npm test` (vitest) → **49 passed, 0 failed** (Phase 0 baseline: 37)
- `cd src-tauri && cargo test` → **103 passed, 0 failed** (baseline: 58)
- `npx vite build` → OK, 499.0 KB (baseline ~476 KB)
- No live LLM calls anywhere in tests.

## Architecture built (src-tauri/src/journal/ + frontend)

- **recorder.rs** — background loop, default 10s (configurable 2–600s),
  selected monitor, ≤1080p JPEG q85 →
  `%APPDATA%\com.workbuddy.app\recordings\YYYYMMDD_HHmmssSSS.jpg`; idle
  seconds (GetLastInputInfo / ioreg / xprintidle) + foreground title per
  shot; skips shots when idle ≥3 min or lock-heuristic (empty title +
  idle ≥60s); hourly retention purge (default 14 days, frames only —
  cards/observations are kept); auto-resumes on app start when
  `recorder_enabled` (default **OFF**).
- **db.rs** — `journal.sqlite` (rusqlite, WAL) with the full schema:
  screenshots, analysis_batches, batch_screenshots, observations,
  timeline_cards, llm_calls, daily_standup_entries.
- **analyzer.rs** — assembler (15–30 min batches, split on >5 min gaps,
  skipped_short/<5 min, skipped_idle), sampler (title changes + 1/45s,
  ≤20 images), Stage 1 frames→observations (offsets from batch start,
  ≤3 attempts), Stage 2 observations+day-cards→revised full-day card set
  (merge-by-default; validation: no overlaps, ≥10 min except last card,
  coverage of inputs; ≤4 attempts with error feedback; day's cards
  replaced transactionally, old set soft-deleted); every attempt logged
  to llm_calls; 10-min scheduler (no-op unless recorder enabled).
- **prompts.rs** — Stage 1/Stage 2/standup prompt text (own words,
  Dayflow-structured); categories: engineering, design, communication,
  research, admin, distraction, other.
- **export.rs** — `journal_export_markdown(from_day, to_day)` (≤62 days).
- **standup.rs** — `journal_generate_standup(day)` (day-1 + day cards →
  {highlights,tasks,blockers,next}, persisted), `journal_get_standup`,
  `journal_week_summary(end_day)` (pure aggregation: category minutes/day,
  focus vs distraction, top apps from card metadata).
- **llm.rs** — `complete_with_images()` non-streaming multi-image path;
  `provider_from_str()` (frontend ids; note "openrouter" vs serde's
  "open_router" — see follow-ups).
- **Frontend** — `pages/Journal.tsx` (Timeline/Standup/Week tabs, date
  nav, card expand with observations + frames, Analyze now, recorder
  mirror, Copy as markdown, "Ask about this day"); `lib/journal.ts`
  helpers; ChatBar: CalendarDays button, recording dot, journal-context
  chip; Settings: Work Journal Recorder section (toggle/interval/
  retention/status + analysis provider/model); `buildSystemPrompt` gained
  a capped `journalContext` param. Config adds recorder_enabled,
  recorder_interval_secs, recorder_retention_days, analysis_provider,
  analysis_model.

Commands are registered in lib.rs invoke_handler; app-defined commands
need no capabilities entries (only plugin permissions live in
capabilities/default.json — same as the pre-existing commands).

## Runtime verification (see final report in the session log)

- Post-strip boot: verified (window process ran, extension server up, no
  panic) before any Phase 1 code.
- Recorder file-writing: **verified** in a dev run with recorder_enabled
  temporarily on (5s interval): recorder auto-started on boot, wrote
  1920x1080 JPEG q85 frames at the configured cadence with rows in
  journal.sqlite (10 rows, idle_seconds populated, all 7 tables present),
  and — because it was 4:30 AM with no real input — correctly SKIPPED
  capturing until input was synthesized, then resumed. Idle-skip works.
- **`recorder_enabled` was flipped back to false**, interval restored to
  10s, and the test frames + journal.sqlite deleted — Jeremy starts from
  a clean opt-in state (ADR-042).
- NOT verified (needs a real workday / API key): live Stage 1/2 analysis
  output quality, standup quality, chat-with-journal answers, macOS/Linux
  idle probes, pointing/STT/TTS regression (nothing in those paths was
  touched, but only Phase 0 gates prove it).

## Known follow-ups (parked — do NOT do unless asked)

- ~~OpenRouter provider id mismatch~~ — FIXED in `ad80361` (serde rename
  to "openrouter" + "open_router" alias, wire-format regression tests).
- PowerShell 5.1 `Out-File -Encoding utf8` writes a BOM; serde_json
  rejects it and config.json gets quarantined/reset. Write configs with
  python/bash, or consider stripping BOM in load_config.
- docs/ cleanup pass (ARCHITECTURE/INVARIANTS/threat model still describe
  removed subsystems + INV-SEC-004), extension host-permission rescoping,
  History page legacy `program` chip, app icon, auto-updater signing.
- Journal niceties: user-editable categories, WTSRegisterSessionNotification
  for real lock detection, Wayland idle probe, week-tab date-range picker.

## Working rules (unchanged)

- NEVER push, never merge to main, never touch other branches.
- Permissive-license deps only (ADR-041 / CLAUDE.md Never-Do #14).
- No live LLM calls in tests. Gate + record numbers at every boundary.
