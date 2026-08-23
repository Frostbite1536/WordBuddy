# WordBuddy — Plan Index

**Product:** a privacy-first, system-wide writing assistant (Grammarly-class):
real-time checking in any text field, color-coded inline suggestions in the
browser, a floating suggestion card near the caret in native apps, one-click
corrections, AI rewrites/tone, weekly writing analytics, and personalization
(dictionary, dialect, goals, snippets).

**Base codebase:** `C:/Users/LCM/Github/WorkBuddy` — Tauri 2 (Rust backend,
React 19 + TypeScript + Tailwind frontend), Chrome/Edge MV3 extension,
multi-provider LLM client, localhost extension relay, Windows UIA reader.
Evaluation that selected this base is summarized in the coordination
directory's `STATE.md` decisions D1–D4.

## How a fresh session orients (read in order)

1. `C:/Users/LCM/Github/WordBuddy-coordination/README.md` — protocol + binding rules
2. `.../coordination/STATE.md` — current phase, baselines, admissions
3. `.../coordination/FRICTION.md` — environment gotchas (read before coding)
4. `docs/CONTRACTS.md` (this repo) — shared schemas, events, invariants
5. The one `PLAN-0N-*.md` for your assigned phase
6. Base-repo reference: `C:/Users/LCM/Github/WorkBuddy/CLAUDE.md` (coding
   conventions inherited by Phase 0's rewritten CLAUDE.md)

## Phase map

| Phase | Doc | Delivers | Depends on |
|---|---|---|---|
| 0 | [PLAN-00-bootstrap.md](PLAN-00-bootstrap.md) | WordBuddy repo bootstrapped from WorkBuddy: strip unrelated subsystems, rename, baseline gates recorded | — |
| 1 | [PLAN-01-check-engine.md](PLAN-01-check-engine.md) | `engine.rs`: harper-core correctness + LLM clarity/engagement/delivery pipeline behind one `/check` contract | 0 |
| 2 | [PLAN-02-browser-inline.md](PLAN-02-browser-inline.md) | Extension v2: editable-field watching, inline color-coded underlines, suggestion cards, one-click fixes in browsers | 1 |
| 3 | [PLAN-03-native-capture.md](PLAN-03-native-capture.md) | `text_monitor.rs`: focused-field text/caret reads via UIA, change-detection loop, app exclusions | 1 |
| 4 | [PLAN-04-widget-apply.md](PLAN-04-widget-apply.md) | Floating widget near caret, one-click native corrections, hotkey selection rewrite | 3 |
| 5 | [PLAN-05-analytics.md](PLAN-05-analytics.md) | Writing analytics: sessions, accuracy, top errors, tone profile, weekly report, dashboard page | 1 (+2,3 for data) |
| 6 | [PLAN-06-personalization.md](PLAN-06-personalization.md) | Goals/dialect/style-guide wiring, snippets text-expansion (keyboard hook), email-reply stretch | 1, 4 |
| 7 | [PLAN-07-hardening-release.md](PLAN-07-hardening-release.md) | Perf budgets verified, packaging/installer, updater decision, threat model, docs, clean-clone preflight | all |
| 8 | [PLAN-08-linux-macos.md](PLAN-08-linux-macos.md) | Linux (X11-first) and macOS compatibility: fill a11y/monitor/clipboard/input stubs, keep invariants + Windows green | 0–7 |

Phases are sequential on `main`. P5 may start its schema work once P1 lands,
but does not close before P2+P3 feed it real events.

## Agent budget per phase (named before fan-out, protocol rule 15)

Default: **1 builder session + 1 verifier session** per phase. No other
fan-out is authorized unless the phase doc names it explicitly. None do.

## Feature traceability (Grammarly parity)

| Grammarly feature | Where |
|---|---|
| Floating desktop widget | P4 |
| Inline color-coded suggestions (red/blue/green/purple) | Browser: P2 (true inline). Native: P4 (card model, D4) |
| One-click corrections | P2 (browser), P4 (native) |
| App & website controls | P2 (host mutes), P3 (process exclusions) |
| Productivity tracker, mastery score, top errors, streaks | P5 |
| Vocabulary stats | P5 |
| Tone profile distribution | P5 (weekly LLM pass) |
| Weekly progress report | P5 (markdown export; no email server) |
| Contextual AI prompts / rewrites | P4 (selection palette), P2 (browser actions) |
| Smart email replies | Stretch in P6; expected to land in ledger instead |
| Snippets (text expansion) | P6 (flagged OFF by default; ledger W6) |
| Personal dictionary | P1 (engine-level), P6 (settings UI) |
| Dialect preferences | P1/P6 (harper coverage risk: ledger W4) |
| Writing goals (domain/formality/audience/intent) | P1 (request fields), P6 (UI) |
| Company style guide / brand tones / team snippets | **Out of scope v1** (enterprise tier; not planned) |
| Plagiarism checker | **Out of scope** (per product owner) |
| Readability metrics + doc diagnostics | Optional task in P5 |
| Percentile comparison ("better than N% of users") | **Cut** — requires a server; ledger W5 |

## Non-negotiable invariants

Defined once in [CONTRACTS.md](CONTRACTS.md) (`INV-*`). Every verifier
admission checks them. The privacy invariants (`INV-PRIV-*`) are
product-defining: WordBuddy monitors typing, which is keystroke-logger-
adjacent; the product only survives if the redaction posture is enforced in
code, not promised in prose.
