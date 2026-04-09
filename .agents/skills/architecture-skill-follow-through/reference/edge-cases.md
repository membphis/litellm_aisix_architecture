# Edge Cases & Boundary Rules

Read this file when: confirmation is ambiguous, user rejects a summary, stabilization changes goals, or multiple change groups exist.

## Confirmation Boundary Semantics

**Explicit confirm** — user unambiguously accepts and wants to proceed:
- `确认`, `this summary is fine`, `yes, proceed with this`

**Explicit waive** — user unambiguously instructs to skip waiting at this boundary:
- `no need to confirm, just continue`, `skip confirmation`, `don't ask again, proceed`

**Ambiguous** — anything else (`continue`, `ok`, `fine`, `sure`, `whatever`, `好的`, `可以`). When ambiguous, ask a short clarifying question instead of guessing.

A waive within an active change group persists for that boundary across revisions, unless:
- User narrows, revokes, or replaces the waiver, OR
- Stabilization materially changes the `Skill Change Intent Summary`'s normalized goals (resets boundary 1 waiver)

Recording: when waived, state "boundary X waived by user instruction, proceeding without confirmation." Never skip the artifact itself.

## Failure Paths

| Situation | Action |
|-----------|--------|
| No relevant arch/spec skill changes in scope | Exit this skill |
| Changes exist but no downstream planning intent | Produce `Skill Change Intent Summary` for review only, then stop |
| Confirmation 1 rejected | Revise artifact 1, re-request boundary 1 |
| Confirmation 2 rejected | Revise artifact 2, re-request boundary 2 |
| Reviewing artifact 2 reveals artifact 1 is wrong | Revise artifact 1, re-run stabilization, regenerate artifact 2 |
| Multiple unrelated change groups | Process one group at a time, or ask user which to proceed first |
| Ambiguous confirmation reply | Ask a short clarifying question |
| Partial confirmation with corrections | Treat as rejection, make targeted revisions |
| Same artifact rejected 3 times consecutively | Pause and ask user whether to continue revising or stop |
| `writing-plans` unavailable or fails | Report error, present `Implementation Impact Summary` as standalone deliverable |

## Grouping Rules

- Group changes only when they contribute to the same downstream planning goal or the same arch/spec theme
- If changes would lead to different downstream plans, treat as separate groups
- When grouping is unclear, prefer splitting over merging
- Never force unrelated arch/spec changes into a single summary

## Stabilization Details

Stabilization is an internal analysis checkpoint after boundary 1 resolves:
1. Identify related skill files affected by the same changeset
2. Check directly related but unchanged skill files for overlaps, boundaries, or conflicts
3. Split unrelated changes into separate groups
4. If grouping is unclear, ask a short clarifying question
5. Check for duplicate, conflicting, or incomplete constraints across files

Stabilization does NOT modify skill files (unless user explicitly requests edits).
Stabilization does NOT include downstream code planning.

If stabilization changes normalized goals, regenerate `Skill Change Intent Summary` and re-process boundary 1 (unless already waived in this group).
