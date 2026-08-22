# Invariant Check Prompt — WorkBuddy

You are verifying that the current codebase satisfies all invariants
documented in `docs/INVARIANTS.md`.

For each invariant (INV-XXX-NNN):
1. Read the rule, rationale, and enforcement method
2. Find the relevant source code
3. Verify the code matches the valid example, not the invalid example
4. Report: PASS, FAIL, or PARTIAL (with explanation)

Check each category:
- INV-SEC-001 through INV-SEC-007 (Security — includes extension token auth + localhost binding)
- INV-ARCH-001 through INV-ARCH-013 (Architecture — includes TTS MIME, UIA spawn_blocking, a11y coord reconciliation)
- INV-DATA-001 through INV-DATA-006 (Data Integrity — includes TTS key gate + set_settings completeness)
- INV-CURR-001 through INV-CURR-004 (Curriculum — includes RAG graceful degrade)

Report format:
```
| Invariant    | Status  | Evidence                                    |
|--------------|---------|---------------------------------------------|
| INV-SEC-001  | PASS    | api_key only in Authorization headers       |
| INV-ARCH-002 | FAIL    | Settings.tsx line 42 uses window.location   |
```

End with: total PASS/FAIL/PARTIAL counts and recommended actions.
