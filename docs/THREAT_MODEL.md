# WorkBuddy — Threat Model

## Assets

| Asset                   | Sensitivity | Location                    |
|-------------------------|-------------|-----------------------------|
| Anthropic API key       | High        | OS config dir (config.json) |
| OpenAI API key          | High        | OS config dir (config.json) |
| Google API key          | High        | OS config dir (config.json) — powers LLM, TTS, and STT |
| Groq / OpenRouter keys  | High        | OS config dir (config.json) |
| ElevenLabs API key      | Medium      | OS config dir (config.json) — powers TTS and STT |
| Screenshots             | High        | In-memory only (transient)  |
| Microphone audio        | High        | In-memory only (transient) — never sent to ElevenLabs/Gemini STT if no provider key configured |
| Accessibility tree data | Medium      | In-memory only (transient) — element names may reflect window titles, email subjects, chat contents |
| Conversation history    | Medium      | SQLite (local)              |
| Student's screen content| High        | Captured each query         |
| RAG vector store        | Low         | SQLite (rag_vectors.db)     |

## Trust Boundaries

```
┌─────────────────────────────────────┐
│  Student's machine (trusted)        │
│  ┌───────────────────────────────┐  │
│  │  WorkBuddy process           │  │
│  │  - Tauri webview (frontend)   │  │
│  │  - Rust backend               │  │
│  │  - Config file (API keys)     │  │
│  │  - SQLite database            │  │
│  │  - Microphone access          │  │
│  └──────────────┬────────────────┘  │
│                 │                    │
└─────────────────┼────────────────────┘
                  │ HTTPS (trust boundary)
    ┌─────────────▼─────────────────┐
    │  External APIs (semi-trusted)  │
    │  - api.anthropic.com           │  LLM
    │  - api.openai.com              │  LLM + Whisper STT + embeddings
    │  - generativelanguage.google.. │  LLM + Gemini TTS + Gemini STT
    │  - api.groq.com                │  LLM
    │  - openrouter.ai               │  LLM
    │  - localhost:11434 (Ollama)     │  LLM (local)
    │  - api.elevenlabs.io           │  TTS + Scribe STT
    │  - huggingface.co              │  OmniParser model download (one-time)
    │  - github.com (auto-update)    │
    │                                 │
    │  Local-only (same machine):     │
    │  - 127.0.0.1:19521 (extension) │  Chrome extension relay
    └───────────────────────────────┘
```

## STRIDE Analysis

### Spoofing
| Threat | Risk | Mitigation |
|--------|------|------------|
| Malicious app impersonates WorkBuddy | Low | App is local-only, no auth to spoof |
| MITM on API calls | Low | All API calls use HTTPS. CSP restricts connect-src. |
| Fake auto-update payload | Medium | tauri-plugin-updater verifies signatures. Update endpoint is HTTPS-only. |

