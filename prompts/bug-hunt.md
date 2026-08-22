# Bug Hunt Prompt — WorkBuddy

You are auditing the WorkBuddy codebase for bugs. This is a Tauri 2 app
with a Rust backend and React/TypeScript frontend.

Reference these documents:
- `docs/INVARIANTS.md` — system rules that must hold (check each one)
- `docs/ARCHITECTURE.md` — expected component behavior and data flow
- `docs/PLAN-cohort-telemetry/INVARIANTS.md` — INV-TEL-001..013
- `docs/PLAN-cohort-telemetry/REDACTION.md` — Tier 2 rule pipeline
- `docs/PLAN-cohort-telemetry/SCHEMA.md` — payload + endpoint contract
- `CLAUDE.md` — coding conventions and "never do" rules

For each file, check:

**Rust (src-tauri/src/):**
- Bare `.unwrap()` on fallible operations (especially Mutex, file I/O)
- Missing error handling on HTTP responses (status code checks)
- Unbounded buffers or streams without size limits
- `reqwest::Client::new()` instead of shared state (INV-ARCH-001)
- Incorrect SSE parsing (Anthropic format vs OpenAI format per provider)
- Anthropic tool_use parsing: content_block_start/input_json_delta/content_block_stop events not handled correctly
- Tool definitions (point_at, highlight) accidentally added to non-Anthropic request bodies
- tool_use_complete event emitted with malformed JSON from accumulated partial_json chunks
- Platform-specific code missing `#[cfg(target_os)]` guards
- Unused dependencies in Cargo.toml
- Audio stream resources not cleaned up in microphone.rs (INV-ARCH-007)
- Mutex poisoning in microphone.rs static state (must use unwrap_or_else)
- stt.rs sending audio to wrong endpoint or leaking API keys (INV-SEC-001)
- Missing commands in capabilities/default.json (INV-ARCH-003)
- rag.rs embedding vectors serialized/deserialized with wrong byte order
- rag.rs cosine similarity returning NaN on zero-norm vectors
- rag.rs opening SQLite without WAL mode (concurrent read/write issues)
- rag.rs chunking splitting mid-word or mid-code-block
- rag.rs not closing rusqlite connections (file lock leaks)
- rag.rs sending user query text to OpenAI embeddings API (privacy: only docs should be indexed, but queries are also embedded at search time — acceptable since queries are short and non-sensitive, but verify no conversation content is sent)
- ui_detect.rs ONNX model not found at expected path (model_path() vs actual download location)
- ui_detect.rs YOLO output shape mismatch (transposed vs standard detection)
- ui_detect.rs NMS IoU threshold too aggressive or too lenient
- ui_detect.rs DETECTOR mutex held during long inference blocking other operations
- ui_detect.rs model download not verifying SHA-256 hash (integrity check)
- llm.rs STREAM_GENERATION counter overflow (wraps at u64::MAX — effectively never)
- llm.rs content timeout not firing when Anthropic sends pings (verify last_content_time updates)
- llm.rs tool definitions sent without screenshot (has_screenshot guard)
- capture.rs JPEG encoding on non-RGB image (RGBA→RGB conversion correctness)
- capture.rs detected_elements included when ui_detection_enabled is false
- stt.rs ElevenLabs STT endpoint or model_id mismatch with current API
- stt.rs Gemini transcribe_gemini missing the 10MB base64 size cap (MAX_AUDIO_B64_SIZE)
- stt.rs Gemini promptFeedback.blockReason not checked before accessing candidates[0]
- stt.rs Gemini empty candidates array returning an error instead of Ok(String::new())
- stt.rs strip_transcript_artifacts chained strip_prefix with shadowed `s` fallback (subtle bug — must rebind after each strip)
- stt.rs Gemini path not using the "google" key service name (must match LLM/TTS)
- tts.rs provider dispatch on config.tts_provider — empty string or unknown value must fall back to elevenlabs
- tts.rs Gemini pcm_to_wav header writing wrong byte_rate (must be sample_rate * channels * bits_per_sample / 8)
- tts.rs Gemini pcm_to_wav header writing wrong block_align (must be channels * bits_per_sample / 8)
- tts.rs Gemini 3-attempt retry loop not preserving last_error on failure
- tts.rs Gemini inlineData["data"] base64 decode failure not propagated
- tts.rs list_tts_voices returning stale hardcoded list — must match tts.rs code
- a11y.rs get_foreground_elements not wrapped in spawn_blocking on Windows (INV-ARCH-012 — COM MTA isolation)
- a11y.rs Handle conversion from HWND — must use isize cast, NOT direct HWND → Handle (different `windows` crate versions)
- a11y.rs UIA walker recursion with no depth limit (browser DOM trees can have thousands of nodes)
- a11y.rs format_elements not subtracting monitor offset (INV-ARCH-013 — coord reconciliation)
- a11y.rs format_elements not clamping to captured monitor bounds (off-screen elements leak into prompt)
- a11y.rs format_elements not escaping " in element names (breaks the "Role" "name" format)
- a11y.rs format_elements missing element count cap (200 to keep prompt size bounded)
- a11y.rs is_interactive_role allow-list missing a useful role or including non-targets
- a11y.rs detect_ui_elements command not gated by config.a11y_detection_enabled
- a11y/macos_impl.rs calling AXIsProcessTrusted from a thread without the main-thread requirement (check crate docs)
- a11y/linux_impl.rs blocking on D-Bus without a timeout (AT-SPI2 can hang if daemon unresponsive)
- capture.rs a11y timeout too short/long — 800ms default balances UX vs accuracy on Chromium lazy activation
- capture.rs a11y min-element gate too high/low (≥5 currently — avoids feeding sparse data to LLM)
- capture.rs mon_offset passed to format_elements before monitor is resolved (borrow ordering)

