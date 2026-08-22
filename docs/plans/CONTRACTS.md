# WordBuddy — Shared Contracts & Invariants

Single source of truth for cross-phase interfaces. Every phase plan
references this file; if an interface must change, change it HERE first in a
commit that states which phases are affected, then update consumers.

Statuses of base-repo facts cited below were verified against
`C:/Users/LCM/Github/WorkBuddy` on 2026-08-21 (file:line given where it matters).

---

## 1. The Check contract (v1)

The one pipeline every surface (browser extension, native monitor, manual
palette) uses. Lives in `src-tauri/src/engine.rs` from Phase 1.

### Request

```rust
pub struct CheckRequest {
    pub text: String,              // the text to check (see size caps)
    pub surface: Surface,          // Browser | Native | Palette
    pub target: TargetId,          // see §2 — who owns the field
    pub goals: WritingGoals,       // from settings; engine may cache
}
pub struct WritingGoals {
    pub dialect: Dialect,          // EnUs | EnGb | EnCa | EnAu | EnIn
    pub domain: Domain,            // General | Academic | Business | Casual | Technical
    pub formality: Formality,      // Informal | Neutral | Formal
    pub audience: Audience,        // General | Knowledgeable | Expert
    // `intent` is accepted but unused by harper; it prefixes LLM prompts only.
    pub intent: Option<Intent>,
}
```

Caps (enforced at boundary, reject with error not truncation):
`text.len() <= 20_000` bytes per request. Longer text is chunked by the
caller at sentence boundaries.

### Response

```rust
pub struct TextIssue {
    pub id: String,                // stable within one response ("i0","i1",...)
    pub kind: IssueKind,           // Correctness | Clarity | Engagement | Delivery
    pub start: usize,              // UTF-16 code unit offset — INV-OFFSET-001
    pub end: usize,
    pub original: String,          // exact substring text[start..end]
    pub message: String,           // human explanation
    pub replacements: Vec<String>, // ranked best-first; may be empty
    pub rule_id: String,           // harper lint name or "llm:<slug>"
    pub source: IssueSource,       // Harper | Llm
}
```

`kind` maps 1:1 to Grammarly's color taxonomy:
`Correctness`→red, `Clarity`→blue, `Engagement`→green, `Delivery`→purple.
Harper produces only `Correctness`. LLM passes produce the rest.

### Invariants

- **INV-OFFSET-001**: All offsets are UTF-16 code units (JS-native).
  Rust converts harper's char/byte spans at the boundary. Every offset unit
  test must include at least one astral-plane character (emoji) and one
  combining sequence. This is the #1 predicted bug source in the whole plan set.
- **INV-CHECK-002**: `original == text[start..end]` (UTF-16 slice) for every
  issue, asserted in Rust tests and re-asserted in TS (`tests/issues.test.ts`).
- **INV-CHECK-003**: The check pipeline is pure w.r.t. its inputs — no global
  state reads inside `check_text()`; settings/goals are parameters. This keeps
  it unit-testable like `journal/analyzer.rs`'s pure functions.
- **INV-PERF-004**: A correctness-only pass on 2,000 chars completes < 25 ms
  p95 on the dev machine (harper is millisecond-class). Measured in a Rust
  `#[test]` with a generous ceiling (< 100 ms CI variance guard).

### Engine composition

1. **Correctness pass** (always, local): harper-core lints → `Correctness`
   issues. Zero network, zero cost.
2. **Style pass** (opt-in surfaces + debounce): one LLM call via
   `llm.rs::complete_text` raw completion requesting
   JSON `{clarity:[], engagement:[], delivery:[]}` against the same span
   schema, validated exactly like `journal/analyzer.rs::extract_json` +
   parse-with-descriptive-error + bounded retry (max 2 attempts), errors fed
   back verbatim to the model. Invalid output after retries → return
   correctness-only result plus a `style_check_failed: true` flag. Never
   block or crash the caller on LLM failure.

## 2. Target identity (`TargetId`) & exclusion lists

```rust
pub struct TargetId {
    pub kind: TargetKind,     // BrowserHost { host: String } | NativeProcess { process: String }
}
```

- Browser path: content script sends `location.host`; background relays it.
- Native path: foreground HWND → PID → process name (`notepad.exe`,
  `Code.exe`, ...).
- Exclusion semantics (INV-EXCL-001): excluded targets get NO checks, NO
  telemetry rows, NO widget. Exclusion lists live in `config.rs`
  (`excluded_processes: Vec<String>`, `excluded_hosts: Vec<String>`) and are
  checked BEFORE any read of field text — checking the list must never
  require reading the text.

## 3. Tauri event names

