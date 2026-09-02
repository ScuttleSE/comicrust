# Risk Register

Review at the start of each phase; add rows as risks are discovered. L/I = likelihood/impact (low/med/high).

| # | Risk | L | I | Early-warning signal | Mitigation | Owner phase |
|---|---|---|---|---|---|---|
| 1 | **ComicDb.xml fidelity gaps** — the app mutates intricate XML (ComicLists tree, custom values, cache attrs); a lossy write destroys user libraries | med | critical | Golden round-trip diffs; user-report of dropped fields after migrate | Golden-file round-trip corpus from Phase 0; `.bak`/`.restore` semantics preserved; never emit format changes without passing tests; corrupt-file quarantine behavior ported | 0-2 |
| 2 | **ItemView behavior parity** — the browser is ~9k LOC of bespoke virtualized interaction (thumbnail/tile/detail, grouping, stacking, columns, drag-drop); it *is* the product | high | high | Daily-driver browsing feels wrong; missing keyboard/search affordances | Port behavior-by-behavior from `ItemView.cs`/`CoverViewItem.cs` (source is the spec, not screenshots); build behind the reader so infrastructure exists first | 4 |
| 3 | **Python 2→3 plugin migration** — ecosystem scripts (ComicVine, FromDucks) are IronPython 2.7; PyO3 shim may not cover every host-API use | high | med | Top-5 community plugins failing acceptance tests; host API churn during Phase 6 | PyO3 + shim per ADR-003; ship 2to3 migration guide + linter; acceptance-test the top-5 plugins; design shim API early against real plugin source | 6 |
| 4 | **GL renderer parity** (transitions, 3D spine, paper-texture MULTIPLY, magnifier) | med | med | Visual artifacts vs C# reader; perf cliffs on large pages | Cairo fallback first (ADR-008, mirrors C# GDI fallback); GL port as optimization; pixel-diff fixtures from reference screenshots | 3 |
| 5 | **Scope gravity in the ~50-dialog surface** — where ports historically die | high | high | Phase 5 slipping; urge to descope silently | Per-phase ship gates; phases 3-4 already usable without full dialogs; descope decisions require a new ADR | 5 |
| 6 | Smart-list semantics drift (76 matchers, custom query language) | med | high | Migrated smart lists select different books than C# | Fixture-based differential tests against C# reference behavior; query-cache comparison | 2 |
| 7 | Reflection-shaped code (property access by string name) leaks into Rust design | med | med | Ad-hoc string-keyed lookups scattered across crates | Property registry as a first-class `cr-core` citizen from Phase 0 (columns, matchers, remote updates, options panels all route through it) | 0 |
| 8 | Licensing deadlock (upstream CE provenance + plugin ecosystem) | low | high | Distribution blocked at Phase 8 | ADR-009 defers decision deliberately; no binaries distributed until resolved; keep dependency license inventory | 8 |
| 9 | Effort underestimation (estimates assume solo sustained work) | high | med | Phase durations consistently exceeded | Re-estimate at each phase gate; phases are independent shippable units so slips degrade gracefully | all |

## Standing rules

1. Any risk that fires becomes an issue/ADR note in this file's changelog — don't let mitigations stay implicit.
2. Risks 1, 2, 3 have concrete acceptance tests defined in `phase-0-kickoff.md` and `port-plan.md` phases — track them there, not here.