**TypeScript (src/):**
- Stale closures in useEffect (missing dependency arrays)
- Event listeners without cleanup — must use cancelled-flag pattern (INV-ARCH-006)
- `window.location.href` navigation (destroys state in Tauri)
- State updates during unmounted components
- API key objects replaced instead of merged (INV-DATA-002)
- Streaming message ID not finalized after completion (INV-DATA-003)
- Screenshots stored in SQLite or on disk (INV-SEC-004)
- CursorOverlay coordinate mapping using wrong dimensions
- useMicrophone hook not cleaning up event listeners
- db.ts queries with SQL injection vectors (check parameterized queries)
- History.tsx loading conversations but not handling empty state
- pointParser.ts regex diverging from pointer.rs regex
- module_map.ts snippet keys referencing non-existent CONTEXT_REGISTRY entries
- module_map.ts missing module entries (all 52 modules across 3 academies must be present)
- Topic snippet files exporting wrong constant names (must match import in module_map.ts)
- ChatBar.tsx not catching search_docs errors (RAG must degrade gracefully — INV-CURR-004)
- prompts.ts ragContext injected without length limit (could exceed token budget)
- prompts.ts TUTOR_MODE_INSTRUCTIONS not injected when tutorMode=true (buildSystemPrompt not wiring flag)
- ChatBar.tsx not passing settings.tutor_mode to buildSystemPrompt (6th argument missing)
- Settings.tsx tutor mode toggle not calling updateSettings correctly
- Settings.tsx RAG section calling ingest_all_documents with empty directory string
- Settings.tsx not refreshing ragStatus after indexing completes
- SentenceBuffer splitting mid-abbreviation (Dr., e.g.) or mid-decimal (3.14)
- SentenceBuffer not handling edge case where sentence boundary spans across two SSE chunks
- TTSQueue not cancelling current Audio playback on reset
- TTSQueue enqueue called after cancel but before reset (race condition)
- SpringValue NaN propagation if dt is 0 or negative
- CursorOverlayWindow rAF loop not stopped on unmount (memory leak)
- SVG spotlight mask id collision if multiple points shown simultaneously
- tool_use_complete listener not following cancelled-flag pattern (INV-ARCH-006)
- ChatBar.tsx streaming TTS gate hardcoded to api_keys.elevenlabs (must be provider-aware — INV-DATA-005)
- ResponsePanel.tsx ttsAvailable hardcoded to api_keys.elevenlabs (must be provider-aware — INV-DATA-005)
- ttsQueue.ts playAudio using fixed audio/mpeg MIME (must switch to audio/wav for Gemini — INV-ARCH-011)
- ResponsePanel.tsx handleListen using fixed audio/mpeg MIME (must switch per provider)
- ttsQueue.ts setProviderGetter not wired by ChatBar.tsx — will always send undefined provider, losing Gemini voice selection
- Settings.tsx TTSSection not calling list_tts_voices when tts_provider changes (voice dropdown shows stale list)
- Settings.tsx switching TTS provider not resetting tts_voice to "default" (leaves invalid voice ID from other provider)
- Settings.tsx a11y_detection_enabled toggle not calling updateSettings correctly
- app.context.tsx Settings interface missing tts_provider or a11y_detection_enabled field (causes TS error)
- app.context.tsx defaultSettings missing tts_provider/a11y_detection_enabled (new installs get undefined)
- prompts.ts DETECTED UI ELEMENTS block injected AFTER vision instructions (must be before per POINTING RULES)
- config.rs set_settings missing tts_provider or a11y_detection_enabled copy (INV-DATA-006 — field persists in memory but not to disk)

