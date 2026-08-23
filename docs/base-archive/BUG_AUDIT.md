> STALE — WorkBuddy-base document; kept for provenance. Authoritative specs live in docs/plans/

# WorkBuddy Bug Audit — 2026-04-24

**Branch:** `claude/audit-workbuddy-bugs-7I5gC`
**Scope:** Full WorkBuddy codebase *except* the browser extension
(`workbuddy-extension/`), per the user's request to defer it.
**Method:** Three parallel specialized scans (Rust backend, React/TS frontend,
telemetry subsystem) plus first-hand line-by-line verification of every
flagged finding. `tsc --noEmit`, `vitest run`, and `cargo check` all pass
clean on this branch.

Severity is assigned by *observable impact to students/instructors*, not by
academic interest. Findings the sub-agents flagged that turned out to be
false positives on close reading are listed at the bottom with the reason
for rejection, so future auditors don't redo the work.

---

## Critical

None. All three invariant-critical paths I checked (stream cancellation,
consent gate on enqueue, preflight gate on upload) held up under close
reading.

---

## High

### H1 — `redactor.ts:90` vs `uploader.ts:84`: `password|passwd|pwd` missing from redactor's `key_generic` rule

The Tier 2 redactor's generic-secret pattern only covers
`api_key|secret|token|bearer`, while the uploader's INV-TEL-002 preflight
additionally covers `password|passwd|pwd`. Concrete consequence:

- Student types `my password = AbCd1234567890abcdEfGh01234`.
- Redactor emits fragment unchanged (no rule matches `password=`).
- `queue.enqueue` writes the plaintext into `telemetry_queue.payload_json`
  in local SQLite.
- At upload time the uploader's preflight matches and calls
  `parkPermanent(..., "_preflight_key_generic")`, so INV-TEL-002 holds
  *on the wire*.
- The plaintext then lives in local SQLite until `sweepRetention` ages it
  out (30 days for queue rows).

INV-TEL-002 is not violated (nothing ships), but the student's plaintext
credential-ish data persists locally much longer than it should, and the
audit table shows a confusing `_preflight_key_generic` row without a
matching redactor trail. The two patterns must agree.

**Fix:** add `password|passwd|pwd` to the `key_generic` alternation in
`redactor.ts:90`, extend the REDACTION.md test matrix with a `password=…`
case, and bump `POLICY_VERSION` per REDACTION.md §Updating (which
triggers re-consent — intentional, since the redaction set changed).

---

### H2 — `ChatBar.tsx:897-901`: Enter submits mid-IME composition

```ts
const handleKeyDown = (e: React.KeyboardEvent) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    handleSubmit();
  }
};
```

On Windows/macOS with a CJK, Korean, Vietnamese, or any IME-composed
input method, the Enter that confirms composition also submits the chat
turn, sending a half-typed question. `React.KeyboardEvent.isComposing` (or
equivalently `e.nativeEvent.isComposing` / `e.keyCode === 229`) must be
checked.

**Fix:** `if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing)`.

---

### H3 — `db.ts:142-169`: `saveConversation` + `saveMessage` not wrapped in a transaction, orphans on partial failure

In `ChatBar.tsx:304-306` we fire three writes with individual
`.catch(() => {})`:

```ts
saveConversation(convId, program, moduleId).catch(() => {});
saveMessage(userMsg.id,   convId, "user",      userMsg.content,    …).catch(() => {});
saveMessage(finalizedId,  convId, "assistant", finalContent,       …).catch(() => {});
```

If `saveConversation` succeeds and either `saveMessage` fails (disk full,
quota, DB locked by a migration), the History page renders a conversation
row with zero or one messages. The user sees ghost conversations they
never had and cannot distinguish them from real ones. Swallowed errors
mean neither telemetry nor the user learn about it.

**Fix:** wrap the three writes in a single `tauri-plugin-sql`
transaction (or a `SAVEPOINT` inside an outer `BEGIN`) so partial state
cannot persist, and surface the error to the user at least via a
console warning.

---

### H4 — `microphone.rs:82-84`: `start_mic_capture` discards an in-flight speech clip

`start_mic_capture` calls `stop_mic_capture_inner()` at line 84 to reset
any prior session — but `stop_mic_capture_inner` returns
`Option<String>` (a WAV of the buffered speech it just terminated), and
`start_mic_capture` throws that return value away with a bare function
call. If the user rapid-fires the mic hotkey, the tail of the prior
utterance is silently deleted instead of being emitted via
`mic-speech-detected`.

**Fix:** capture the returned `Option<String>` in `start_mic_capture`
and, when present, `app.emit("mic-speech-detected", …)` before starting
the new stream.

---

## Medium

### M1 — `wotch.rs:235, 301`: per-request `reqwest::Client::builder()` violates INV-ARCH-001

