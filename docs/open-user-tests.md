# Open user tests

Every test below waits for a person. A build that passes is not proof
that the user interface behaves.

`docs/current-status.md` names which of these are open. This file holds
the steps.

## Before you start

Build once and use that binary for every test.

```sh
cd /home/scuttle/Downloads/repo/comicrust
cargo build -p cr-app --release
./target/release/comicrust
```

A test that says "restart" means: close the window, wait for the
process to end, then start the binary again.

The files the tests refer to:

| What | Where |
|---|---|
| Configuration | `~/.config/comicrust/comicrust.toml` |
| Library database | `~/.local/share/comicrust/ComicDb/ComicDb.xml` |
| Comic Vine cache | `~/.local/share/comicrust/plugins/comic-vine-scraper/cvcache.sqlite` |
| Scrape history | `~/.config/comicrust/plugins/comic-vine-scraper/prior_series.json` |

**Back up `ComicDb.xml` before test 5, 9, 12, or 15.** Those tests scan
or write the library. A copy costs nothing.

```sh
cp ~/.local/share/comicrust/ComicDb/ComicDb.xml ~/ComicDb.xml.backup
```

For every test, report three things: what passed, what failed, and what
you could not tell.

---

## 1. Comic Vine cache (Phase 15, ADR-037, ADR-038)

You need a Comic Vine API key and an `.mcl` snapshot file.

1. **File ▸ Import Comic Vine MCL File…** Pick the `.mcl` file.
   *Expect:* a report with the volume count, the issue count, the
   snapshot date, and the number of skipped lines.
   *Report:* the four numbers.
2. **File ▸ Update Comic Vine Cache.**
   *Expect:* a sweep from the snapshot date to today, then a report of
   the pages, issues, and volumes, and "The window is complete."
   *Report:* the counts and how long it took.
3. Run **Update Comic Vine Cache** again at once.
   *Expect:* "The cache is already current for today."
4. Scrape ONE book of a large series (a series with more than 50
   issues). Watch the bottom of the scrape window.
   *Report:* the "budget N/200 per hour" number before and after.
5. Scrape a SECOND book of the SAME series.
   *Expect:* it is much faster, and the budget falls by far fewer.
   *Report:* the budget numbers again.
6. Open **Preferences ▸ Comic Vine Scraper**, and put
   `CACHE_RATE_LIMIT=3` on its own line in the advanced settings box.
   Press OK. Scrape a book.
   *Expect:* after three requests the window says "budget spent,
   resuming at HH:MM". It must NOT stall with no word, and it must NOT
   fail with an error burst.
   **Then remove that line and press OK again.**
7. **File ▸ Warm Comic Vine Cache.**
   *Expect:* a report of volumes looked at, read, already fresh,
   failed, and requests spent.
   *Report:* the five numbers.
8. Run **Warm Comic Vine Cache** again at once.
   *Expect:* almost every volume is "already fresh", and the requests
   spent is near zero.
9. Right-click a book of a series you scraped ▸ **Fill Missing
   Issues…**.
   *Expect:* a list of the issues the library does not hold, each with
   its number, its year, and its title. Some rows are ticked.
10. Untick a few rows, then press **Create Books**.
    *Expect:* new fileless books appear in the grid, already selected,
    with the right series, volume number, and issue number. Only the
    ticked rows became books.
11. Right-click a book of a series you have NEVER scraped ▸ **Fill
    Missing Issues…**.
    *Expect:* a message that no book names a Comic Vine volume. No book
    is created.

---

## 2. Navigator tree state (2026-09-12)

1. In the navigator on the left, expand two or three folders.
2. Restart.
   *Expect:* the same folders are expanded.
3. Collapse them. Restart.
   *Expect:* they are collapsed.
4. Make a new folder in the navigator.
   *Expect:* it starts expanded.

---

## 3. Detail column add and remove (2026-09-12)

1. Switch the browser to **Detail** view.
2. Right-click the column header and uncheck **Opened**.
   *Expect:* the column goes away at once.
