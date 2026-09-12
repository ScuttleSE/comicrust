# Guide: documentation

Each document has one responsibility. A fact has one home. When you must
repeat a fact, link to its home instead.

## Ownership

| File | Holds | Must not hold |
|---|---|---|
| `AGENTS.md` | The rules, the startup order, the invariants | Status, history, task records |
| `docs/current-status.md` | The active phase, the current task, the list of open user tests, the last verification | Old phases, resolved defects, a session log, user-test steps |
| `docs/open-user-tests.md` | The steps of every open user test | A status, a verdict, a closed test |
| `docs/phases/<phase>.md` | The scope, the locked decisions, the tasks, the acceptance criteria | A full debugging narrative |
| `docs/decisions.md` | The ADRs and their reasons | Task status |
| `docs/backlog.md` | Unscheduled work | Completed work |
| `docs/port-plan.md` | The architecture and the phase sequence | The current state |
| `docs/guides/` | Stable, repeatable procedure and lessons | A dated status report |
| `docs/archive/` | Closed phases and their evidence | Instructions for current work |
| `docs/config-reference.md` | Every configuration parameter | Implementation notes |
| `docs/risk-register.md` | Open risks | Closed risks |

## Size limits

- `AGENTS.md`: about 250 lines. It is read at every start.
- `docs/current-status.md`: under 150 lines. Replace, do not append.
- A phase file: keep the task notes short. Move a long design to an ADR.

## The phase template

```markdown
# Phase N: Title

## Status
Planned | Active | Implemented, user test pending | Complete | Deferred

## Goal
One short paragraph.

## Scope
- Included item

## Exclusions
- Excluded item

## Locked decisions
- The ADR link and the short decision

## Tasks
- [ ] T1: The deliverable and its acceptance condition

## Verification
- The automated checks, the probes, the user test

## Open issues
- Current blockers only

## Completion record
Fill this in only when the phase closes.
```

## The phase lifecycle

1. Create the phase file in `docs/phases/` BEFORE implementation starts.
2. Name it in `docs/current-status.md`.
3. Work the tasks. Keep the status line current.
4. Run the user test.
5. Write the completion record.
6. Move the file to `docs/archive/phases/`.
7. Set `docs/current-status.md` to "No active phase".
8. Pick the next work from `docs/backlog.md`.

## What to remove when you archive a phase

Delete these from the archived file. Git holds them.

- A round-by-round debugging narrative.
- An old test count.
- A fixed crash stack trace.
- A timing measurement that no longer applies.
- A repeated statement of a decision that is already an ADR.

Keep the goal, the final scope, the decisions, the deviations, the delivered
result, and the links to the commits.

## Language

Write every document in Simplified Technical English (ASD-STE100). Use the
`asd-ste100` skill. Short sentences. Active voice. One instruction per
sentence. Keep every hedge that carries the author's confidence.
