# Guide: Incoming folders

`CODE-READ`: An Incoming folder is a temporary holding area for new comic
files. Incoming books stay outside the main Library until you adopt or discard
them.

## Before you start

`CODE-READ`: Adoption uses a Library Organizer profile in Move mode.

1. Open **Tools > Configure Library Organizer...**.
2. Create or select a profile.
3. Set **Mode** to **Move**.
4. Set the base folder to a folder in your main library.
5. Configure the folder and file templates.
6. Save the profile.

Use Preview Adoption before the first real move.

`CODE-READ`: Preview shows the planned paths and does not move files.

## Add an Incoming folder

1. Open **Preferences**.
2. Select **Libraries**.
3. Select **Add Folder...**.
4. Select the folder that receives new books.
5. Change its role from **Library** to **Incoming**.
6. Select **OK**.

`CODE-READ`: An Incoming folder is always monitored. Its **Watch** option stays
selected and cannot be cleared. You can configure more than one Incoming
folder.

`CODE-READ`: If the folder already has records in the main Library, ComicRust
shows the number of affected records. Select **Move and Save** to transfer them
into Incoming. Select **Cancel** to keep the current folder roles and records.

`MEASURED`: The transfer keeps complete metadata, book IDs, and saved-list
references.

## Add books to Incoming

Put supported comic files in an Incoming folder.

`CODE-READ`: ComicRust monitors the folder and its subfolders. New files enter
the separate Incoming catalog. They do not enter the main Library.

Select **Incoming** in the library navigator to review the books.

## Review Incoming books

`CODE-READ`: Incoming has five main dynamic views. Duplicates has two child
views.

- `CODE-READ`: **All** shows every unresolved Incoming book.
- `CODE-READ`: **Gap Fills** shows books that can fill a gap in a Library series.
- `CODE-READ`: **Duplicates** shows books that match another Incoming or
  Library record.
- `CODE-READ`: **Library Duplicates** shows Incoming books that match a Library
  record.
- `CODE-READ`: **Incoming Duplicates** shows Incoming books that match another
  Incoming record.
- `CODE-READ`: **New Series** shows series that do not occur in the main
  Library.
- `CODE-READ`: **Needs Review** shows books that have no useful match.

`CODE-READ`: A book can occur in more than one view. A book that matches both
catalogs occurs in both duplicate child views and the combined Duplicates view.

`CODE-READ`: Series matching uses Series, Volume, Format, and Language.
ComicRust uses values from the file name when stored metadata is empty.

## Compare copies

1. Select one or more Incoming books.
2. Right-click the selection.
3. Select **Compare**.

`CODE-READ`: The report shows each selected Incoming book. It shows matching
copies from Incoming and the Library in separate sections. It includes issue
details, page counts, and file paths.

## Preview adoption

1. Select one or more Incoming books.
2. Right-click the selection.
3. Select **Preview Adoption**.
4. Select a Library Organizer Move profile.
5. Confirm the profile selection.
6. Review the Organizer report.

`MEASURED`: Preview uses simulation mode. It does not move files or transfer
records.

## Adopt books

1. Select one or more Incoming books.
2. Right-click the selection.
3. Select **Adopt**.
4. Select a Library Organizer Move profile.
5. Confirm the profile selection.
6. Resolve destination conflicts if ComicRust finds them.
7. Wait for the Organizer operation to finish.

`MEASURED`: After a successful adoption, ComicRust moves the file to the
Organizer destination. It transfers the complete record into the main Library.
Its ID and metadata stay unchanged.

`CODE-READ`: The adopted book disappears from all Incoming views.

`CODE-READ`: ComicRust remembers the selected profile. Failed, skipped, and
canceled books stay in Incoming.

`CODE-READ`: The destination-conflict dialog lets you cancel the conflict,
rename the new file, or replace the existing destination.

## Undo an adoption

Open **Tools > Library Organizer - Revert Last Move**.

`CODE-READ`: For an adopted book, Undo moves the file to its original Incoming
path. It also transfers the same record back into Incoming. The ID and metadata
stay unchanged.

`CODE-READ`: Undo applies to the last successful Library Organizer operation.
A later successful Organizer operation replaces the current undo state.

## Discard books

1. Select one or more Incoming books.
2. Right-click the selection.
3. Select **Discard**.
4. Select **OK** to move the files to trash.

`CODE-READ`: Trash is the default. To bypass trash, select **Delete permanently
(do not use the trash)** before you select **OK**. Permanent deletion cannot be
undone.

`CODE-READ`: ComicRust removes an Incoming record only after file deletion
succeeds. A failed deletion keeps the record and shows a failure report.

## Refresh Comic Vine gaps

`CODE-READ`: Opening or refreshing Incoming does not contact Comic Vine.
Normal Incoming views use only data that is already in the Comic Vine cache.

To update selected series:

1. Select Incoming books that have Comic Vine series IDs.
2. Right-click the selection.
3. Select **Refresh from Comic Vine**.
4. Wait for the cache operation to finish.
5. Review **Gap Fills** again.

`CODE-READ`: Books without a Comic Vine series ID cannot request a series
refresh.

## Remove an Incoming folder

Resolve all books below the Incoming folder before you remove its role. Adopt
the books that you want to keep. Discard the other books.

`CODE-READ`: ComicRust blocks role removal while unresolved records remain.

After you resolve all books:

1. Open **Preferences > Libraries**.
2. Change the role to **Library**, or select the folder and select **Remove**.
3. Select **OK**.

## Recommended routine

1. Put new books in an Incoming folder.
2. Review **Gap Fills**.
3. Review **Duplicates** and compare the copies.
4. Review **New Series**.
5. Inspect the books in **Needs Review**.
6. Preview the adoption paths.
7. Adopt the books that you want to keep.
8. Discard the other books.
9. Confirm that **Incoming > All** is empty.

## Storage and recovery

`MEASURED`: The main Library stays in `ComicDb.xml`. Incoming records use the
separate `IncomingDb.xml` catalog under the ComicRust data directory.

`CODE-READ`: ComicRust records adoption, undo, discard, conversion, and
Incoming scans in a durable journal. If ComicRust stops during one of these
operations, startup completes the recorded operation before it starts folder
monitoring.
