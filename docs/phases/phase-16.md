# Phase 16: Comic Vine scraper quality of life

## Status

PLANNED. No task started. Phase 15 is complete and user-tested.

## Goal

Port the usability work from the `Fableton/comic-vine-scraper-ce` fork
of the Comic Vine Scraper plugin. The comicrust scraper was ported from
the **upstream v1.0.102** release, so none of the fork's work is
present. The Phase 15 cache (ADR-037) makes the cover art and the date
columns cheap to fetch.

## The source

- **Fork:** `https://github.com/Fableton/comic-vine-scraper-ce`
  (`CHANGELOG.md` and `ROADMAP.md` list every change).
- **Upstream:** `https://github.com/cbanack/comic-vine-scraper`.
- The fork is IronPython 2 over WinForms. The target is its BEHAVIOUR,
  not its layout. Read the Python when a rule is unclear.

## What exists today (measured 2026-09-12)

Do not re-derive this. Verify a line number before you trust it.

| Feature | State |
|---|---|
| Issue picker | `gtk4::ListBox` of single labels, `dialogs/scrape.rs:476`. Row text from `issue_row_text` (`:462`): `"Issue #N — Title"`. No columns, no filter, no sort |
| Series picker | `ListBox` of per-row `Grid`s, `dialogs/scrape.rs:317`, built by `series_grid` (`:288`). Four columns aligned by `width_chars` only. No filter, NO sort of any kind |
| Search dialog | plain `gtk4::Entry`, `dialogs/scrape.rs:221`. No history, no prefill |
| Cover art | none in either picker. `IssueRef.thumb_url` and `SeriesRef.thumb_url` exist and the UI never reads them |
| `IssueRef` | `cv/models.rs:8` — `issue_num`, `issue_key`, `title`, `thumb_url`. **No `cover_date`** |
| Issue `field_list` | `cv/queries.rs:314` and `:335` — `"name,issue_number,id,image"`. No `cover_date` |
| Config dialog | one flat grid + a raw `KEY=VALUE` `TextView`, `dialogs/scrape_config.rs:209` |
| Advanced keys | 22 in `config::ADVANCED_KEYS` (`cr-scrape/src/config.rs`), all text-only |
| `WELCOME_DIALOG` | parsed into `AdvancedSettings.welcome_dialog`, **nothing reads it** |
| `summary_dialog` | persisted flag, **nothing reads it**. `shell.rs` drops the `ScrapeSummary` |
| Error reporting | `ScrapeUi::error` writes "⚠" into the progress label. No dialog |
| Window size | not persisted for any scrape dialog |

Useful existing pieces:

- `SeriesResult` and `IssueResult` (`cr-scrape/src/engine.rs:34`, `:48`)
  already carry a `Permskip` arm that NO UI path produces. `Previous`
  goes beside them.
- `freshness.rs:276` already asks for `cover_date` for the cache. The
  picker path (`queries.rs:314`) does not.
- `dialogs/missing_issues.rs` is a recent, working example of a worker
  thread plus a channel plus a GTK dialog in this codebase.

## Scope

- **Issue picker:** a `ColumnView` with Issue, Title, Year, and Month
  columns, a sort on every column, and a debounced filter row.
- **Series picker:** a `ColumnView` with Series, Year, Issues, and
  Publisher columns, a shift-click tie-breaker sort, a filter row, a
  cover-art pane, and an editable issue number that drives the preview
  AND the issue match.
- A **"Previous Comic"** button in both pickers.
- **Search dialog:** a history of the last 20 terms, a prefill from the
  current terms, and a year-range override for one search only.
- An **ignore list for publishers**, persistent and session-only, with a
  right-click command in the series picker.
- A **tabbed configuration dialog** with an information button per tab.
- The three flags the code stores but never reads: the welcome dialog,
  the summary dialog, and a real error dialog.
- **Window size persistence** for the scraper dialogs.

## Exclusions

Four fork changes this port does not need.

- The appearance scale slider (75% to 150%). A WinForms workaround; GTK
  takes the text scale from the desktop and from CSS.
