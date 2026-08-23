# PLAN-08 — Linux & macOS Compatibility

**Status:** Ready to start
**Depends on:** Phases 0–7 (all landed; Windows build is the reference implementation)
**Author context:** Written 2026-08-22 after the 2026-08-22 repo-wide audit was
remediated through commit `83ed69b`. Read `AUDIT-2026-08-22.md` for the defect
classes that shipped on Windows — this plan is written so the same classes
don't ship twice.

---

## 1. Mission

Make WordBuddy compile, run, and pass its invariants on Linux (X11 first,
Wayland documented as degraded) and macOS (Accessibility-permissioned),
without regressing Windows. The Windows path is the reference: every stub you
fill in must preserve the same observable contract, not merely compile.

## 2. Ground rules (non-negotiable)

1. **Invariants are platform-independent.** `docs/CONTRACTS.md` (`INV-*`)
   binds every OS. Especially:
   - **INV-PRIV-001**: password fields must be detected *before any value
     read* and **fail closed**. On error/unknown → treat as password, skip.
     (Audit F5: `.unwrap_or(false)` shipped here once. Never again.)
   - **INV-EXCL-001**: process/host exclusion resolves before any read.
   - **INV-OFFSET-001**: offsets are UTF-16 code units across the Rust↔JS
     boundary. **Per-platform caveat:** UIA proved to count `Character`
     units as UTF-16 (see §5), but AX/AT-SPI may differ — measure first,
     per platform, before wiring selection spans (see §6 probe requirement).
2. **Windows must stay green.** Gates: `cargo test`, `tsc --noEmit`,
   `vitest` run before every push. Baseline at plan time: cargo 126/0,
   tsc clean, vitest 21/21. Any regression you cause, you fix.
3. **Fail-closed over fail-open**, always. A stub returning "unsupported"
   beats a partial reader that leaks.
4. Match the existing patterns in this repo — there is one way things are
   done here (see `CLAUDE.md`). Second patterns beside existing ones get
   rejected.

## 3. Platform surface inventory (what is real vs stub today)

Verified by reading the tree at `main` = `83ed69b`.

### Already cross-platform — do NOT touch

| Surface | File(s) | Notes |
|---|---|---|
| LLM streaming | `llm.rs` | reqwest + SSE parsing |
| Check engine | `engine/*` | harper-core, pure Rust |
| Extension relay server | `extension.rs` | plain TCP on 127.0.0.1 |
| Secrets vault | `secrets.rs` | keyring 3.6 — Credential Manager / Keychain / Secret Service already wired; file fallback logs loudly on headless Linux (no keyring daemon). M13 done here on purpose. |
| Config | `config.rs` | dirs_next paths |
| Analytics storage/aggregation | `analytics/db.rs`, `jobs.rs` | rusqlite; retention purge included (M10) |
| Global hotkeys | `shortcuts.rs` | tauri-plugin-global-shortcut (cross-platform) |
| Frontend | all of `src/` | Tauri APIs |

### Stubs to implement

| Surface | File | Current state | macOS route | Linux route |
|---|---|---|---|---|
| A11y element detection | `a11y/macos_impl.rs`, `a11y/linux_impl.rs` | Return `Vec::new()`; plans sketched in-file | AXUIElement walk via `accessibility` crate | AT-SPI2 via `atspi` crate |
| Focused-field text reader | `text_monitor.rs` `stub_reader` | Always `ReadOutcome::Unsupported` | Same AX backend, selected-text/focused-element attrs | AT-SPI Text + EditableText interfaces |
| Clipboard snapshot/restore | `clipboard.rs` `stub` module | All ops Err | NSPasteboard change-count + restore | X11: XFixes+clipboard owner dance is hard; pragmatic v1: text-only save/restore via `arboard`, document format loss |
| Input injection | `input_inject.rs` | All fns Err | CGEvent taps (`core-graphics` crate) | X11: XTEST via `x11rb`; Wayland: unsupported v1 |
| Snippets keyboard hook | `snip_hook.rs` | `start()` returns Err | CGEventTap (needs Accessibility permission) | X11: Xi/XRecord or XRecord-less polling; **Wayland: out of scope** (ledger W6 keeps snippets default-OFF anyway) |
| Selection capture | `widget.rs::capture_selection_impl` | Returns failed | AXSelectedTextRange / AXSelectedText attribute | AT-SPI Text::get_selection |
| TZ offset | `analytics/aggregate.rs::capture_local_offset` non-Windows | Stores 0 (UTC) | Read system timezone; simplest correct: `iana-time-zone` crate + fixed current offset, refreshed nightly like the Windows path (audit M9 made it refreshable — keep that) | Same |

### Already implemented per-platform — verify, don't rewrite

- `context.rs::get_active_window_title` — xdotool (X11) / osascript / Win32.
  Wayland returns empty by design (documented in-file).
- `diagnostics.rs` open-log-folder — explorer/open/xdg-open.

### Cargo.toml target sections already prepared

```toml
[target.'cfg(target_os = "macos")'.dependencies]
accessibility-sys = "0.2"        # used by the permission check only today

[target.'cfg(target_os = "linux")'.dependencies]
atspi = "0.29"
```

You will likely add: `accessibility = "0.2"` (macOS, full AX walk),
`core-graphics`/`core-foundation` (macOS input), `arboard` (clipboard),
`x11rb` (Linux X11 input). Follow the unused-dep rule noted inside
`a11y/macos_impl.rs`: deps land with the code that uses them.

## 4. Lessons from `C:/Users/LCM/Github/studybuddy-public`