Both `probe_wotch_api()` and `push_prompt_to_wotch()` construct fresh
`reqwest::Client` instances instead of reaching for
`app.state::<HttpClient>()` like the rest of the backend. `probe_wotch_api`
runs inside `await_wotch_api_up`'s 250 ms loop after `launch_wotch`, so
every "Open in Wotch" click builds ~N clients back-to-back.

Impact is modest — Wotch probes are short-lived and local — but this is
the exact pattern CLAUDE.md item 1 forbids and it sets a precedent.

**Fix:** accept `&reqwest::Client` (or `AppHandle` and pull the state)
into both functions and thread it through.

---

### M2 — `pointParser.ts:52`: `[POINT:…]` regex strips tags from inside Markdown code fences

```ts
cleanText = text.replace(/\[POINT:\d+(?:\.\d+)?,\d+(?:\.\d+)?:[^\]]+\]/g, "");
```

The regex runs over the entire rendered response without code-fence
awareness. If the model demonstrates pointer syntax inside a
```` ```text ```` block (e.g., "here's an example tag:
`[POINT:100,200:Save]`"), the tag is stripped from the visible code
example even though the user's intent was to see the literal syntax.

The twin concern — code-fence content firing real pointer events — is
NOT present here because `parsePointTags` is only consulted by the
overlay's `pointer_show` dispatcher, not by the Markdown render; the
regex is only cosmetic. But the cosmetic break is real.

**Fix:** walk the string once, tracking code-fence state, and only apply
the strip/extract outside fences. A minimal implementation splits on the
``` ``` ``` fence regex, processes non-fence segments, and re-joins.

---

### M3 — `ChatBar.tsx:210-258`: `listen()` rejection path leaks subscription slot

The cancelled-flag cleanup pattern is applied correctly for *resolved*
`listen` promises. The rejection branch, however, has no handler — if
any of the four `listen(...)` calls throws (unlikely but possible on a
permission-denied or disconnected backend), the `await` propagates out
of the async IIFE unhandled, and any unlisteners captured *before* the
rejection stay in `unlisteners` but are returned from a promise that
never resolves, so the React cleanup sees an empty local array. On the
next mount a second set of listeners is installed alongside the first.

Low probability of firing in practice, but the cost of a `try/catch`
around each `const uN = await listen(...)` is negligible.

**Fix:** wrap each `await listen(...)` in `try/catch` that either pushes
the unlistener on success or logs + proceeds on failure, so the outer
cleanup always has the right set of callbacks.

---

### M4 — `extension.rs:504-507`: `SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()` stamps `last_scan_ms = 0` on clock error

On a system where the wall clock steps backward past 1970-01-01 — a
corrupted RTC, a misbehaving VM, a WSL-on-suspended-host wake — the
`duration_since` call errors and `unwrap_or_default()` substitutes zero.
With `last_scan_ms = 0`, the extension-freshness check
(`now - last_scan_ms > 10s`) evaluates true forever, which permanently
demotes the extension to "disconnected" until the backend restarts.

Impact is small (same machine, rare), but the fix is one line.

**Fix:** log + early-return instead of silently stamping 0:

```rust
match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
    Ok(d) => lock.last_scan_ms = d.as_millis() as u64,
    Err(e) => eprintln!("[ext] clock error on /scan: {e}"),
}
```

---

### M5 — `microphone.rs:226-232`: `.ok()` swallows `encode_wav` failure on `stop_mic_capture`

```rust
return encode_wav(&s.speech_samples, s.sample_rate, s.channels).ok();
```

If `hound` fails to encode (bad sample rate / channel count / IO) the
function returns `None` and the frontend gets `Ok(None)` — identical to
"no speech captured". The user's spoken prompt is silently lost without
any signal that something went wrong.

**Fix:** propagate the error (`Result<Option<String>, String>`) or at
minimum `eprintln!` the failure before dropping to `None`.

---

## Low

### L1 — `app.context.tsx:148`: ref assigned during render

```ts
const isStreamingRef = useRef(false);
isStreamingRef.current = isStreaming;
```

React 19's concurrent renderer may discard and re-run a render; setting
`isStreamingRef.current` during render can therefore reflect a render
that never committed. In practice the 3-second poll always sees the
latest committed value by the time it fires, so this is cosmetic — but
the documented pattern is `useEffect(() => { ref.current = v; }, [v])`.

### L2 — `redactor.ts:369`: `too_many_keys` drop triggers on legitimate literal strings

Stage 5's confidence check counts `[key]` / `[jwt]` markers *in the
post-redaction text*. A student who paraphrases an error message
containing literal `"[key]"` / `"[jwt]"` inherits the marker count and
can be dropped with `_too_many_keys` even though no real secret was
redacted. Fix: count replacements done by *this run*
(`counts.key_generic + counts.key_sk_prefix + …`) instead of scanning
for marker strings.

### L3 — `uploader.ts:334-342`: retry schedule has no jitter

