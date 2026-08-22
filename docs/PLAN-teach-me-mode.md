# PLAN — Teach Me Mode

**Status:** Shipped (Phases 1–3 complete across all three academies)
**Owner:** Jeremy
**Depends on:** Existing prompt pipeline, module detection via window title, config persistence

## Motivation

PM Academy, API Academy, and Agents Academy (all under the limitless-academy umbrella) each ship with a set of **educator-facing lesson plans** (markdown, ~150 lines each) that describe how to conduct a tutoring session against the matching HTML module: learning objectives, warm-up questions, section-by-section probes, common pitfalls, checkpoints, extensions.

Those plans are currently unused by WorkBuddy. They're a natural fit for an AI tutor. Rather than leaving them as static docs, this feature lets WorkBuddy *become* the educator — loading the relevant plan for the module the student is on and walking them through it conversationally.

**Coexists with tutor mode:**
- `tutor_mode` = Socratic general tutor (already exists, works everywhere)
- `teach_mode` = lesson-plan-structured tutor (new, falls back to tutor_mode when no plan exists)

## Architecture decision: bundle, not RAG

WorkBuddy has an existing RAG pipeline (`src-tauri/src/rag.rs`) for ingesting markdown docs. **Not using it for lesson plans.** Reasons:

1. **RAG requires OpenAI.** Embeddings are OpenAI-only. Gating teach_mode behind a paid dep unrelated to the user's chosen LLM is wrong
2. **Chunking destroys structure.** Lesson plans are designed whole — warm-up → session flow → checkpoints. Splitting across embeddings loses the conversational arc
3. **Lookup is deterministic.** Student on Module 03 → lesson plan 03. That's a file path, not similarity search

Instead: **bundle lesson plans as Tauri resources** and load by `module_id` at prompt-build time. No embeddings, no API dep, no chunking. Plans are ~4–10KB each; fit comfortably in any context window.

### Bundled paths

After the limitless-academy rename + Infrastructure / Dashboard /
TraderControlPanel module additions, the bundled layout is:

```
lesson_plans/
  pm_academy/             ← limitless-academy/academies/pm_academy/lesson_plans/*.md         (22 files)
  api_academy/            ← limitless-academy/academies/api_academy/lesson_plans/*.md        (18 files)
  agents_academy/         ← limitless-academy/academies/agents_academy/lesson_plans/*.md     (16 files)
  limitless_trader_lab/   ← limitless-academy/programs/limitless_trader_lab/lesson_plans/*.md (8 files: day-0..day-7)
```

File naming: `NN_Name.md` matching the HTML module `NN_Name.html` (e.g., `01_api101.md` ↔ `01_api101.html`).

Resolution at runtime: given `(program, module_id)`, read directory → match first file whose name starts with `{module_id}_`.

## File changes

### Backend (Rust)

**`src-tauri/tauri.conf.json`** — register resources under `bundle.resources`. Paths are relative to the `src-tauri/` directory and read from the in-tree bundled copies (which `scripts/sync-lesson-plans.{sh,ps1}` mirrors from the sibling `limitless-academy/` repo):

```json
"resources": {
  "lesson_plans/pm_academy/*.md":            "lesson_plans/pm_academy/",
  "lesson_plans/api_academy/*.md":           "lesson_plans/api_academy/",
  "lesson_plans/agents_academy/*.md":        "lesson_plans/agents_academy/",
  "lesson_plans/limitless_trader_lab/*.md":  "lesson_plans/limitless_trader_lab/"
}
```

**`src-tauri/src/lesson_plans.rs`** (new, ~40 LOC):

```rust
use tauri::Manager;

#[tauri::command]
pub async fn load_lesson_plan(
    app: tauri::AppHandle,
    program: String,
    module_id: String,
) -> Result<Option<String>, String> {
    let subdir = match program.as_str() {
        "api_academy"    => "api_academy",
        "agents_academy" => "agents_academy",
        "pm_academy"     => "pm_academy",
        _                => return Ok(None),
    };

    let dir = app.path_resolver()
        .resolve_resource(format!("lesson_plans/{}", subdir))
        .ok_or_else(|| format!("Resource dir not found: {}", subdir))?;

    if !dir.exists() { return Ok(None); }

    let prefix = format!("{}_", module_id);
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&prefix) && name.ends_with(".md") {
            let content = std::fs::read_to_string(entry.path())
                .map_err(|e| format!("Read lesson plan: {}", e))?;
            return Ok(Some(content));
        }
    }
    Ok(None)
}
```

**`src-tauri/src/lib.rs`** — register module + command:

```rust
mod lesson_plans;
// ...
.invoke_handler(tauri::generate_handler![
    // ...existing
    lesson_plans::load_lesson_plan,
])
```

**`src-tauri/src/config.rs`** — add field to `AppConfig` with `#[serde(default)]` for backward compatibility:

