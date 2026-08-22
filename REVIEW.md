# WorkBuddy — Code Review Rules

## Severity Levels

### Critical
Must fix before merge. Blocks the PR.
- Security: API key exposure, permission bypass, missing auth
- Data loss: config corruption, state destruction
- Invariant violation: any rule in `docs/INVARIANTS.md` broken
- Crash: unwrap on fallible operation, panic in production path

### High
Should fix before merge unless strong justification provided.
- Performance: unbounded buffer, missing timeout, per-request client creation
- Correctness: wrong API protocol (e.g., OpenAI SSE format for Anthropic)
- Platform: code that silently breaks on one OS
- Resource leak: audio stream not released, event listener not cleaned up

### Medium
Fix in follow-up PR acceptable.
- Code quality: unused dependencies, dead code, missing error handling
- UX: broken layout, missing accessibility attributes
- Documentation: invariant not documented, architecture diagram outdated

### Nit
Optional. Author's discretion.
- Style: naming, formatting, comment wording
- Minor optimization that doesn't affect correctness

### Pre-existing
Issue exists in code the PR didn't modify. Note it, don't block the PR.

## Review Checklist

### Security
- [ ] No API keys logged, printed, or sent to unintended endpoints (INV-SEC-001)
- [ ] Config file permissions set to 0600 on Unix (INV-SEC-002)
- [ ] No third-party analytics added; cohort telemetry changes stay
      inside the INV-TEL-* flow — see the Cohort Telemetry section
      below (INV-SEC-003 + INV-TEL-001..013)
- [ ] Screenshots not persisted to disk or SQLite (INV-SEC-004)
- [ ] Audio recordings not persisted to disk or SQLite (INV-SEC-005)
- [ ] No new `target="_blank"` links (use `open()` from plugin-shell)

### Architecture
- [ ] HTTP requests use shared `HttpClient` state (INV-ARCH-001)
- [ ] No `window.location.href` navigation (INV-ARCH-002)
- [ ] New Tauri commands have capability permissions (INV-ARCH-003)
- [ ] SSE parsing uses correct protocol per provider; tool_use only for Anthropic (INV-ARCH-004)
- [ ] No bare `.unwrap()` on Mutex (INV-ARCH-005)
- [ ] Event listeners use cancelled-flag cleanup pattern (INV-ARCH-006)
- [ ] Audio resources cleaned up on stop (INV-ARCH-007)
- [ ] Streaming TTS doesn't block UI; SentenceBuffer/TTSQueue are refs (INV-ARCH-008)
- [ ] Extension server binds to 127.0.0.1 with token auth (INV-SEC-006, INV-SEC-007)
- [ ] Extension state shared via `Arc<tokio::sync::Mutex>` (INV-ARCH-009)
- [ ] Extension freshness threshold respected (INV-ARCH-010)
- [ ] TTS MIME type selected per provider: `audio/wav` for Gemini, `audio/mpeg` for ElevenLabs (INV-ARCH-011)
- [ ] UIA calls wrapped in `tokio::task::spawn_blocking` (INV-ARCH-012)
- [ ] Accessibility element coords reconciled to capture-relative space (INV-ARCH-013)

### Data Integrity
- [ ] `set_settings` doesn't touch `api_keys` (INV-DATA-001)
- [ ] `updateSettings` merges keys, not replaces (INV-DATA-002)
- [ ] Streaming messages finalize with unique ID (INV-DATA-003)
- [ ] Conversations persist to SQLite on stream complete (INV-DATA-004)
- [ ] TTS key gate is provider-aware — checks `google` for Gemini, `elevenlabs` otherwise (INV-DATA-005)
- [ ] All new persisted config fields copied in `set_settings` (INV-DATA-006)

### Cross-Boundary
- [ ] Tauri command parameter names match frontend `invoke()` calls
- [ ] Rust struct field names match TypeScript interface names
- [ ] Event names match between `app.emit()` and `listen()`
- [ ] CaptureResult shape (`base64`, `width`, `height`, `detected_elements`) consistent across boundary
- [ ] ToolUsePayload (`name`, `input`) matches TypeScript listener type
- [ ] UIElement shape (`name`, `role`, `bounding_rect`, `automation_id`, `depth`) consistent across boundary
- [ ] `screenshot_dims` event received by overlay window before pointing starts
- [ ] Settings interface includes all config fields (`tts_provider`, `a11y_detection_enabled`, etc.)

### Dependencies
- [ ] New Rust crate is actually used in source code
- [ ] New npm package is actually imported
- [ ] No duplicate functionality with existing deps

### Platform
- [ ] Tested or reviewed for macOS, Windows, and Linux paths
- [ ] No hardcoded paths or OS-specific assumptions without cfg guards
- [ ] Audio capture handles missing microphone gracefully

### Cohort Telemetry (src/lib/telemetry/**, CohortEnroll.tsx, CohortReConsent.tsx)
Required only when a PR touches any of the files above. Full rule
set: `docs/PLAN-cohort-telemetry/INVARIANTS.md`.
- [ ] No direct `INSERT INTO telemetry_queue` outside `queue.enqueue()`
      (INV-TEL-001; gate also calls `hasSweepSucceeded` for INV-TEL-006)
- [ ] New preflight / redactor / audit `rule_id`s don't contain vendor
      names (`anthropic`, `openai`, `groq`, `ollama`, `openrouter`) —
      INV-TEL-012 grep runs in CI
- [ ] `tagFragment` called on the redacted fragment, not raw student text
      (REDACTION.md pipeline order)
- [ ] `endpointUrlAllowed` scheme check stays case-insensitive
- [ ] Tier 2 enqueue/send paths require BOTH Tier 1 and Tier 2 active
      (INV-TEL-008; cascade + uploader backstop)
- [ ] Permanent rejections call `queue.parkPermanent`, not plain
      `recordFailure` (prevents attempt_count ratchet across launches)
- [ ] `TIER2_IDLE_MS` idle gate preserved on the Tier 2 collector path
- [ ] `POLICY_VERSION` bumped iff the PR changes payload shape,
      redaction rules, or user-visible policy text (INV-TEL-013;
      see REDACTION.md §Updating)
- [ ] `npm test` green (redactor matrix + tagger + uploader preflight)
