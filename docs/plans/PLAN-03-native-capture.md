# PLAN-03 — Native Text Capture (`text_monitor.rs`)

Goal: read the focused editable field's text + caret position in native
Windows apps (Notepad, VS Code/Electron, Office-class, browsers' own
windows) on a change-detection loop, honoring exclusion lists and password
skips. This is the capability class neither base repo has (verified:
`windows_impl.rs` reads names+rects only; zero input hooks anywhere).

Depends on: PLAN-01 merged.
Agent budget: 1 builder + 1 verifier.

---

## Design

New module `src-tauri/src/text_monitor.rs` (+ `text_monitor/` submodules if
it grows), extending — not modifying the contract of — `a11y/`.

Loop shape copies `journal/recorder.rs` conventions exactly:
- statics: `RUNNING: AtomicBool`, `GENERATION: AtomicU64` (stale loops exit),
  status snapshot struct + command.
- one async tick loop spawned by `start()`; idempotent start; `stop()` flips
  generation.
- all UIA calls inside `tokio::task::spawn_blocking` (**FRICTION**: COM MTA
  isolation).

### Tick algorithm (every 250 ms)

1. Foreground HWND (`GetForegroundWindow`) → PID → process name.
2. Exclusion check against `excluded_processes` → if excluded: emit nothing,
   read nothing (**INV-EXCL-001**). Sleep long (1 s) while excluded.
3. Focused element: `UIAutomation::get_focused_element()`.
4. Password gate: `IsPasswordPropertyId` / control type edit-with-password
   → skip entirely (**INV-PRIV-001**).
5. Read value: `ValuePattern` → full text. Unavailable (rich canvas apps) →
   try `TextPattern.DocumentRange.GetText(-1)`; still unavailable → mark
   target unsupported, back off 2 s.
6. Caret: `TextPattern.GetSelection()` → first range endpoints + bounding
   rects. If only ValuePattern exists: caret unknown (widget docks to field
   rect instead).
7. Hash text (SHA-1 of bytes); unchanged → tick ends. Changed → debounce
   300 ms quiet period (typing bursts), then re-read once and emit.
8. Emit `wb://field-focus {target_key, caret}` + run `engine::check_text`
   with `TargetId::NativeProcess{process}`; emit `wb://issues`.

### Correctness rules

- **INV-MON-001**: any UIA failure degrades to skip-and-backoff; a broken
  probe must never panic the loop or spam logs (log once per target per
  minute max).
- Offsets into the field text follow CONTRACTS INV-OFFSET-001 (UTF-16);
  UIA returns UTF-16 natively — keep it that way end-to-end, converting
  only at harper's boundary (single conversion point, reuse P1 helpers).
- Field text lives only in memory for the check; never logged
  (**INV-PRIV-002**): log target/process/hash prefix at most.

## Tasks

### Task 1 — UIA primitives behind traits
`FocusedFieldReader` trait with a real impl + a fake for tests. Unit-test
tick logic (debounce state machine, generation supersede, backoff ladder)
against fakes — no COM in unit tests.

### Task 2 — Process/exclusion resolution
HWND→PID→name via existing `windows` crate deps (no new crates unless
unavoidable). Case-insensitive match; settings editor lands in Task 5.

### Task 3 — Monitor loop + commands
`monitor_start / monitor_stop / monitor_status` commands, registered in both
places (duality rule), wired to a Settings toggle + "supported apps"
diagnostics readout (last tick result per foreground process — helps users
understand coverage without exposing text).

### Task 4 — Emission plumbing
`wb://field-focus` + `wb://issues` events (payload shapes in CONTRACTS §3).
A debug-only frontend listener prints issue counts (not text) to console for
verification.

### Task 5 — Settings
"Native monitoring" toggle (default ON after this phase ships? No — default
ON is the product; but first release of the phase defaults ON with tray
indicator dot while active), process exclusion list editor, per-process
status column.

## Behavioral verification (gate)

Manual smokes on real apps, evidence recorded as screenshots/log excerpts in
status/builder.md (tool-agnostic):

1. **Notepad.exe** (classic ValuePattern): type seeded errors → within ~1 s
   of pause, devtools shows issues event with correct spans; caret known.
2. **VS Code** (Electron/Chromium a11y): same in a plain-text file buffer;
   expect TextPattern path or documented degradation.
3. **Password field** (e.g., browser login form focused from the desktop):
   monitor emits nothing; diagnostics show "password-skipped".
4. **Excluded process**: add `notepad.exe`, repeat smoke 1 → no events.
5. **Kill-switch resilience**: rename/close app windows rapidly for 30 s →
   no panic, log rate respected, loop recovers.
6. Idle CPU: monitor running, no typing → <1% CPU sustained (Task Manager
   reading recorded).

Standard gates at final head on main.

## Risks

- **UIA provider quality varies wildly per app** — the unsupported-target
  backoff exists precisely for this; coverage table (app → pattern that
  worked) becomes a doc artifact in PLAN-07, seeded from smoke notes.
- **Office uses rich custom controls** — may land in "unsupported v1";
  record, don't fight it.
- **Focus flicker between fields** — debounce keyed by target identity, not
  just text hash.
- **Admin-elevated windows** — UIA from non-elevated app cannot read them;
  document as known limitation, detect `ERROR_ACCESS_DENIED` → mark target
  elevated-unsupported.

## Non-goals

Showing anything (P4 widget), applying fixes (P4), analytics writes (P5),
macOS/Linux (ledger W1).
