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
