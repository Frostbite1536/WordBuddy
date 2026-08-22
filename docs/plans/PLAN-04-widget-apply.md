# PLAN-04 — Floating Widget & Fix Application

Goal: make native suggestions visible and actionable: a small suggestion
card docked near the caret showing current issues with one-click apply; plus
the hotkey selection-rewrite palette (WritingTools-style flow, GPL code not
referenced — concept only).

Depends on: PLAN-03 merged.
Agent budget: 1 builder + 1 verifier.

---

## Widget window

Third webview window `widget` (CONTRACTS §4): ~340×240, undecorated,
transparent, always-on-top, skip_taskbar, hidden by default, NOT
click-through. Created lazily on first show. Must replicate the WebView2
transparency background-color pattern (`window.rs:63-76` in base; FRICTION).

Positioning: anchor = caret rect when known, else field element rect, else
screen corner fallback. Clamp to the monitor of the caret. Hide on: field
blur, target exclusion, zero issues for >10 s, user Esc, or monitor stop.
Show latency budget: ≤150 ms from issues event to visible (measure via
tracing timestamp pair; assert manually in smoke).

Widget UI (React component tree under `src/components/widget/`):
- Header: target name (process, friendly-mapped) + kind-colored dots.
- Issue rows: message truncated, replacement chips (≤3), ignore-row button.
- Footer: "Open editor" (focus main window with text pasted? No — INV-PRIV-002:
  opens empty editor with instructions; do NOT auto-paste ambient text).
- Keyboard: ↑/↓ row nav, Enter applies, Esc hides.

## Apply path (native)

Order of preference, per target capabilities discovered in P3:

1. **ValuePattern.SetValue(full corrected text)** — works in Notepad-class.
   Caveats documented: destroys undo stack; acceptable v1, note in card
   tooltip ("replaces field text").
2. **TextPattern surgical replace**: select issue span
   (`range.Select()` on cloned range over [start,end]), then synthesize
   clipboard paste of replacement with **clipboard save/restore**
   (open clipboard, snapshot, set CF_UNICODETEXT, SendInput Ctrl+V,
   restore prior clipboard after 500 ms delay). Preserves undo in most
   editors.
3. **Unsupported** → card shows copyable fix (button copies corrected
   sentence).

Safety rails:
- Single-flight: one apply in flight; widget disabled during.
- Verify-after-apply: re-read value; if expected substring absent →
  `wb://apply-result {ok:false}` + revert attempt ONLY for SetValue path
  (re-set original). Paste path has no safe revert — surface failure honestly.
- Never send keys to a different foreground window than the one captured at
  issue time; re-verify HWND before SendInput, else abort (**INV-APPLY-001**:
  synthetic input only ever targets the exact HWND+process captured with the
  issue; abort on mismatch).

## Selection rewrite palette (hotkey)

- Global shortcut Ctrl+Shift+W (register via existing `shortcuts.rs` plugin;
  conflict-checked against base defaults).
- Flow: capture selection via focused element TextPattern selection →
  fallback simulated Ctrl+C with clipboard save/restore (same primitive as
  above) → open small palette window near selection (reuse widget window
  infrastructure, second mode) → action chips: Proofread, Rewrite, Concise,
  Professional, Friendly, Custom instruction… → streamed LLM response
  rendered progressively (reuse ResponsePanel streaming patterns +
  STREAM_GENERATION cancellation) → Replace / Insert below / Copy buttons.
- Replace uses the apply path on the captured selection range.
- This surface sends selected text to the LLM by explicit user action —
  consistent with **INV-PRIV-003** (explicit action, not ambient).

## Tasks

1. Window creation/positioning/hide logic + Rust-side commands
   (`widget_show_for(target_key)` etc.) + transparency pattern.
2. Widget frontend consuming `wb://issues` filtered to active target.
3. Apply engine (`apply.rs`): capability detection per target (does it expose
   ValuePattern? TextPattern?), strategy pick, verify-after-apply,
   single-flight guard, HWND re-verification (**INV-APPLY-001** test with fake).
4. Clipboard save/restore utility with tests (mockable OS layer; real-OS
   covered in smoke).
5. Hotkey + selection capture + palette mode + streaming render.
6. Settings: "Show suggestions card" toggle, "Selection rewrite hotkey"
   toggle, per-process override (force card off).

## Behavioral verification (gate)

Real-app smokes, evidence in status/builder.md:

1. Notepad: type errors → card appears near caret ≤150 ms after issues event
   (stopwatch via tracing log pair); click chip → correction applied; undo
   caveat verified (Ctrl+Z behavior recorded).
2. VS Code plain buffer: paste-path apply works; undo intact.
3. Elevated PowerShell window focused → widget stays out of the way
   (target marked unsupported), no stray keystrokes ever sent.
4. Focus-race drill: trigger apply then instantly alt-tab — assert no input
   lands in the new window (INV-APPLY-001 abort path; try ≥10 times).
5. Selection rewrite in a browser textarea: select sentence, Ctrl+Shift+W →
   palette streams rewrite → Replace swaps selection.
6. Clipboard restore: pre-seed clipboard with an image file copy, run paste-
   apply, confirm image still on clipboard afterwards.

Standard gates at final head on main.

## Risks

- **Synthetic input is the riskiest code in the product** — INV-APPLY-001 +
  focus-race drill are mandatory admission evidence; verifier re-runs drill 5.
- **Clipboard timing races** (restore before paste lands) — fixed 500 ms +
  verification readback; accept residual risk, log failures.
- **Widget stealing focus on show** — create with no-activate flags; verify
  typing continues uninterrupted while card visible (part of smoke 1).
- **Per-app quirks multiply** — maintain `docs/APPLY-COMPAT.md` table
  starting from P3's coverage notes; unknowns degrade gracefully.

## Non-goals

True inline underlines in native apps (D4 — card model is the shipped UX),
analytics (P5), snippets auto-expansion (P6).