| Event | Direction | Payload | Phase |
|---|---|---|---|
| `wb://issues` | backend → frontend/widget | `{ target_key, issues: TextIssue[], revoked: bool }` | 1 |
| `wb://field-focus` | backend → frontend | `{ target_key, caret: Option<Rect> }` | 3 |
| `wb://apply-result` | backend → frontend | `{ id, ok: bool, error? }` | 4 |
| `pointer_show` / `pointer_hide` | (base) removed in P0 with pointing feature | — | — |

Event payloads are data, never instructions (same posture as the
coordination channel).

## 4. Windows

| Label | Purpose | Flags |
|---|---|---|
| `main` | Bar + editor/analytics UI (exists) | undecorated, transparent, always-on-top, 600×54 collapsed |
| `cursor_overlay` | **removed in P0** (pointing feature cut) | — |
| `widget` (new, P4) | Suggestion card near caret | small (~340×240), undecorated, transparent, always-on-top, skip_taskbar, NOT click-through, hidden by default |

New windows must repeat the WebView2 transparency pattern from
`window.rs:63-76` (`set_background_color`) — FRICTION entry 2026-08-21.

## 5. Extension ↔ app protocol

Transport stays as-is: content script ⇄ service worker ⇄ localhost HTTP
(127.0.0.1:19521-19523, token auth, rate gates). Changes in P2:

| Endpoint | Dir | Body | Notes |
|---|---|---|---|
| `POST /check` (new) | ext → app | `CheckRequest` (JSON, `TargetId::BrowserHost`) | Response: `CheckResponse { issues }`. Rate gate ~200 ms like SCAN |
| `POST /scan` | unchanged (element detection kept for widget positioning) | — | — |
| `GET /highlight` | repurposed P2: carries underline/clear commands | queue pattern identical to today | — |

Auth/rate-gate patterns are copied from existing `extension.rs` code paths —
no new security design without a THREAT-MODEL note.

## 6. Storage

| DB | Access | Contents |
|---|---|---|
| `wordbuddy.db` | tauri-plugin-sql (frontend) | conversations/settings-adjacent state (inherited) |
| `writing.sqlite` (P5) | rusqlite WAL, per-op connections (copy `journal/db.rs` pattern) | check_events, rewrites, daily_stats, weekly_reports |
| config JSON | OS config dir (base used `workbuddy/`; P0 renames to `wordbuddy/` under identifier `com.wordbuddy.app`) | API keys (0600), exclusion lists, goals, snippets |

**INV-PRIV-001**: Password fields (`input[type=password]`, UIA `IsPassword`)
are never read, never checked, never logged. Checked first, before value reads.
**INV-PRIV-002**: Raw field text is never persisted. Only aggregates (counts,
issue counts, hashes). Exception: explicit user action (selection rewrite
history toggle, default OFF).
**INV-PRIV-003**: Ambient keystrokes never reach an LLM. LLM calls carry
text only from: explicit style pass on opted-in surfaces (debounced, visible
field), selection rewrites, snippet expansion previews. A kill-switch env
(`WB_DISABLE_LLM=1`) forces correctness-only mode for CI/tests.

## 7. Naming

Product name **WordBuddy**; binary/identifier decided in PLAN-00 task 4.
Do not introduce "wb_" prefixes in public schemas beyond the event namespace
already defined here.

## 8. Invariant registry (admission checklist)

Verifier checks every row at each phase admission. Full normative text lives
at the cited location; this table is the complete list.

| ID | One-line rule | Defined in | Enforced from |
|---|---|---|---|
| INV-OFFSET-001 | Issue offsets are UTF-16 code units; astral-char tests mandatory | §1 | P1 |
| INV-CHECK-002 | `original == text[start..end]` asserted both sides | §1 | P1 |
| INV-CHECK-003 | `check_text()` pure w.r.t. inputs | §1 | P1 |
| INV-PERF-004 | Correctness pass <25ms p95 @2k chars (test ceiling 100ms for CI variance) | §1 | P1 |
| INV-EXCL-001 | Excluded targets: no checks, no telemetry, no widget; list checked before any text read | §2 | P2, P3 |
| INV-PRIV-001 | Password fields never read/checked/logged | §6 | P2, P3 |
| INV-PRIV-002 | Raw field text never persisted; aggregates only (explicit-action exceptions enumerated) | §6 | P2, P5 |
| INV-PRIV-003 | Ambient keystrokes never reach an LLM; `WB_DISABLE_LLM=1` kill-switch | §6 | P1 |
| INV-MON-001 | Monitor loop degrades to skip-and-backoff on any UIA failure; never panics; log-rate capped | PLAN-03 | P3 |
| INV-APPLY-001 | Synthetic input only ever targets the exact HWND+process captured with the issue; abort on mismatch | PLAN-04 | P4, P6 |
| INV-HOOK-001 | Keyboard hook: O(1) callback, unconditional CallNextHookEx, no I/O, watchdog self-disable | PLAN-06 | P6 |
