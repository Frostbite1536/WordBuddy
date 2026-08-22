# Code Review Prompt — WorkBuddy

You are reviewing a pull request for WorkBuddy. Use the severity levels
and checklist defined in `REVIEW.md`.

For each file changed:
1. Read the full file (not just the diff) for context
2. Check against `docs/INVARIANTS.md` — flag any violation as Critical
3. Check cross-boundary consistency:
   - Tauri command parameter names match frontend invoke() calls
   - Event names match between Rust emit() and TypeScript listen()
   - Rust struct fields match TypeScript interfaces
4. Check the "never do" list in `CLAUDE.md`
5. Verify new dependencies are actually used in source code

Format your review as:

```
## filename.rs (or .tsx)

### [Severity] Brief description
Line N: explanation
Suggested fix: ...
```

End with a summary: total issues by severity, and whether the PR is
ready to merge, needs changes, or needs discussion.
