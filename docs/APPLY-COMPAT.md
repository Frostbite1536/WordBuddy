# Apply & Compatibility Matrix

Where WordBuddy's checking, widget, and apply paths actually work, with the
evidence for each row. Seeded from PLAN-03/PLAN-04 smoke evidence (P3/P4) —
coordination channel entries 0005–0011, 0014 — not from aspiration.
Anything not listed below is **untested**: absence from this table means
"nobody has run it", not "doesn't work".

Last updated: PLAN-07 Task 5 (2026-08-22).

## Native apps (Windows)

| App | Checking | Widget | Apply | Evidence |
|---|---|---|---|---|
| Notepad (classic edit control) | **Proven live** | **Proven live** (renders + keyboard/click chip nav) | Partially proven (see note) | Checking via UIA `ValuePattern`: channel 0007 behavioral gate ("notepad 5-issues PASS", seed text matches `scripts/p3-smoke-notepad.ps1` verbatim). Widget render + chip-click driving a real apply IPC cycle CDP-verified: channel 0011. Smoke drivers: `scripts/p4-smoke-edge.ps1`, `scripts/p4-smoke-apply.ps1`, `scripts/p4-drill.ps1`. |

**Apply caveat (honest scope):** the *end-to-end synthetic paste* into
Notepad was NOT RUN live — channel 0011/0014 record it deferred (machine
owner actively using the desktop; synthetic-input hazard). What WAS proven
live: widget renders issues, chip click fires the real apply command cycle,
and INV-APPLY-001's text-match/identity abort fired correctly in the field
when the drill's readback landed on the owner's browser session (channel
0014). The apply strategies themselves are code-verified with unit fakes:
ValuePattern verify/revert (`apply.rs:151-191`), TextPattern select+paste
(`apply.rs:192-238`), abort-on-changed-text (`apply.rs:152-158`, `:193-199`;
tests at `apply.rs:628-656`).

| App | Status | Why |
|---|---|---|
| VS Code / Electron editors | **Documented degradation** — no checking | Electron's focused element exposes no UIA Value/Text pattern to the reader → `ReadOutcome::Unsupported` → 2 s backoff. Proven live in P3 (channel 0007 point 2; driver `scripts/p3-smoke-vscode.ps1`; code-read channel 0008). Also excluded from snippets below. |
| WordPad, Microsoft Word, LibreOffice, other rich editors | **Untested** | No P3/P4 evidence exists. Rich-edit controls may expose TextPattern (the surgical-replace path) — nobody has run it. |
| Terminals (Windows Terminal, cmd, PowerShell) | Untested for checking; snippets never fire | Terminals are not edit controls; expect `Unsupported`. Text expansion is hard-denied regardless. |
| Browsers as native windows (no extension) | Untested | Chromium exposes some UIA text patterns but this path was never driven; browser checking goes through the extension instead. |

## Browser fields (Chrome/Edge via extension)

| Surface | Checking | Apply | Evidence |
|---|---|---|---|
| `<textarea>` / `<input>` | **Proven live** | **Proven live** (apply + Ctrl+Z revert preserved undo) | P2 behavioral gate on real Chromium + extension + live app: 7-point evidence incl. password-zero-posts and excluded-host probe (channels 0005/0006). Apply = `setSelectionRange` + `execCommand insertText`, fallback `setRangeText` + dispatched input (channel 0006 code-read of `checker.js`). |
| contenteditable | **Proven live** | **Proven live** (span-select + execCommand) | Channel 0006 behavioral point 5 + code-read (`checker.js:480-486`). |
| Same-page iframes | **Proven live** | By design (`all_frames`) | Channel 0006 behavioral point 6; `manifest.json` `all_frames: true, match_about_blank: true`. |
| Shadow DOM | By design (overlay renders inside shadow roots); field detection across open shadow hosts untested beyond plain layouts | — | Channel 0006 MOST-LIKELY-WRONG flag: mirror-rect math covered textarea/contenteditable/iframe only. |
| Password fields | Never checked, by design | — | Triple guard (focusin/input/read-time), proven live: zero network posts while typing in a password field (channel 0006 point 4). |
| Sites outside the manifest match list | Not active | Not active | Content scripts inject only where `wordbuddy-extension/manifest.json` `matches` sends them (currently `*.limitless.exchange`, `*.github.com`, `localhost`, `127.0.0.1`). Edit that list to extend coverage. |

## Text-expansion snippets (keyboard hook)

The hook carries its own conservative deny-list — trigger characters are
shell/IDE syntax there, so expansion **never fires** in:

> Windows Terminal, cmd, PowerShell, pwsh, conhost, VS Code, Visual Studio
> (devenv), JetBrains IDEs (idea/goland/pycharm/rider/clion), vim, nvim,
> Notepad, Notepad++

Source: `DEFAULT_EXCLUDED_PROCESSES`, `src-tauri/src/snip_hook.rs:31-36`
(user-configurable additions honored alongside it). The hook itself is OFF
unless enabled in Settings (`snippets_enabled` defaults false, config.rs).
All other applications: untested individually.

## Exclusion model (applies everywhere)

- **Browser:** per-host exclusions (`excluded_hosts` in Settings). Checked
  before any text use server-side; an excluded host gets an empty response
  and the engine is never called (`extension.rs` `/check` ordering, channel
  0006 code-read).
- **Native monitor:** per-process exclusions (`excluded_processes` in
  Settings, default empty). Enforced at the reader boundary — when the
  foreground process matches, the reader resolves process identity ONLY and
  returns without reading any pattern or value (INV-EXCL-001;
  `text_monitor.rs:364-384`; regression test
  `reader_boundary_excludes_before_value_read`, verifier finding 0008).
- **Snippets:** the fixed deny-list above plus user additions.
- Excluded ⇒ no checks, no telemetry, no widget, nothing logged.

## Apply-path safeguards (all native applies)

Synthetic input only ever targets the exact process captured with the issue:
focused-element + foreground-window re-resolution before any write or key
synthesis, foreground re-check again inside the clipboard window,
text-changed-since-capture abort, single-flight process-wide (INV-APPLY-001,
`src-tauri/src/apply.rs:12-17`, unit tests `apply.rs:628-656`). All offsets
are UTF-16 code units end-to-end (INV-OFFSET-001). Clipboard used by the
paste strategy is snapshotted and restored afterwards (`clipboard.rs`).
