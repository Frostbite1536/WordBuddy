# WordBuddy v0.1.0-rc1 — Release Notes

Tagged on the verifier-admitted P7 head `66a7cd9` (coordination entry 0021).

## What ships

- Local correctness checking (harper-core, zero network) across browser
  fields (via extension), native app fields (floating widget + apply),
  and a selection-rewrite palette.
- Writing goals (dialect/domain/formality/audience) honored from Settings
  on all check surfaces; personal style-guide replacement rules.
- Snippets text-expansion — **default OFF**, explicit opt-in in Settings.
- Stats dashboard backed by counts-only analytics (no text persisted).
- Multi-provider LLM config (Anthropic/OpenAI/etc.) with a first-class
  local-only mode that needs no API key.

## Artifacts

- `src-tauri/target/release/bundle/nsis/WordBuddy_0.1.0_x64-setup.exe`
  (9,226,645 bytes, unsigned)
- `src-tauri/target/release/bundle/msi/WordBuddy_0.1.0_x64_en-US.msi`
  (10,465,280 bytes, unsigned)

## Known limitations (verbatim from the residual ledger)

- **Unsigned binaries.** Windows SmartScreen will warn. Code signing is
  HUMAN-INBOX Q2 (default until answered: unsigned local builds only,
  no distribution beyond the author's machines). Auto-updater stays OFF
  pending signing (ledger W2).
- **Cross-platform native support with scoped limitations.** macOS and Linux
  implement accessibility-based detection, widget suggestions, selection
  capture, and platform-specific clipboard/input support where available.
  Native apply and snippets remain Windows-only; see the platform matrix and
  PLAN-08 for runtime prerequisites and untested combinations.
- **Installer install→launch smoke NOT RUN** (interactive session
  deferral) — bundling is proven; install/uninstall behavior on a clean
  profile is unproven. Must close before v0.1.0 final (verifier R1).
- **Correctness pass p95** measured 28.7–31.9 ms @2k chars vs the 25 ms
  budget (p50 within budget); profile evidence owed before the number is
  treated as met or the budget revisited (verifier R2).
- **Snippets live keyboard-expansion smoke never run** — the feature is
  default OFF and must stay OFF until a supervised live smoke passes
  (ledger W6). The unit tests written for P6 resolution caught a latent
  matcher bug that had made expansion un-matchable below a full ring.
- Palette Replace / Insert-below affordances are not shipped; the palette
  degrades to copy-back (P4 verifier residual).
- Browser keystroke→underline p95 latency budget: not yet instrumented.

## Verification

Five gates green at HEAD: cargo test 116/116, cargo check clean, tsc
clean, vitest 21/21, vite build ok. Clean-clone preflight PASS following
only README instructions. Threat model receipt spot-checks HELD (entry
0021).
