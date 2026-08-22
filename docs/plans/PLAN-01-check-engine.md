# PLAN-01 — Check Engine (`engine.rs`)

Goal: one local-first checking pipeline behind the `check_text()` contract
(CONTRACTS §1): harper-core correctness always, optional validated LLM
style pass, personal dictionary, caching. No UI, no extension work.

Depends on: PLAN-00 merged (baseline recorded).
Agent budget: 1 builder + 1 verifier.

---

## Task 1 — harper-core integration spike (time-boxed)

1. Add `harper-core` (latest crates.io release) to `src-tauri/Cargo.toml`.
   **Consult docs.rs for the current API surface at implementation time** —
   do not code from memory: construct its dictionary + linter, lint a buffer,
   read lint spans/replacements.
2. Verify which English dictionary ships by default and whether dialect
   variants (en-GB/CA/AU) exist. Write findings into FRICTION + ledger W4.
   If variants don't exist: record fallback design (dialect word-diff lists
   applied as extra lints) — implement only en-US this phase.
3. License check in commit message: Apache-2.0 confirmed, no copyleft
   transitive deps introduced (`cargo license` or equivalent).

Acceptance: a `#[cfg(test)]` lints `"teh recieve"` and asserts ≥2 issues
with non-empty replacement lists. This test is the spike's exit ticket.

## Task 2 — Correctness pass (pure)

New module `src-tauri/src/engine/` (or `engine.rs`) exposing:

```rust
pub fn correctness_pass(text: &str, goals: &WritingGoals,
                        dict: &PersonalDictionary) -> Result<Vec<TextIssue>, String>;
```

Requirements:
- Map harper spans → UTF-16 offsets (**INV-OFFSET-001**). Conversion helpers
  live in one place (`offsets.rs`) with tests: ASCII, emoji (`🚀`),
  combining marks, CJK. A wrong conversion must fail these tests loudly.
- Assert `original == text[start..end]` per issue (**INV-CHECK-002**) in tests.
- Rule ids: `harper:<lint-name>`.
- Deterministic ordering: sort by `(start, end)`.
- Personal dictionary: preferred path is feeding accepted words into the
  harper dictionary wrapper if its API supports mutation/custom dicts;
  fallback is post-filtering issues whose `original` lowercases into the
  dictionary. Chosen approach documented in module docs.

## Task 3 — LLM style pass

1. Port the validate-with-retry pattern from base `journal/analyzer.rs`
   (`extract_json`, descriptive parse errors, bounded attempts ≤ 2, error
   text fed back verbatim). Pure functions, canned-JSON tests, no live calls
   (same test convention).
2. Prompt builder `prompts.rs`: system prompt requests ONLY JSON
   `{clarity:[], engagement:[], delivery:[]}` where each item is a CONTRACTS
   `TextIssue` shape with `rule_id: "llm:<slug>"`, char offsets into the
   provided text. Goals (domain/formality/audience/intent) prefix the prompt
   (e.g. "Formal register for an expert audience").
3. Transport: add `complete_text()` helper beside `llm.rs::complete_with_images`
   sharing the same `HttpClient` state and per-request timeout conventions
   (never a fresh reqwest::Client — CLAUDE.md rule 1).
4. Kill-switch: `WB_DISABLE_LLM=1` env short-circuits to correctness-only
   (**INV-PRIV-003** enforcement point for CI/tests).
5. Failure semantics: after retries, return correctness-only +
   `style_check_failed: true`; never error the whole call.

## Task 4 — Orchestration + caching

```rust
pub async fn check_text(req: CheckRequest) -> Result<CheckResponse, String>
```

- Compose passes; merge + dedupe overlapping spans (correctness wins on overlap).
- Cache keyed by `(SHA-256(text), goals-hash)` in an LRU (≤64 entries)
  behind a mutex with poison recovery (CLAUDE.md convention). Cache stores
  correctness results permanently per key; style results only when enabled.
- Enforce 20 KB input cap (reject, don't truncate).

## Task 5 — Tauri command wiring

- `#[tauri::command] check_text` registered in `lib.rs` invoke_handler AND
  `capabilities/default.json` (duality rule).
- Emit nothing yet (`wb://issues` starts when consumers exist in P2/P3).

## Verification gate

```
cd src-tauri && cargo test        # new engine suite green, incl. offset/astral/perf tests
cd src-tauri && cargo check
npx tsc --noEmit                  # unchanged tree still typechecks
npm test && npx vite build
```

Behavioral note (honest scope): there is no UI consumer yet, so the
observable end-to-end proof lands in PLAN-02. P1 closes on the test suite +
verifier code-read of INV-OFFSET-001/002 conformance — verifier specifically
re-derives one offset conversion by hand against a test vector containing an
emoji before the issue span.

## Non-goals

Dialects beyond en-US (W4), UI, extension/native consumers, streaming,
server anything.

## Risks

- **Offset conversion bugs** — mitigated by dedicated test vectors + verifier
  hand-check above.
- **harper API drift** — spike first, pin exact version in Cargo.toml.
- **LLM JSON drift across providers** — validation-with-retry plus
  correctness-only degradation keeps worst case = Grammarly-red-only.