- The Ctrl+Backspace fix. A GTK `Entry` already deletes the previous
  word.
- The `TableLayoutPanel._can_change_page` crash fix. WinForms only.
- The "Configure… loads an unbuilt copy" fix. An Ant packaging defect.

The fork's open roadmap item is also out of scope: a marker for a
collected edition in the series results. The API reference page exposes
no such field. It is on `docs/backlog.md` for research first.

## Locked decisions

- **ADR-031** — the work stays in `cr-scrape` and the `cr-ui` scrape
  dialogs. `cr-scrape` takes no GTK dependency.
- **ADR-033** — new configuration goes to
  `[plugins.comic-vine-scraper]`. New advanced keys join
  `config::ADVANCED_KEYS` AND `docs/config-reference.md`; a drift gate
  fails otherwise.
- The pickers move from `ListBox` to `ColumnView`. Function comes before
  look and feel, per ADR-031.

## Tasks

- [ ] **T1** — add `cover_date` to `IssueRef` (`cv/models.rs:8`) and to
      the issue `field_list` (`cv/queries.rs:314`, `:335`). Derive the
      year and the month. Acceptance: a mock-server gate reads a cover
      date through `query_issue_refs`.
- [ ] **T2** — the issue picker `ColumnView`, its sorters, and its
      debounced filter. Acceptance: a probe filters a 500-row list and
      sorts on each column.
- [ ] **T3** — the series picker `ColumnView`, its multi-sorter
      (shift-click adds a tie-breaker), its filter, its cover-art pane,
      and its preview issue number. The corrected number must pick the
      issue for the book, not only the preview image.
- [ ] **T4** — "Previous Comic" in both pickers. The engine steps back
      one book, reverts its scraped count, and forces a non-cached
      rescrape so the earlier wrong choice is not reused. This is the
      only tranche that changes engine control flow
      (`engine.rs::scrape`); do it after T2 and T3.
- [ ] **T5** — the search term history (20, deduplicated, most recent
      first) and the per-search year override. The override is NEVER
      written to disk.
- [ ] **T6** — the publisher ignore lists and the series picker
      right-click menu ("Ignore Publisher", "Ignore for this session").
      The combobox fills from publishers already seen; no extra API
      call.
- [ ] **T7** — the tabbed configuration dialog: Scrape Fields,
      Behavior, Search Filters, Publisher Aliases, Publishers, Advanced,
      Manual. The raw text box moves to Manual behind an "Enable manual
      editing" check. The `advancedSettings` string MUST still
      round-trip unchanged.
- [ ] **T8** — the welcome dialog with "don't show again", the
      finished-scraping summary, a real error dialog, and window size
      persistence.

## Traps, learned from the Phase 15 user test

Read these before writing a dialog. All four reached the user.

1. **A GTK4 window is invisible until `present()`.** The scrape
   progress window was never presented from Phase 12 until 2026-09-12.
   `scrape_probe` GATE V checks it. Add the same gate for any new
   window.
2. **A dialog needs `.transient_for(window)` and `.modal(true)`.** An
   `.application(...)` parent lets the window manager put it behind the
   main window. Use `show_report_dialog` / `show_failure_dialog` in
   `browser/shell.rs`.
3. **A worker thread must never write a UI thread-local.** It writes its
   own copy. Send over `mpsc`; let the main-thread pump write. See
   `ShellState::run_cv_job`.
4. **Long work needs a progress indicator and a cancel.** The
   status-bar lamp and the Tasks window row are the two surfaces.

## Verification

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo test --workspace` (671 pass at the phase start).
- Mock-server gates in `cr-scrape/tests/cv_mock.rs` for T1.
- `scrape_probe`, `scrapeconfig_probe`, and `scrapeprefs_probe` gain
  gates. Run them in RELEASE under Xvfb with isolated XDG paths; see
  `docs/current-status.md`.
- User test: scrape a book through both pickers, go back one book, and
  confirm the earlier choice is not reused. Write the steps into
  `docs/open-user-tests.md`.

## Open issues

None.

## Completion record

Not started.
