# Phase 19: The Missing Issues gap view

## Status

IMPLEMENTED (2026-09-19). T1-T4 are done and `cargo fmt --all`, `cargo
clippy --workspace --all-targets -- -D warnings`, and `cargo test
--workspace` all pass. The UI probe and the user test are still open —
see the Completion record.

## Goal

Add a read-only "Missing Issues" view that lists every issue a series
should have but the library does not hold. The list of expected issues
comes from the local Comic Vine cache skeleton, which the user seeds
offline by importing an **MCL** file (ADR-038). The gap is the cached
issue list of a volume minus the issue numbers the library owns.

The view is a details list (no thumbnails). It refreshes only on an
explicit user action. Its input series can be the whole library or the
series in a smart list the user picks, so the gap report can run on a
subset.

The view creates no books and writes nothing to the database. It is a
report, not an edit.

## The user request, in the user's words

- "A global list of the missing issues across the library."
- "The list should just be the details view, no thumbnails."
- "Only a manual refresh" (the pass may take a while).
- "Have it work on a subset of the total library" by choosing a smart
  list as the input.

The user first asked for this as a smart-list *rule*. That is not
possible and the reason is in "Rejected shape" below. The subset need is
met by choosing a smart list as the INPUT to the gap pass, not by a new
matcher.

## Rejected shape: a smart-list rule (do not attempt)

`CODE-READ`: A smart-list matcher is a predicate over an existing
`ComicBook` (`crates/cr-engine/src/matcher/eval.rs:356`,
`match_value(book, ...) -> bool`). Every matcher kind keeps or drops a
book that is already in the input set. A matcher cannot invent rows that
are not in the library. Missing issues are, by definition, not in the
library. So no matcher can produce them. Do not add a "gap" matcher.

The subset requirement is met a different way: `evaluate_smart_list`
(`crates/cr-engine/src/smart_list.rs:43`) returns the books a smart list
selects. The gap pass takes the distinct series of THAT book set as its
input. The smart-list engine selects the input series; it does not
produce the gaps.

## Locked design decisions (researched, do not re-derive)

1. **Reuse the ItemView in Detail mode. Do not build a new widget.**
   `CODE-READ`: Detail mode is a custom Cairo owner-draw
   `DrawingArea` (`crates/cr-ui/src/browser/item_view.rs:335`), not a
   `ColumnView`. Each row is a full `ComicBook` in a `Vec<ComicBook>`
   (`crates/cr-ui/src/browser/view_state.rs:126`). The renderer pulls
   only per-column strings from each book through the property registry
   (`crates/cr-ui/src/browser/columns.rs:339`, `cell_text`). The draw
   and selection paths never read `library::session()`. The Incoming
   view already feeds the ItemView books that are NOT in the database
   session (`crates/cr-ui/src/library.rs:1212`, gated by
   `is_incoming_view()`). Build synthetic `ComicBook` rows for the
   missing issues and hand them to `ItemView::set_books`
   (`item_view.rs:551`).

2. **Local cache only. Never the network.**
   `CODE-READ`: The gap read must call the cache-only path
   `CvCache::issues_of_volume` (`crates/cr-scrape/src/cache/sqlite.rs:230`,
   a single indexed SQLite SELECT on `issue_skeleton`). Do NOT call
   `freshness::issues_of_volume`
   (`crates/cr-scrape/src/cache/freshness.rs`); that path can reach the
   Comic Vine network to revalidate an open volume. The gap arithmetic
   (`crates/cr-scrape/src/cache/missing.rs:50`, `missing_issues`) and the
   volume-id vote (`missing.rs:79`, `volume_id_of`) are pure. This view
   needs no API key and spends no request budget. The report is only as
   complete as the last MCL import; a series with no cached issues yields
   no rows, by design.

