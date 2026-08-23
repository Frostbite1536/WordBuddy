> STALE — WorkBuddy-base document; kept for provenance. Authoritative specs live in docs/plans/

# WorkBuddy Production Readiness — 2026-04-24

This document captures the full production-readiness audit that ran
against the post-`fbc15c4` tree (BUG_AUDIT.md remediation + the two
plan deferred items already merged via PR #31). Five parallel audits
covered Security, Concurrency + Resource Leaks, Production Operations,
Dependencies + Test Coverage, and UX + Accessibility.

The PR that introduced this document (`claude/production-readiness-audit`)
fixes everything in §Fixed-in-this-PR. Everything in §Deferred is
documented here so the team can address it consciously rather than
finding out at pilot time.

---

## Fixed in this PR

### Security (S-tier)

| ID | Severity | What changed |
|---|---|---|
| **S1** | Critical | `/ask` no longer auto-submits. The endpoint now sets `externalQuestion` state and ResponsePanel renders a confirmation banner with explicit Submit / Discard. Threat: a local process holding the extension token could otherwise drive the user's LLM quota silently. |
| **S2** | High | CSP rewritten: `connect-src 'self' https: http://localhost:11434` (allows arbitrary HTTPS for instructor endpoints — was previously hard-coded to LLM hosts only and silently broke telemetry uploads). Added `frame-ancestors 'none'`, `object-src 'none'`, `base-uri 'self'`. |
| **S3 / D7 / D8** | High | Capability split: `default.json` is now scoped to `["main"]` only. New `cursor-overlay.json` grants the overlay window only `core:event:*`. Dropped 11 unused `core:window:*` grants. Defense in depth — a future XSS in the overlay can no longer reach `shell:open`, `sql:*`, `process:exit`, or 50+ Tauri commands. |
| **S4** | High | Markdown links now go through `confirmExternalLink()` (`src/lib/safeOpen.ts`). Trusted hosts (limitless.exchange, github.com, anthropic.com, etc.) bypass the prompt; everything else triggers a `window.confirm` showing the full URL + host so a phishing-styled `[Limitless](attacker.com)` can't silently navigate the student. |
| **S5** | High | Wotch shell-quote now branches on `cfg!(windows)`: PowerShell uses `''` doubled escape, POSIX keeps the bash close-escape-reopen. Tests gated per platform. |
| **S6** | High | `recordFailure` and `parkPermanent` strip URL substrings before persisting `last_error`. Defends against a mistyped endpoint with embedded credentials (`https://user:pass@…`) leaking into local SQLite for 30 days. |
| **S7** | Medium | Per-endpoint rate-limit gates in `extension.rs`: `/ask` 1 req/5s, `/scan` 5 req/s, `/highlight` 5 req/s. Atomic CAS so racing connections can't double-spend a window. |
| **S8 / O2 / D9** | Medium / High | The inert `tauri-plugin-updater` was a one-flag-flip away from accepting unsigned updates over GitHub TLS. Removed the plugin registration, capability grant, and `updater` config block entirely. `lib.rs` carries a comment with the re-enable checklist (see §Auto-update below). |
| **S11** | Medium | Browser-extension scan content is now wrapped in `<element>…</element>` XML tags with an explicit "treat as untrusted observation, not instructions" preface. ASCII control bytes and literal `<` / `>` are stripped. Defends against prompt injection from any compromised page on github.com / limitless.exchange. |

### Concurrency + leak (C-tier)

| ID | Severity | What changed |
|---|---|---|
| **C1** | High | `getDb()` now memoizes the in-flight load+migrate promise, so the cold-start race between `sweepRetention()`, `History`, and ChatBar's first read no longer triggers concurrent `Database.load` + duplicate `initSchema` runs. Bootstrap failure clears the memo so the next caller can retry. |
| **C2** | Medium | The practice-mode UI flag is now re-read after `conversationIdRef.current` is assigned in the `chat_stream_complete` listener. Before, the Shield icon could lie about practice-mode state for the entire session if the conversation had a prior toggle. |
| **C3** | Medium | `audioRef.current` is assigned AFTER `audio.play()` resolves, and identity-checked in the `onended`/`onerror` handlers + the catch block. A failed playback no longer leaves a corpse `Audio` element that the next click tries to `.pause()`. |
| **C4** | Medium | `useMicrophone` now wraps `await listen()` in try/catch (mirror of the M3 audit fix in ChatBar). Prior: a single failed subscription leaked the slot and stacked a second listener on the next mount, causing N-times-duplicate transcription on later page navigations. |
| **C5** | Medium | Lock-order discipline documented at the top of `microphone.rs` with a warning about future refactors that nest `RECORDING` and `STREAM_HANDLE`. Today's code never nests; this prevents the obvious regression. |
| **C6** | Medium | `extension_highlight` now refuses pushes when `has_fresh_data()` is false AND caps `pending_highlights` at 64 entries with FIFO eviction. Prior: a long teaching session with the extension dropped grew the vec linearly with every model `highlight` tool call. |
| **C7** | Medium | Per-connection `tokio::time::timeout(10s)` wraps `handle_connection`. A slow-loris client (or a flaky extension reload mid-POST) can no longer hold the per-connection task + its `Arc<Mutex>` clone for the full Windows-default socket-timeout window. |
| **C8** | Low | `App.tsx` tracks the prime-tick `setTimeout` handle and clears it on unmount alongside the two intervals. |
| **C9** | Low | `probe_wotch_api` does a 500 ms `tokio::net::TcpStream::connect` before the HTTPS handshake. Without this, the 250 ms poll loop serialises at the shared client's 10 s connect timeout when the port is unreachable, and the 5-second deadline always expires on cold start. |
| **C10** | Low | `/status` handler builds the response body inside a tight scope so the `state.lock()` drops BEFORE `write_response` awaits on the socket. A slow client can no longer block `/scan` and `/ask` system-wide. |
| **C11** | (verified clean) | Audit's "stuck spinner on tool-continuation error" was a near-miss — every Err path is caught by the frontend's `invoke().catch` which sets `setIsStreaming(false)`. Added a contract comment to `llm.rs` documenting the asymmetry so the next refactor doesn't both emit AND throw. |
| **C13** | Low | `CursorOverlayWindow.showPoint` now calls `stopAnimation()` before reassigning the spring refs. Eliminates a brief jitter on rapid-fire pointing. |

### Operations + observability (O-tier)

| ID | Severity | What changed |
|---|---|---|
| **O3** | High | New versioned migration framework in `db.ts` using `PRAGMA user_version`. `MIGRATIONS[v0]` carries the entire current schema; future column additions go in `MIGRATIONS[v1]`, `[v2]`… with `SCHEMA_VERSION` bumped accordingly. A DB written by a newer build than this binary throws at startup rather than silently breaking column reads. |
| **O5** | Medium | New `getDiagnostics()` aggregates queue health (parked count, oldest pending age, top error reasons). Settings → Cohort Telemetry now shows an amber banner when `parked_count > 0` or `oldest_pending_age > 1h`, with the top errors inline. Prior: 100% of rows could be parked and the user would only discover it in the per-row payload modal. |
| **O6** | Medium | `load_config()` now quarantines a corrupt `config.json` to `config.json.corrupt-<timestamp>` before resetting to defaults, with a loud `eprintln!`. Prior: the file was silently overwritten with defaults, wiping every API key + cohort enrollment. |
| **O7** | Medium | `debug_log` now passes through `redact_for_log()`: drops control bytes, truncates to 200 chars, gut-redacts known credential prefixes (`sk-ant-…`, `sk-…`, `AIza…`). Prevents a future log-to-file pipeline from promoting PII straight from the JS layer into a persisted file. |
| **O10** | Low | `release.yml` gains a "Verify version consistency" step that fails the build when the git tag, `tauri.conf.json:version`, and `package.json:version` disagree. Prevents updater-loop bugs once auto-update is re-enabled. |

### Dependency hygiene (D-tier)

| ID | Severity | What changed |
|---|---|---|
| **D1** | Low | Dropped the unused `@tauri-apps/plugin-global-shortcut` npm dep. (Rust crate stays — the actual binding is Rust-only.) |
| **D2** | High | `openssl 0.10.77 → 0.10.78` (RUSTSEC-2025-0022, UAF in `Md::fetch` / `Cipher::fetch`). |
| **D3** | Medium | `rustls-webpki 0.103.12 → 0.103.13` (RUSTSEC-2025-0036, cert-path validation). |
| **D10** | Medium | `ci.yml` now gates on `npm audit --audit-level=high --omit=dev` and (Linux only) `cargo audit`. The two RUSTSECs above would have been blocked at PR time with this in place. |
| **D11 / S10** | Medium | Pinned `sqlx = "=0.8.2"` in `src-tauri/Cargo.toml` to force a patched version through `tauri-plugin-sql`'s transitive pin. RUSTSEC-2024-0363, sqlx <0.8.1 binary-protocol arg-length misinterpretation. Bumped `rusqlite 0.31 → 0.32` in both workspace members to keep `libsqlite3-sys` major aligned (Cargo's `links` constraint requires one version site-wide). |

