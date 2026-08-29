# WordBuddy — Threat Model

Rewritten for WordBuddy reality (PLAN-07 Task 4). The old StudyBuddy/WorkBuddy-era
STRIDE table is gone; this document describes the product as it exists at main
HEAD `66a7cd9` (verified 2026-08-23 by reading every cited file; receipts
re-spot-checked by verifier entry 0021 after the llm.rs prune).

**Premise:** WordBuddy's core feature is _keystroke-adjacent monitoring_ — it
watches what the user types (browser fields, native editable fields, optionally
a low-level keyboard hook) and synthesizes input to apply fixes. The threat
surface is therefore: what text leaves the machine, what text is written to
disk, who can talk to the local services, and what the synthetic-input paths
can be tricked into touching.

All paths are relative to the repo root; line numbers are valid at `f064f3b`.

---

## 1. What leaves the machine

Only three things ever cross the network boundary, and all of them are HTTPS
calls to LLM/API providers:

| Egress                       | Trigger                                            | What is sent                                                                        | Receipt                                                                                                                                                                                                                                                 |
| ---------------------------- | -------------------------------------------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Style pass (engine)          | Automatic, **Browser surface only**, unless killed | The checked text + a system prompt rendered from the user's writing goals           | `run_style_pass` builds `user_prompt = text.to_string()` and one goals-derived `system_prompt` (`src-tauri/src/engine/style.rs:163-174`); production wiring passes `crate::llm::complete_text` as the transport (`src-tauri/src/engine/mod.rs:617-625`) |
| Chat streaming / vision chat | User-initiated from the UI                         | `system_prompt`, `user_message`, optional `screenshot_base64`, conversation history | `src-tauri/src/llm.rs:233-241`                                                                                                                                                                                                                          |
| API-key validation           | Settings "validate" button                         | The key itself, in a minimal request to `api.anthropic.com`                         | `src-tauri/src/config.rs:292-298`                                                                                                                                                                                                                       |

**Local correctness never networks.** The correctness pass is harper-core,
in-process: "Correctness pass — always, local, zero network" is the engine's
own contract (`src-tauri/src/engine/mod.rs:4`); harper types are imported and
linted directly, no client involved (`src-tauri/src/engine/mod.rs:34-37`,
`src-tauri/src/engine/mod.rs:332-395`). A tree-wide search for network-client
construction finds request-making code only in `llm.rs`, the shared
`HttpClient` (`src-tauri/src/lib.rs:23`, built at `src-tauri/src/lib.rs:69-72`),
and the key validator (`src-tauri/src/config.rs:292-295`) — nothing in the
engine, monitor, hook, or analytics modules.

**Egress destinations are pinned twice:**

- The webview CSP `connect-src` allowlists exactly the provider hosts
  (`src-tauri/tauri.conf.json:27`).
- The style pass runs only on the opted-in Browser surface and only when the
  `WB_DISABLE_LLM=1` kill-switch (INV-PRIV-003) is unset
  (`src-tauri/src/engine/style.rs:227-240`; kill-switch definition
  `docs/plans/CONTRACTS.md:155`). Native-monitor and palette checks are
  correctness-first and never invoke the LLM transport implicitly
  (`src-tauri/src/engine/style.rs:236-239`).
- A configured Ollama URL is rejected unless it is loopback
  (`src-tauri/src/llm.rs:55-86`).

**Style-pass retry disclosure:** if the model returns invalid JSON, the retry
attempt re-sends the checked text plus the previous error string
(`src-tauri/src/engine/style.rs:166-173`). Max 2 attempts
(`src-tauri/src/engine/style.rs:17`). No other augmentation of the payload.

---

## 2. What persists

**`writing.sqlite` analytics (counts only — INV-PRIV-002).**
`src-tauri/src/analytics/db.rs:1-6` states the rule in the module header:
"a strict no-field-text rule (INV-PRIV-002) — rows carry counts and rule names
only." Concretely, the `CheckEvent` row is timestamp, surface name, target
host/process name, word count, unique-word count, rare-vocabulary percentage,
and per-kind/per-rule count maps (`src-tauri/src/analytics/db.rs:99-110`);
the INSERT statement has no text column (`src-tauri/src/analytics/db.rs:117-125`).
Vocabulary stats are computed at record time from transient in-memory tokens
that are then dropped (`src-tauri/src/engine/mod.rs:698-716`). Failed writes
are dropped behind a counter, never buffered (`src-tauri/src/analytics/db.rs:13-19`).
Invariant definition: `docs/plans/CONTRACTS.md:152-154`; analytics restatement:
`docs/plans/PLAN-05-analytics.md:50-52`.

