# PLAN-05 — Writing Analytics & Weekly Report

Goal: Grammarly's stats pillar, local-only: words checked, accuracy
(mastery), top errors, vocabulary diversity, tone distribution (weekly LLM
pass), writing streaks, and a Weekly Report — rendered in a dashboard page
and exportable as markdown.

Depends on: PLAN-01 merged to start; **closes only after PLAN-02 + P3 feed
real events**.
Agent budget: 1 builder + 1 verifier.

---

## Data model (`writing.sqlite`, rusqlite WAL)

Copy the base `journal/db.rs` patterns wholesale: per-op connections,
idempotent `init_schema`, `day_bounds_local` local-midnight day math,
select/delete separation for retention, an `llm_calls` audit table. New
schema:

```sql
check_events(
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL,
  surface TEXT NOT NULL,            -- 'browser' | 'native'
  target TEXT NOT NULL,             -- host or process name
  word_count INTEGER NOT NULL,
  issue_counts_json TEXT NOT NULL,  -- {"Correctness":n,"Clarity":n,...}
  rule_counts_json TEXT NOT NULL    -- {"harper:typo":n,...} top-level only
);
rewrites(
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, kind TEXT NOT NULL,
  action TEXT NOT NULL              -- 'applied' | 'copied' | 'dismissed'
);
daily_stats(day TEXT PRIMARY KEY, words INTEGER, checks INTEGER,
  accuracy REAL, vocab_unique INTEGER, vocab_rare_pct REAL,
  top_errors_json TEXT, streak_len INTEGER);
weekly_reports(week_start TEXT PRIMARY KEY, payload_json TEXT NOT NULL,
  created_at INTEGER NOT NULL);
```

Emission points:
- Browser: P2's content script already knows issue counts per check — batch
  them into `/check` responses server-side; app persists per response.
  (No new extension surface.)
- Native: P3 emits through `engine::check_text`; persist at the same choke
  point so both surfaces flow through ONE insertion function
  (`analytics::record_check`) — single-writer discipline inside the app.
- Rewrites recorded from P4 apply/copy/dismiss outcomes.

**INV-PRIV-002 restated for analytics**: rows above carry counts and rule
names only. No text samples unless the user enables "retain snippets for
tone analysis" (default OFF, per-check consent toggle in Settings).

## Tasks

### Task 1 — Schema + record path
`analytics/db.rs` mirroring journal db.rs conventions + tests on
in-memory connections. `record_check()` called from the P1/P2/P3 choke point;
backpressure: if DB write fails, drop the row and increment a dropped-events
counter surfaced in diagnostics — checking must never stall on analytics.

### Task 2 — Aggregation job
Nightly (local 03:00) + on-demand command, guarded by an `AtomicBool`
exactly like analyzer's ANALYZING flag. Computes/refreshes daily_stats from
raw events (pure SQL + Rust aggregation functions with unit tests):
accuracy = 1 − correctness_issues/words (rolling definition documented in
module docs); vocab via tokenization + a small rare-word list (top-20k lemma
list file checked into repo, source noted) — honest heuristic, labeled as
such in UI; streaks = consecutive local days with ≥1 check event and ≥50 words.

### Task 3 — Weekly report generator
Monday-morning scheduler tick (or first app open after week rollover).
Assembles payload from daily_stats + rule rollups; if snippet retention is
ON, includes ≤10 sampled snippets in the LLM prompt for tone distribution +
narrative summary (uses `complete_text`, validated like engine style pass);
else tone section renders "enable snippet retention to unlock". Output:
markdown via `render_markdown()` (pure, unit-tested) → saved to
`writing.sqlite` + exported to `%USERPROFILE%/Documents/WordBuddy/reports/`.

### Task 4 — Dashboard UI (`pages/Stats.tsx`)
Cards: words this week (vs prior week delta), accuracy %, current streak,
top 5 errors (rule names humanized), vocabulary stats, tone bars when
available. Week/day navigation. Report viewer + "Export markdown" button
(`safeOpen.ts` for folder reveal). Empty states written honestly (no fake
data anywhere).

### Task 5 — Optional readability panel
Flesch Reading Ease + word/char/sentence counts computed locally on demand
in the manual editor view (pure fn + tests). Small, self-contained; cut
without ceremony if it threatens phase scope (mark skippable — deliverable-
first rule).

## Behavioral verification (gate)

1. Unit: aggregation functions against seeded fixtures (fixed timestamps —
   no `now()` in pure paths; clock injected).
2. Integration smoke: run browser playground + Notepad smokes from P2/P3
   for ~2 minutes of typing → `sqlite3 writing.sqlite` queries show
   non-zero check_events rows for BOTH surfaces with expected JSON shapes.
3. Time-travel test: temp-dir DB + injected clock → generate two weekly
   reports across a month boundary; assert streak math and week bucketing
   (this is where off-by-one bugs live — write the adversarial test FIRST
   per non-vacuity rule).
4. Dashboard renders real numbers from that DB (screenshot evidence);
   export produces a readable markdown file at the expected path.
5. Privacy audit: `strings`-level scan of DB file confirms no field-text
   fragments appear (verifier runs this independently).

Standard gates at final head on main.

## Non-goals (binding)

- Percentile comparison vs other users — requires a server; cut (ledger W5).
- Any network sync of analytics.
- Email delivery of reports (file export only).

## Risks

- **Vocab/tone metrics are heuristics** — label methodology in-app ("how this
  is computed" popover). Overclaiming precision here would be the product lie
  class the base repo's docs got burned by (stale-docs ledger W3 analog).
- **Clock/DST edge cases in streaks** — time-travel tests mandatory.
- **Write volume** — trivial at human typing rates; dropped-event counter
  guards pathological cases anyway.