```rust
pub struct AppConfig {
    // ...existing
    #[serde(default)]
    pub teach_mode: bool,
}
```

### Frontend (React / TypeScript)

**`src/lib/curriculum/prompts.ts`** — new instruction block + extended signature:

```typescript
const TEACH_MODE_INSTRUCTIONS = `
━━━ TEACH ME MODE ━━━
You have a structured lesson plan for this module (below). Conduct a paired
tutoring session, not ad-hoc Q&A.

PHASES (follow in order):
1. Warm-Up — Start with the warm-up questions from the plan. Don't move on
   until the student has engaged with at least two of them.
2. Session Flow — Walk sections one at a time. For each: (a) reference the
   module prose, (b) ask the probe question from the plan, (c) wait for
   their answer, (d) only then advance.
3. Checkpoints — At section boundaries, verify the checkpoint criteria.
   If the student can't meet one, rewind.
4. Extensions — Offer only when every checkpoint is clear.

RULES:
- One concept at a time. Never dump the whole plan in one message.
- Probe first, explain second.
- Use Common Pitfalls proactively. If the student's approach matches one,
  surface it before they run and fail.
- Celebrate progress — mark checkpoints explicitly ("checkpoint 2 of 5 ✓").
- Never skip phases when the student wants to jump ahead. Redirect:
  "We'll get to that — first let's make sure X."
━━━━━━━━━━━━━━━━━━━━
`;

export function buildSystemPrompt(
    program, moduleId, moduleTitle, tier, ragContext, tutorMode,
    hasScreenshot, detectedElements, screenshotWidth, screenshotHeight,
    teachMode: boolean = false,
    lessonPlan: string | null = null,
): string {
    let prompt = PROGRAM_PROMPTS[program] ?? GENERIC_PROMPT;
    // ...existing module_context substitution

    if (teachMode && lessonPlan) {
        prompt += TEACH_MODE_INSTRUCTIONS;
        prompt += `\n\n--- LESSON PLAN FOR THIS MODULE ---\n${lessonPlan}\n--- END LESSON PLAN ---\n`;
    } else if (tutorMode) {
        prompt += TUTOR_MODE_INSTRUCTIONS;
    }
    // ...rest unchanged
}
```

Note the precedence: when both modes are on, `teach_mode` + valid `lessonPlan` wins. If the plan failed to load, fall through to tutor_mode. This means enabling teach_mode is strictly additive — the user always gets *at least* tutor mode.

**`src/contexts/app.context.tsx`** — add to Settings interface:

```typescript
interface Settings {
  // ...existing
  teach_mode: boolean;
}
```

**`src/components/ChatBar.tsx`** — toggle button + lesson fetch on submit:

```typescript
// New toggle next to tutor-mode BookOpen:
<button
    title={settings.teach_mode ? "Teach Me mode (on)" : "Teach Me mode (off)"}
    onClick={() => updateSettings({ teach_mode: !settings.teach_mode })}
    className={`p-2 rounded transition-colors ${settings.teach_mode
        ? "bg-accent/20 text-accent"
        : "bg-zinc-900 text-zinc-500 hover:text-zinc-300"}`}
    aria-label="Teach Me mode"
>
    <GraduationCap size={18} />
</button>

// In handleSubmit, before buildSystemPrompt:
let lessonPlan: string | null = null;
if (settings.teach_mode && currentContext?.program && currentContext?.module_id) {
    try {
        lessonPlan = await invoke<string | null>("load_lesson_plan", {
            program:  currentContext.program,
            moduleId: currentContext.module_id,
        });
    } catch (err) {
        console.warn("Lesson plan load failed, falling back:", err);
    }
}

const systemPrompt = buildSystemPrompt(
    // ...existing args
    settings.teach_mode,
    lessonPlan,
);
```

**`src/pages/Settings.tsx`** — toggle near tutor mode with description:

> *"Use structured lesson plans when available for the current module. Falls back to tutor mode otherwise."*

**Ambient indicator** — when `teach_mode && lessonPlan` both truthy, show `● LESSON` badge in accent color near the context badge in ChatBar. Tells the student when the mode is actually active vs silently falling back.

## Phases

### Phase 1 — MVP

Everything above. Feature works for all three academies (API Academy, Agents Academy, PM Academy) and the Limitless Trader Lab. Falls back to tutor mode when no plan exists.

**Success criteria:**
- Open Module 03 of API_Academy, toggle Teach Me on, type any prompt
- WorkBuddy responds with warm-up questions from the lesson plan (not generic Socratic)
- Walks §01 → probes → §02, etc.
- Toggle off → standard chat resumes

**Files touched (approx.):**
- `src-tauri/tauri.conf.json`
- `src-tauri/src/lesson_plans.rs` (new, ~40 LOC)
- `src-tauri/src/lib.rs` (+2 lines)
- `src-tauri/src/config.rs` (+2 lines)
- `src/lib/curriculum/prompts.ts` (+40 lines)
- `src/contexts/app.context.tsx` (+1 line)
- `src/components/ChatBar.tsx` (+25 lines)
- `src/pages/Settings.tsx` (+8 lines)