**Transient-by-default field text.** The native monitor keeps field text in
memory for the duration of a check only; logs carry at most process name and a
hash prefix (INV-PRIV-002, `src-tauri/src/text_monitor.rs:20-21`). Password
fields are detected before any value read and are never watched, read, or
logged (INV-PRIV-001, `src-tauri/src/text_monitor.rs:17-18`).

**Keyboard ring buffer is never durable.** The snippet hook's 32-char ring
exists transiently for trigger matching — "never persisted, never logged"
(`src-tauri/src/snip_hook.rs:11-12`; buffer at `src-tauri/src/snip_hook.rs:103`,
cleared on match at `src-tauri/src/snip_hook.rs:122-126`).

**Explicit user-authored text IS stored locally, in plaintext config.**
`personal_dictionary` (`src-tauri/src/config.rs:43-46`), `style_rules`
(`src-tauri/src/config.rs:72-75`), and snippet definitions including their
expansion bodies (`src-tauri/src/config.rs:84-86`) live in
`<OS config dir>/wordbuddy/config.json` (`src-tauri/src/config.rs:138-144`).
This is the enumerated INV-PRIV-002 exception — explicit user action
(`docs/plans/CONTRACTS.md:153-154`) — but it means disk readers on the same
account can read the user's vocabulary, style rules, and snippet bodies.

**Opt-in tone samples stay off.** `retain_snippets` (short text samples for
weekly tone analysis) defaults OFF (`src-tauri/src/config.rs:76-79`).

---

## 3. Browser extension surface

**Placement allowlist (where the checker may even run).** Content scripts
auto-run only on the built-in Limitless Exchange and GitHub matches. Other
HTTP(S) origins, including localhost, require a user-initiated per-origin
optional-host grant from the popup; the service worker then injects after
navigation. The connector runs only in top-level, non-incognito documents.
Permanent `host_permissions` are limited to the three localhost relay ports.

**Runtime exclusion deny-list (layered, both sides).** The desktop app pushes
`checkingEnabled` + `excludedHosts` with every authenticated `/scan` poll;
those app-owned preferences live in session storage only. The checker fails
closed until both local privacy settings and authenticated app preferences
load, then gates all activity on enabled/unpaused/non-excluded state before
attaching to a field. Password, credential, payment, OTP, token/secret, and
GitHub fields are hard-excluded before any text read. The server
does not trust the client: `/check` extracts the host first and returns empty
issues for excluded/disabled targets before any use of the text (INV-EXCL-001,
`src-tauri/src/extension.rs:679-707`; host-match semantics
`src-tauri/src/extension.rs:116-125`).

**Transport and auth posture (as implemented).**

- Server binds `127.0.0.1` only, ports 19521–19523
  (`src-tauri/src/extension.rs:830-860`).
- Every endpoint except `GET /status` requires `Authorization: Bearer <hex>`;
  failures get 401 (`src-tauri/src/extension.rs:556-569`). The token is a
  256-bit OS-CSPRNG hex string stored in the operating-system credential
  vault. A legacy plaintext `wordbuddy/extension-token` is migrated once and
  deleted. Tokens are compared in constant time and regenerable from Settings.
- Unauthenticated `/status` exposes only a connected flag + version string
  (`src-tauri/src/extension.rs:572-585`).
- Responses carry no CORS headers, so ordinary web pages cannot read them;
  the extension relies on `host_permissions` instead
  (`src-tauri/src/extension.rs:510-512`).
- Per-endpoint rate gates bound abuse even for a token holder: `/ask` 5 s,
  `/scan` `/highlight` `/check` 200 ms (`src-tauri/src/extension.rs:23-36`),
  CAS-safe slot advance (`src-tauri/src/extension.rs:48-75`). Each connection
  has a hard 10 s budget (`src-tauri/src/extension.rs:869-885`).

