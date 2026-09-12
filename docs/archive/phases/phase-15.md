# Phase 15: Comic Vine cache, rate budget, and missing-issue fill

## Status

COMPLETE. User-tested 2026-09-12.

## Goal

Make the Comic Vine scraper survive a low API rate limit. The port keeps
a local mirror of the Comic Vine issue skeleton on disk, counts every
request per resource, and never re-fetches data that cannot have changed.
The mirror also lets the user create fileless books for the issues of a
series that the library does not hold.

## Scope

- A plugin-local SQLite cache database with two layers. The skeleton
  layer holds volumes and issue numbers. The detail layer holds issue
  records, images, and search results.
- An MCL file reader and writer. The user supplies the file. The reader
  seeds the skeleton layer with no API request.
- An incremental sweep over `/issues` with the
  `filter=date_last_updated:<start>|<end>` form. The sweep keeps the
  skeleton current and resumes after a restart.
- A freshness rule that reads evidence, not a clock. A closed volume
  serves from the cache until the user asks for a refresh.
- Per-resource request accounting that survives a restart, with a
  configurable ceiling and a visible budget state.
- An optional warm task that spends idle budget on the volumes that the
  library already names by Comic Vine id.
- A "Fill Missing Issues" command that compares the owned issues of a
  series against the skeleton and creates fileless books for the gaps.

## Exclusions

- Image downloads do not pass the budget. They come from the image
  host, not from an API resource, so they belong to no bucket.

- No scraper dialog changes. Those are Phase 16.
- No automatic download of an MCL snapshot from a third-party host.
- No change to `ComicDb.xml`. The cache is a separate file.
- No change to the library database format. ADR-029 stays reserved.
- The Comic Vine `search` resource keeps its current behavior in this
  phase, except for the request accounting.

## Locked decisions

- ADR-037 — the Comic Vine cache is a plugin-local SQLite file with a
  skeleton layer and a detail layer.
- ADR-038 — the MCL interchange format, its writer defects, and the
  incremental sweep query.
- ADR-031 — one crate per module. All new code stays in `cr-scrape`,
  except the one UI command in `cr-ui`.
- ADR-033 — configuration goes to `[plugins.comic-vine-scraper]`.

## Tasks

- [x] T1 — the cache store. `rusqlite` (bundled) behind a `CvCache`
      trait. Tables: `volume`, `issue_skeleton`, `issue_detail`,
      `image_blob`, `search_result`, `sweep_state`, `request_log`. The
      file is `$XDG_DATA_HOME/comicrust/plugins/comic-vine-scraper/
      cvcache.sqlite`. Acceptance: the trait has an in-memory test
      implementation, and the schema migrates from empty.
- [x] T2 — the MCL reader and writer. Acceptance: fixtures pin the
      trailing comma, the `.&@1` and `.&@2` escapes, the quoted list
      form, and the volume 77901 issue number `1,5`. A read and a write
      of the same file agree on the data, not on the bytes.
- [x] T3 — the incremental sweep on a worker thread. Acceptance: a mock
      server test pages the sweep, stops it, and resumes it from
      `sweep_state`. The File menu drives it through "Update Comic Vine
      Cache", which sweeps from the last window end, or from the MCL
      snapshot date, to today.
- [x] T4 — the freshness rule. A volume is closed when the cached
      `count_of_issues` equals the cached issue count and the last cover
      date is older than the horizon. Acceptance: a closed volume makes
      zero requests, an open volume makes one revalidation request.
- [x] T5 — request accounting. One chokepoint logs every request.
      Acceptance: the budget survives a restart, the client blocks at
      the ceiling, and the scrape window shows the remaining budget and
      the resume time.
- [x] T6 — the warm task. Off by default, budget-capped, cancellable.
      Acceptance: the task stops at the budget and on cancel. The
      status-bar lamp and the Tasks window "Comic Vine cache" row
      carry every cache job (2026-09-12, the Phase 15a round); the
      earlier completion-dialog deviation is CLOSED.
- [x] T7 — "Fill Missing Issues". The command compares the owned issue
      numbers of a series against the skeleton, shows the gaps, and
      creates fileless books through `new_fileless_book()`. Each new
      book carries the series, the volume, the issue number, and the
      Comic Vine issue id. Acceptance: a series with no known Comic Vine
      id asks the user to pick the volume.

## Visibility

Every cache job shows a Comic Vine lamp in the status bar, with its
live line in the tooltip and a "Cancel Comic Vine cache job" row on
click. The same line is a "Comic Vine cache" row in the Tasks window,
which the "Abort all User Tasks" button also stops. One job runs at a
time: two sweeps would race on the one `sweep_state` row.

## Commands

The File menu gains three rows. The C# plugin had no cache, so it had
no such commands.

- **Import Comic Vine MCL File…** — seeds the skeleton from a file the
  user supplies. No API request. The snapshot date becomes the start of
  the next sweep.
- **Update Comic Vine Cache** — one incremental sweep over the issues
  that changed since that date.
- **Warm Comic Vine Cache** — spends idle budget on the volumes the
  library already names.

The book menu gains **Fill Missing Issues…**.

## Verification

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo test --workspace`.
- Mock-server tests for the sweep, the freshness rule, and the budget.
- The Fill Missing Issues logic is gated headless in `cr-scrape`
  (`cache::missing`). The dialog itself has no probe; the user test
  covers it, as the navigator list command does in Phase 14.

## User test

The steps are test 1 in `docs/open-user-tests.md`.

## Open issues

- The per-resource ceiling of 200 requests per hour is not in the Comic
  Vine reference documentation. The figure comes from a Comic Vine
  statement elsewhere. The key is configurable for this reason.
- A library series carries a Comic Vine volume id only after a scrape.
  T7 tells the user to scrape one book first. It does NOT offer a
  volume picker yet; that is the smaller half of the planned fallback
  and it goes to `docs/backlog.md`.

## Completion record

COMPLETE. User-tested 2026-09-12, including the Phase 15a visibility
round.

Delivered: a two-layer SQLite cache (ADR-037), MCL import and export
(ADR-038), the incremental `date_last_updated` sweep, freshness by
evidence, a per-resource request budget that survives a restart, the
warm task, and "Fill Missing Issues".

Measured, in API requests: a closed volume 0, an unchanged open volume
1, a changed open volume 2, an unknown volume 1 plus its pages.

### Phase 15a — the visibility round

The user test found that the cache commands showed nothing and could
not be stopped. Three defects, all fixed and gated.

1. The scrape progress window had NEVER been presented (since Phase
   12). Every scrape ran with no status list and no reachable Cancel
   button. `scrape_probe` GATE V now checks visibility.
2. `show_info_dialog` and `show_error_dialog` had no transient parent,
   so the report appeared behind the main window. They were the only
   two such dialogs in `cr-ui`.
3. The cache jobs discarded their progress callback and held their
   cancel flag nowhere. They now show a status-bar lamp with a live
   tooltip, a Tasks window row, and a working cancel. The Phase 15
   Tasks deviation is CLOSED.

A fourth defect came from test 5: the cover-chip tooltip described the
scan chip only, so a book with both the "?" and the "!" chip explained
one of them.

### Deviation that stands

"Fill Missing Issues" needs a Comic Vine volume id, which a series
carries only after a scrape. With no id the command says so and stops.
A volume picker is on `docs/backlog.md`.