Total: ~120 LOC.

### Phase 2 — UX polish (later)

- **Progress indicator** — parse `## Session Flow` from plan, show `§02 of 4` in ambient UI
- **Cross-module detection** — when `currentContext.module_id` changes mid-conversation, prompt: *"You moved from Module 02 to Module 03 — continue thread, or start the new lesson?"*
- **Auto-intro** — on teach_mode activation with a valid plan, WorkBuddy sends a terse opener unprompted: *"Teach Me active for Module 03. Ready when you are."* No auto-question — keeps tone dry. Requires a Tauri-side trigger or React-side side-effect
- **Checkpoint markers** — parse `**checkpoint N of M ✓**` from the assistant stream and render as UI pills

### Phase 3 — Progress tracking

**3a — SQLite persistence (shipped)**
- Table `lesson_progress(conversation_id, program, module_id, checkpoints_hit, checkpoints_total, updated_at)` — composite PK on `(conversation_id, module_id)`, indexed on `(program, module_id, updated_at DESC)` for the resume-lookup query.
- Helpers in `src/lib/db.ts`: `saveLessonProgress`, `getLatestModuleProgress`. `deleteConversation` was extended to also cascade-delete progress rows.
- ChatBar's `chat_stream_complete` handler parses each finalized assistant message with `extractCheckpointMarker` and upserts progress. Uses `currentContext.program` (the **detected** program) rather than `settings.program` so the (program, module_id) key matches the plan-lookup key.

**3b — Resume-on-reentry (shipped)**
- `resumeNotice` / `resumeFromCheckpoint` state added to `app.context.tsx`.
- ChatBar probes `getLatestModuleProgress` whenever the plan loads on a fresh conversation. Auto-intro now waits on the probe (new `resumeProbedModuleId` state) and yields to the resume banner when progress exists.
- Banner in ResponsePanel presents **Pick up** (sets `resumeFromCheckpoint`, injects a one-line "Picking up at §N" assistant message) and **Start fresh** (dismisses — auto-intro then fires normally because its deps include `resumeNotice`).
- `buildSystemPrompt` accepts a one-shot `resumeHint` param that appends a `--- RESUMING FROM CHECKPOINT N ---` block instructing the tutor to skip warm-up and open at §N+1. handleSubmit consumes + clears the hint on first submit.
- `clearMessages` now also clears `moduleChange`, `resumeNotice`, and `resumeFromCheckpoint` to keep all teach-mode UI state in sync with conversation state.

**3c — Academy-side gating (dropped)**
Originally proposed: auto-tick the module HTML checklist when WorkBuddy verifies a checkpoint. Dropped, not deferred. The pedagogical "commit" of the student ticking their own box is a feature, not friction; auto-ticking would remove a deliberate pause. The cost-side was also high (JS in 56+ module HTMLs, extension-bridge auth, sync edge cases when a student ticks manually or closes WorkBuddy mid-session), and the PM Academy graduation gate is already Limitless-API-verified trades, not checklist state. Phases 3a+3b stand on their own as the persistence layer.

## Edge cases

| Case | Behaviour |
|---|---|
| No plan for detected module | Silent fallback to tutor mode. If tutor is off, plain chat. One-line note in first response: *"Teach Me is on, but no lesson plan exists for this module yet."* |
| Module changes mid-conversation | Plan re-fetches on every submit. LLM sees updated plan from next turn. Phase 2 adds explicit confirm |
| User toggles mode mid-session | Takes effect on next submit. Past messages unchanged |
| Plan file unreadable | Rust command returns `Ok(None)`; frontend falls through to tutor mode; console warning |
| Very long plans | Longest is ~10KB (Module 16 ProductionBot). Well under context budgets. No chunking needed |
| LLM drifts off phase structure | Prompt-following problem. Opus/Sonnet reliable; Haiku/weaker models may skip ahead. Phase 2 auto-intro + stronger phase markers help |

## Open decisions (resolved)

- **Bundle vs filesystem:** bundled via Tauri resources
- **Coexist or replace tutor_mode:** coexist — teach is an upgrade when plan exists
- **PM Academy plans:** landed; all 22 bundled alongside API Academy (18 after limitless-academy rename) and Agents Academy (16 after rename)
- **Auto-intro wording:** terse, no auto-question (Phase 2)
- **Academy-side checklist auto-tick (Phase 3c):** dropped — student's manual tick is the commit moment; not worth the complexity

## Build order

1. Rust command + bundle config (testable via `tauri dev` devtools console calling `invoke('load_lesson_plan', ...)`)
2. Frontend wiring: Settings interface → ChatBar fetch → prompts extension
3. Toggle button + Settings UI
4. Ambient indicator
5. Manual test: Module 03 of API_Academy, verify warm-up questions match plan