3. **Manual refresh only.**
   The view does not auto-recompute on library edits, MCL import, or
   scrape. It shows a Refresh control and a last-computed state. This is
   independently justified: the pass wall-time is UNMEASURED (see the
   named unknown below), so no automatic trigger may assume it is cheap.

4. **The gap pass runs on a worker thread (Rule 9).**
   Mirror `refresh_incoming_external_gaps_async`
   (`crates/cr-ui/src/library.rs:1562`): spawn a worker, publish results
   over an `mpsc` channel, and let the MAIN thread write shared state and
   the ItemView. A worker must never write a UI thread-local
   (`ShellState::run_cv_job` is the reference).

5. **Avoid the O(S x N) trap the team already fixed once.**
   `CODE-READ` + `MEASURED`: The existing
   `project_incoming_external_gaps` (`crates/cr-ui/src/library.rs:1528`)
   still collects owned numbers with a FULL library scan PER series
   (`library.rs:1544`), each recomputing `incoming_identity`
   (proposed-parse + `normalize_series` + SipHash,
   `crates/cr-engine/src/incoming.rs:505`). That O(identities x books)
   shape is the same class of bug that drove idle CPU to 100% on
   2026-09-18 (the fix was inside `incoming_volume_ids_for`; the per-
   series scan in `project_incoming_external_gaps` was left). See
   `docs/current-status.md` lines 44-55. The NEW gap engine must build an
   owned-numbers-by-series map in ONE O(N) pass, then iterate series
   against that map. Do not scan the whole library once per series.

## Compatibility and constraints

- No ComicDb.xml change. The view writes nothing. Compatibility
  invariant 1 is not touched.
- ADR-031: `cr-scrape` takes no GTK dependency. The gap engine stays in
  `cr-scrape` / `cr-engine`; only the view lives in `cr-ui`.
- Rule 9: all archive/database/cache/decode work runs off the GTK
  thread.

## Named unknown (must be measured during T2, not guessed)

`UNKNOWN`: The wall-clock time of one `issues_of_volume` read and of a
full-library gap pass. There is no perf test for it
(`docs/current-status.md` and the `tests/` search found none). Rule 0:
do not state a duration you did not measure. T2 adds the measurement.

## Scope

- A navigator entry, "Missing Issues", that opens a Detail-mode list of
  the gap rows.
- A scope selector on the view: input series = the whole library, or the
  series in a smart list the user picks.
- A Refresh control that starts the worker pass and shows a running /
  last-computed state.
- Columns: Series, Number, Title, Year, and the Comic Vine issue id.
  Read-only rows, no cover column.

## Exclusions

- No book creation. This view never writes fileless books. (Book
  creation is the separate, existing "Fill Missing Issues" command,
  `crates/cr-ui/src/browser/shell.rs:4136`.)
- No smart-list "gap" matcher (see "Rejected shape").
- No network access, no API key requirement, no request-budget spend.
- No auto-refresh.
- No row activation behavior in the first cut. A double-click on a gap
  row resolves no path (`library::book_path` returns `None` for a
  non-library id, `shell.rs:2429`) and is a harmless no-op. If later
  wanted, follow the `is_incoming_view()` alternate-lookup pattern.

## Tasks

- [x] **T1 — the gap engine (pure, local-cache only).**
      Add a function that, given the library books in scope and a
      `CvCache`, returns the missing issues grouped by series. Build the
      owned-numbers-by-series map in ONE O(N) pass (locked decision 5),
      derive each series' volume id with `volume_id_of`
      (`missing.rs:79`), read that volume once with `issues_of_volume`
      (cache-only, locked decision 2), and diff with `missing_issues`
      (`missing.rs:50`). A series with no cached issues or no voted
      volume id produces no rows. Place it beside the existing pure gap
      code in `cr-scrape` (or `cr-engine` if it must see library types;
      keep `cr-scrape` GTK-free per ADR-031).
      Acceptance (unit tests): owned-subtraction across several series;
      a series with no volume id is skipped; an empty cache yields
      nothing; the single-pass owned map is verified (no per-series full
      scan); a book whose owned number is not in the volume is ignored.