3. Right-click the header again.
   *Expect:* the "Opened" row is unchecked.
4. Check "Opened" again from the **All** page of that menu.
   *Expect:* the column comes back.
5. Remove it again, then check it from its letter page (**G-O**).
   *Expect:* the column comes back.
6. Restart.
   *Expect:* your choice held.

---

## 4. Right-click rescans (Phase 14, ADR-036)

This test needs a book that failed its scan. Test 5 makes them.

1. Select one book that carries a red "!" chip. Right-click ▸ **Rescan
   Book File(s)**.
   *Expect:* the book re-reads. A file that is still bad re-marks with
   a fresh verdict, and a summary window appears.
2. Right-click a book that is NOT selected.
   *Expect:* only that book becomes selected.
3. Select three books. Right-click one of the three.
   *Expect:* all three stay selected. Check that **Edit**, **Update
   Book File(s)**, **Export**, **Remove from Library**, and
   **Properties** act on all three.
4. Right-click a smart list in the navigator ▸ **Scan List Contents**.
   *Expect:* the list's books are scanned, and one report appears at
   the end.
5. Right-click the **Library** root, then a plain folder.
   *Expect:* "Scan List Contents" does NOT appear on either.
6. Switch to the **Files** view and repeat step 2 and step 3 there.
   *Expect:* the same selection behaviour.

---

## 5. Scan robustness and problem markers (ADR-034, ADR-035)

Back up `ComicDb.xml` first.

1. Rescan the real library.
   *Expect:* it runs to the end with no stall. The files that used to
   take minutes each now take under a second.
   *Report:* the total time, and whether any single file took more than
   a few seconds.
2. Look at the grid.
   *Expect:* a book that could not be read carries a red "!" chip at
   the top left of its cover. A book whose content does not match its
   file name carries an amber "≠" chip AND still shows its pages.
3. Hover over a chip.
   *Expect:* the tooltip gives the verdict, the format disagreement,
   and the reason.
4. *Expect:* one summary window at the end of the scan.
5. Make a smart list. Paste this query:
   `Match [Custom Value] regex "comicrust.scan.status" "."`
   *Expect:* it lists exactly the books that carry a chip.
6. Repair or replace one bad file. Rescan.
   *Expect:* its chip disappears with no other action.
7. Start a scan of a large folder. Click the scan lamp in the status
   bar, and press **Skip current file**. (The same row is in the Tasks
   window.)
   *Expect:* the scan moves on, and the skipped book is marked
   "Skipped".

---

## 6. Double-click open crash fix (commit `140ba4c`)

1. Double-click a book in the grid.
   *Expect:* the reader opens. The app does not abort.
2. Read some pages, then close the reader tab.
   *Expect:* the green read-ribbon moves in the grid at once, with no
   second click.

---

## 7. Phase 13 config unification (ADR-033)

1. Start the app.
   *Expect:* `~/.config/comicrust/comicrust.toml` exists and holds a
   `[settings]` section and a `[data.imprints]` section.
   *Note:* the old `Config.xml`, `comicrust.ini`, and the plugin
   `settings.json` stay on disk and are ignored. Delete them by hand at
   any time.
2. Re-enter the preferences you use: the API key on **Preferences ▸
   Comic Vine Scraper**, the theme, the quick-open size, and the cache
   sizes. Press OK. Restart.
   *Expect:* every value persists.
3. Toggle **Browse ▸ Dark Mode**, and change the cache-folder row on
   **Preferences ▸ Advanced**. Restart.
   *Expect:* both persist.
4. Close the app. Add a line such as `"My Imprint" = "DC Comics"` under
   `[data.imprints]` in the TOML file by hand. Start the app. Scrape a
   book whose Comic Vine publisher is "My Imprint".
   *Expect:* the publisher resolves to "DC Comics", and the imprint is
   recorded. No rebuild is needed. A changed or removed line applies
   the same way.
5. Run a scrape end to end.
   *Expect:* it uses the API key from
   `[plugins.comic-vine-scraper]`, and `prior_series.json` stays under
   `plugins/comic-vine-scraper/`.