**Context & RAG system (src/lib/curriculum/ + src-tauri/src/rag.rs):**
- Context snippet content contradicting official Limitless docs (verify facts against limitless-academy/docs/ and limitless-academy/academies/<name>/*.html)
- module_map.ts mapping a module to SDK snippets the module doesn't use (e.g., Go SDK for agents_academy)
- RAG chunks too large (>500 tokens) or too small (<20 chars) to be useful
- getContextReference() returning null when both module-level and tier-level lookups fail (should always return something)
- resolveModuleContext() silently dropping unknown snippet keys via .filter(Boolean)

**Cohort Telemetry (src/lib/telemetry/, src/pages/CohortEnroll.tsx, CohortReConsent.tsx, tests/):**

Enqueue path (INV-TEL-001 / 006 / 008):
- Any `INSERT INTO telemetry_queue` that is not inside `queue.enqueue()` — every writer must route through the gate
- New code path that `await d.execute(...telemetry_queue...)` from outside `queue.ts`
- `queue.enqueue()` missing the `hasSweepSucceeded()` check (INV-TEL-006 would regress)
- `queue.enqueue()` missing `requireActiveConsent` (INV-TEL-001)
- Tier derived from caller argument rather than `payload.schema` (caller could pass wrong tier and bypass the Tier 2 consent check)
- Tier 2 enqueue path not double-checking Tier 1 active — `requireActiveConsent(cohort, 2)` must itself call `hasActiveConsent(cohort, 1)` (INV-TEL-008)

Uploader (INV-TEL-002 / 003 / 007 / 008 / 013):
- `preflightScan` called AFTER `fetch()` — must run before any network call
- `endpointUrlAllowed` using case-sensitive `startsWith("https://")` (RFC 3986 schemes are case-insensitive)
- Localhost dev exception gated on anything other than `import.meta.env.DEV === true` (must be unreachable in production builds)
- `API_KEY_PATTERNS` generic rule missing any of: `api_key`, `password`, `passwd`, `pwd`, `secret`, `token`, `bearer` context anchors
- `API_KEY_PATTERNS` rule IDs containing vendor names (`anthropic`, `openai`, `google`, `groq`, `ollama`, `openrouter`) — must use prefix-shape names, INV-TEL-012 grep rejects vendor names in src/lib/telemetry/
- `BINARY_SIGNATURES` missing common image magics (`iVBORw0KG`, `/9j/`, `R0lGOD`, `UklGR`, `JVBERi`, `data:image/`)
- `findBinaryString` checking only top-level strings instead of recursing into arrays/objects
- `BINARY_SIG_MIN_LEN` too low (false positives on normal prose) or too high (missed screenshot leaks) — spec says 1024
- `uploadOne` using `recordFailure` (increments attempt_count) for permanent rejections — must use `parkPermanent` (sets to 999) or the row retries forever across app restarts
- `uploadOne` missing the `row.attempt_count >= MAX_ATTEMPTS` gate at entry — without it a parked row re-runs preflight on every tick
- `uploadOne` for Tier 2 not also loading `activeReceipt(cohort, 1)` (INV-TEL-008 backstop; cascade keeps these in sync but uploader must still check)
- `uploadOne` not comparing `receipt.policy_version` to `POLICY_VERSION` explicitly — `activeReceipt` does filter, but the check should be belt-and-suspenders
- `uploadOne` not re-checking the payload's own `payload.policy_version` against the current constant (catches rows enqueued before a version bump)
- AbortController `clearTimeout` missing in either the success or error path of `fetch` — leaks the timer
- `fetch` body using `JSON.stringify(payload)` instead of `row.payload_json` — different bytes means preflight could pass while the body leaks content
- `Authorization` header missing the `Bearer ` prefix or using the cohort token in the URL query instead
- Retry schedule `RETRY_SCHEDULE_MS` diverging from spec (must be 2s, 8s, 32s, 2m, 10m with MAX_ATTEMPTS = 5)
- `tickInFlight` not released in a `finally` block — a thrown error leaves uploads permanently gated
- `scheduleBackoff` using `Date.now() + wait` when `wait === Infinity` instead of storing `Infinity` directly (arithmetic poisons the map)
- 4xx non-401 status handled as retryable (must be permanent via `parkPermanent`); 401 and 429 handled as permanent (must be backoff-retryable)
- New `markRetentionSweepOk()` export or any duplicate copy of the sweep flag — queue.ts owns `hasSweepSucceeded()`; uploader reads it

Redactor (INV-TEL-010):
- `redactFragment` not wrapping stages in try/catch — any thrown exception must become `_pipeline_error` drop, not propagate
- Regex literal with unbounded nested quantifiers (ReDoS) — check every new rule's pattern for catastrophic backtracking
- Rule ordering broken: `key_sk_ant_prefix` must run before `key_sk_prefix` (otherwise `sk-` matches first and strips the `-ant-` prefix); `url_with_query` must run before `url`
- Standard-branch `replace` callback returning the raw replacement template when the rule's `replacement` contains `$1`/`$2` — the engine does NOT auto-expand `$n` inside a function return, must expand manually from `args[n]`
- `credit_card` regex replacing matches without the Luhn validation — a non-card digit run (order number, hash) must NOT drop the fragment
- Luhn validator returning true for empty string (length check before `sum % 10 === 0`)
- `dropOnMatch` rule (ssn_us / credit_card) only setting `dropReason` when unset — correct; if a new dropOnMatch rule is added, ensure the `dropReason` type union covers it
- Stage 2 (length gate) at 4096 chars — larger than spec allows implausibly long pastes to reach the regex pass
- Stage 5 confidence check: `keyMarkers > 2` threshold must count both `[key]` and `[jwt]` markers; `countAlnum < 8` on the redacted text (not the original)
- Stage 6 truncate: 200 chars including the `…` ellipsis; check `out.slice(0, 199) + "…"` not `slice(0, 200) + "…"` (off-by-one)
- `_RULES_FOR_TEST` or other test-only export leaking into the prod bundle (shouldn't affect correctness but indicates a dead surface)

Tagger:
- `tagFragment` called with raw student message instead of the redacted fragment — PII enters the keyword scan (REDACTION.md pipeline stage 7 runs AFTER stage 4)
- `tokens()` splitting on incorrect char classes (must match `/[A-Za-z][A-Za-z0-9_]{2,}/g`; too-loose allows 2-char stopwords to pollute)
- `STOPWORDS` list missing common filler — new tokens in the corpus drift confidence
- `snippetKeywordCache` never evicted — fine for current 52 modules but would need a cap for a larger curriculum
- Confidence formula diverging from `0.4 + 0.6·primary − 0.2·bestOther` without a rationale + POLICY_VERSION bump
- Missing clamp to [0, 1] on the final value
- `moduleKeywords` ignoring the program → empty set returns 0.5 neutral; make sure this is intentional (collector's threshold is 0.6 so 0.5 drops correctly)
- `overlapScore` not deduplicating tokens — repeated tokens over-count
- `TAG_CONFIDENCE_THRESHOLD` changed from 0.6 without updating the collector gate + tests

Collector:
- Tier 1 `pendingCollectionRows` JSON extraction using a literal value inline (e.g. `cohort_id = 'literal'`) instead of `$1` parameter (SQL injection via cohort_id)
- Tier 1 not joining on `practice_mode_flag` — INV-TEL-009 skipped
- Tier 1 `repeat_questions` computed from `conversations.module_id` != `lesson_progress.module_id` path (multi-module conversations)
- Tier 1 `time_ms` computed in seconds when spec says milliseconds, or vice-versa for `started_at`/`ended_at` (spec: session times in seconds, module.time_ms in ms)
- Tier 1 `student` field derived from anything other than `enrollment.student_pseudonym` (INV-TEL-004 pseudonym must be the only identifier)
- Tier 2 collector omitting the `TIER2_IDLE_MS = 5 * 60 * 1000` idle gate — emits mid-conversation and freezes out future messages (one-payload-per-session)
- Tier 2 collector idle gate using `>=` instead of `<` on `MAX(timestamp) < now - 5min` — either way idempotent via LEFT JOIN but verify intent
- Tier 2 collector not filtering practice-mode conversations in the same CTE (must be a single SQL round-trip, not post-filter in JS)
- Tier 2 collector reading `conversations.module_id` as the tag source when the spec says tag should come from `tagFragment(redacted, program, module_id)` — confidence signal is lost if module_id is pulled from elsewhere
- Tier 2 collector emitting payload with zero fragments (check `fragments.length > 0` before `enqueue`)
- Tier 2 collector's LEFT JOIN on `json_extract(payload_json, '$.session_id')` using the wrong JSON path for the tier — paths must be `$.session.id` for Tier 1 and `$.session_id` for Tier 2 per SCHEMA.md
- Trigger paths (App.tsx, app.context.tsx clearMessages, ChatBar X button, pagehide/beforeunload) missing either Tier 1 or Tier 2 flush
- 10-min fallback interval cleared on StrictMode unmount but NOT restarted on remount (use cancelled-flag pattern, not a plain unmount cleanup)

Consent / Re-consent / Practice:
- `grantConsent` not writing a fresh row at current `POLICY_VERSION` (re-grants must capture the current version)
- `withdrawConsent` Tier 1 not cascading to Tier 2 (INV-TEL-008) — or cascading back from Tier 2 to Tier 1 (must NOT)
- `withdrawConsent` with `deletePast=true` not deleting `telemetry_queue` rows for that (cohort, tier) — or deleting across ALL cohorts
- `reConsent` not withdrawing tiers the student previously had active but did NOT include in `tiersToGrant` — audit trail gap
- `reConsent` mutating the original receipt row's `withdrawn_at` (must be append-only per CONSENT.md audit rule)
- `reConsent` not bumping `cohort_enrollment.policy_version` (CohortReConsent wouldn't disappear after re-consent)
- Practice-mode toggle writing `practice_mode_flag` for a `conversation_id` that doesn't yet exist in `conversations` — FK hazard if SQLite's `PRAGMA foreign_keys = ON` ever flips
- Practice-mode state in ChatBar derived from stale closure instead of `conversationIdRef.current` at click time
- `newPseudonym()` taking any parameter (INV-TEL-004 bans identity-derived pseudonyms) — must be `crypto.randomUUID()` with no input

Retention sweep (INV-TEL-006):
- `sweepRetention` not deleting `redaction_audit` rows older than 30 days (rule 2 of the invariant)
- `sweepRetention` not deleting across BOTH `telemetry_queue` AND `redaction_audit` for expired cohorts (ends_at + 90d)
- `sweepRetention` not setting `sweepSucceededThisSession = true` on success, or setting it in a path that swallows an error
- `sweepRetention` setting the flag BEFORE all three deletes complete — a failure mid-sweep would flag success

Upload history / audit UI (INV-TEL-011):
- Modal pretty-printing stripping fields (must render the literal payload_json)
- Modal closing on backdrop click without `stopPropagation()` on the content div (click inside modal closes it)
- Audit summary showing content instead of counts (audit table stores only counts)
- Audit summary rule_id list missing the `_preflight_*` or `_pipeline_error` or `_tag_confidence_below_threshold` pseudo-rule entries

INV-TEL-012 grep (CI-enforced):
- Any occurrence of `anthropic|openai|groq|ollama|openrouter` anywhere under `src/lib/telemetry/` — in imports, comments, string literals, rule IDs, or audit reasons. Check before every telemetry commit:
  `grep -rn "anthropic\|openai\|groq\|ollama\|openrouter" src/lib/telemetry/`
  (must return zero matches)

POLICY_VERSION (INV-TEL-013):
- Constant bumped in a commit that also changes redaction rules / payload shape / UI policy strings — required per REDACTION.md §Updating (✓)
- Constant bumped in a commit that does NOT change any of the above — forces every enrolled student to re-consent for no reason (must not)
- Constant not bumped when one of the above changed — receipts stay "active" under stale policy text (INV-TEL-013 intent violated)

Tests (tests/, vitest):
- `redactor.test.ts` missing any rule from REDACTION.md (rule coverage drift)
- `tagger.test.ts` asserting a threshold value that diverges from the collector's constant
- `uploader.preflight.test.ts` missing case for mixed-case HTTPS scheme (bug 2 from the full-branch audit)
- New rule added without a corresponding positive + negative test

**Cross-boundary (Rust ↔ TypeScript):**
- CaptureResult shape mismatch (must have base64, width, height)
- Event name typos between emit() and listen()
- Tauri command parameter name mismatch with invoke() calls
- PointTarget struct fields not matching between Rust and TypeScript
- DocChunk fields in rag.rs not matching TypeScript invoke<> type in ChatBar.tsx
- search_docs topK parameter name casing mismatch between invoke() and Rust command (camelCase vs snake_case — Tauri auto-converts)
- tool_use_complete event payload shape mismatch between Rust ToolUsePayload struct and TypeScript listener type annotation
- CursorOverlayWindow screenshotDims not received if overlay window loads after screenshot is taken (timing issue)

Report each bug with: file, line, severity (critical/high/medium/low),
description, and suggested fix.