**Data flow into `/check`.** After all privacy gates pass, the content script
reads the focused eligible field's text (`value` for inputs/textareas,
`textContent` for contenteditable), hashes to deduplicate refires, debounces
300 ms, and chunks by the relay's 20,000-byte UTF-8 limit without splitting
astral characters. UTF-16 issue offsets remain unchanged within each chunk.
It POSTs a CONTRACTS
`CheckRequest` (surface, host, text, goals) through the service worker to
`http://127.0.0.1:<port>/check` with the bearer token
(`wordbuddy-extension/background.js:93-110`). Server-side, the request is
re-validated: 20 KB cap rejects oversized bodies rather than truncating
(`src-tauri/src/extension.rs:37-39`, `:708-718`), Settings writing-goals
override whatever the wire carried (`src-tauri/src/extension.rs:722-726`),
surface is forced to `Browser` so the style-pass policy applies
(`src-tauri/src/extension.rs:719-721`), and the result comes back as issue
spans only — the corrected text never transits the socket; replacements are
rendered client-side from issue data.

---

## 4. Keyboard hook guarantees (INV-HOOK-001)

Invariant definition: `docs/plans/CONTRACTS.md:183` ("O(1) callback,
unconditional CallNextHookEx, no I/O, watchdog self-disable"). Implementation
receipts in `src-tauri/src/snip_hook.rs`:

- **O(1) admission-blocking callback.** Module contract at `:3-10`. The
  callback body does exactly: printable-key classification, one ring append,
  bounded trigger scan, post-to-worker — single exit point with unconditional
  `CallNextHookEx`, even mid-expansion (`:135-186`, chain call at `:183-185`).
  Expansion work happens on a worker thread via mpsc, never in-callback
  (`:166-175` post, `:204-254` worker).
- **Watchdog self-disable >2 ms.** Budget constant `CALLBACK_BUDGET_NS =
2_000_000` (`:21-22`); the callback measures its own elapsed time and sets
  `WATCHDOG_TRIPPED` on overrun (`:178-182`); the pump thread observes the
  flag, breaks, unhooks, and clears `HOOK_ACTIVE`
  (`:267-289`). Verifier line-audit of this body at P6:
  `WordBuddy-coordination/channel/0017-verifier-p6-verdict.md:51-57`.
- **Deny-list.** Expansion never fires in terminals/IDEs/editors where trigger
  characters are syntax: `DEFAULT_EXCLUDED_PROCESSES`
  (`src-tauri/src/snip_hook.rs:31-36`), enforced in the worker against the
  actual foreground process before any injection
  (`src-tauri/src/snip_hook.rs:218-235`), plus the user-configured
  `excluded_processes` (`src-tauri/src/config.rs:59-62`).
- **Default OFF.** `snippets_enabled` serde default is absent/false
  (`src-tauri/src/config.rs:80-83`) and the factory default is `false`
  (`src-tauri/src/config.rs:113`). The hook installs only through an explicit
  start call while enabled (`src-tauri/src/snip_hook.rs:188-298`); a global
  pause flag suppresses processing without uninstalling
  (`src-tauri/src/snip_hook.rs:24-25`, `:56-58`).
- **Buffer privacy.** See §2: ring is transient, never logged/persisted
  (`src-tauri/src/snip_hook.rs:11-12`).

---

## 5. Apply-path safeguards (INV-APPLY-001)

Invariant definition: `docs/plans/CONTRACTS.md:182`; module contract:
`src-tauri/src/apply.rs:12-17` ("synthetic input only ever targets the exact
process captured with the issue … abort on any mismatch").

- **Process verification is atomic with the capability probe.** The expected
  process check happens _inside_ the fresh-at-apply-time probe
  (`src-tauri/src/apply.rs:49-62`); a mismatch aborts with an INV-APPLY-001
  message naming actual vs expected (`src-tauri/src/apply.rs:137-145`).
- **Stale-text aborts.** If the field text changed since the issue was
  captured, both strategies refuse to write: SetValue path
  (`src-tauri/src/apply.rs:151-158`), surgical-paste path
  (`src-tauri/src/apply.rs:192-199`).
- **Focus-race double-check on the paste path.** Foreground is re-verified
  right before the synthetic paste (`src-tauri/src/apply.rs:200-215`) and
  _again inside_ the clipboard/paste window
  (`src-tauri/src/apply.rs:216-225`).
- **Verify-after-apply with revert.** The SetValue strategy re-reads the text
  post-write and re-sets the original on mismatch
  (`src-tauri/src/apply.rs:160-183`); splice correctness is a pure, tested
  UTF-16 helper (`src-tauri/src/apply.rs:80-94`; offsets invariant
  INV-OFFSET-001, `docs/plans/CONTRACTS.md:62`).
- **Single-flight + clipboard serialization.** One apply process-wide
  (`src-tauri/src/apply.rs:78`, `:103-112`), clipboard lock held for the whole
  operation (`src-tauri/src/apply.rs:122`).
- **Reader boundary (monitor → apply).** The monitor captures the top-level
  window handle + process _while the field is focused_ into `FieldSnapshot`;
  apply re-acquires the element from that hwnd when the suggestion card itself
  holds focus at apply time (`src-tauri/src/text_monitor.rs:74-77`). The
  monitor side enforces exclusions before any value read
  (`src-tauri/src/text_monitor.rs:23-24`).

---

## 6. Residual risks, stated plainly

1. **Expansion-worker focus gap (known, ledger-tracked).** The snippet
   expansion worker injects into whatever window has focus _when the worker
   runs_, after an asynchronous hop; there is no HWND capture/verify, and the
   process deny-list is checked only after that hop
   (`src-tauri/src/snip_hook.rs:204-252`). This is verifier residual (a) of
   entry 0017 (`WordBuddy-coordination/channel/0017-verifier-p6-verdict.md:97-103`):
   weaker than INV-APPLY-001; a fast focus change between keypress and worker
   run can misdirect the expansion. Mitigations today: default-OFF, deny-list,
   ring-clear-on-match. Must be closed before snippets ship ON.
2. **Unsigned binaries, no updater.** The Tauri bundle config has signing or
   updater configuration nowhere (`src-tauri/tauri.conf.json:30-45`: bundle +
   sql plugin only). Distribution is unsigned: Windows SmartScreen /
   antivirus will warn on first run, there is no auto-update channel, and
   users have no signature to verify downloads against.
3. **Same-user local processes can hold the extension token.** Anything
   running as the user can read `wordbuddy/extension-token`
   (`src-tauri/src/extension.rs:330-343`) and then drive `/scan`, `/check`,
   `/ask`, `/highlight` within rate limits — spending the user's LLM budget
   or feeding forged page context. `/status` is deliberately unauthenticated
   (`src-tauri/src/extension.rs:556-557`).
4. **Chat screenshots leave the machine.** Vision chat streams an optional
   base64 screenshot to the provider when the user chats with screen context
   (`src-tauri/src/llm.rs:237`). User-initiated, but it is real screen
   content crossing the boundary.
5. **Watchdog trip is quiet.** A hook watchdog trip surfaces as stderr + a
   status flag; no tray notification (verifier residual (b),
   `WordBuddy-coordination/channel/0017-verifier-p6-verdict.md:104-105`).
6. **Analytics undercounts on cache hits.** A cache hit returns early and
   skips `record_check` (`src-tauri/src/engine/mod.rs:602-605` vs the record
   path at `:673-716`) — a reporting-fidelity gap, not a privacy one
   (`0017-verifier-p6-verdict.md:123`).

---

## Platform honesty

WordBuddy now supports native field detection and suggestion widgets on
Windows, macOS, and Linux through UIA, AX, and AT-SPI2 respectively. The
remaining platform boundaries are deliberate: fix application and the
keyboard-hook snippet feature are Windows-only; macOS/Linux report apply as
unsupported, and Wayland does not permit global synthetic input. macOS
requires Accessibility permission; Linux requires a working AT-SPI2 session.
Runtime coverage is strongest on Windows, and non-Windows app/desktop
combinations remain subject to the compatibility matrix.
