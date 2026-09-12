# Phase 16: Comic Vine scraper quality of life

## Status

Planned. Phase 15 comes first.

## Goal

Port the usability work from the `Fableton/comic-vine-scraper-ce` fork of
the Comic Vine Scraper plugin. The port took its scraper from the
upstream v1.0.102 release, so none of the fork's changes are present. The
Phase 15 cache makes the cover art and the date columns cheap.

## Scope

- Issue picker: a `ColumnView` with Issue, Title, Year, and Month
  columns, a sort on every column, and a filter row.
- Series picker: a `ColumnView` with Series, Year, Issues, and Publisher
  columns, a shift-click tie-breaker sort, a filter row, a cover-art
  pane, and an editable issue number for the preview.
- A "Previous Comic" button in both pickers.
- Search dialog: a history of the last 20 search terms, a prefill from
  the current terms, and a year-range override for one search.
- An ignore list for publishers, both persistent and session-only, with
  a right-click command in the series picker.
- A tabbed configuration dialog with an information button on each tab.
- Three flags that the code stores but never reads today: the welcome
  dialog, the summary dialog, and a real error dialog.
- Window size persistence for the scraper dialogs.

## Exclusions

The fork holds four changes that this port does not need.

- The appearance scale slider (75% to 150%). It is a WinForms
  workaround. GTK takes the text scale from the desktop and from CSS.
- The Ctrl+Backspace fix. A GTK `Entry` already deletes the previous
  word.
- The `TableLayoutPanel._can_change_page` crash fix. WinForms only.
- The "Configure… loads an unbuilt copy" fix. An Ant packaging defect.

The fork's open roadmap item is also out of scope: a marker for a
collected edition in the series results. The API documentation exposes no
such field. The item goes to `docs/backlog.md` for research first.

## Locked decisions

- ADR-031 — the work stays in `cr-scrape` and the `cr-ui` scrape
  dialogs.
- ADR-033 — new configuration goes to `[plugins.comic-vine-scraper]`.
- The pickers move from `ListBox` to `ColumnView`. Function comes before
  look and feel, per ADR-031.

## Tasks

- [ ] T1 — add `cover_date` to `IssueRef` and to the `field_list` of the
      issue queries. Derive the year and the month.
- [ ] T2 — the issue picker `ColumnView`, its sorters, and its filter.
- [ ] T3 — the series picker `ColumnView`, its multi-sorter, its filter,
      its cover-art pane, and its preview issue number. The corrected
      number picks the issue for the book.
- [ ] T4 — "Previous Comic" in both pickers. The engine steps back one
      book, reverts its count, and forces a fresh scrape.
- [ ] T5 — the search term history and the per-search year override. The
      override is never written to disk.
- [ ] T6 — the publisher ignore lists and the series picker right-click
      menu.
- [ ] T7 — the tabbed configuration dialog. The `advancedSettings`
      string must still round-trip without a change.
- [ ] T8 — the welcome dialog, the summary dialog, the error dialog, and
      the window size persistence.

## Verification

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo test --workspace`.
- The `scrape_probe`, `scrapeconfig_probe`, and `scrapeprefs_probe`
  examples get new gates.
- User test: scrape a book through both pickers, go back one book, and
  confirm the earlier choice is not reused.

## Open issues

None.

## Completion record

Not started.
