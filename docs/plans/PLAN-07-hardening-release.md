# PLAN-07 — Hardening, Packaging & Release

Goal: turn the feature-complete app into an installable, honest,
documented product: perf budgets verified with numbers, installer + update
decision, threat model refreshed for what WordBuddy actually does, docs
de-staled, clean-clone preflight green.

Depends on: PLAN-00…06 merged.
Agent budget: 1 builder + 1 verifier (no fan-out; review depth stays at
diff-read + gate re-run per blast-radius rule).

---

## Task 1 — Performance budgets (measure, don't assert)

Instrument then verify on the dev machine; record literal numbers in
STATE.md:

| Budget | Target | Measured how |
|---|---|---|
| Keystroke→underline (browser) | p95 < 150 ms | tracing timestamps: content-script input event → decoration applied |
| Native tick CPU (idle) | < 1% sustained | 10-min sampling run |
| Correctness pass | < 25 ms p95 @2k chars | existing INV-PERF-004 test output |
| Style pass added latency | bounded by debounce, never blocks UI | code-read + timing log |
| App cold start to interactive bar | < 2 s | tauri dev/prod timestamp pair |
| Widget show latency | ≤ 150 ms | P4 smoke re-run |
| Installer size | record, no hard target yet | artifact property |

Any miss → finding with profile evidence, not a budget edit. Budget edits
require a stated reason and human inbox visibility if user-facing.

## Task 2 — Cleanup ledger

- W7: prune `llm.rs` pointing/tool_use plumbing left API-intact in P0 —
  now that all callers are known, delete dead paths; gates must stay green.
- Close or consciously carry every ledger row (W1–W6). Anything carried
  gets an owner and a phase-target or moves to a v2 wishlist section.
- Delete debug affordances added during phases (console listeners,
  playground pages move under `extension/test-pages/` only).

## Task 3 — Packaging & distribution

- `npx tauri build` producing NSIS installer; document build prereqs.
- Code signing: HUMAN-INBOX entry (recommendation: buy OV cert before any
  distribution beyond self; default: unsigned local builds + README warning
  about SmartScreen). Updater (W2): remains OFF until signing exists;
  re-enable checklist already lives in base `PRODUCTION_READINESS.md`.
- First-run experience pass: Onboarding wizard trimmed to WordBuddy flow
  (provider key OR skip-for-local-only mode must be a first-class path),
  privacy explainer screen stating the INV-PRIV rules in plain language.

## Task 4 — Security & privacy documentation

- Rewrite `docs/THREAT_MODEL.md` for reality: keystroke-adjacent monitoring
  is the threat surface. Enumerate: what leaves the machine (LLM calls:
  which text, when), what persists (writing.sqlite shapes), what the
  extension can read (hosts allowlist posture), the hook's guarantees
  (INV-HOOK-001), apply-path safeguards (INV-APPLY-001).
- Every claim carries file:line or test-name receipts (stale-docs lesson).
- Secrets pre-commit hook (protocol §3.3 mechanism) installed in this repo:
  grep staged content for credential shapes; wire via `.git/hooks/pre-commit`
  committed as `scripts/install-hooks.sh`.

## Task 5 — Docs & compatibility table

- `README.md`: real screenshots, honest platform line ("Windows today;
  macOS/Linux not implemented"), quick start, feature list matching shipped
  reality (the anti-goal: docs claiming unshipped behavior).
- `docs/APPLY-COMPAT.md`: per-app support matrix seeded from P3/P4 smokes.
- Purge remaining STALE banners either by rewriting or archiving inherited
  base docs into `docs/base-archive/` (closes W3).

## Task 6 — Clean-clone preflight (outsider test)

On a machine-fresh clone (or CI): `npm install && npx tauri dev` following
ONLY README instructions. Every gap hit is a doc bug fixed in the same
phase. Record result in STATE.md. This is the base repo's PR-#104 lesson
applied before anyone external suffers it.

## Task 7 — Release candidate + archive close

- Tag `v0.1.0-rc1` on the admitted head; installer artifact attached in
  release notes with known-limitations section lifted verbatim from ledger.
- Coordination close-out per protocol: final STATE entry, channel entry,
  `git tag sprint-end` in coordination repo, HUMAN-INBOX drained (every row
  answered or defaulted-with-note).

## Verification gate

All standard gates + Task 1 numbers recorded + Task 6 preflight PASS +
installer installs and launches on a clean user profile (smoke: install →
first-run wizard → one browser check works end-to-end). Verifier admission
requires reading the threat model against actual code paths (spot-check 3
claims to their receipts).

## Risks

- **Signing/updater limbo** — acceptable: unsigned local-first release is
  coherent; the inbox question forces an explicit call rather than drift.
- **Perf misses discovered late** — budgets were named per-phase where they
  belonged (P1/P3/P4); this phase should confirm, not discover.
- **Docs rot resuming post-release** — mitigation: every doc claim needs a
  receipt (this plan's rule); future phases inherit it.