### Tampering
| Threat | Risk | Mitigation |
|--------|------|------------|
| Tampered config file (modified API key to attacker's key) | Medium | 0600 file permissions on Unix. Windows: user-only access in AppData. |
| Tampered SQLite database | Low | Local-only, no remote sync. Content is non-critical (conversation text). |
| Malicious Tauri plugin | Low | No third-party plugins beyond official tauri-plugin-* crates. |
| Tampered binary (supply chain) | Medium | GPL-3.0 source available. Build from source. Future: code signing. |

### Repudiation
| Threat | Risk | Mitigation |
|--------|------|------------|
| Student denies asking a question | Low | Not a concern — no accountability system. Local-only history. |

### Information Disclosure
| Threat | Risk | Mitigation |
|--------|------|------------|
| API key leaked in logs | Medium | No logging of keys (INV-SEC-001). INV-SEC-003 forbids third-party analytics; the opt-in cohort feature runs a key-shape preflight (INV-TEL-002) on every payload before upload, so a student-pasted key is rejected before it reaches the instructor endpoint. |
| Student pastes API key into a study question | Medium | Redactor strips `sk-ant-...`, `sk-...`, `AIza...`, and `(api_key\|password\|passwd\|pwd\|secret\|token\|bearer)=<blob>` patterns to `[key]` before the tagger sees the fragment; uploader runs the same regex set as a last-line preflight and `parkPermanent`s the row on match (INV-TEL-002). |
| Screenshot accidentally routed through telemetry | High | Screenshots are never stored (INV-SEC-004). Uploader preflight additionally rejects any payload containing an >1024-char string with a PNG/JPEG/GIF/WebP/PDF/data-URI magic (INV-TEL-003). |
| Cohort token intercepted over the wire | Medium | INV-TEL-007 refuses any `endpoint_url` not starting with `https://` (case-insensitive); the `http://localhost:*` exception is compile-gated on `import.meta.env.DEV` so production builds cannot reach it. |
| Student enrolled in cohort without knowing | Low | INV-TEL-005: cohort ID is user-entered on the enrollment screen; the app never auto-discovers cohorts from files, env vars, or network probes. INV-TEL-004: pseudonym is `crypto.randomUUID()` with no input, never derived from identity. |
| Policy rotated to upload shape the student didn't agree to | Medium | INV-TEL-013: `POLICY_VERSION` change makes all active receipts stale; uploader `parkPermanent`s pending rows and the re-consent banner surfaces in Settings until the student explicitly re-affirms per tier. |
| Screenshot captures sensitive content | High | Screenshots are in-memory only (INV-SEC-004). Sent only to configured LLM API. Student controls when screenshots are taken. |
| Microphone captures ambient audio | High | Audio is in-memory only (INV-SEC-005). Push-to-talk requires explicit button hold. Audio sent only to the selected STT provider (Whisper / ElevenLabs / Gemini) — never to multiple providers simultaneously. |
| Accessibility tree reveals content | Medium | Element names from a11y (window titles, menu items, tab labels) may reflect sensitive content. a11y data is in-memory only and included only in the prompt for the current query. Student can disable via `a11y_detection_enabled` toggle in Settings. |
| Browser extension leaks form-field values | Medium | Non-password `<input>` and `<textarea>` values (emails, search queries, drafts) are included in scan data sent to the LLM. Password fields are unconditionally scrubbed by the extension (INV-SEC-008). Users who want additional coverage can enable `mask_form_inputs` in Settings > Browser Extension; this replaces values with type-aware placeholders like `[input: email]`, preserving field type/position context without leaking user-entered text. Default is off (current behavior) so existing users aren't disrupted. |
| Google key reused across 3 Gemini services | Low | The same `google` API key powers Gemini LLM, Gemini TTS, and Gemini STT. Losing the key exposes usage of all three, not separate surfaces. Mitigation: user can set per-service spending limits in Google AI Studio. |
| TTS text sent to provider | Low | LLM response text is educational content. Streaming sentence TTS sends each sentence to the configured provider (ElevenLabs or Gemini). Short strings with no sensitive data. |
| Config file readable by other users | Medium | 0600 permissions (INV-SEC-002). |
| Conversation history contains PII | Medium | SQLite is local-only. No cloud sync. Student can clear/delete conversations. |
| Google Fonts CDN tracks usage | Low | Fonts load with display=swap. No cookies. Future: bundle locally. |

### Denial of Service
| Threat | Risk | Mitigation |
|--------|------|------------|
| Unbounded SSE buffer consumes memory | Medium | 10MB buffer limit in llm.rs. |
| API rate limiting blocks the student | Low | Student controls request frequency. Multiple providers available as fallback. |
| Hung API request freezes UI | Medium | 10s connect timeout, 60s request timeout. UI remains responsive (async). |
| Microphone stream resource leak | Medium | stop_mic_capture drops Stream handle and clears Recording state (INV-ARCH-007). |
| SQLite database grows unbounded | Low | Local file. Student can delete conversations. 100-conversation query limit. |
| RAG indexing sends docs to OpenAI | Low | Only Limitless public documentation is indexed. No user content sent for embedding. |
| Streaming TTS sends response text to ElevenLabs | Low | LLM response text is educational content, not sensitive. Each sentence is a separate API call. |
| Tool_use exposes tool definitions to Anthropic | Low | Tool schemas (point_at, highlight) contain no sensitive data — just coordinate types and labels. |

### Elevation of Privilege
| Threat | Risk | Mitigation |
|--------|------|------------|
| XSS in markdown rendering | Medium | ReactMarkdown sanitizes HTML by default. Links intercepted and opened via shell plugin. |
| Tauri command injection | Low | Commands use typed parameters (Serde deserialization). No shell command construction from user input. |
| Claude response contains malicious instructions | Low | Responses are displayed as markdown, not executed. No eval() or dynamic code execution. |
| Microphone permission escalation | Low | cpal uses standard OS audio APIs. Permission granted at OS level, not by app. |
| Accessibility API permission abuse | Low | Windows UIA requires no permission; macOS requires explicit user grant in System Settings; Linux AT-SPI2 runs over user session bus. WorkBuddy only reads (never synthesizes input events). Data stays local. |

## Recommendations

### Immediate (v0.1)
- [x] File permissions 0600 on config.json
- [x] No third-party analytics; optional cohort telemetry is opt-in,
      enforced by INV-TEL-001..013
- [x] HTTPS-only API calls with CSP enforcement
- [x] Buffer limits on streaming responses
- [x] HTTP timeouts on all requests
- [x] Screenshots in-memory only
- [x] Audio in-memory only
- [x] Push-to-talk requires explicit button hold (not always-on)

### Future (v0.2+)
- [ ] Migrate API keys from plaintext JSON to OS keychain (keyring crate)
- [ ] Bundle Google Fonts locally for offline/privacy
- [ ] Add code signing to release binaries
- [ ] Add integrity checks on config file (detect tampering)
- [ ] Offer screenshot review before sending to LLM (privacy UX)
- [ ] Add audio level indicator during recording (transparency)
- [ ] SQLite encryption for conversation history (sqlcipher)

---

## Dependency Advisory Analysis

Dependabot may flag transitive dependencies in our `Cargo.lock` that we
cannot update independently because they're pinned by Tauri's dependency
graph. Each section below documents why a flagged advisory is not currently
exploitable in WorkBuddy's usage, and what would be required to upgrade.

### GHSA / Dependabot: `rand` unsound with custom logger

**Advisory:** `rand >= 0.7.0, < 0.9.3` has a soundness issue when a
custom `log` crate logger reentrantly calls `rand::rng()` during reseed.

**Our tree contains:**
- `rand 0.7.3` — build-dep via `phf_generator 0.8.0` → `phf_codegen` →
  `selectors 0.24.0` → `kuchikiki` (Tauri's speedreader fork). Build-time
  only (proc-macro / codegen).
- `rand 0.8.5` — build-dep via `phf_generator 0.10.0` → `phf_macros`
  proc-macro → `cssparser` → `kuchikiki`. Proc-macro / build-time only.
- `rand 0.9.3` — runtime via `image 0.25.10`, `rav1e`, `xcap`,
  `paddle-ocr-rs`. This is the **fixed** version per the advisory
  (affected range is `< 0.9.3`).

**Exploitability:** Not reachable.
1. WorkBuddy does not install a custom `log::Log` implementation. We use
   plain `eprintln!` for diagnostics. The `log` crate isn't even a
   direct dependency of our code.
2. The two affected versions (0.7.3, 0.8.5) are build-time-only proc-macro
   generators. At build time no custom logger is installed, so the
   re-entrancy conditions cannot occur.
3. The runtime version (0.9.3) is already the fix.

**Upgrade path:** Requires new `tauri-utils` that picks newer `selectors`
and newer `cssparser` (dropping `kuchikiki`'s old phf chain). Tracked
upstream; will land with a future Tauri bump.

### GHSA / Dependabot: `sqlx <= 0.8.0` binary protocol length-prefix overflow

**Advisory:** Values > 4 GiB encoded through the Postgres/MySQL binary
protocol can overflow the `u32` length prefix, allowing protocol-level
query smuggling.

**Our tree:** `sqlx 0.8.0` → `tauri-plugin-sql 2.4.0` → workbuddy.

**Exploitability:** Not reachable in our usage.
1. We use SQLite exclusively (not Postgres/MySQL). The advisory's
   Postgres length-prefix example doesn't apply identically, though
   SQLite's encode path has similar truncating casts flagged by the
   advisory authors.
2. Stored values are bounded in practice:
   - Chat message content: streamed through `chat_stream_chunk` with a
     10MB SSE buffer limit in `llm.rs` (INV-ARCH-004 context).
   - Conversation/message IDs: UUIDs, ~36 chars each.
   - Program/module IDs: enum-like strings, < 100 chars.
   - Screenshots and audio are never persisted (INV-SEC-004, INV-SEC-005).
3. An attacker would need to inject a 4 GiB value through the chat input
   or conversation metadata — no such path exists in the UI. Even an
   extremely long LLM response is capped at 10MB by the stream buffer.

**Upgrade path:** `cargo update -p sqlx --precise 0.8.1` fails because
sqlx 0.8.1 pulls in `libsqlite3-sys 0.30.1`, which conflicts via the
native `links = "sqlite3"` constraint with our `rusqlite 0.31.0 →
libsqlite3-sys 0.28.0`. Only one crate can own the native SQLite
linkage. Resolution requires upgrading both `tauri-plugin-sql` (for new
sqlx) *and* `rusqlite` (for matching `libsqlite3-sys`) together. Wait
for a Tauri release that pulls sqlx 0.8.1+ and bump rusqlite in lockstep.

### GHSA / Dependabot: `glib < 0.20.0` VariantStrIter unsoundness

**Advisory:** `VariantStrIter::impl_get` passed `&p` (immutable) to a C
function that mutates through it, producing UB. Newer rustc versions
strip these writes when optimizing, causing NULL-pointer dereferences.

**Our tree:** `glib 0.18.5` — **Linux only.** Transitive via
`tauri → gtk-rs → webkit2gtk → glib`. Pulled in by `[target.'cfg(target_os =
"linux")']` features of Tauri.

**Exploitability:** Not known to be exercised.
1. Windows and macOS builds do not link glib at all.
2. On Linux, whether `VariantStrIter` is called depends on wry/webkit2gtk
   internals — not invoked from our own code.
3. Impact is NULL-pointer dereference (crash), not RCE. Affects
   availability on Linux if triggered, not confidentiality/integrity.

**Upgrade path:** `cargo update -p glib --precise 0.20.x` fails because
`gtk 0.18.2` (pulled by Tauri 2.10.3) requires `glib ^0.18`. Resolution
requires Tauri to upgrade to `gtk 0.20+`. Tracked upstream in Tauri.

### GHSA / Dependabot: `rustls-webpki` accepted invalid URI name constraints

**Advisory:** `rustls-webpki < 0.103.12` incorrectly accepted URI
subjectAltNames under Name Constraints extensions that should have been
rejected. A name-constrained intermediate CA could mint certs with URI
SANs outside its constraint and rustls would accept them.

**Our tree:** `rustls-webpki 0.103.11` → `rustls 0.23.38` → `hyper-rustls`
+ `rustls-platform-verifier` + `tokio-rustls`, all pulled in by
`reqwest 0.13.2` which is itself pulled in by `tauri-plugin-updater
2.10.1`. Our direct `reqwest = "0.12"` uses the default `native-tls`
feature, so the CHAT/TTS/STT/embeddings API calls do NOT exercise rustls.
Only the updater (GitHub release checks) uses rustls-webpki.

**Exploitability:** Low. The updater connects only to `github.com` via
its API. GitHub's cert chain is DigiCert-issued with no Name Constraints
extension, so the vulnerable code path (validating a chain containing a
name-constrained CA) is not exercised in normal operation. Exploitation
would additionally require a MITM on the user's connection.

**Resolution:** Fixed by `cargo update -p rustls-webpki` bumping to
`0.103.12`. Clean patch upgrade — no downstream dep conflicts. Applied
2026-04-18.

### GHSA / Dependabot: `rustls-webpki` accepted wildcard-name constraints

**Advisory:** `rustls-webpki < 0.103.12` incorrectly accepted Name
Constraints against certificates asserting wildcard subjectAltNames. A
name-constrained CA could issue `*.attacker.com` under a constraint
intended to limit it, and rustls would accept it.

**Our tree:** Same code path as the URI-names advisory above —
`rustls-webpki 0.103.11` via `tauri-plugin-updater`'s `reqwest 0.13.2`.

**Exploitability:** Low — same reasoning as the URI-names advisory. Only
the updater uses rustls; only targets GitHub; no name-constrained CAs in
the chain.

**Resolution:** Fixed by the same `rustls-webpki 0.103.12` bump. Applied
2026-04-18.

### Summary of flagged advisories

| Advisory | Affected version | Our use | Exploitable? | Status |
|----------|-----------------|---------|-------------|--------|
| rand unsound-with-logger | 0.7.3, 0.8.5 | Build-time only | No (no custom logger) | Blocked: Tauri `kuchikiki` fork |
| sqlx length-prefix overflow | 0.8.0 | Conversation store | No (no 4 GiB input path) | Blocked: `libsqlite3-sys` native-links conflict with rusqlite 0.31 |
| glib VariantStrIter UB | 0.18.5 | Linux webview only | Unlikely (crash, not RCE) | Blocked: Tauri `gtk 0.18` pin |
| rustls-webpki URI name constraints | 0.103.11 | Updater → GitHub | Low (MITM + name-constrained CA required) | **Fixed** 2026-04-18 (→ 0.103.12) |
| rustls-webpki wildcard constraints | 0.103.11 | Updater → GitHub | Low (MITM + name-constrained CA required) | **Fixed** 2026-04-18 (→ 0.103.12) |

The three `Blocked` advisories will resolve when Tauri publishes a release
that bumps its internal dep graph. Monitor `tauri`, `tauri-plugin-sql`,
and `webkit2gtk-rs` release notes. Re-run `cargo tree -i <crate>` after
any Tauri bump to verify the vulnerable version is gone.
