---
name: architecture-skill-follow-through
description: Use when the user has modified architecture or specification skills and wants downstream implementation-planning follow-through based on those changes
---

# Arch Skill Change Follow-Through

Transform arch/spec skill changes into downstream implementation plans. Core: stabilize skill layer first, map implementation impact, then hand off to `writing-plans`.

## Flow

```
Detect → [Summary 1] → Confirm 1 → Stabilize → [Summary 2] → Confirm 2 → writing-plans
```

No skipping, merging, or reordering. Before `writing-plans` handoff: no code edits, no execution, no detailed plan drafting.

**On edge cases** (rejection / ambiguous reply / multiple groups / stabilization shifts goals): read `reference/edge-cases.md`.

| Step | Action | Key Rules |
|------|--------|-----------|
| 1. Detect | Check arch/spec skill diffs in scope | Anchor to user's specified source; don't expand to unrelated changes. Clean workspace but user points to recent changes → ask a short clarifying question |
| 2. Summary 1 | Produce **Skill Change Intent Summary**: changed files, per-file summary, normalized intent changes, `Goals To Drive Forward` | Inspect actual old/new content, no guessing from filenames. Pre-stabilization draft |
| 3. Confirm 1 | Ask user to confirm summary captures desired changes | Wait for reply. No downstream intent → present for review then stop |
| 4. Stabilize | Analyze/coordinate changed skill set (no file edits): identify related files, check overlaps/conflicts, split unrelated changes | Internal checkpoint, not a user gate. If goals changed → back to Step 2 |
| 5. Summary 2 | Produce **Implementation Impact Summary**: affected modules, must-do/optional/deferred items, items for plan | Check code/docs before claiming impact. This is NOT a plan — scope only, no task sequencing |
| 6. Confirm 2 | Ask user to confirm downstream scope before invoking writing-plans | Wait for reply |
| 7. Handoff | Invoke `writing-plans` with resolved Summary 2 as input | Skill terminates when writing-plans completes |

## Hard Constraints

- Both summaries + both confirmation boundaries must complete before calling `writing-plans`
- The two confirmations cannot be merged into one
- Don't edit arch/spec skill files while using this skill (unless user explicitly asks)
- Confirmations can be explicitly waived by user, but artifacts themselves cannot be skipped
- See `reference/edge-cases.md` for details
