> STALE — describes the WorkBuddy base; authoritative specs live in docs/plans/

# WorkBuddy ↔ Wotch Integration Plan

**Status:** Proposed
**Date:** 2026-04-19
**Supersedes:** `docs/base-archive/Original Docs/05_ROADMAP.md` v0.3 "Wotch Integration (Future)"
**Target:** WorkBuddy v0.3 and Wotch v1.2 (Wotch changes optional, see §5)

---

## 0. TL;DR for a coding agent

Build two independent but complementary deliverables:

| Deliverable | Repo | Effort | Required? |
|-------------|------|--------|-----------|
| **C — WorkBuddy as an MCP server** for Claude Code (stdio transport, curriculum-aware tools) | WorkBuddy only | 5–7 days | **Yes — primary** |
| **D — Launch integration** ("Open in Wotch" button + optional "Ask WorkBuddy" in Wotch's command palette) | WorkBuddy mandatory; Wotch upstream PR optional | 1–2 days | **Yes — secondary** |
| ~~A — WorkBuddy polls Wotch's `/v1/status`~~ | — | — | **Skipped** (§2) |
| ~~B — Pill badge showing current curriculum module on Wotch~~ | — | — | **Skipped** (§2) |

The skipped paths are not needed once C is in place — they duplicate information that Claude Code already gets via the MCP server and that the human student already has on screen.

Read §3 (Architecture) before coding. Start with §6 (Phase 1 — MCP server skeleton) and work phase-by-phase through §6.5.

---

## 1. Context — why the original roadmap is stale

The 2026-04-13 roadmap anticipated building a custom IPC layer ("shared curriculum context via local file or localhost WebSocket"). Since then, **Wotch has shipped all of the plumbing that roadmap assumed we'd invent**. As of Wotch v1.1 (src/api-server.js, src/claude-integration-manager.js, src/mcp-server.js):

1. **Wotch has a local HTTP+WebSocket API** on `127.0.0.1` with bearer-token auth (`~/.wotch/api-token`), DNS rebinding protection, rate limiting, and a 64 KB body cap — essentially the same design as WorkBuddy's `extension.rs`. Base port is `19519`, with fallback attempts up to `19528` (`apiPort` setting + 10 `MAX_PORT_ATTEMPTS`). The active port is written to `~/.wotch/api-port`. All responses use an envelope: `{ ok: boolean, data?: ..., error?: string, code?: string }`.
2. **Wotch auto-registers its own MCP server** in `~/.claude.json` and speaks the Anthropic Model Context Protocol (stdio transport, 8 tools).
3. **Wotch has a Hook Receiver** on `:19520` that Claude Code posts lifecycle events to, and an MCP IPC server on `:19523`.
4. **Wotch has a Plugin SDK and Agent SDK.**

Since WorkBuddy and Wotch share the same maintainer, Wotch-side changes ship directly (no third-party PR coordination). That opens up two simplifications this plan takes advantage of:
- **Coordinated `~/.claude.json` writers** — both apps must atomically read-modify-write (see §4.6).
- **Port coordination** — Wotch's base 19519 + fallback up to 19528 overlaps WorkBuddy's 19521–19523 extension range. Left as-is; both apps discover via port files. Flagged in §12.

The right integration shape is therefore NOT "two peer apps negotiate IPC." It's "WorkBuddy becomes an MCP server that any Claude Code instance — including Claude Code running inside Wotch — can query." The curriculum-awareness lives with the curriculum provider (WorkBuddy), not in a shared intermediate layer.

---

## 2. Why C+D only — why A and B add no marginal value

### Option A — WorkBuddy polls Wotch's `/v1/status`

The thought was: the tutor knows when Claude is working on something in Wotch, so it can tailor answers like "Claude is mid-edit, wait for it."

**Why skip:** The student is already looking at their screen when they ask WorkBuddy a question. They see Wotch's pill color, the terminal output, the code Claude is writing. WorkBuddy already captures the screenshot. The marginal information from polling Wotch's status API is redundant with what the LLM sees in the screenshot. Adds network polling overhead (once per 3 s × both processes running) and a new optional dependency (Wotch not running → disable the feature → code path bloat).

### Option B — Pill badge on Wotch showing current curriculum module

The thought was: if WorkBuddy detects "Module 03 Orders", Wotch's pill shows "M03" badge so the student sees curriculum context even in the terminal.

**Why skip:** The student is the one who navigated to Module 03 Orders; they know what module they're on. The badge is decorative. It also requires Wotch renderer changes (upstream PR, review cycle, version skew across Wotch versions). Option C gets the same module info to the consumer that *doesn't* already know — Claude Code — through a clean protocol. If desired later, B is a trivial follow-up that reads the same data MCP already exposes.

### Summary

| Consumer of "what module is the student on" | Option | How they get it |
|---------------------------------------------|--------|-----------------|
| The human student | n/a | They're reading the module, they know |
| WorkBuddy's tutor prompt | existing | `detect_active_window` in `context.rs` |
| **Claude Code in Wotch (or anywhere)** | **C (MCP)** | **Call `workbuddy:get_current_module` tool** |
| Wotch's pill UI (decorative) | (future B) | Would read WorkBuddy's context file or MCP — not in v1 |

C closes the one information gap that actually matters for code quality. D closes the one UX gap (getting from WorkBuddy → terminal quickly). Everything else is noise.

---

## 3. Architecture

### 3.1 System diagram

```
┌────────────────────────────────────────────────────────────────────┐
│  Student's machine                                                 │
│                                                                    │
│  ┌──────────────────────────────┐   ┌──────────────────────────┐   │
│  │  WorkBuddy (Tauri app)      │   │  Wotch (Electron app)    │   │
│  │                              │   │                          │   │
│  │  ┌─────────┐  ┌───────────┐  │   │  ┌────────────────────┐  │   │
│  │  │ChatBar  │  │extension  │  │   │  │   xterm tab        │  │   │
│  │  │+ "Open  │  │HTTP server│  │   │  │ ┌────────────────┐ │  │   │
│  │  │ in Wotch│  │:19521     │◄─┼───┼──│ │ Claude Code    │ │  │   │
│  │  └─────────┘  └───────────┘  │   │  │ │ (stdio)        │ │  │   │
│  │       │          ▲           │   │  │ └────────────────┘ │  │   │
│  │       │          │ /ask (D)  │   │  └──────┬─────────────┘  │   │
│  │       │          │           │   │         │ (spawns)       │   │
│  │       ▼          │           │   │         ▼                │   │
│  │  ┌─────────────────────────┐ │   │  ┌───────────────────┐   │   │
│  │  │  launch_wotch (D)       │─┼───┼─▶│  wotch binary     │   │   │
│  │  │  Tauri command          │ │   │  │  (on PATH)        │   │   │
│  │  └─────────────────────────┘ │   │  └───────────────────┘   │   │
│  │                              │   │                          │   │
│  │  ┌─────────────────────────┐ │   │                          │   │
│  │  │  config.json            │ │   │                          │   │
│  │  │  rag_vectors.db         │ │   │                          │   │
│  │  │  lesson_plans/          │ │   │                          │   │
│  │  │  extension-token        │ │   │                          │   │
│  │  └─────────┬───────────────┘ │   │                          │   │
│  │            │ shared files    │   │                          │   │
│  │            ▼                 │   │                          │   │
│  │  ┌─────────────────────────┐ │   │                          │   │
│  │  │  workbuddy-mcp         │ │   │                          │   │
│  │  │  (standalone binary)    │◄┼───┼── spawned by Claude Code │   │
│  │  │  stdio MCP transport    │ │   │   via ~/.claude.json     │   │
│  │  │  (C)                    │ │   │                          │   │
│  │  └─────────────────────────┘ │   │                          │   │
│  └──────────────────────────────┘   └──────────────────────────┘   │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
        │                                         │
        └── to Anthropic / OpenAI / etc ──────────┘
```

Legend:
- **(C)** — WorkBuddy's new MCP server, stdio transport, spawned by Claude Code whenever the student runs Claude Code. Works anywhere Claude Code runs — inside Wotch, in a plain terminal, in an IDE MCP client, etc.
- **(D)** — Launch integration. WorkBuddy spawns Wotch via `launch_wotch` Tauri command. Optional Wotch-side PR lets Wotch's command palette POST a question to `extension.rs::POST /ask`.

### 3.2 Why a standalone binary (not a subprocess of the Tauri app)

MCP servers are launched by the MCP client (Claude Code) as a subprocess and communicate over stdio. Three constraints drive the "standalone binary" choice:

1. **Tauri app owns its own stdio.** The main WorkBuddy process cannot also be a stdio MCP server without conflict.
2. **Claude Code may run when WorkBuddy isn't running.** The MCP server must start even if the main Tauri app isn't open.
3. **One binary = one Cargo crate = clean dependency boundary.** Add a new workspace member `workbuddy-mcp` alongside `workbuddy` (the Tauri app).

The MCP server reads the same on-disk state the Tauri app writes (config, RAG DB, lesson plans, bundled curriculum JSON). No runtime IPC between them is needed in v1.

### 3.3 Data shared between main app and MCP binary

| Data | Location | Writer | MCP reader behavior |
|------|----------|--------|---------------------|
| API keys, settings | `~/.config/workbuddy/config.json` (platform equivalent) | Tauri app | Read-only; fail gracefully if absent |
| RAG vector DB | `~/.config/workbuddy/rag_vectors.db` | Tauri app | Read-only; `search_docs` returns `[]` if absent |
| Bundled lesson plans | Resource dir (release) or `lesson_plans/` (dev) | Build-time | Always present via `include_dir!` or Tauri resource resolver |
| Bundled curriculum JSON | Resource dir (release) or `workbuddy-mcp/curriculum.json` (dev) | Build-time via `scripts/generate-curriculum.mjs` | Always present via `include_str!` |
| Extension auth token | `~/.config/workbuddy/extension-token` | Tauri app | Not needed by MCP; only used by D's Wotch→WorkBuddy channel |

### 3.4 Shared library (future)

For v1, duplicate the minimum needed from `src-tauri/src/` (window-title parsing from `context.rs`, MD parsing for lesson plans) into `workbuddy-mcp/src/`. When the duplication cost exceeds the refactor cost (~3rd duplicated function), refactor into a `workbuddy-core` workspace crate both binaries depend on. Do **not** do this up front — YAGNI.

---

## 4. Option C — WorkBuddy as an MCP server

### 4.1 Crate layout

Add a new workspace member under `src-tauri/`:

```
src-tauri/
├── Cargo.toml                    # existing — add to [workspace] members
├── src/                          # existing Tauri app crate "workbuddy"
└── workbuddy-mcp/               # NEW workspace crate
    ├── Cargo.toml                # binary crate, name = "workbuddy-mcp"
    ├── curriculum.json           # generated from TS (§4.4)
    └── src/
        ├── main.rs               # binary entry; builds the server
        ├── tools/
        │   ├── mod.rs
        │   ├── get_current_module.rs
        │   ├── get_lesson_plan.rs
        │   ├── get_module_context.rs
        │   ├── list_modules.rs
        │   ├── get_ui_elements.rs
        │   └── search_docs.rs
        ├── curriculum.rs         # parses curriculum.json at startup
        ├── config.rs             # reads main app's config.json (read-only)
        ├── context.rs            # window-title module detection (ported)
        └── rag.rs                # OpenAI embeddings + cosine similarity (ported)
```

Update the workspace `Cargo.toml`:

```toml
[workspace]
members = [".", "workbuddy-mcp"]
resolver = "2"
```

The existing `src-tauri/Cargo.toml` becomes the workspace root. The Tauri package stays at `src-tauri/` (path `.`), the MCP binary at `src-tauri/workbuddy-mcp/`.

### 4.2 Dependencies (`workbuddy-mcp/Cargo.toml`)

```toml
[package]
name = "workbuddy-mcp"
version = "0.1.0"
edition = "2021"
license = "AGPL-3.0"

[[bin]]
name = "workbuddy-mcp"
path = "src/main.rs"

[dependencies]
# MCP protocol — check crates.io for the latest stable rmcp version at impl time.
# If rmcp is not yet stable, implement the protocol directly per modelcontextprotocol.io/spec.
rmcp = { version = "0.4", features = ["transport-io", "server"] }

tokio = { version = "1", features = ["rt-multi-thread", "macros", "io-std", "io-util", "process", "sync"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# HTTP client for OpenAI embeddings (search_docs). Keep connect_timeout short.
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }

# SQLite access for the RAG vector DB (same version as main crate to avoid native-links conflicts)
rusqlite = { version = "0.31", features = ["bundled"] }

# Config / paths
dirs-next = "2.0"
thiserror = "1"

# Window-title detection (same crates as main app, platform-gated)
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = ["Win32_UI_WindowsAndMessaging", "Win32_Foundation"] }

[target.'cfg(target_os = "linux")'.dependencies]
# xdotool is spawned via std::process::Command; no extra crate needed.

[target.'cfg(target_os = "macos")'.dependencies]
# osascript is spawned via std::process::Command; no extra crate needed.
```

> **Note on `rmcp`:** At time of writing, `rmcp` is the most widely-used Rust MCP SDK but the API is stabilizing. Before coding, run `cargo search rmcp` and check https://crates.io/crates/rmcp for the latest version and API shape. If `rmcp` is unsuitable, fall back to a direct JSON-RPC 2.0 implementation over stdio (~150 lines) following the protocol spec at https://modelcontextprotocol.io.

### 4.3 Tool definitions (authoritative contracts)

All tool inputs and outputs are serde-serializable JSON. `null` and omitted fields are equivalent where noted.

#### Tool 1 — `get_current_module`

**Purpose:** Return the curriculum module the student is currently viewing, detected from the foreground window title (or from extension meta tags when available).

**Input schema:** `{}` (no parameters)

**Output schema:**
```json
{
  "program": "pm_academy | api_academy | agents_academy | limitless_trader_lab | limitless_exchange | ide | terminal | null",
  "module_id": "string | null",
  "module_title": "string | null",
  "tier": "string | null",
  "source": "window_title | extension_meta | none",
  "window_title": "string"
}
```

**Implementation notes:**
- Port `context.rs::detect_active_window` into `workbuddy-mcp/src/context.rs`. Reuse the same `match_curriculum_context` logic and the ADR-030 `title_overlaps_extension` / `has_title_separator` helpers.
- The MCP binary does NOT have access to the main app's extension state (which lives in Tauri state). For `extension_meta` source, the MCP binary can optionally read `~/.config/workbuddy/extension-meta.json` if the main app is updated to write it. **For v1, skip the extension-meta path** and always report `source: "window_title"`. Graceful degradation.
- Must not panic: return `{program: null, ...}` rather than erroring if the OS detection call fails.

**Example call (from Claude Code's perspective):**
```
Tool: workbuddy:get_current_module
Input: {}
Output: {
  "program": "api_academy",
  "module_id": "03",
  "module_title": "Orders",
  "tier": "Foundations",
  "source": "window_title",
  "window_title": "Module 03: Orders — API Academy — Mozilla Firefox"
}
```

#### Tool 2 — `get_lesson_plan`

**Purpose:** Return the markdown lesson plan for a module, if one is bundled.

**Input schema:**
```json
{
  "program": "string (optional — defaults to current module's program)",
  "module_id": "string (optional — defaults to current module's id)"
}
```

**Output schema:**
```json
{
  "found": "boolean",
  "program": "string | null",
  "module_id": "string | null",
  "plan_markdown": "string | null",
  "session_count": "integer",
  "checkpoint_count": "integer"
}
```

**Implementation notes:**
- If `program` or `module_id` is missing, call `get_current_module` internally; if still missing, return `{found: false, ...}`.
- Lesson plans are bundled. In development: read from `src-tauri/lesson_plans/{program}/{module_id}_*.md` or `{module_id}.md`. In release: use `include_dir!` to embed the entire `lesson_plans/` tree into the binary.
- Compute `session_count` by counting `^### §\d+` headers under the `## Session Flow` block (port `lib/curriculum/lessonProgress.ts::countSessionFlowSections`).
- Compute `checkpoint_count` by counting `^- \[ \] checkpoint` lines or similar marker — see existing lesson plan files for the canonical format.

#### Tool 3 — `get_module_context`

**Purpose:** Return the composed reference-material snippets WorkBuddy already injects into its own system prompt for this module.

**Input schema:**
```json
{
  "program": "string (optional)",
  "module_id": "string (optional)",
  "max_chars": "integer (optional, default 20000 — caller cap)"
}
```

**Output schema:**
```json
{
  "found": "boolean",
  "program": "string | null",
  "module_id": "string | null",
  "context_markdown": "string",
  "truncated": "boolean",
  "ui_elements": "string | null"
}
```

**Implementation notes:**
- Resolution mirrors `src/lib/curriculum/context/index.ts::getContextReference`:
  1. Module-level lookup via `curriculum.json` (see §4.4) — if found, concatenate snippets.
  2. Tier-level fallback (per-program tier → snippet bundle).
  3. Default fallback per-program.
- Append the per-module UI elements string from `ui_elements` in the JSON.
- Respect `max_chars` — truncate at the nearest snippet boundary, set `truncated: true`. Callers pass their context budget.
- Tool description must note: "Use this to ground code suggestions in the student's current lesson — includes API reference, SDK snippets, and known patterns for this module."

#### Tool 4 — `list_modules`

**Purpose:** Enumerate the modules WorkBuddy knows about.

**Input schema:**
```json
{
  "program": "string (optional — filters to one program)"
}
```

**Output schema:**
```json
{
  "modules": [
    {
      "program": "string",
      "module_id": "string",
      "module_title": "string",
      "tier": "string",
      "has_lesson_plan": "boolean"
    }
  ]
}
```

**Implementation notes:**
- Iterates the `modules` section of `curriculum.json`.
- `has_lesson_plan` probes the bundled `lesson_plans/{program}/` directory for a matching file. Cache the result on first call.

#### Tool 5 — `get_ui_elements`

**Purpose:** Return the per-module UI element description (e.g., "Place Order button, YES/NO tabs, amount input").

**Input schema:**
```json
{
  "program": "string",
  "module_id": "string"
}
```

**Output schema:**
```json
{
  "elements": "string | null"
}
```

**Implementation notes:**
- Trivial lookup in `curriculum.json`. Separate tool so Claude Code can pull JUST the UI element hints (useful for writing test fixtures, selectors, or mock data without pulling the whole module context).

#### Tool 6 — `search_docs`

**Purpose:** Run the same RAG query WorkBuddy's chat uses — OpenAI embed → cosine similarity vs the indexed Limitless docs.

**Input schema:**
```json
{
  "query": "string",
  "top_k": "integer (optional, default 5, max 20)"
}
```

**Output schema:**
```json
{
  "chunks": [
    {
      "source_file": "string",
      "content": "string",
      "score": "number (0.0–1.0)"
    }
  ],
  "indexed": "boolean"
}
```

**Implementation notes:**
- Port `src-tauri/src/rag.rs::search_docs` into `workbuddy-mcp/src/rag.rs`. Share the same DB schema.
- Reads OpenAI API key from the main app's `config.json` via `config.rs::read_api_key("openai")`.
- If no API key is configured OR the DB doesn't exist, return `{chunks: [], indexed: false}` — **never error**. The MCP contract is "graceful empty on missing dependencies."
- `top_k` is clamped to `[1, 20]`.
- Tool description must note the MCP client's cost: "Each call sends the query text to OpenAI's embeddings endpoint. Use when curriculum context alone isn't specific enough."

### 4.4 Curriculum JSON generation

The TypeScript sources in `src/lib/curriculum/context/` are the single source of truth. The Rust MCP binary consumes a bundled JSON derived from them.

**Build script:** `scripts/generate-curriculum.mjs`

- Reads `src/lib/curriculum/context/module_map.ts`, `ui_elements.ts`, and every file in `src/lib/curriculum/context/topics/`.
- Evaluates them via `esbuild --bundle --format=esm` + dynamic import (or use `ts-node` if already installed) to get the runtime values.
- Emits `src-tauri/workbuddy-mcp/curriculum.json` with this schema:

```json
{
  "schema_version": 1,
  "generated_at": "ISO-8601 timestamp",
  "source_commit": "git rev-parse HEAD",
  "modules": {
    "pm_academy": {
      "01": {"title": "Prediction Markets 101", "tier": "Fundamentals", "snippet_keys": ["WHAT_IS_LIMITLESS", "CLOB_ORDERBOOK"]},
      "02": {"title": "Implied Leverage", "tier": "Fundamentals", "snippet_keys": [...]},
      ...
    },
    "api_academy": {...},
    "agents_academy": {...}
  },
  "snippets": {
    "WHAT_IS_LIMITLESS": "string (the snippet markdown)",
    "CLOB_ORDERBOOK": "string",
    ...
  },
  "ui_elements": {
    "pm_academy": {"01": "UI Elements: ...", "02": "..."},
    "api_academy": {...},
    "agents_academy": {...},
    "exchange": "..."
  },
  "tier_fallbacks": {
    "pm_academy": {"default": ["LIMITLESS_PLATFORM", "MARKET_MECHANICS"]},
    "api_academy": {
      "Foundations": ["LIMITLESS_API_REFERENCE", "API_TIER_FOUNDATIONS", "SDK_TYPESCRIPT", "SDK_PYTHON", "SDK_GO"],
      "Real-Time": [...],
      ...
    },
    "agents_academy": {...},
    "limitless_trader_lab": {"default": [...]},
    "limitless_exchange": {"default": [...]},
    "ide": {"default": [...]},
    "terminal": {"default": [...]}
  }
}
```

**Package.json additions:**
```json
{
  "scripts": {
    "generate-curriculum": "node scripts/generate-curriculum.mjs",
    "prebuild": "npm run generate-curriculum"
  }
}
```

**Pre-commit hook (optional but recommended):** If the TS sources change but `curriculum.json` doesn't, block the commit. Add to `.husky/pre-commit` or document in `CONTRIBUTING.md` if husky isn't installed.

**MCP binary loading:** In `workbuddy-mcp/src/curriculum.rs`:

```rust
const CURRICULUM_JSON: &str = include_str!("../curriculum.json");

pub fn load() -> Result<Curriculum, serde_json::Error> {
    serde_json::from_str(CURRICULUM_JSON)
}
```

### 4.5 Window-title detection port

Port the entirety of `src-tauri/src/context.rs` (excluding the Tauri `#[tauri::command]` wrapper and the `extension meta` path) into `workbuddy-mcp/src/context.rs`. This is ~250 lines of pure logic:

- `match_curriculum_context` (the big string-matching function)
- `extract_module_number`
- `extract_trader_lab_day`
- `first_segment`
- `match_module_in_list`
- `title_overlaps_extension` — not needed (extension path skipped in v1)
- `has_title_separator` — not needed (only used by title_overlaps_extension)
- `get_active_window_title` — port the Windows, macOS, Linux branches as-is

If duplication becomes painful post-v1, refactor to a shared `workbuddy-core` crate. Not in scope for v1.

### 4.6 Auto-registration

On first launch of the main WorkBuddy app (OR on an explicit Settings toggle), offer to register the MCP server in `~/.claude.json`.

**Settings toggle name:** `claude_code_mcp_registered: bool` (add to `AppConfig` following INV-DATA-006).

**Registration logic** (new Tauri command `register_claude_mcp`):

1. Resolve the absolute path to the `workbuddy-mcp` binary:
   - Dev: `{project_root}/src-tauri/target/debug/workbuddy-mcp{EXE_SUFFIX}`
   - Packaged: `app.path().resource_dir()?.join("workbuddy-mcp{EXE_SUFFIX}")` (bundled via Tauri sidecar; in Tauri 2 the resolver is `AppHandle::path()` → `PathResolver::resource_dir()`).
2. **Atomic read-modify-write** (Wotch writes to the same file — see §4.6.1). Read `~/.claude.json` (create with `{"mcpServers": {}}` if absent), deserialize as `serde_json::Value`, merge in WorkBuddy's entry preserving all other `mcpServers` keys (especially `wotch`), serialize, write to a temp file in the same directory, then `rename` atomically over the target.
3. Entry to merge:
   ```json
   {
     "mcpServers": {
       "workbuddy": {
         "type": "stdio",
         "command": "<absolute path from step 1>",
         "args": [],
         "env": {}
       }
     }
   }
   ```
4. Write via atomic temp-file-then-rename so Wotch's concurrent write can't half-clobber (see §4.6.1 below). Preserve source file permissions (`0o600` on Unix) when setting mode on the temp file.
5. Set `claude_code_mcp_registered: true` and persist.

**Reverse — unregister:** Tauri command `unregister_claude_mcp` removes the `workbuddy` key from `mcpServers` (preserving all other entries). Called when user toggles the setting off. Same atomic write semantics.

**Idempotency:** Re-registration must be safe (overwrites the path in case it changed across installs). Don't throw on missing `~/.claude.json`.

### 4.6.1 Concurrent writers coordination

Wotch and WorkBuddy both auto-register their MCP servers in `~/.claude.json`. Without coordination, a race between Wotch's write and WorkBuddy's write can clobber one of the entries. Mitigation (applies to BOTH codebases):

1. **Atomic rename write.** Write to `~/.claude.json.tmp-<pid>-<rand>` in the same directory, fsync, then `rename` over the target. On Unix this is atomic; on Windows, use `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`. Rust: `std::fs::rename` on Unix; for Windows use the `replace` crate or `fs::rename` which maps to `MoveFileExW` in recent stdlib.
2. **Read-merge-write, not read-overwrite.** Each writer reads the current file, merges only its own `mcpServers.<name>` entry, and writes. Other entries (the other app's, and any user-added entries) are preserved verbatim.
3. **Last-write-wins on same entry key is fine.** If Wotch and WorkBuddy both write at the same moment, one wins. The loser's entry was identical to a prior successful write, so no data is lost — the other tool re-registers next launch if it detects drift.
4. **Wotch-side coordination (this repo ships the change):** update Wotch's `~/.claude.json` writer to follow (1) + (2) if it doesn't already. Verify by grepping Wotch's `claude-integration-manager.js` for the write-back logic.

A lock file (`~/.claude.json.lock`) is NOT used — too heavy for this write frequency (once per app launch at most). Atomic rename suffices.

**Bundle the binary as a Tauri sidecar:** in `src-tauri/tauri.conf.json`:

```json
{
  "bundle": {
    "externalBin": [
      "binaries/workbuddy-mcp"
    ]
  }
}
```

Tauri 2 sidecar bundling requires the binary to be named with the target triple: `workbuddy-mcp-x86_64-pc-windows-msvc.exe`, `workbuddy-mcp-aarch64-apple-darwin`, etc. A build script (`src-tauri/build.rs` already exists) should copy the output of `cargo build -p workbuddy-mcp` into `src-tauri/binaries/` with the correct triple suffix before Tauri packages it. At runtime, resolve via:

```rust
let mcp_path = app.path().resource_dir()?
    .join(format!("workbuddy-mcp{}", std::env::consts::EXE_SUFFIX));
```

Consult https://tauri.app/develop/sidecar/ for the exact current-version sidecar incantation — the naming convention is strict and has churned between Tauri versions.

### 4.7 MCP server main.rs skeleton

```rust
// workbuddy-mcp/src/main.rs
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber;

mod config;
mod context;
mod curriculum;
mod rag;
mod tools;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // MCP servers MUST NOT write to stdout — it's the protocol channel.
    // Redirect logs to stderr only.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("info,rmcp=warn")
        .init();

    // Load bundled curriculum once at startup.
    let curriculum = curriculum::load()?;

    // Build the server with all tools registered.
    let server = tools::build_server(curriculum);

    // Run over stdio. Blocks until Claude Code disconnects.
    server.serve(stdio()).await?.waiting().await?;

    Ok(())
}
```

The exact `ServiceExt` / tool-registration shape depends on the `rmcp` version. If `rmcp` isn't a good fit, implement the JSON-RPC loop manually — tool dispatch is a match on method name, validation is serde.

### 4.8 Security invariants for the MCP binary

Add to `docs/INVARIANTS.md`:

- **INV-ARCH-015:** `workbuddy-mcp` NEVER writes to stdout except MCP protocol messages. Logging goes to stderr only. (Violating this corrupts the JSON-RPC stream and hangs Claude Code.)
- **INV-ARCH-016:** `workbuddy-mcp` reads but never writes `~/.config/workbuddy/config.json`. The Tauri app is the sole writer.
- **INV-SEC-009:** `workbuddy-mcp`'s HTTP calls to OpenAI embeddings honor INV-SEC-001 (API keys go only to `api.openai.com`). Build a fresh `reqwest::Client` per call (no shared-state infrastructure), connect timeout 10 s, request timeout 30 s.
- **INV-SEC-010:** `workbuddy-mcp` MUST NOT spawn subprocesses except the platform window-title detectors (`xdotool`, `osascript`) already used by `context.rs`. No arbitrary shell execution from tool inputs.

### 4.9 Error-handling contract

Every tool call must return a well-formed JSON response. An internal error (DB unreadable, embedding request failed, OS call failed) is reported as one of:

- **Graceful empty** — `{chunks: []}`, `{found: false}`, `{program: null}` — preferred when the missing data is optional.
- **Structured error** — `{ "error": "human-readable description" }` — only when the caller needs to know something specific is broken.

**Never:**
- Panic (kills the process, breaks MCP)
- Return a non-JSON response
- Log the OpenAI API key or any user-typed query to stdout

### 4.10 `AppConfig` additions (main Tauri app)

Following INV-DATA-006, every persisted field added to `AppConfig` must be copied in `set_settings` and mirrored in the TS `Settings` interface + `defaultSettings`.

```rust
// src-tauri/src/config.rs additions:

#[serde(default)]
pub claude_code_mcp_registered: bool,   // default false; flipped true after successful register
```

```typescript
// src/contexts/app.context.tsx additions:
claude_code_mcp_registered: boolean;
// defaults: claude_code_mcp_registered: false,
```

---

## 5. Option D — Launch integration

### 5.1 WorkBuddy → Wotch: "Open in Wotch" button

**Goal:** One click from WorkBuddy's chat into a Wotch terminal tab, optionally with the student's latest question pre-filled as a Claude Code prompt.

**New Tauri command:** `launch_wotch`

```rust
// src-tauri/src/lib.rs (add to mod listing)
mod wotch;

// src-tauri/src/wotch.rs (new)
use tauri::AppHandle;

#[tauri::command]
pub async fn launch_wotch(
    app: AppHandle,
    initial_prompt: Option<String>,
) -> Result<WotchLaunchResult, String> {
    // 1. Detect wotch binary.
    let path = detect_wotch_path().ok_or_else(|| {
        "Wotch is not installed or not on PATH. Visit \
         https://github.com/Frostbite1536/Wotch/releases to install.".to_string()
    })?;

    // 2. Spawn wotch. Use std::process::Command so we don't block the Tauri runtime.
    //    Use spawn (not output) — Wotch is a long-running GUI app.
    tokio::task::spawn_blocking(move || {
        std::process::Command::new(&path)
            .spawn()
            .map_err(|e| format!("Failed to spawn Wotch: {e}"))
    })
    .await
    .map_err(|e| format!("spawn task failed: {e}"))??;

    // 3. If an initial prompt was provided, push it to Wotch via its HTTP API
    //    AFTER a short delay so Wotch's API server is up.
    if let Some(prompt) = initial_prompt {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        if let Err(e) = push_prompt_to_wotch(&app, &prompt).await {
            eprintln!("[wotch] prompt push failed (non-fatal): {e}");
        }
    }

    Ok(WotchLaunchResult { spawned: true })
}
```

**Binary detection** — try in order, return the first hit. Wotch's `electron-builder` config packages differently per platform, so the known-location fallbacks differ:

1. **Running-process shortcut:** if `~/.wotch/api-port` exists AND a `GET /v1/health` on that port + token auth succeeds, Wotch is already running — skip spawning a new instance, just push the prompt to the existing one.
2. `PATH` lookup: use the `which` crate (2.0) — cross-platform. On Linux, the `.deb` package installs `wotch` to `/usr/bin/`.
3. Known install locations:
   - **Windows (NSIS installer):** `%LOCALAPPDATA%\Programs\Wotch\Wotch.exe`, fallback `%PROGRAMFILES%\Wotch\Wotch.exe`.
   - **macOS (DMG):** `/Applications/Wotch.app/Contents/MacOS/Wotch`. Also check `~/Applications/Wotch.app/...`.
   - **Linux (deb):** `/usr/bin/wotch`, `/usr/local/bin/wotch`.
   - **Linux (AppImage):** no canonical location — scan `~/Applications/`, `~/.local/bin/`, `~/bin/` for `Wotch*.AppImage` or `wotch*.AppImage`.
4. Return `None`.

When running an `.AppImage`, the binary may need the `--no-sandbox` flag depending on the user's kernel / chrome-sandbox availability. Try without the flag first, retry with it on exit code 1 from an Electron-recognizable stderr pattern.

**Prompt push via Wotch's HTTP API** (so Claude Code starts with the student's question already typed). Wire format matches Wotch's `api-server.js` exactly — verified against `POST /v1/tabs` (line 650) and `POST /v1/tabs/:tabId/input` (line 711):

- Request bodies: `{"cwd": "..."}` for tab creation; `{"data": "..."}` (NOT `"text"`) for input.
- Response envelope on ALL endpoints: `{ok: boolean, data?: ..., error?: string, code?: string}`. The `tabId` lives at `response.data.tabId`.

```rust
async fn push_prompt_to_wotch(_app: &AppHandle, prompt: &str) -> Result<(), String> {
    // Read Wotch's token + port. Files end with "\n" — trim.
    let home = dirs_next::home_dir().ok_or("no home dir")?;
    let token = std::fs::read_to_string(home.join(".wotch/api-token"))
        .map_err(|e| format!("read ~/.wotch/api-token: {e}"))?
        .trim()
        .to_string();
    let port: u16 = std::fs::read_to_string(home.join(".wotch/api-port"))
        .map_err(|e| format!("read ~/.wotch/api-port: {e}"))?
        .trim()
        .parse()
        .map_err(|e| format!("parse port: {e}"))?;

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    // Create a tab. Response shape: { ok: true, data: { tabId, type, cwd } }.
    let resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/v1/tabs"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"cwd": home}))
        .send()
        .await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;
    if !resp["ok"].as_bool().unwrap_or(false) {
        return Err(format!("Wotch /v1/tabs refused: {}", resp));
    }
    let tab_id = resp["data"]["tabId"]
        .as_str()
        .ok_or_else(|| format!("no data.tabId in Wotch response: {resp}"))?;

    // Push the prompt to the terminal. Wotch's /input expects `{"data": "..."}`
    // and writes the string verbatim to the PTY, so it's typed as if by
    // keyboard. Include a trailing \r so the shell executes it.
    let command = format!("claude {}\r", shell_quote(prompt));
    client
        .post(format!("http://127.0.0.1:{port}/v1/tabs/{tab_id}/input"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"data": command}))
        .send()
        .await.map_err(|e| e.to_string())?;

    Ok(())
}

/// Quote `s` for POSIX shell. On Windows, cmd.exe prefers different quoting,
/// but Claude Code is usually run under PowerShell or WSL where single-quote
/// / backslash-escape rules differ from cmd. For v1, use single-quote
/// wrapping with embedded single quotes closed-reopened — works on bash,
/// zsh, PowerShell (where single quotes are literal). Document as
/// Unix-first; Windows users running cmd.exe may see literal quotes.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}
```

**Edge cases:**
- Wotch not installed → return error; frontend hides the button OR shows an install prompt.
- Wotch's API server isn't up yet after spawn → the 1500 ms delay handles cold-start. If `push_prompt_to_wotch` still fails, log and continue (Wotch is still launched; the student can type their question manually).
- `~/.wotch/api-port` missing → treat as "Wotch is installed but hasn't completed first launch" — still spawn, skip prompt push.

**Frontend wiring** — `src/components/ChatBar.tsx`:

- Add a `Terminal` icon button (from `lucide-react`) next to the Settings button.
- Only render when `settings.wotch_integration_enabled` is true AND a probe on mount has confirmed Wotch is reachable (or at least installed).
- Optional: a small "wotch is running" indicator dot (polls `~/.wotch/api-port` existence once per 10 s).
- `onClick`: `invoke("launch_wotch", { initialPrompt: input.trim() || null })`. Clears the input on success.

**New AppConfig field:** `wotch_integration_enabled: bool` (default `true`). Add to INV-DATA-006 list, mirror in TS `Settings` + `defaultSettings`.

### 5.2 Wotch → WorkBuddy: "Ask WorkBuddy" command palette entry

**Goal:** Inside Wotch, a student can press `Ctrl+Shift+P` → "Ask WorkBuddy" → type a question → WorkBuddy receives it, surfaces a notification, opens its chat panel, streams a response.

WorkBuddy + Wotch share a maintainer, so this lands as coordinated changes across both repos. Ship WorkBuddy's `/ask` endpoint first (it's useful for CLI tooling independently), then the Wotch command-palette entry in a follow-up Wotch release.

#### WorkBuddy side (ship now)

New `extension.rs` endpoint: `POST /ask`

**Prerequisite refactor:** the current `extension.rs::handle_connection` signature is `(stream: TcpStream, state: Arc<Mutex<ExtensionState>>)` — it has no access to `AppHandle`, so it cannot emit Tauri events. To support `/ask`, thread an `AppHandle` into the server:

1. Change `start_extension_server` to accept `AppHandle`.
2. Change `handle_connection` to accept `AppHandle` (clone per connection — it's cheap).
3. In `lib.rs`, pass `app.handle()` when calling `start_extension_server`.

After the refactor, add the new route inside `handle_connection`'s `match (req.method.as_str(), path_only)`:

```rust
("POST", "/ask") => match serde_json::from_str::<AskRequest>(&req.body) {
    Ok(ask) => {
        // Emit an event to the frontend; the frontend shows the chat panel,
        // populates the input with ask.question, and submits.
        let _ = app.emit("external-question", serde_json::json!({
            "source": ask.source,
            "question": ask.question,
            "context": ask.context,
        }));
        let body = serde_json::to_string(&serde_json::json!({"ok": true}))
            .unwrap_or_default();
        write_response(&mut stream, 200, "OK", &body).await;
    }
    Err(e) => {
        let body = serde_json::to_string(&ErrorResponse {
            error: format!("invalid JSON: {e}"),
        }).unwrap_or_default();
        write_response(&mut stream, 400, "Bad Request", &body).await;
    }
},
```

Where `AskRequest`:
```rust
#[derive(Debug, Deserialize)]
struct AskRequest {
    question: String,
    #[serde(default)]
    source: String,  // "wotch", "cli", etc.
    #[serde(default)]
    context: Option<String>,  // optional free-form context (e.g., current tab's last 200 lines)
}
```

**Auth:** reuse the existing `Authorization: Bearer <token>` check already gating non-`/status` routes in `handle_connection` — no new auth mechanism.

**Body size:** existing 1 MB cap in `read_request` applies (line ~377 in `extension.rs`). `question.len()` should also be checked at the handler level (cap at 4 KB) to avoid massive prompts tying up the UI.

**Frontend event listener** — in `ChatBar.tsx`, inside the main listener `useEffect`:

```tsx
const u5 = await listen<{source: string, question: string, context?: string}>(
  "external-question",
  (event) => {
    const { question } = event.payload;
    if (!question.trim()) return;
    // Show main window if hidden.
    invoke("toggle_visibility_show").catch(() => {});
    setIsExpanded(true);
    // Submit with the external question.
    handleSubmitRef.current(question);
  },
);
if (cancelled) { u5(); return; }
unlisteners.push(u5);
```

(Requires a small `toggle_visibility_show` command — or reuse existing `toggle_visibility` if it can be idempotent-show.)

#### Wotch side (same maintainer, coordinated release)

- Add a command palette entry "Ask WorkBuddy: …" (invokable via `Ctrl+Shift+P`).
- Read token + port from platform WorkBuddy config dir:
  - **Linux:** `$XDG_CONFIG_HOME/workbuddy/extension-{token,port}` (default `~/.config/workbuddy/`)
  - **macOS:** `~/Library/Application Support/workbuddy/extension-{token,port}`
  - **Windows:** `%APPDATA%\workbuddy\extension-{token,port}`
  - Use Node's equivalent: `path.join(os.homedir(), ".config/workbuddy/...")` won't work on macOS/Windows — use a cross-platform config dir resolver (e.g. the `env-paths` npm package or hand-roll by inspecting `process.platform`).
- POST `{question, source: "wotch", context?: <last ~4 KB of current tab's buffer>}` to `http://127.0.0.1:<port>/ask` with `Authorization: Bearer <token>`.
- Show "Sent to WorkBuddy" toast; if the HTTP call fails (ECONNREFUSED / 401 / timeout), show a fallback "WorkBuddy not running — launch it from the tray?" toast.
- Add a Wotch setting `workbuddy.integrationEnabled: boolean` (default `true`) to gate the command palette entry. Silently no-op when disabled OR when the WorkBuddy config files don't exist.

Implementation scope in Wotch: ~80 lines of renderer/settings/command-palette code. Lives in a new file `src/workbuddy-integration.js` and is imported from `src/renderer.js` when the setting is on.

### 5.3 Launch integration is the ONLY place both repos change

Everything else is WorkBuddy-only. This is deliberate: it lets WorkBuddy ship the integration even if the Wotch PR takes time to land.

---

## 6. Implementation phases

Each phase is self-contained, compiles, and delivers user-visible value. A coding agent should finish and verify a phase before moving on.

### Phase 1 — MCP server scaffolding (1–2 days)

**Goal:** An MCP binary that Claude Code can discover, lists one working tool (`get_current_module`), and returns real data.

**Tasks:**
1. Create `src-tauri/workbuddy-mcp/` workspace member with the crate layout in §4.1.
2. Add `rmcp` dependency (pick version from crates.io).
3. Port `context.rs::match_curriculum_context` and `get_active_window_title` (minus the `#[tauri::command]` wrapper).
4. Implement `get_current_module` tool.
5. Build binary: `cargo build -p workbuddy-mcp`.
6. Manual test: run the binary with a simple JSON-RPC stdin script that sends `initialize` → `list_tools` → `call_tool get_current_module`. Confirm correct JSON-RPC responses.

**Acceptance:** `cargo test -p workbuddy-mcp` passes. Manual stdio probe works.

### Phase 2 — Curriculum JSON + remaining read-only tools (1–2 days)

**Goal:** `get_lesson_plan`, `get_module_context`, `list_modules`, `get_ui_elements` all work.

**Tasks:**
1. Write `scripts/generate-curriculum.mjs` (§4.4).
2. Add `npm run generate-curriculum` + hook into `prebuild`.
3. Bundle `lesson_plans/` via `include_dir!` crate.
4. Port `lessonProgress.ts::countSessionFlowSections` logic to Rust.
5. Implement the four remaining read-only tools.
6. Add unit tests for `curriculum.rs` loading + resolution — at minimum one test per program for module-level lookup and one for tier fallback.

**Acceptance:** From Claude Code, calling `workbuddy:list_modules` returns all 52 academy modules + the Trader Lab days. `workbuddy:get_module_context` for `api_academy/03` returns the same snippets the TS `getContextReference` produces.

### Phase 3 — RAG search tool (1–2 days)

**Goal:** `search_docs` works against the same RAG DB as the main app.

**Tasks:**
1. Port `rag.rs::search_docs` + `embed_text` + `cosine_similarity` + `blob_to_embedding`.
2. Read the OpenAI key from the main app's `config.json`.
3. Handle "no key configured" and "DB missing" by returning `{chunks: [], indexed: false}`.
4. Add one integration test that hits a fixture DB (small, committed) with a known query → known top-1 result.

**Acceptance:** With a RAG DB populated via the main app's Settings flow, Claude Code can call `workbuddy:search_docs` and get relevant chunks.

### Phase 4 — Auto-registration (1 day)

**Goal:** On first launch, WorkBuddy offers to register its MCP server in `~/.claude.json`. Toggle exposed in Settings.

**Tasks:**
1. Add `claude_code_mcp_registered: bool` to `AppConfig` (follow INV-DATA-006 — mirror in TS Settings + defaults + set_settings copy).
2. Implement Tauri commands `register_claude_mcp` and `unregister_claude_mcp` per §4.6.
3. Bundle the MCP binary as a Tauri resource / sidecar (`tauri.conf.json` changes).
4. Add a Settings toggle "Claude Code MCP integration" in the Extension section or a new "Developer Tools" section.
5. One-time prompt on first launch post-upgrade: "WorkBuddy can expose curriculum context to Claude Code. Enable?" with "Enable / Not now / Never" — "Never" persists a setting to avoid nagging.

**Acceptance:** After registration, `cat ~/.claude.json | jq '.mcpServers.workbuddy'` shows the binary path. A fresh `claude` session in any terminal lists `workbuddy` as a connected MCP server.

### Phase 5 — Launch integration (1 day)

**Goal:** "Open in Wotch" button in WorkBuddy works.

**Tasks:**
1. Add `wotch_integration_enabled: bool` to `AppConfig` (INV-DATA-006 compliance).
2. Create `src-tauri/src/wotch.rs` with `launch_wotch` + `detect_wotch_path` + `push_prompt_to_wotch` per §5.1.
3. Register command in `lib.rs` invoke_handler.
4. Frontend: add Terminal icon button in ChatBar.tsx, probe for Wotch presence on mount.
5. Add `/ask` endpoint in `extension.rs` and `external-question` event listener in ChatBar.
6. Add Settings toggle "Wotch integration".

**Acceptance:** Clicking the button with Wotch installed launches Wotch; if the input field had text, Claude Code in Wotch starts with that prompt. Clicking with Wotch uninstalled shows a "Wotch is not installed" toast with a link to the release page.

### Phase 6 — Documentation, ADR, student onboarding (0.5 day)

**Goal:** The integration is discoverable and explained.

**Tasks:**
1. Add ADR-034 to `docs/DECISIONS.md` — short, references this doc for details.
2. Add INV-ARCH-015, INV-ARCH-016, INV-SEC-009, INV-SEC-010 to `docs/INVARIANTS.md`.
2. Update `docs/base-archive/Original Docs/05_ROADMAP.md` v0.3 section with a note: "Superseded by `docs/WOTCH_INTEGRATION.md`."
4. Update `CLAUDE.md` tech stack line to mention the MCP binary.
5. In the Limitless Trader Lab cohort kickoff doc (find it; likely in `docs/` or `docs/base-archive/Original Docs/`), add a "Pre-work: install WorkBuddy + Wotch" section.
6. Add a "Claude Code integration" section to `docs/base-archive/TUTORIAL.md` showing example Claude Code prompts that exercise the MCP tools.

**Acceptance:** Someone reading the repo for the first time can find and understand the integration without reading any source code.

### Phase 7 — Wotch command palette entry (coordinated release)

**Goal:** "Ask WorkBuddy" command palette entry in Wotch.

**Tasks:** See §5.2. In the Wotch repo:
- Add `src/workbuddy-integration.js`: token/port reader, HTTP client, error handling.
- Register a command palette entry and an optional global hotkey.
- Settings toggle `workbuddy.integrationEnabled` (default on) in Wotch's settings panel.
- Unit tests: mock the WorkBuddy `/ask` endpoint; verify token is sent, 401 is handled, ECONNREFUSED is handled.
- Bump Wotch minor version; note in the Wotch CHANGELOG + roadmap that this pairs with WorkBuddy v0.3.

**Acceptance:** Fresh Wotch install + fresh WorkBuddy install + running both → Ctrl+Shift+P in Wotch → "Ask WorkBuddy" appears → sending a question surfaces it in WorkBuddy's chat.

**Atomic `~/.claude.json` writes (also Wotch-side):** while in the Wotch repo, check that `claude-integration-manager.js` writes `~/.claude.json` atomically (temp file + rename, merge preserving other `mcpServers` entries). If not already doing this, ship the fix alongside the "Ask WorkBuddy" feature. Required for §4.6.1 coordination.

---

## 7. File inventory

### New files (WorkBuddy)

| Path | Purpose |
|------|---------|
| `src-tauri/workbuddy-mcp/Cargo.toml` | MCP binary crate manifest |
| `src-tauri/workbuddy-mcp/src/main.rs` | Entry point, stdio serve loop |
| `src-tauri/workbuddy-mcp/src/tools/mod.rs` | Tool registration |
| `src-tauri/workbuddy-mcp/src/tools/get_current_module.rs` | Tool 1 |
| `src-tauri/workbuddy-mcp/src/tools/get_lesson_plan.rs` | Tool 2 |
| `src-tauri/workbuddy-mcp/src/tools/get_module_context.rs` | Tool 3 |
| `src-tauri/workbuddy-mcp/src/tools/list_modules.rs` | Tool 4 |
| `src-tauri/workbuddy-mcp/src/tools/get_ui_elements.rs` | Tool 5 |
| `src-tauri/workbuddy-mcp/src/tools/search_docs.rs` | Tool 6 |
| `src-tauri/workbuddy-mcp/src/curriculum.rs` | curriculum.json loader |
| `src-tauri/workbuddy-mcp/src/config.rs` | Read-only config.json accessor |
| `src-tauri/workbuddy-mcp/src/context.rs` | Window-title module detection (ported) |
| `src-tauri/workbuddy-mcp/src/rag.rs` | RAG search (ported) |
| `src-tauri/workbuddy-mcp/curriculum.json` | Generated; committed |
| `src-tauri/src/wotch.rs` | launch_wotch Tauri command |
| `scripts/generate-curriculum.mjs` | Build-time JSON generator |
| `docs/WOTCH_INTEGRATION.md` | This file |

### Modified files (WorkBuddy)

| Path | Why |
|------|-----|
| `src-tauri/Cargo.toml` | Workspace manifest: `[workspace] members = [".", "workbuddy-mcp"]` |
| `src-tauri/src/lib.rs` | Register `wotch::launch_wotch`, `register_claude_mcp`, `unregister_claude_mcp` commands; `mod wotch;` |
| `src-tauri/src/config.rs` | Add `claude_code_mcp_registered`, `wotch_integration_enabled` fields + Default impl + set_settings copy |
| `src-tauri/src/extension.rs` | Add `POST /ask` endpoint, `AskRequest` type, emit `external-question` event |
| `src-tauri/tauri.conf.json` | Bundle MCP binary as sidecar/externalBin |
| `src/contexts/app.context.tsx` | Settings interface + defaults for two new fields |
| `src/components/ChatBar.tsx` | Terminal icon button, external-question listener |
| `src/pages/Settings.tsx` | Two toggles — "Claude Code MCP integration" and "Wotch integration" |
| `package.json` | `generate-curriculum` + `prebuild` scripts |
| `docs/INVARIANTS.md` | Four new invariants (§4.8) |
| `docs/DECISIONS.md` | New ADR-034 |
| `docs/base-archive/Original Docs/05_ROADMAP.md` | Deprecation note pointing to this doc |
| `CLAUDE.md` | Tech-stack line mentioning MCP binary |

### Files NOT touched

- The browser extension (`workbuddy-extension/`) stays as-is.
- `rag.rs` in the main Tauri app stays as-is (duplicated, not refactored, in v1).
- No changes to curriculum source TS files — they remain the source of truth, JSON is derived.

---

## 8. Invariants

Add all four to `docs/INVARIANTS.md`:

- **INV-ARCH-015: MCP server uses stderr only for logging.** `workbuddy-mcp` must never write to stdout except as part of the MCP JSON-RPC protocol. Violation corrupts the protocol stream.
- **INV-ARCH-016: MCP server has read-only access to main app state.** `workbuddy-mcp` reads `config.json`, `rag_vectors.db`, and bundled curriculum/lesson resources but never writes to them. The main Tauri app is the sole writer.
- **INV-SEC-009: MCP embedding calls honor INV-SEC-001.** `workbuddy-mcp::search_docs` sends query text only to `api.openai.com/v1/embeddings` with the user's OpenAI key. No other outbound traffic from the MCP binary. No telemetry.
- **INV-SEC-010: MCP server spawns no arbitrary subprocesses.** The only subprocesses `workbuddy-mcp` may spawn are the platform window-title detectors (`xdotool` / `osascript`) inherited from `context.rs`. No shell execution from tool inputs.

Update INV-DATA-006's persisted-fields list with `claude_code_mcp_registered` and `wotch_integration_enabled`.

---

## 9. Threat model additions

Append to `docs/THREAT_MODEL.md` under **Spoofing** and **Information Disclosure**:

- **Spoofing — malicious MCP server impersonating WorkBuddy:** Claude Code's `~/.claude.json` is user-writable. A malicious tool on the user's system could register an MCP server named "workbuddy" pointing at a different binary. Mitigation: the registration command writes an absolute path derived from Tauri's resource resolver; the user sees the path in Settings; re-registration is idempotent and corrects the path. Not a realistic attack vector unless the system is already compromised (in which case, the user has bigger problems).

- **Information Disclosure — MCP tool output contains curriculum context in Claude's request to Anthropic:** When Claude Code calls `workbuddy:get_module_context`, the returned markdown becomes part of Claude's context and is sent to `api.anthropic.com` (or whichever model the user configured in Claude Code). Curriculum markdown is already public Limitless documentation, so no new leak. Document this explicitly for students.

- **Information Disclosure — Wotch reading WorkBuddy's token file:** The `/ask` endpoint is protected by the same bearer-token scheme the browser extension uses. Wotch needs read access to `~/.config/workbuddy/extension-token`. Both apps run under the same user, so this is not a privilege escalation — merely a local coupling. Document in Wotch's privacy-policy section of the upstream PR.

- **Information Disclosure — WorkBuddy reading Wotch's token file:** Symmetric; same reasoning.

---

## 10. Testing plan

### 10.1 MCP server unit tests

In `workbuddy-mcp/src/tools/*.rs`, add `#[cfg(test)] mod tests` blocks:

- `get_current_module`: fixture window titles (see existing `context.rs` tests in the main app) produce expected outputs.
- `get_lesson_plan`: known module IDs return known plans; unknown IDs return `{found: false}`.
- `get_module_context`: pm_academy/03 returns snippets containing known strings; tier fallback fires when module_id is unknown; `max_chars` truncation works.
- `list_modules`: returns 52 modules across 3 academies + Trader Lab days.
- `search_docs`: fixture DB → known chunk returned for a known query; empty DB → `{chunks: [], indexed: false}`.

### 10.2 MCP integration test (manual, documented)

1. Start a fresh `claude` session in a scratch directory.
2. Verify `claude mcp list` shows `workbuddy` as connected.
3. Send a prompt: "What module is the student currently on?" — expect Claude to call `workbuddy:get_current_module` and summarize.
4. Send: "Give me the lesson plan for API Academy Module 3." — expect Claude to call `workbuddy:get_lesson_plan {program: 'api_academy', module_id: '03'}` and relay the plan.
5. Send a coding-style prompt: "Using the current lesson context, write a TypeScript function that places a BUY order for 10 YES shares at 45 cents." — expect Claude to call `get_current_module` + `get_module_context`, use EIP-712 signing, correct tokenId for YES (positionIds[0]), 1_000_000× USDC scaling, etc.

### 10.3 Launch integration test (manual)

1. With Wotch installed but not running, click "Open in Wotch" in WorkBuddy — Wotch opens, no tab has a pre-filled prompt.
2. Type "how do I place a BUY order?" in WorkBuddy's input, click "Open in Wotch" — Wotch opens, a tab has `claude "how do I place a BUY order?"` typed in.
3. Uninstall Wotch (or rename the binary). Click — toast: "Wotch is not installed. Visit …".
4. With the upstream Wotch PR merged: in Wotch, Ctrl+Shift+P → "Ask WorkBuddy: what's Module 03 about?" → WorkBuddy window appears, question streams into the chat.

### 10.4 Security tests

- Register MCP, verify `~/.claude.json` has absolute path.
- Unregister, verify key is removed cleanly without touching other mcpServers entries.
- Rename WorkBuddy install dir, re-register — path is updated.
- Start `workbuddy-mcp` standalone with no stdin, verify it hangs cleanly (no panics, no stdout writes).

---

## 11. Student-facing changes

### For the Limitless Trader Lab

- Week 0 pre-work adds: "Install Wotch (for coding sessions) and enable the 'Claude Code MCP integration' toggle in WorkBuddy Settings."
- Day 3 (First Programmatic Order) instructions gain: "When you're ready to write the order placement, click 'Open in Wotch' in WorkBuddy to drop into a Claude Code session with today's lesson context already loaded."

### For API Academy and Agents Academy

- Existing module docs: a sidebar note "Using Claude Code? WorkBuddy exposes this module's context automatically via MCP — no need to paste reference material."

### For PM Academy (no coding)

- No change. PM Academy students don't typically use Claude Code; the MCP integration is invisible to them.

---

## 12. Open questions

Document in an "Open Questions" section of ADR-034; revisit during implementation:

1. **rmcp version stability** — if the API churns mid-implementation, fall back to a direct JSON-RPC 2.0 implementation. See §4.2 note.
2. **Lesson plan embedding size** — `include_dir!`'ing all lesson plans may bloat the MCP binary. If binary size is a problem, switch to reading from the Tauri resource dir at runtime.
3. **Curriculum JSON drift** — the `scripts/generate-curriculum.mjs` step is manual-by-default. Consider a CI check that regenerates and fails on diff.
4. **First-launch prompt UX** — the "register MCP?" prompt should not appear during onboarding (too much at once). Defer to first post-onboarding settings visit, or surface as a subtle inline banner.
5. **Wotch release cadence** — WorkBuddy's side of the launch integration ships independently. Wotch's "Ask WorkBuddy" command needs a Wotch release to ship. Coordinate the version story so students installing either tool fresh don't see half a feature.
6. **Port range collision** — Wotch's HTTP API uses base port `19519` with fallback up to `19528`. Wotch's hook receiver is `:19520`, Wotch's MCP IPC is `:19523`. WorkBuddy's extension HTTP server uses `19521` with fallback `19521–19523`. Both apps use port-discovery files (`~/.wotch/api-port`, `~/.config/workbuddy/extension-port`), so the overlap is not functionally broken — but a fresh Wotch install can grab 19521 before WorkBuddy launches, pushing WorkBuddy to 19522. Debugging tip: always consult the port files, never hardcode. Consider bumping WorkBuddy's extension range to `19525–19528` in a future release to remove the overlap entirely.

---

## Appendix A — Example MCP tool exchanges

**Example: Claude Code being asked a curriculum-grounded coding question**

```
[Claude Code → workbuddy-mcp]  initialize
[workbuddy-mcp → Claude Code]  { capabilities: { tools: {...} }, serverInfo: {...} }

[Claude Code → workbuddy-mcp]  tools/list
[workbuddy-mcp → Claude Code]  { tools: [
    {name: "get_current_module", ...},
    {name: "get_lesson_plan", ...},
    {name: "get_module_context", ...},
    {name: "list_modules", ...},
    {name: "get_ui_elements", ...},
    {name: "search_docs", ...}
]}

# Student in terminal: "Write a function to place a BUY order for 10 YES at 45 cents"

[Claude Code → workbuddy-mcp]  tools/call get_current_module {}
[workbuddy-mcp → Claude Code]  { program: "api_academy", module_id: "03", module_title: "Orders", tier: "Foundations", ... }

[Claude Code → workbuddy-mcp]  tools/call get_module_context { program: "api_academy", module_id: "03" }
[workbuddy-mcp → Claude Code]  { context_markdown: "EIP-712 domain: ... tokenId: positionIds[0] = YES ... USDC has 6 decimals ...", ... }

# Claude writes code using the concrete conventions from the lesson.
```

## Appendix B — `~/.claude.json` post-registration

```json
{
  "mcpServers": {
    "wotch": {
      "type": "stdio",
      "command": "/Applications/Wotch.app/Contents/Resources/mcp-server.js",
      "args": []
    },
    "workbuddy": {
      "type": "stdio",
      "command": "/Applications/WorkBuddy.app/Contents/Resources/workbuddy-mcp",
      "args": [],
      "env": {}
    }
  }
}
```

Both MCP servers coexist; Claude Code can call either at will. Wotch's tools are terminal/git-focused; WorkBuddy's tools are curriculum-focused. No functional overlap.

---

## Appendix C — Minimum viable schema for a direct JSON-RPC MCP impl

If `rmcp` turns out to be unsuitable, implement the protocol directly. MCP is JSON-RPC 2.0 over stdio with a small handshake. Key messages:

**Client → Server:**
- `initialize` — capability negotiation
- `tools/list` — list available tools
- `tools/call` — invoke a tool with input JSON

**Server → Client:**
- Responses to the above
- Optional: `notifications/*` for progress/logs

Start with these three request types. A JSON-RPC 2.0 codec over stdio is ~100 lines; dispatch by method name is a match statement. See https://modelcontextprotocol.io/spec for the full message shapes.

---

**End of integration plan.** After implementation, add ADR-034 to `docs/DECISIONS.md` summarizing the decision and linking back to this document.