Five students enrolled from the same cohort-enroll handout and started
simultaneously will hit retry `#1` at exactly `+2 s`, then `+10 s`, etc.
Adding `Date.now() + wait + Math.random() * wait * 0.1` avoids a
thundering-herd against a single instructor endpoint. Not a correctness
issue, operational only.

### L4 — `consent.ts`: `grantConsent` is not idempotent per (cohort, tier, policy_version)

A double-click on the enroll button writes two rows with identical
`policy_version` and near-identical `granted_at`. `activeReceipt` still
returns the right one (`ORDER BY granted_at DESC LIMIT 1`), so behavior
is correct — but the audit trail has duplicates.

### L5 — `ErrorBoundary.tsx:18-20`: `componentStack` from `errorInfo` is never surfaced

Only `error.message` is displayed/logged. When a production crash
happens there's no breadcrumb of *which* component threw, making
diagnosis from a user bug report effectively impossible.

### L6 — `a11y/windows_impl.rs:25-29`: cross-crate-version HWND round-trip via `isize` cast

Documented on-purpose because `uiautomation` pins an older `windows`
crate than WorkBuddy's direct dependency. Works today on x64 and
ARM64 Windows but is fragile — if either crate ever changes HWND's
ABI, enumeration will silently return garbage. Worth a
`debug_assert!(handle as usize != 0)` and a comment pinning the
current version constraint.

### L7 — `sentenceBuffer.ts:107-125`: decimal / abbreviation detection is ASCII-only

`isAbbreviation` lowercases against a fixed ASCII set, `isDecimal`
treats only `'0'..'9'`. A sentence like `"The value was 3,14. Then…"`
(European decimal comma) or `"π.e."` tokenizes incorrectly. Minor —
English curriculum is the dominant case — but TTS streaming will
occasionally chunk mid-word for non-English content.

---

## False positives from the sub-agent scans (rejected on verification)

Recorded so we don't re-flag them in future audits:

- **"a11y.rs:112-119 double-subtracts monitor offset."**
  `UIElement::center()` (a11y.rs:32-37) returns raw bounding-rect
  coordinates without any offset applied; the single `- monitor_offset`
  at line 113-114 is correct. The existing test at lines 198-204
  confirms the current math is right.

- **"Capabilities file is missing entries for `synthesize_speech`,
  `stream_response`, etc."**
  In Tauri 2, app-defined commands registered via `generate_handler!`
  are allowed by default for the window the capability applies to;
  explicit entries are required only for *plugin* commands and scoped
  core APIs. The capability file is correct as-is — plugins
  (`shell:default`, `sql:default`, `global-shortcut:default`, etc.)
  and the core window APIs are individually granted.

- **"ChatBar.tsx setMessages updater generates duplicate IDs under
  Strict Mode."** `finalizedId = crypto.randomUUID()` is generated
  *outside* the `setMessages` updater at line 276 and captured by
  closure; the updater itself is pure. Strict Mode's double-invocation
  sees the same closed-over ID both times.

- **"ttsQueue.ts pushes stale audio after generation bump."** The
  cancellation/generation check at line 158 runs *before* `this.ready.push(...)`
  at line 159; a superseded synth promise returns without enqueuing.

- **"CursorOverlayWindow.tsx re-instantiates `SpringValue` per queued
  point."** New `SpringValue`s are only constructed on the
  first-point branch (`!processingRef.current`, line 184-185);
  subsequent points hit `queueRef.current.push(point)` and call
  `springXRef.current.setTarget(...)` on the existing instances.

- **"ResponsePanel handleListen captures settings from later render."**
  Both the `synthesize_speech` invoke and the `audio/wav` vs
  `audio/mpeg` MIME-type decision read from the same
  `settingsRef.current` snapshot on lines 178 and 183, so they can't
  disagree.

- **"set_settings wipes api_keys from disk."** `config.rs:348` has an
  explicit comment: "api_keys intentionally NOT copied." The frontend
  includes api_keys in the payload, but the backend simply ignores
  them; keys are persisted only via `set_api_key`.

---

## Build / test health

At time of audit on this branch:

```
npx tsc --noEmit   → 0 errors
npm test           → vitest run: all passed (0 exit)
cargo check        → 0 errors
```

The existing vitest matrix covers the redactor (REDACTION.md §Test
matrix, 33 cases), tagger threshold, and uploader preflight. It does
*not* cover the H1 `password=…` case; adding it should be part of the
H1 fix.

---

## Recommended order of remediation

1. **H1** (redactor/uploader parity) — bump POLICY_VERSION at the same
   time, extend the redactor test matrix.
2. **H2** (IME compose guard) — one-line fix, zero risk.
3. **H4** (mic clip discard) — small backend change, preserves user audio.
4. **H3** (SQLite transaction) — refactor ChatBar's post-stream writes
   into a single transaction helper.
5. **M1-M5** in any order; independent.
6. **L1-L7** opportunistically.