6. Check `ComicDb.xml`.
   *Expect:* its bytes and its modification time did not change.

---

## 8. Config seed and reference document (commit `d5c52b3`)

1. Move the config file away, then start the app:
   `mv ~/.config/comicrust/comicrust.toml ~/comicrust.toml.old`
   *Expect:* the new file holds EVERY `[extended]` and `[engine]` key
   at its default value.
2. Close the app. Set `DatabaseBackgroundSaving = 60` in the file.
   Start the app and run a scan.
   *Expect:* the database saves every minute during the scan.
3. Close the app. Delete one key line. Start the app.
   *Expect:* the key returns at its default.
4. Pick five keys at random from the file and look them up in
   `docs/config-reference.md`.
   *Expect:* every one is documented.

---

## 9. Mid-scan background save (commit `d9262a4`)

Back up `ComicDb.xml` first.

1. Delete `~/.local/share/comicrust/ComicDb/ComicDb.xml`, then start a
   scan of a large folder.
2. Wait about 10 minutes. Do NOT close the app.
   *Expect:* `ComicDb.xml` exists on disk and holds the books found so
   far.

---

## 10. Smart-list rule delete and clipboard

Run this on a real desktop, not under Xvfb. Xvfb stalls the clipboard
reads.

1. Open a smart list editor.
   *Expect:* every rule row and every group carries a small ▾ button at
   its right edge.
2. Open that button.
   *Expect:* New Rule, New Group, Delete, Cut, Copy, Paste, Move Up,
   and Move Down. Each is enabled only when it can act. Report any row
   that is enabled but does nothing.
3. Press **Delete** on a rule.
   *Expect:* the rule goes.
4. Press **Copy** on a rule, then **Paste**.
   *Expect:* a clone appears.
5. Press **Cut** on a rule, then **Paste** somewhere else.
   *Expect:* the rule moves.
6. Switch the editor to the Query view and back.
   *Expect:* the query text matches the rules, and nothing is lost.

---

## 11. "No metadata" tag

1. Look at the grid in **Thumbnail** and in **Tile** view.
   *Expect:* a book whose scan found no metadata carries a small dark
   "?" chip at the top left of its cover.
2. Open **Properties** on one of those books, fill in a key field such
   as Series, and save.
   *Expect:* the chip goes.
3. Scrape another one from Comic Vine.
   *Expect:* the chip goes.

---

## 12. Scan and open metadata import

Back up `ComicDb.xml` first.

1. Rescan a folder that holds magazines with `ComicInfo.xml` inside
   them.
   *Expect:* the NEW files carry series, title, writer, and page
   metadata.
2. Add a file through **File ▸ Open File…**.
   *Expect:* it carries its metadata too.
3. Open **Properties** on a comic that is not in the library.
   *Expect:* it shows that comic's metadata.
4. Look at books that were imported as empty before this change.
   *Expect:* they stay empty. There is no backfill; that was your
   decision.

---

## 13. Detail view round (commits `c21086a`, `f20a690`)

1. Switch the browser to **Detail**.
2. Drag the size slider in the status bar ONCE, then let go.
   *Expect:* the text size and the row rhythm match ComicRack. The
   saved `ItemRowHeight` value of 48 is an artifact that this one drag
   clears, and the slider re-ranges to 12..48.
3. Look at the rows.
   *Expect:* they alternate grey and white, starting with grey, and a
   selected row keeps its highlight over the stripe.
4. Look at the column separators.
   *Expect:* thin vertical lines run through the header AND the rows.
5. Right-click the column header.
   *Expect:* the 13 default rows, then **All** (alphabetical), then the
   letter pages **A-B**, **C-F**, **G-O**, **P-R**, **S**, **T-Y**.
6. Toggle one column from the All page and one from a letter page.
   *Expect:* both work.
7. Open a smart list editor.
   *Expect:* a rule row picks its type from the same All and letter
   menus.

---

## 14. Group, lamp, and thumbnail batch

