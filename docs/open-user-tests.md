# Open user tests

Every test here needs a person. A successful build does not prove UI
behavior. Build once and use that binary for all tests:

```sh
cargo build -p cr-app --release
./target/release/comicrust
```

Back up `~/.local/share/comicrust/ComicDb/ComicDb.xml` before a test that
changes the library.

On 2026-09-18 the user ran a full pass. The tests that passed are removed from
this file. The headings keep their original numbers so cross-references stay
valid. Only the open items remain: the failures, the passable item, and the
partial pass.

## 1. Library-tree gauge badges

`FAIL` on 2026-09-18 (user): Only the red (New) and green (Total) badges
appear. The orange (Unread) badge never renders. The red and green badges
always show the same number. See Outstanding issue in `docs/current-status.md`.

Open lists, folders, and the Library root. Confirm that green Total, orange
Unread, and red New badges match known counts. A zero count must be hidden.
Read a book to 100%, then delete one book. Confirm that the badges update.
Restart and confirm that the last counts appear before the refresh completes.

## 4. Select Worst Duplicates

`PASSABLE` on 2026-09-18 (user): Works but needs some improvement. The
specific improvement is not yet named. See Outstanding issue in
`docs/current-status.md`.

Create duplicate pairs with different formats, sizes, page counts, and
timestamps. Confirm that Show Duplicates limits the list. Run Select Worst
Duplicates with all rules enabled, with the CBR rule disabled, and with all
rules disabled. Confirm the ADR-048 tie-break. Identical copies must remain
unselected.

## 9. Library Organizer simulation

`FAIL` on 2026-09-18 (user): Simulate ran but no report appeared. See
Outstanding issue in `docs/current-status.md`.

Open Library Organizer for a book. Confirm the Default profile and templates.
Select Simulate and a scratch base folder. Run it. Confirm that the report
shows the planned operations and that no file moves.

## 16. Per-list view settings

`PASS` on 2026-09-18 (user): Two smart lists keep separate view modes when the
user switches between them. Per-list isolation works for smart lists.

Give two lists different view modes and sizes. Switch between them and
confirm that each setting returns. Create a list and confirm that it inherits
the current view. Reset one list's view settings. Restart and confirm that
the saved and reset states persist.

## 22. Missing Issues gap report

Not yet run. See `docs/phases/phase-19.md` and ADR-059. A 2026-09-19 user
test found and fixed a real bug in the scoped case (below) — the fixed
scenario still needs a user confirmation on real data.

Import a Comic Vine MCL file (Preferences ▸ Comic Vine Scraper, or the
Import Comic Vine MCL File command) for a series already in the library.
Open the Missing Issues navigator entry. Confirm it opens as a plain
details list with no thumbnails and a Refresh button. Click Refresh with
the scope set to Whole Library and confirm the details list shows the
gaps (Series, Number, Title, Year, Comic Vine Issue Id columns) for the
imported series. Create or pick a smart list that selects only that
series, switch the scope to it, click Refresh again, and confirm the row
set narrows to that series' gaps. Confirm the library's book count is
unchanged after both refreshes — this view must never add a book.

**Scoped-series regression check** (the exact bug a 2026-09-19 user test
found): pick a series where only SOME of your owned copies have ever been
scraped/linked to Comic Vine. Build a smart list that selects only the
UN-linked copies of that series (e.g. by file path, format, or another
distinguishing matcher — not by series name alone, since that would also
match the linked copies). Scope Missing Issues to that smart list and
Refresh. Confirm it reports the series' real gaps, not "0 missing" — the
Comic Vine volume link must resolve from the whole library even when the
scoped smart list itself holds no linked copy.

## 23. Link Series from Cache

Not yet run. See ADR-060. Built in response to a real case found while
testing #22: a series with an MCL-imported cache but no book ever linked to
Comic Vine.

Pick a series with an MCL-imported cache and NO book yet linked to Comic
Vine (check the Properties/custom-values of a few books, or just try a
series you know you've never scraped). Right-click one book of that series
and choose "Link Series from Cache…". Confirm exactly one Comic Vine
request fires (check Preferences ▸ Comic Vine Scraper's request log, or a
network trace) and that a "pick the volume" dialog appears. Pick the
correct volume and confirm a summary reports N of M books linked. Open
Missing Issues, scope it to that series, Refresh, and confirm real gaps now
show. Then: narrow the browser to a smart list or a quick search that only
shows SOME of that series' books, right-click one of the still-unlinked
copies, and confirm only the currently visible books get linked — copies
outside the view must stay untouched. Finally, re-run the command on a
series where a book already carries the link and confirm no Comic Vine
request fires at all (the existing vote is reused).

## 17. Incoming folder setup and review views

`PARTIAL PASS` on 2026-09-18 (user): "So far so good." No failure reported,
but the full checklist is not yet confirmed complete.

In Preferences, change a scratch Library folder to Incoming. Confirm the count
before conversion. Cancel once and confirm that nothing changes. Repeat and
save. Confirm that Watch stays selected and cannot be cleared. Confirm that the
books leave Library and appear under Incoming. Check All, Gap Fills,
Duplicates, Library Duplicates, Incoming Duplicates, New Series, and Needs
Review. Confirm that Duplicates contains the combined set. Restart and confirm
the same state.

## 24. Find Missing Issues in Incoming

Not yet run. See ADR-061.

Create a Library Organizer Move profile that targets a scratch Library folder.
Open **Preferences > Libraries**. Select that profile in **Gap Fill adoption
profile**, save, reopen Preferences, and confirm that the selection persists.

Put files for known gaps into an Incoming folder. Include two copies of one
issue and no copy of another issue. Refresh **Missing Issues**, select both
rows, right-click, and select **Find in Incoming**. Confirm that the summary
shows the matched, unmatched, and multiple-match counts. Select one of the two
copies and select **Adopt Matches**.

Confirm that the selected file moves according to the profile. Confirm that
its complete record enters the main Library and leaves Incoming. Confirm that
the unselected copy stays in Incoming. Confirm that the unmatched issue stays
in Missing Issues and that the adopted issue disappears. Test **Library
Organizer - Revert Last Move** and confirm that the adopted record and file
return to Incoming.

Open **Incoming > Gap Fills** and select one or more books. Select **Preview
Adoption**. Confirm that no profile selector appears, the configured profile is
used, and no file moves. Select **Adopt**. Confirm that a summary shows the
selected count and profile before the move. Confirm that other Incoming views
still show the general profile selector.

During Preview, Adopt, or another Library Organizer run, confirm that the main
progress area shows preparation, current source and destination, and completed
rows. Confirm that the bottom progress count continues to update.

## 25. Cached series metadata propagation

Not yet run. See ADR-063.

Choose a series that has several owned issues with blank Publisher, Imprint,
and volume year. Keep nonblank test values on at least one other issue. Fully
scrape one previously unlinked issue from that series. Confirm that the cache
task appears after the scrape. Confirm that all matching books in the library
receive the blank shared fields. Confirm that matching issue numbers also
receive Comic Vine volume and issue IDs. Confirm that the nonblank test values
do not change.

Clear a blank shared field and the Comic Vine IDs on one issue. Narrow the
current view so that it contains this issue but excludes another unlinked issue.
Run **Link Series from Cache**. Confirm that the visible issue receives the
blank shared field and both IDs. Confirm that the excluded issue does not
change. Confirm that the summary reports linked, metadata-filled, changed, and
unmatched counts.