- [x] **T2 — the worker pass and its measurement.**
      Wire T1 behind an async worker that mirrors
      `refresh_incoming_external_gaps_async`
      (`crates/cr-ui/src/library.rs:1562`): spawn, publish over `mpsc`,
      write shared state on the main thread. Add a `CvJobKind` or an
      equivalent so the running state is visible (the status-bar lamp /
      Tasks row, phase-16 trap 4). Measure the full-library pass on the
      real library and record the number as `MEASURED` in
      `docs/current-status.md` (this resolves the named unknown). The
      worker builds synthetic `ComicBook` rows (series, number, volume,
      title, year in fields; Comic Vine issue id in a custom value, as
      "Fill Missing Issues" does at `shell.rs:4212`) for the view to
      show.

- [x] **T3 — the navigator entry and the Detail view.**
      Add the "Missing Issues" node (follow `IncomingView`,
      `crates/cr-ui/src/browser/navigator.rs:115`). On open, show the
      cached gap rows in Detail mode via `ItemView::set_books`
      (`item_view.rs:551`). Force Detail mode and suppress the cover
      column for this view. No auto-refresh.

- [x] **T4 — the scope selector and Refresh control.**
      A control to choose the input: whole library, or a smart list.
      When a smart list is chosen, run `evaluate_smart_list`
      (`crates/cr-engine/src/smart_list.rs:43`) to get its books, take
      their distinct series as the T1 input. A Refresh button starts the
      T2 pass. Show a running / last-computed state.

## Traps (from the phase-16 and Incoming work; read before coding)

1. A GTK4 window/widget is invisible until presented. Verify the view
   actually shows in a probe.
2. A worker thread must never write a UI thread-local. Send over `mpsc`;
   the main-thread pump writes. Reference `ShellState::run_cv_job`.
3. Long work needs a visible running state and, if it can be long, a
   cancel. The status-bar lamp and the Tasks window row are the
   surfaces.
4. Do NOT call `freshness::issues_of_volume`. Cache-only
   `issues_of_volume` (locked decision 2). A network call in a "local
   report" is a scope breach.
5. Do NOT re-introduce the per-series full-library scan (locked
   decision 5). Build the owned map once.