1. Set **Group by Series** in **Thumbnail**, **Tile**, and **Detail**
   view.
   *Expect:* header strips appear with true counts.
2. Single-click the disclosure triangle of one group.
   *Expect:* ONE group collapses or expands.
3. Double-click a disclosure triangle.
   *Expect:* ALL groups collapse or expand.
4. Use the collapse-all and expand-all row in the **Views** menu.
   *Expect:* it works, and it greys out when no grouping is set.
5. Start a scan.
   *Expect:* the scan lamp animates. A click on it opens the "Cancel
   scan" menu.
6. Turn thumbnails off on **Preferences ▸ Advanced**.
   *Expect:* placeholders appear.
7. Run **File ▸ Generate Cover Thumbnails**.
   *Expect:* the covers fill in.

---

## 15. Scan control

Back up `ComicDb.xml` first.

1. Start a real scan. Open **Tasks** and press **Abort Scanning**.
   *Expect:* the scan stops.
2. Start another scan. Mid-scan, close the window.
   *Expect:* the app exits promptly, and it does not hang.
3. Start the app again.
   *Expect:* the books found before the exit are there.
4. Start a scan again, and press Ctrl+C in the terminal that runs the
   app.
   *Expect:* the same prompt exit, and the same kept books.

*Already passed on 2026-09-10:* the progressive fill and the
no-glitch append.

---

## 16. Export freeze fix

1. Select a `.cbr` book ▸ **Export…** ▸ Format = eComic (ZIP) ▸ OK.
   *Expect:* the window stays responsive for the whole export, and the
   progress bar ticks. It must not freeze.

---

## 17. Write-back fix

1. Open **Properties** on a `.cbr` or `.cb7` book, change a field, and
   save.
2. Run **Update Book File(s)**.
   *Expect:* the interface stays responsive, the write lands in the
   archive, and the book leaves the "Files to update" list.

---

## 18. Phase 10 export and write-back steps

`rar` must be installed for the write-back part. `CR_RAR` is optional.

1. Open a library with a `.cbr`. Open **Properties**, change a field,
   and run **Update Book File(s)**.
   *Expect:* no error. The book leaves "Files to update".
2. Check the archive: `7z l <file.cbr>`
   *Expect:* the edited `ComicInfo.xml` is inside.
3. Take `rar` off the path (or set `CR_RAR=/nonexistent`) and repeat
   the edit and the update.
   *Expect:* a clear "rar executable not found" error, and no data
   loss.
4. Select a `.cbr` book ▸ **Export…** ▸ Target = "Replace source",
   Format = eComic (ZIP) ▸ OK.
   *Expect:* a `.cbz` sits next to the old file, the `.cbr` is in the
   trash, the library book points at the `.cbz`, it opens, its reading
   position is kept, and it left "Files to update".
5. Repeat with "Delete original files after export" UNCHECKED and
   Target = "Export to new folder".
   *Expect:* both files remain, and the library book still points at
   the `.cbr`.
6. Repeat with "Add exported files to the library" and a new folder.
   *Expect:* the library gains a SECOND book for the export.

---

## 19. Phase 11 install steps

This test needs a tagged release that the workflow already built, and
an Arch machine and a Debian 13 machine (containers are acceptable).

1. Download `comicrust-<version>-source.tar.gz` from the release. On
   Arch, extract it, run `makepkg -f` in the PKGBUILD directory, then
   `pacman -U` the package.
2. Start it from the desktop menu (the icon must show) AND from a
   terminal in an unrelated directory.
   *Expect:* icons, papers, and backgrounds render. The reader Display
   dialog lists 4 papers and 14 backgrounds.
3. `pacman -R comicrust`
   *Expect:* it removes cleanly.
4. Download `comicrust_<version>-1_amd64.deb`. On Debian 13:
   `sudo apt install ./comicrust_<version>-1_amd64.deb`
5. Repeat step 2.
6. `sudo apt remove comicrust`
   *Expect:* it removes cleanly.
7. Run the portable tarball.
   *Expect:* it still runs, and the assets beside the binary win.
