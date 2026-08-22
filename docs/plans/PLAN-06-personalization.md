# PLAN-06 — Personalization & Snippets

Goal: goals/dialect/style-guide actually shaping checks end-to-end; personal
dictionary UI; text-expansion snippets behind a low-level keyboard hook
(highest-risk feature class in the plan set — flagged ledger W6, default
OFF); email-reply stretch only if everything else lands green.

Depends on: PLAN-01, PLAN-04 merged.
Agent budget: 1 builder + 1 verifier.

---

## Task 1 — Goals & dialect plumbing (mostly wiring)

- Settings UI: dialect picker, domain/formality/audience/intent pickers
  (CONTRACTS §1 enums), persisted via `config.rs` merge semantics
  (set-individually pattern; never blanket-set excluding keys).
- Verify the full chain: change formality → next browser check request
  carries it → engine prompt prefix changes → observable difference in
  suggestions for a crafted sentence (e.g., contraction-heavy casual line
  flags under Formal). Smoke evidence recorded.
- Dialect: en-US only until harper variant verification (W4) says otherwise;
  picker shows other options disabled with tooltip if unsupported — honest
  UI over aspirational UI.

## Task 2 — Personal dictionary UI

- List/add/remove terms; import from clipboard (one term per line).
- Engine effect verified: add "Kubernetesy" → red underline disappears on
  next check without restart.
- Ignore-rule persistence also lands here (P2 session ignores get a
  "always" checkbox writing dictionary/rules list).

## Task 3 — Style-guide rules (personal tier)

- Simple ordered replacement pairs ("term → preferred term", optional
  case-sensitive flag), applied as engine post-pass issues with kind
  Delivery and rule_id `styleguide:<n>`.
- Import/export as JSON. Tests: ordering, case sensitivity, no overlap with
  correctness spans (correctness wins per P1 dedupe).

## Task 4 — Snippets / text expansion (FEATURE FLAG: OFF by default)

Mechanism: WH_KEYBOARD_LL low-level hook thread in Rust (`snip_hook.rs`,
`windows` crate; no new dependency unless justified in commit message).

Hard rules (**INV-HOOK-001**, admission-blocking):
- Hook callback does O(1) work only: append printable char to a ring buffer,
  match against trie of triggers (`;meet` style), NEVER do I/O, NEVER block,
  always `CallNextHookEx` unconditionally — even during expansion.
- Expansion = synthetic input via the PLAN-04 apply utilities targeting the
  same verified HWND (**INV-APPLY-001** applies verbatim).
- Watchdog: any hook-callback overrun (>2 ms measured) or missed heartbeat
  → hook removed, feature self-disables, tray notification, FRICTION entry.
- Kill switches: Settings master toggle (OFF default), per-snippet disable,
  global pause hotkey.
- Buffer privacy: ring holds last 32 chars transiently for matching only;
  never persisted, never logged.

Snippet model (config JSON): `{trigger, body, cursor_offset, scopes?}`.
Body supports `$CURSOR$` marker. Editor UI in Settings with live preview +
test box (a local input that simulates expansion WITHOUT the global hook —
so the dangerous path is exercisable in isolation).

Verification smokes (all recorded):
1. Hook latency: keystroke echo latency in a terminal while hook active —
   imperceptible (<1 ms added, measured via timestamped input logger test app
   or acceptable manual assertion with rationale).
2. Expansion in Notepad: `;addr` → full address block, caret lands at offset.
3. Focus race: expand then instantly alt-tab ≥10 times → zero stray input
   elsewhere (INV-APPLY-001 drill reused).
4. Watchdog: artificially slow the callback (debug flag) → self-disable fires.
5. Disabled state: fresh install → zero hooks installed (verify via
   diagnostics listing active hooks).

## Task 5 — (STRETCH, skippable) Smart email reply detection

Browser-only: on recognized webmail hosts (gmail.com/outlook.live.com),
when compose window opens with empty body and a read message present, offer
"Draft reply" chip → existing selection-palette flow pre-filled with a
summarized-context prompt. If webmail DOM proves brittle, file finding and
stop — this is the first feature ever allowed to die by design (it was
stretch in the product evaluation too).

## Verification gate

Standard gates at final head on main + all smokes above. Verifier
specifically audits INV-HOOK-001 conformance line-by-line (hook callback code)
before admission — this task cannot be admitted on tests alone.

## Risks

- **WH_KEYBOARD_LL bugs degrade the whole system's typing** — the watchdog +
  default-OFF flag + verifier line-audit are the three independent guards.
- **Trigger false-positives in terminals/IDEs** — snippet scoping: default
  scope excludes processes in a built-in conservative list (terminals, IDEs);
  user-editable.
- **Goals chain silently breaking** (settings not reaching prompts) — the
  Task 1 observable-difference smoke is the guard; do not weaken it to
  "request contains field".

## Non-goals

Team/enterprise sharing of any of the above; cloud sync of snippets;
macOS equivalents of the hook (W1).