StudyBuddy is the direct ancestor; its `src-tauri/src/a11y/{macos,linux}_impl.rs`
are byte-for-byte the ancestors of ours and its docs contain research we
inherited but never executed. What's worth mining:

1. **`docs/ACCESSIBILITY_POINTING_PLAN.md` is your spec.** It contains the
   exact crate APIs (AXUIElement attribute calls; AT-SPI2
   `AccessibleProxy::get_role/get_children` +
   `ComponentProxy::get_extents(CoordType::Screen)`), target-app priority
   order (VS Code/Electron first, terminals, browsers), role filtering lists
   (skip `Pane`/`Group`/`Separator`...), and measured-latency expectations
   (AT-SPI D-Bus overhead 100–500ms per query — budget for caching).
2. **Operational gotchas already paid for** (do not relearn):
   - Chromium apps lazy-activate their AX tree on macOS: first query costs
     100–500ms; cache per window.
   - Electron on Linux needs `ACCESSIBILITY_ENABLED=1` env at launch.
   - macOS silently returns nothing without Accessibility permission —
     StudyBuddy's pattern of checking `AXIsProcessTrusted()` and degrading
     to empty (not error) is already in our `macos_impl.rs`. Keep it, and
     add the missing Settings UI prompt it asks for.
   - Linux needs no permissions but requires the `at-spi2-core` daemon;
     detect its absence and log clearly.
3. **Coordinate-space reconciliation** (plan §"Phase 1.5"): a11y APIs emit
   screen coordinates; multi-monitor and mixed-DPI need normalization before
   mixing with browser-extension viewport coords. WordBuddy's widget
   positioning consumes these rects — port the normalization rules, don't
   improvise.
4. **What NOT to copy:** StudyBuddy's YOLO+OCR fallback stack, RAG,
   telemetry proxy, TTS/STT were all stripped from WordBuddy deliberately
   (see root-level banners in the base-archive docs and PLAN-00). Do not
   reintroduce them to fill gaps; degrade gracefully instead.
5. **Their CI/release matrices exist** (`.github/workflows/*.yml` with linux/
   macos targets) if you need packaging reference — but re-audit them against
   our hardened `ci.yml` (SHA-pinned actions, `npm ci`, pinned cargo-audit);
   do not regress supply-chain hygiene.

## 5. The offset-units trap (read before touching apply/select)

The 2026-08-22 audit assumed UIA `TextUnit::Character` advances per Unicode
scalar; runtime probing (`src-tauri/examples/uia_probe.rs`) falsified this —
on RichEdit it advances per UTF-16 code unit. The lesson generalizes:

**Never assume text-unit semantics on a new platform. Build the equivalent
probe first, run it against a real control, then wire the conversion.**

Concretely: write `examples/ax_probe.rs` (macOS: AXSelectedTextRange on
TextEdit) and `examples/atspi_probe.rs` (Linux: AT-SPI Text interface on
gedit) that insert emoji-bearing text and measure which unit counts select
which characters. Record results next to `select_range`'s comment block in
the new backend before implementing span selection.

## 6. Suggested work order (each step ships green gates)

1. **macOS a11y reader** (`a11y/macos_impl.rs`) — highest product value;
   permission check exists; follow ACCESSIBILITY_POINTING_PLAN API sketch.
   Add the Settings permission-prompt UI the stub requests.
2. **macOS text monitor + selection capture** — reuse the AX connection;
   password fields = `AXRole == "AXSecureTextField"` (fail closed).
3. **macOS clipboard + input injection** — arboard fallback for formats;
   CGEvent for paste/backspaces.
4. **Linux a11y reader + monitor** — atspi; password =
   `ROLE_PASSWORD`/`STATE_IS_PASSWORD`; handle missing at-spi2 daemon.
5. **Linux input injection (X11)** — XTEST; Wayland stays unsupported with
   a clear runtime message (same posture as `context.rs` Wayland note).
6. **TZ offset off-stub** — replace the UTC-only non-Windows branch; keep
   the nightly refresh hook from audit M9.
7. **Snippets last, best-effort** — CGEventTap on macOS; X11-only on Linux.
   It is default-OFF (ledger W6); shipping without it on v1 linux/macos is
   acceptable and must be stated in release notes, not hidden.
8. **CI**: extend `ci.yml` matrix carefully (macos/windows runners already
   exercised by release.yml patterns from studybuddy); add Linux GUI-less
   test caveats (a11y tests need a session bus — gate them behind an
   opt-in job, not the PR-critical path).

## 7. Acceptance criteria

- [ ] `cargo check --target` clean for all three targets (use cross or
      GitHub runners; Windows host can't link macOS natively).
- [ ] Every stub replaced by either a real implementation or an explicit
      `Unsupported` that surfaces in UI as "feature unavailable on this OS"
      — no silent no-ops left undocumented.
- [ ] Password fail-closed verified per platform with a runtime probe
      (secure field / ROLE_PASSWORD) — screenshot or log excerpt in the PR.
- [ ] Offset-semantics probes checked in under `examples/` with recorded
      results, mirroring §5.
- [ ] All three gates green on Windows; no Windows behavior change
      (diff-review every file a new `#[cfg]` touches).
- [ ] PRIVACY_POLICY.md updated only where behavior actually differs
      (e.g., macOS Accessibility permission disclosure).

## 8. Session hygiene

Commit per milestone, message style follows `git log` (imperative, body
explains why). Push only when told. If a finding contradicts this plan or
the audit, stop and surface it with evidence — this repo has already caught
one audit premise being wrong at runtime (F4/uia_probe); runtime evidence
outranks documents here too.