## Verification

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo test --workspace`.
- T1 unit tests as listed in its acceptance.
- T2 records a `MEASURED` full-library pass time in
  `docs/current-status.md`.
- A UI probe that opens the view, refreshes with the whole library, then
  refreshes with a chosen smart list, and confirms the row set narrows.
  Run in RELEASE under Xvfb with isolated XDG paths; see
  `docs/current-status.md` environment notes.
- User test (add the steps to `docs/open-user-tests.md`): import an MCL
  file, open Missing Issues, refresh, confirm the details list shows the
  gaps and no thumbnails; then scope it to a smart list and confirm the
  list narrows to that subset; confirm no book is added to the library.

## Decisions to record

Add one ADR to `docs/decisions.md` (confirm the next free number at
commit time; ADR-059 is expected) that states: the gap report is a
read-only Detail view, reads the local cache only, never the network,
creates no books, refreshes only on demand, and scopes its input series
through a smart list. Note that a smart-list matcher cannot produce
missing issues because a matcher filters existing books.

## Open issues

None. The one named unknown (pass wall-time) is resolved by T2's
measurement, not before.

## Completion record

Implemented 2026-09-19: T1 (`crates/cr-scrape/src/cache/missing.rs`,
`missing_issues_of_library` + `year_of_cover_date`, 6 new unit tests), T2
(`crates/cr-ui/src/library.rs`, `refresh_missing_issues_async` +
`CvJobKind::MissingIssuesGap` + `smart_list_scope_options`), T3
(`crates/cr-ui/src/browser/navigator.rs` fixed id + row,
`crates/cr-ui/src/browser/shell.rs` dispatch/forced-view-config,
`crates/cr-ui/src/browser/columns.rs` column 215, `crates/cr-engine/src/
display_text.rs` `ComicVineIssueId` arm), T4
(`crates/cr-ui/src/browser/missing_issues_bar.rs`, wired into `shell.rs`).
ADR-059 recorded. `docs/open-user-tests.md` test 22 added.

`cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D
warnings`, and `cargo test --workspace` all pass. A synthetic timing
gate (`crates/cr-scrape/tests/missing_perf.rs`) resolves the named
unknown at 22,000-book/2,000-series scale: 63.5 ms release, 354 ms
debug — see `docs/current-status.md`.

The RELEASE + Xvfb UI probe (`crates/cr-ui/examples/missing_issues_probe.rs`)
ran and passed three gates (forced Detail view/columns; whole-library refresh;
smart-list-scoped refresh narrows the row set; no book created in either
case), stable across four consecutive runs.

**Bug found and fixed, 2026-09-19 (first user test):** scoping to a smart
list that happened to exclude every one of a series' Comic-Vine-linked
copies made the whole series silently report "0 missing" instead of its
real gaps. `missing_issues_of_library` now takes the scope for owned-number
counting and the whole library, always, for the Comic Vine volume-id vote —
see ADR-059's follow-up and `docs/current-status.md`. A new regression test
(`a_scope_missing_every_linked_copy_still_votes_from_the_whole_library`)
covers it; `cargo fmt`/`clippy`/`test`, the synthetic timing gate, and the
UI probe were all re-run clean after the fix.

Still open: the user test (test 22 in `docs/open-user-tests.md`, including
its new scoped-series regression check), and the real-library pass timing.

## Find in Incoming extension

Implemented 2026-09-19 under ADR-061. The Missing Issues context menu now has
**Find in Incoming**. It matches selected gaps to the Incoming catalog by
normalized Series, Volume, and Number. A summary dialog requires a choice when
more than one Incoming file matches an issue. The command uses the dedicated
Move profile from **Preferences > Libraries**, then runs the existing durable
Incoming adoption path. A successful run refreshes the report.

`MEASURED`: The release `find_incoming_probe` passed under Xvfb with isolated
XDG paths. It presented two candidates for one gap, adopted the selected copy,
left the other copy in Incoming, inserted the adopted record into the main
Library, and removed the filled gap from the refreshed report.

Follow-up 2026-09-19, ADR-062: **Incoming > Gap Fills** Adopt and Preview
Adoption now use the same dedicated profile as Find in Incoming. Adopt shows a
summary and confirmation. Preview runs the profile directly in simulation
mode. Other Incoming views keep the general profile selector. Organizer runs
now write preparation, current-operation, and completion rows into the progress
window instead of leaving its main area empty on successful operations.

`MEASURED`: The expanded release `find_incoming_probe` confirmed direct Gap
Fills preview, no profile selector, no preview mutation, the dedicated-profile
Adopt confirmation, and the existing Find in Incoming adoption. `UNKNOWN`: the
progress rows need a user observation during a real-library operation.

## Cached series metadata extension

Implemented 2026-09-19 under ADR-063. Normal Comic Vine searches and volume
queries now retain reusable volume metadata in SQLite. Complete issue-list
queries also retain their issue number and ID map. After a newly linked issue
finishes a full scrape, a worker fills blank Publisher, Imprint, volume year,
and Comic Vine links across the matching library series. **Link Series from
Cache** fills the same blank fields within its existing current-view scope.
Existing values remain unchanged. The worker result lands through one bulk
library update.

`MEASURED`: Unit tests confirm blank-only enrichment, preservation of existing
values, unmatched issue handling, and cache writes from series and volume
queries. The required workspace verification passes. `UNKNOWN`: the GTK flow
has not run against the user's real library. See open user test 25.
