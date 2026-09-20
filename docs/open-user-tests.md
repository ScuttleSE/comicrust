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

`PASS` (user, 2026-09-20): verified on real data, including the
scoped-series regression below. The Comic Vine volume link resolves from
the whole library even when the scoped smart list holds no linked copy;
the book count stayed unchanged.

See `docs/phases/phase-19.md` and ADR-059 for the procedure.

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

**Diagnostic log for an incorrect group or count:** Start the app from the
repository with this command:

```sh
CR_TRACE=1 cargo run -p cr-app --release 2> /tmp/comicrust-missing.log
```

Select **Missing Issues**. Select the affected scope and click **Refresh**.
Close the app after the refresh finishes. Extract the relevant lines:

```sh
grep 'missing issues' /tmp/comicrust-missing.log
```

The group lines show matched Series, stored Volume, scope counts, blank stored
Series and Number counts, Comic Vine volume votes, cached issue counts, and
missing counts. The linked example lines show an issue that the report marks
missing although a scoped book has the same Comic Vine issue ID. These lines do
not contain file paths or book IDs.

## 23. Link Series from Cache

`PASS` (user, 2026-09-20): batch link across multiple series worked;
unselected books stayed unchanged; a linked series made no search
request and an unlinked series made at most one.

See ADR-060 for the procedure.

Open a view that contains books from two or more series. Select multiple books
from each series. Leave at least one visible book unselected. Choose **Link
Series from Cache…**. Confirm that the first selected series opens first. Pick
its volume. Confirm that all selected books in that series leave the queue and
that the next selected series opens. Continue until the batch completes.
Confirm that one final summary includes all selected series. Confirm that the
unselected book did not change.

For a series with an existing Comic Vine volume link, confirm that no search
request occurs. For an unlinked series, confirm that no more than one search
request occurs. Cancel one volume-selection dialog and confirm that the command
does not process the remaining groups.

## 17. Incoming folder setup and review views

`PASS` (user, 2026-09-20): the Incoming folder setup and review views
are confirmed working.

In Preferences, change a scratch Library folder to Incoming. Confirm the count
before conversion. Cancel once and confirm that nothing changes. Repeat and
save. Confirm that Watch stays selected and cannot be cleared. Confirm that the
books leave Library and appear under Incoming. Check All, Gap Fills,
Duplicates, Library Duplicates, Incoming Duplicates, New Series, and Needs
Review. Confirm that Duplicates contains the combined set. Restart and confirm
the same state.

## 24. Find Missing Issues in Incoming

`PASS` (user, 2026-09-20): matching, adoption, and the counts are
confirmed, including the real `2000 AD` number `2498` retest.

See ADR-061 for the procedure.

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

Partial pass on 2026-09-19. See ADR-063 and ADR-065. The user confirmed that
real volume 19752 now links books whose stored Number is blank from their
enabled filename-derived Proposed Number. The remaining metadata propagation
checks below are still open.

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

**PASS** (user, 2026-09-19): a book whose stored Number was blank linked from
its enabled filename-derived Proposed Number during cached series propagation.
The stored Number stayed blank. The disabled Enable Proposed case still needs a
user check.

## 26. Comic Vine cache manager

`PASS` (user, 2026-09-20): both API modes verified against a real
volume — metadata persists across reopen, the API update creates and
fills the volume, and a canceled Complete Update resumes to zero
remaining details with no library book changed.

See `docs/phases/phase-20.md` and ADR-064 for the procedure.

Open **File > Manage Comic Vine Cache...**. Enter a volume ID from the imported
MCL file and select **Search**. Confirm that the dialog shows the issue IDs and
numbers. Change the name, publisher, and start year. Select **Save Metadata**,
close the dialog, reopen it, and confirm that the values persist.

Use an API key and an ID that is not in the local cache. Select **Update from
API**. Confirm that the dialog creates the volume, shows all volume metadata,
and shows the complete issue-ID and number list. Confirm that issue titles and
cover dates stay blank when no earlier complete detail exists.

Select **Complete Update from API**. Confirm that titles and cover dates fill
as issue details arrive. Cancel before a multi-issue volume completes. Close
and reopen the dialog, select the same ID, and confirm that it shows unfinished
issue details. Select **Complete Update from API** again. Confirm that it
resumes the unfinished issues and reaches zero remaining details. Confirm that
no library book changes.

## 27. Update Comic Vine Cache — pre-flight and page cap

`PASS` (user, 2026-09-20): the pre-flight counts, a capped run, and a
run-to-completion are confirmed; no library book changed.

See ADR-075 for the procedure.

Always test against a copy of the cache, never the live library.

Open **File > Update Comic Vine Cache...** with an API key set. Confirm the
pre-flight dialog appears and shows, per resource (publishers, people, volumes,
issues), the number of rows changed since the cache last synced that resource,
plus a rough time estimate and a "resumable" note. Confirm the "Stop after N
pages per resource" field defaults to 20 (or the last value you chose).

Set the cap to 1 and select **Start**. Confirm the run stops after one page per
resource and the report says the page cap was reached. Reopen the command and
confirm the pre-flight now shows fewer remaining rows (the watermark did not
advance for a capped resource, but the resume offset moved). Confirm no library
book changes.

Set the cap to 0 (run to completion) on a small window and select **Start**.
Watch the cache request log. Confirm the run fills `date_last_updated` on the
touched rows and that every resource reports "complete". If the rate limit is
reached, confirm the progress line reads that it is waiting until a clock time,
and that the run is resumable.