### UX + accessibility (U-tier)

| ID | Severity | What changed |
|---|---|---|
| **U1** | High | Push-to-talk button now uses Pointer Events (mouse + touch + pen) AND `Space` / `Enter` key handlers. Tab-focused mic button is keyboard-operable. Added `aria-pressed`, helpful `aria-label`, and an `onBlur` safety release so alt-tab while recording doesn't leak the mic stream. |
| **U2** | Medium | Onboarding "Get an API key" anchor → real `<button>`. On `open()` failure, the URL is copied to the clipboard with a fallback `alert()` so the student has a recovery path. |
| **U3** | High | `handleValidateKey` now distinguishes 401 / 429 / 5xx / network / save-error and renders actionable text. `validate_api_key`'s previous behavior (single boolean for every failure mode) was a hard dead-end on the most common first-day path. |
| **U4** | Critical | The Skip path no longer drops the student into chat with no provider configured. Onboarding tracks `skippedKey` and renders an amber banner on the "Ready" step warning to configure Settings before asking the first question. |
| **U5** | High | `friendlyStreamError()` (`src/lib/friendlyError.ts`) maps raw provider errors into human-readable guidance with category-specific hints (auth, rate, billing, network, server). Falls through to a `Details: …` line for power users. |
| **U6** | High | `useMicrophone` now exposes `micError` + `clearMicError`. ChatBar pops the chat shell and surfaces the error as an assistant message — no more silent dead-end on denied OS permission, missing STT key, or transcription failures. |
| **U7** | Medium | Skip-with-typed-key now warns `window.confirm("…will discard it. Continue anyway?")` so the student doesn't lose a freshly-typed key by accident. |
| **U8** | Medium | Global `@media (prefers-reduced-motion: reduce)` rule in `index.css` disables animation-iteration-count and zeroes transition-duration, plus explicit overrides for `animate-pulse-ring`, `animate-bounce`, `animate-spin`, `animate-pulse`. The cursor overlay's overshoot easing — a known vestibular-disorder trigger — now collapses cleanly. |
| **U9** | High | Global `:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }` in `index.css`. Tailwind's `focus:outline-none` is liberally applied; without `:focus-visible` the keyboard-only experience was completely invisible focus on a dark theme. |
| **U10** | Medium | `role="alert" aria-live="polite"` on the error displays in `Onboarding.tsx` (key validation) and `CohortEnroll.tsx`. Missing input-level `aria-invalid`/`aria-describedby` are wired on the onboarding key input. |
| **U11** | Medium | API key input in `Onboarding.tsx` now: trims + strips internal whitespace before save, has a show/hide eye-icon toggle, uses `font-mono` for legibility. Settings-side equivalents are in §Deferred. |
| **U12** | High | `set_api_key` failures in onboarding are caught separately from the validate call and surface their actual error text. Final `handleFinish` re-issues `set_api_key` defensively so a successful validate followed by a failed persist doesn't ship the student into chat with an empty key. |
| **U13** | Medium | Cohort enrollment now URL-validates `endpoint_url` (must be a parseable `https:`); inline amber hint shows when the field is non-empty but invalid. |
| **U15** | Medium | TelemetryPayloadModal now closes on Escape, has `role="dialog" aria-modal="true" aria-labelledby="…"`, and a real id linking title → dialog. |

