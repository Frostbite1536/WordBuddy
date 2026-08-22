# PLAN-02 — Browser Inline Checking (Extension v2)

Goal: the first user-visible Grammarly behavior: type in any allowed web
text field, see color-coded inline underlines (red/blue/green/purple), hover
for the issue card, click a replacement to apply it — all through the
existing extension ⇄ localhost relay.

Depends on: PLAN-01 merged.
Agent budget: 1 builder + 1 verifier. Behavioral verification requires a
driven real browser (Chrome/Edge with the unpacked extension loaded).

---

## Architecture recap (what exists, what changes)

Existing (verified in base): MV3 manifest; `content.js` pushes DOM scans of
interactive elements every 3 s (`pushScan`, MAX_ELEMENTS=400) and polls
highlight commands every 300 ms (`pollHighlights`); service worker relays to
127.0.0.1:19521+ with token auth; app-side `extension.rs` keeps scan cache +
highlight queue behind rate gates (SCAN/HIGHLIGHT 200 ms).

Changes:
1. Content script gains an **editable-field watcher** (new): focusin/focusout
   + `input` events on textarea/input-text/contenteditable.
2. Debounced `POST /check` (300 ms quiet-period, cancellable generation
   counter — same supersede pattern as llm STREAM_GENERATION).
3. Underline renderer: absolutely-positioned decoration overlay derived from
   a hidden mirror div (measure `Range.getClientRects()` per issue span),
   color-coded by `kind`.
4. Suggestion card: single floating element near the active underline;
   message / replacements list / ignore button.
5. Apply: splice `replacements[0]` into the field preserving undo history
   (`document.execCommand('insertText')` inside selection for contenteditable;
   setRangeText + input event for inputs), then re-check.
6. Host mute list: config-driven `excluded_hosts`; watcher never activates
   there. GitHub-class sites keep the existing forced-masking posture — but
   WordBuddy's purpose is reading draft text, so the default flips: checking
   IS reading field values. Sites in `excluded_hosts` get zero reads
   (**INV-EXCL-001**). Password fields never watched (**INV-PRIV-001**:
   skip `input[type=password]` and any `autocomplete="current-password"`).

## Tasks

### Task 1 — App side: `/check` endpoint
`extension.rs`: `POST /check` accepting the JSON CheckRequest
(`TargetId::BrowserHost{host}`), token-authed, rate-gated (~200 ms min
interval, CAS-retry gate copied from existing code), calling
`engine::check_text`. Response `{issues}`. Unit tests mirror existing
handler tests (auth reject, rate reject, happy path with canned engine stub).

### Task 2 — Content script watcher + debouncer
- Watch editable fields; on change: hash text (cheap djb2) — skip `/check`
  if hash unchanged (focus juggling).
- Generation counter cancels in-flight highlight application for superseded
  checks.
- Never run on excluded hosts or password fields (checked before any value read).
- Keep legacy `pushScan` (widget positioning needs element rects later).

### Task 3 — Underline + card rendering
- Shadow-DOM-isolated styles (page CSS can't break us; we can't break pages).
- Colors fixed constants matching CONTRACTS kind map; visible focus ring for
  keyboard users (a11y baseline).
- Card shows: message, up to 3 replacement chips, "Ignore" (adds rule to
  session-local ignore set; persistent ignores are PLAN-06).
- All DOM injected elements namespaced `wb-*`.

### Task 4 — Apply path
- Preserve undo (execCommand insertText route) — verify manually in a
  contenteditable (Gmail compose class) and a plain textarea.
- After apply: immediate local re-check of the edited region (no full
  debounce wait).

### Task 5 — Service worker relay update
- Relay `/check` bodies (size cap: fields >20 KB are chunked client-side by
  sentence boundaries per CONTRACTS; merge responses by offset shift).
- Keep token flow unchanged.

### Task 6 — Settings surface (minimal)
- Main-window Settings page: extension status card already exists — add
  "Browser checking" toggle + excluded-hosts list editor (writes
  `config.rs` fields; INV-EXCL-001 honored by both content script AND
  server-side pre-check).

## Behavioral verification (gate — tool-agnostic spec)

On a driven real browser profile with the unpacked extension:

1. Open a data: URL or local test page (checked into repo as
   `extension/test-pages/playground.html`: textarea, contenteditable,
   password input, an iframe field).
2. Type seeded errors ("This is teh smae recieve with a mispeling"). Expect:
   red underlines within 500 ms of typing stop; blue/green/purple appear
   after the style pass (LLM configured) or their documented absence when
   `WB_DISABLE_LLM=1`.
3. Click a red underline chip → text corrected in place; press Ctrl+Z →
   revert works (undo preserved).
4. Password field: DevTools network panel shows ZERO `/check` posts while
   typing into it (INV-PRIV-001 evidence).
5. Add host to exclusions → underlines vanish and no `/check` fires.
6. Record literal gate lines + smoke PASS/FAIL evidence (screenshots or DOM
   dumps) in status/builder.md.

Standard gates (cargo/tsc/vitest/vite) also run at final head on main.

## Risks

- **Underline drift on scroll/resize/font-load** — recompute rects on
  scroll/resize/selectionchange; accept minor drift on exotic layouts,
  file as finding rather than over-engineering v1.
- **contenteditable diversity** (React-controlled inputs, Draft.js, Lexical)
  — apply-path may fail silently on exotic editors: detect failure (value
  unchanged) → fall back to clipboard-paste apply; if that fails, show
  "copy fix" affordance. Never loop retries.
- **CSP-strict sites** blocking injection — degrade to card-only mode
  (no underlines) rather than fighting site policy.
- **Rate-gate vs fast typists** — 200 ms gate coalesces naturally via
  debouncer; ensure 429-style rejection surfaces as "skip this cycle," not error toast.

## Non-goals

Native apps (P3/P4), analytics emission (P5 hooks land here but writing to
writing.sqlite is P5), snippets, enterprise controls.
