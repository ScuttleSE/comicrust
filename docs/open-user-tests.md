# Open user tests

Every test here needs a person. A successful build does not prove UI
behavior. Build once and use that binary for all tests:

```sh
cargo build -p cr-app --release
./target/release/comicrust
```

Back up `~/.local/share/comicrust/ComicDb/ComicDb.xml` before a test that
changes the library.

## 1. Library-tree gauge badges

Open lists, folders, and the Library root. Confirm that green Total, orange
Unread, and red New badges match known counts. A zero count must be hidden.
Read a book to 100%, then delete one book. Confirm that the badges update.
Restart and confirm that the last counts appear before the refresh completes.

## 2. Library-tree drag and drop

Move a reading list into a folder. Move a list above another list. Move a
list to the empty area below the tree. Confirm each resulting position.
Confirm that the Library row cannot move. Confirm that a folder cannot move
into its own descendant. Restart and confirm that the order persists.

## 3. Library-tree folder sort

Right-click a folder. Confirm that Sort is between Rename and Delete. Select
Sort. Confirm that folders come first and lists follow in name order. Confirm
that a list has no Sort command. Restart and confirm that the order persists.

## 4. Select Worst Duplicates and Incoming path

Create duplicate pairs with different formats, sizes, page counts, and
timestamps. Confirm that Show Duplicates limits the list. Run Select Worst
Duplicates with all rules enabled, with the CBR rule disabled, and with all
rules disabled. Confirm the ADR-048 tie-break. Identical copies must remain
unselected. Set `DuplicatesIncomingPath`. Confirm that the copy under that
path is selected when a copy outside it exists. Remove the selection.

## 5. Keyboard navigation and visibility

In Details view, test Down and PageDown from the middle of the Library. In
Thumbnails view, hold Down, Up, PageDown, and PageUp. Confirm that the
selected book stays visible. Change the view mode and confirm that the
selected book stays visible.

## 6. Watch-folder removal

In Preferences, select a watch folder and remove it. Select Cancel and
confirm that nothing changes after restart. Remove it again and select OK.
Confirm that it stays absent, existing books remain, and new files in that
folder are not scanned.

## 7. Permanent delete in the browser

Open Remove from Library. Confirm that both check boxes start clear each
time. Confirm that permanent delete is disabled until file deletion is
selected. Test trash deletion and permanent deletion. Confirm the file and
library results. Select Cancel and confirm that nothing changes.

## 8. Files-view deletion and failure handling

In Files view, use Move to Recycle Bin with permanent deletion enabled.
Confirm that the file does not enter the trash. Force deletion to fail with
a file in a directory without write permission. Confirm that the book stays
in the library and that the failure message appears. Repeat the failure in
the browser flow.

## 9. Library Organizer simulation

Open Library Organizer for a book. Confirm the Default profile and templates.
Select Simulate and a scratch base folder. Run it. Confirm that the report
shows the planned operations and that no file moves.

## 10. Library Organizer move and conflicts

Move a few books with Library Organizer. Confirm the template paths, updated
library paths, and `~/.config/comicrust/plugins/library-organizer/undo.dat`.
Test an existing destination. Confirm Cancel, Rename, and Replace. Rename
must create ` (1)`. Replace must trash the old file and preserve its read
percentage on the moved book.

## 11. Library Organizer undo and profile exchange

Select Library Organizer - Revert Last Move. Confirm that books return and
the undo file disappears. A second undo must report Nothing to Undo. Import
an existing `losettingsx.dat`, correct Windows paths, and export a profile.
Open the export in Windows ComicRack and confirm the round trip.

## 12. Details thumbnails, Ctrl+A, and Delete

Scroll Details view and confirm that it creates no thumbnail-cache files.
Switch to Thumbnails and confirm that covers appear. Press Ctrl+A and confirm
that all books select. Press Delete and confirm that Remove Books opens.
Cancel, then confirm that no book was removed.

## 13. macOS archive-junk recovery

Open a previously broken CBR or CBZ that contains `__MACOSX` entries. Confirm
that its thumbnail appears and page 1 is a real page. Confirm that a book
already in the library recovers without a rescan.

## 14. Smart-list dialog responsiveness

Run the smart-list creation flow that previously paused for 25 to 30 seconds.
Use `CR_TRACE=1`. Confirm that OK responds quickly. Confirm one
`nav: fire_selected` line and one `smartlist:` block with no repeated cycle.

## 15. Detail-column text overflow

Narrow a Details column that contains long text. Confirm that the text ends
in an ellipsis and does not paint over the next column.

## 16. Per-list view settings

Give two lists different view modes and sizes. Switch between them and
confirm that each setting returns. Create a list and confirm that it inherits
the current view. Reset one list's view settings. Restart and confirm that
the saved and reset states persist.

## 17. Incoming folder setup and review views

In Preferences, change a scratch Library folder to Incoming. Confirm the count
before conversion. Cancel once and confirm that nothing changes. Repeat and
save. Confirm that Watch stays selected and cannot be cleared. Confirm that the
books leave Library and appear under Incoming. Check All, Gap Fills,
Duplicates, Library Duplicates, Incoming Duplicates, New Series, and Needs
Review. Confirm that Duplicates contains the combined set. Restart and confirm
the same state.

## 18. Incoming adoption, comparison, and undo

Select an Incoming-only duplicate and a duplicate that has a Library copy.
Confirm that Compare shows two books side by side with both covers and details.
Use the book controls to move through the selected books. Use the match controls
to move through all Incoming and Library matches. Confirm that a missing cover
shows a placeholder and that the window stays responsive while covers load.
Run Preview Adoption and confirm that no file or catalog changes. Run Adopt with
a Move profile. Confirm the destination, preserved metadata, and removal from
Incoming. Test Cancel, Rename, and Replace for a destination conflict. Run
Library Organizer - Revert Last Move. Confirm that the same record and ID return
to Incoming at the original path.

## 19. Incoming discard and Comic Vine refresh

Discard one Incoming book to trash. Discard another with permanent deletion.
Force one delete failure and confirm that its record remains. Select books with
Comic Vine series IDs and run Refresh from Comic Vine. Confirm that only the
explicit command uses the network and that the Gap Fills view updates. Confirm
that opening and refreshing Incoming does not start a Comic Vine request.

## 20. Incoming responsiveness and role protection

Use a large Incoming catalog. Switch through all review views, open Compare,
and change folder roles while observing the window. Confirm that the UI stays
responsive. Try to remove an Incoming folder that still has unresolved books.
Confirm that ComicRust blocks the change. Resolve the books and remove the role.