---

## Deferred — needs operational decisions

These items are documented rather than fixed because they require choices that should not be made unilaterally by Claude.

### Auto-update (O1 + O2 + S8 + D9)

The updater plugin was removed entirely in this PR (it was inert: `active: false` + empty `pubkey` would have either failed at build or accepted unsigned binaries). To re-enable, the team needs to:

1. Generate a Tauri signing keypair: `tauri signer generate -w ~/.tauri/workbuddy.key`. Save the password in 1Password.
2. Commit the **public** key to `tauri.conf.json` under `plugins.updater.pubkey`.
3. Add `TAURI_SIGNING_PRIVATE_KEY` (the contents of the `.key` file) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` to GitHub Actions secrets.
4. Pass them through in `release.yml` to `tauri-action`.
5. Re-add `tauri-plugin-updater = "2"` to `Cargo.toml`, register the plugin in `lib.rs::run()`, and re-add the capability grant `updater:default` to `default.json`.
6. Re-add the `plugins.updater` block to `tauri.conf.json` with `"active": true`.
7. Wire a frontend `check()` call behind a Settings toggle, with a 5-second timeout + silent fallback so a downed GitHub doesn't block app launch.

Until then: students update by downloading new releases manually. Communicate version bumps via Discord / email.

### Crash + error reporting (O1) — RESOLVED 2026-04 (Option A)

Original problem: `eprintln!` and `console.error` go to a stderr/console that students don't see, especially on the Windows release build (`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` detaches stderr entirely).

Resolution: Option A from the original audit — log file, no telemetry. `tauri-plugin-log` writes to `app_log_dir()`; rotation 5 MB per file, KeepOne so disk usage stays bounded. `diagnostics::install_panic_hook()` is set before the Tauri builder runs and captures panic location + payload + backtrace to both the log stream and stderr. Settings → Diagnostics exposes two buttons:

- **Open log directory** — opens the OS file manager at `app_log_dir()` (`%LOCALAPPDATA%\<id>\logs` on Windows, `~/Library/Logs/<id>` on macOS, `~/.local/state/<id>/logs` on Linux).
- **Copy last 5 MB** — reads the tail of the most-recent log file and writes it to the clipboard. Useful for support handoff.

Conformance: matches `docs/PRINCIPLES.md` §97 — no error-reporting SDK, no auto-upload. The user is the only path the log can reach a third party.

Option B (Sentry / Rollbar) remains a future possibility but explicitly requires a separate privacy-policy update + consent flow; not pursued.

### Lesson plans bundle path (O4) — RESOLVED 2026-04

Original problem: `tauri.conf.json:53-57` referenced `../../PM_Academy/lesson_plans/*.md` — paths that resolved to a sibling tree above the `workbuddy-app` repo. Cloning the standalone `Frostbite1536/WorkBuddy` repo and running `npx tauri build` failed the resource-bundling step with "no files matched glob" because the `PM_Academy` tree only existed in the monorepo checkout.

Resolution: option (a) — bundle in-tree. `tauri.conf.json` now references `lesson_plans/<academy>/*.md` relative to `src-tauri/`. 60 lesson plans live under `src-tauri/lesson_plans/{pm_academy,api_academy,agents_academy,limitless_trader_lab}/`. `scripts/sync-lesson-plans.{sh,ps1}` mirrors the canonical `PM_Academy/` upstream into the bundled tree — run with `--apply` (bash) or `-Apply` (PS) after a curriculum edit, then commit the resulting diff. Standalone clones build with no PM_Academy access; the upstream repo is only required for editorial sync.

### Test coverage of new critical paths (D5 + D6)

Today the vitest suite covers redactor (33 cases from REDACTION.md), tagger, and uploader preflight — 56 total. Untested critical paths added in PR #29 / PR #31 / this PR:

- `db.ts::saveTurn()` — the BEGIN/COMMIT/ROLLBACK transaction for conversation + messages.
- `db.ts::SCHEMA_VERSION` migration walk + downgrade refusal.
- `queue.ts::evictUntilFits()` — the FIFO eviction at the 10 MB cap.
- `queue.ts::recordSent()` — receipt write idempotency.
- `consent.ts::grantConsent()` — `(cohort, tier, POLICY_VERSION)` idempotency.
- `consent.ts::reConsent()` / `withdrawConsent()` — Tier 1 → Tier 2 cascade.
- `pointParser.ts::segmentByCode()` — fenced + inline + unterminated fence cases.
- `healthz.ts::isKillSwitchActive()` — cache TTL, null-on-failure semantics.
- `safeOpen.ts::confirmExternalLink()` — host allowlist + URL parse error path.
- `friendlyError.ts::friendlyStreamError()` — error category mapping.

Realistic target: ~40 added vitest cases. Should be a dedicated follow-up PR — adding all of them in this audit PR would obscure the actual fixes from review.

Rust subsystems with zero `#[cfg(test)]`:
- `llm.rs` per-provider SSE parsers (Anthropic `message_stop` vs OpenAI `[DONE]`, `tool_use` content blocks). The "Never use `[DONE]` for Anthropic" rule in `CLAUDE.md` is exactly the kind of thing fixture tests should defend against.
- `useMicrophone` state machine (vitest hook harness).
- `STREAM_GENERATION` cancellation behavior.

### `windows` crate sprawl (D4)

Cargo.lock contains `windows 0.54`, `0.58`, `0.61`, `0.62` simultaneously. This PR converged some via the rusqlite + sqlx bumps; further convergence is upstream-bound (cpal/coreaudio chain pulls 0.54). No active CVE today; document for future.

### Windows DACL on token files (S9)

`extension-token`, `config.json`, and the SQLite DB all apply `mode 0o600` only on `cfg(unix)`. On Windows they inherit the parent dir ACL, typically readable by any process running as the user. The fix is ~30 lines of `unsafe` Rust per file (`CreateFileW` + `SetSecurityInfo` with a per-user-SID DACL), plus DPAPI `CryptProtectData` for the SQLite-stored cohort tokens. Worth doing before the M3 pilot but needs careful security review of the unsafe code.

### Release-binary signing (O8 partial)

The release pipeline doesn't currently:
- Notarize macOS builds (Gatekeeper warnings on every install).
- Authenticode-sign Windows builds (SmartScreen "unrecognized publisher" warnings — a major install-rate killer).
- Run `cargo clippy --all-targets -- -D warnings` or `cargo fmt --check` (style/lint regressions slip through).

Apple notarization needs `APPLE_CERTIFICATE`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` secrets. Authenticode needs an EV cert (Azure Trusted Signing is the cheapest current option). Both are operational decisions / paid services.

### Heavy native deps + release profile tuning (O9)

`ort = "=2.0.0-rc.10"` (~30-200 MB native ONNX runtime) + `paddle-ocr-rs` + `tokio = { features = ["full"] }` + no `[profile.release] lto = true, codegen-units = 1, strip = true`. First-launch download size is large; cold-start on a modest student laptop is noticeably delayed even when UI detection is disabled. Worth a focused performance pass with profile + feature trimming after M3 pilot data tells us what students actually use.

### Settings page IA (U14)

Settings is a 580-line single-scroll render on a 600 px window. The custom 6 px webkit scrollbar is invisible on dark bg, the cohort telemetry section is buried at line 1803, and there's no jump-to-section nav. Big UX pass — out of scope for the audit PR.

### RAG folder picker (U16)

Adding a folder picker requires `@tauri-apps/plugin-dialog` (npm + Rust crate + capability + plugin registration). Larger plumbing change than the audit PR should carry. Documented as a clear improvement.

### Settings-side API key input parity with Onboarding (U11 partial)

The trim + show/hide pattern was added to Onboarding only. Settings has 9 different key inputs (`Settings.tsx:1244-1273` provider, `:1275-1298` ElevenLabs, `:469-479` Google inline, plus six more) — applying the pattern uniformly is a substantial refactor that should land alongside the U14 IA pass.

### Operational milestones (M3, M5, M6 from ROLLOUT.md)

These are not code:
- **M3**: pilot cohort (1 instructor, 5-10 students, 2-week run).
- **M5**: second cohort with both tiers.
- **M6**: general release docs / blog post / privacy-policy update.

Cannot be addressed by code; included here so the next reader has a single place to see the full open list.

---

## Verification

```
npx tsc --noEmit       # 0 errors
npm test               # 56/56 passing (no test changes — new paths covered in §Deferred)
INV-TEL-012 grep       # 0 vendor names in src/lib/telemetry/
cargo check            # 0 errors (lesson_plans glob warning is a separate pre-existing issue, see §Deferred)
npm audit --omit=dev   # 0 vulnerabilities
cargo audit            # 0 advisories after the dep bumps
```

---

## What did NOT change

The audits explicitly verified clean and these areas needed no work:

- No `dangerouslySetInnerHTML` anywhere; react-markdown renders HTML escaped (no XSS surface).
- No `danger_accept_invalid_certs` / `danger_accept_invalid_hostnames` in any reqwest builder.
- All SQL uses parameterized `$1`-style placeholders; no string interpolation into queries.
- All `.unwrap()` instances on `Mutex::lock()` use `unwrap_or_else(|e| e.into_inner())` (mutex-poison-safe).
- No SSRF surface — provider URLs are hardcoded except validated-loopback Ollama.
- `npm audit --omit=dev`: 0 vulnerabilities.
- Cross-platform CI matrix wired (Linux / macOS / Windows).
- Lock files committed; INV-TEL-012 grep enforced.

The gap that remains is mostly operational: signed releases, crash reporting, the M3 pilot, and the test coverage push for new code paths.
