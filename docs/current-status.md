# Current status

Update this file at the end of every work session. Replace stale content.
Do not append a history. History lives in `docs/archive/` and in git.

## Active phase

**Phase 16 — Comic Vine scraper quality of life.**
File: `docs/phases/phase-16.md`. Status: PLANNED. No task started.

Phases 0-8 and 10-15 are COMPLETE. Phases 13, 14, and 15 were
user-tested on 2026-09-12 and are archived. Phase 9 is DEFERRED to
`docs/backlog.md`.

## Current task

**Side task after v0.1.0: the duplicate cleanup — Select Worst
Duplicates** (user request, 2026-09-13). A PORT ADDITION with no C#
counterpart; the semantics are ADR-044 (selection-only mark, score-sum
rules, Preferences page, fileless ranks worst).

1. **The engine ranking.** The duplicate grouping moved out of
   `match_duplicates` into `matcher::eval::duplicate_groups` (one
   shared grouping; the C# ternary-chain quirk stays in one place).
   `cr-engine/src/duplicates.rs` adds `worst_duplicate_ids`: per
   duplicate group, each enabled rule adds one penalty to a copy
   strictly worse than the group best (CBR worse only when a CBZ is
   in the group, physical format from the path extension; unknown
   size counts smallest; unknown page count lowest; a fileless copy
   loses every enabled rule). The command selects the copies whose
   penalty exceeds the group minimum — an all-tie group marks
   nothing, and the accepted score-sum consequence is that a CBR
   winning on size and pages survives a smaller CBZ.
2. **The command.** `win.select-worst-duplicates` (`ShellState::select_worst_duplicates`,
   registered in `commands.rs` with no accelerator, in the book
   context menu between "Fill Missing Issues…" and "Remove from
   Library"). It ranks over `item_view.displayed_books()` (the
   current filtered view, the `FillBookList` shape) and REPLACES the
   selection through `reselect` — an empty result clears the
   selection.
3. **The rules.** Three `Settings.Duplicates*` port-addition fields
   (default on) + a new Preferences Duplicates page (hand-built
   checkboxes, the clone-and-commit-on-OK pattern); the keys are
   documented in `docs/config-reference.md` (the `config_doc` gate
   caught the missing entries on the first run).

The prior navigator side task (drag-and-drop + Sort) is complete; its
record is in git history and its user tests 4 and 5 below are still
open.

Phase 16 T1 (add `cover_date` to `IssueRef` and the issue queries) is
unchanged and NOT STARTED.

## Previous task

**v0.1.0 is TAGGED.** The first stable release. The tag is annotated
and points at the thumbnail-warm-up fix. Publishing is manual and is
the user's step, in this order, both from the Gitea Actions UI:

1. **Tagged release** — enter `v0.1.0`. It re-runs fmt, clippy and the
   tests at the tag, then builds and publishes the portable tarball.
2. **Packaging** — enter `v0.1.0`. It must run SECOND: it attaches the
   vendored source tarball, the Arch package and the `.deb` to the
   release that step 1 created, and fails hard if that release is
   missing.

After the release is out, bump nothing by hand: ADR-043 derives the
rolling version from the newest stable tag, so the next push to main
publishes `0.1.1`.

Done 2026-09-12 (the release-readiness pass):

1. **The licence is decided — GPL-2.0-only (ADR-041, supersedes
   ADR-009).** The deciding fact is MEASURED, and it is not the code
   port: comicrust redistributes ComicRack CE artwork verbatim — 186
   icons, the paper and background textures, and the 16 `assets/pages`
   frames of `ReadPagesAnimation.gif`. Upstream `LICENSE.txt` is the
   canonical FSF GPLv2 text (sha256 `8177f975…b880643`), and upstream
   never elects "or later": no `.cs` file carries a GPL header and the
   CE README carries no licence statement. A permissive licence was
   asked for and is not available. `LICENSE` is now at the repo root,
   byte-identical to upstream. The `LicenseRef-Proprietary`
   placeholders are gone from the PKGBUILD and the metainfo;
   `Cargo.toml` carries the identifier; the `.deb` now ships
   `/usr/share/doc/comicrust/copyright` (Debian policy 12.5).
2. **The portable tarball dropped the page-lamp assets.** Both release
   workflows copied four asset kinds and omitted `pages`, so the
   ADR-040 lamp could not animate in a tarball install and degraded
   silently (`assets::find` returns `None`). The PKGBUILD and the deb
   script were already correct. Both workflows now loop over all five
   kinds, and the `.deb` content check asserts every kind plus the
   copyright file instead of `papers` alone.
3. **A tagged build stamped the wrong version into About.** `VERSION`
   was set only on the Package step, but `cr-ui/build.rs` reads it at
   COMPILE time, so a v0.1.0 tarball would have reported `0.0.369`.
   The Build step of both workflows now sets it. The rolling track was
   hiding the same bug, because its version is the commit count.
4. **The rolling build counter restarts at every stable tag
   (ADR-043, supersedes ADR-042).** The version is
   `<major>.<minor>.<commits since the newest stable tag>`, with the
   major and minor READ from that tag, so the build after `v0.1.0` is
   `0.1.1` and a `v0.2.0` tag restarts the count as `0.2.1`. ADR-042
   was wrong: it published `0.1.371` while no `v0.1.0` tag existed.
   The tag filter is a strict regex, because a git glob would accept
   `v0.1.0-rc1`; it also excludes the moving `rolling` tag and the
   legacy `v0.0.*` tags, whose patch field was a total commit count.
5. **Dependency licence audit.** `zopfli` (Apache-2.0) was REMOVED,
   not excepted. It arrived through the zip crate's `deflate`
   meta-feature, which is defined as `["flate2/rust_backend",
   "deflate-zopfli", "deflate-flate2"]` and so pulls zopfli in
   unconditionally. No code asks for zopfli compression, so the
   workspace takes `deflate-flate2` plus `flate2` instead. zip's
   default tail (aes, bzip2, lzma, xz, zstd, time) left the lock file
   with it, removing three `-sys` C builds. Two findings remain
   unresolved and are recorded in `deny.toml` as explicit exceptions —
   `ring` and `webpki-roots`, both reached through `ureq` in
   cr-scrape. `cargo deny` is therefore NOT a CI gate yet.
6. Metainfo gained homepage, bugtracker, and developer entries and now
   passes `appstreamcli validate`. It still has NO `<screenshots>`:
   that element needs hosted image URLs the project does not have.

7. **Generate Cover Thumbnails no longer freezes the app.** A Rule 9
   violation found by the user test that gated this tag; the detail is
   in the user-test section below. `statusbar_probe` gained gate M,
   which invokes the real action instead of the lamp widget.

Phase 16 T1 (add `cover_date` to `IssueRef` and the issue queries) is
unchanged and NOT STARTED.

## Open licence question (inherited by v0.1.0 knowingly)

Apache-2.0 is incompatible with GPL-2.0-only. Three dependencies sit
on that line: the `cr-scrape` port of Cory Banack's Apache-2.0 Comic
Vine Scraper, plus `ring` and `webpki-roots`. ADR-041 records the two
exits and takes neither. The cheaper one is to ask maforget to elect
"GPL-2.0-or-later" for ComicRack CE; comicrust could then move to
GPL-3.0-or-later, under which Apache-2.0 is compatible. No claim is
made that the present combination is permissible.

## Verification record

The duplicate cleanup (2026-09-13): `cargo fmt --all` and `cargo
clippy --workspace --all-targets -- -D warnings` — green.
`CR_FORMAT_TESTS=1 cargo test --workspace --locked` — 702 passed, 0
failed (692 + 10 new, all in `duplicates.rs`). `cargo build --release
--locked -p cr-app` — green (run because the change touched no
dependencies, as the Feature-unification lesson requires anyway).

New probe `duplicates_probe` (release, Xvfb :99, isolated XDG pair),
gates A to F all green: A the boot grid holds all 9 seeded books and
the Views toggle narrows it to the 8 duplicate members, B the command
selects exactly the CBR of the clear group and the fileless entry, C
with the format rule off the conflict group's smaller CBZ marks (the
score-sum consequence, ADR-044), D with every rule off nothing
selects, E the real context menu opens and its "Select Worst
Duplicates" row fires the same command, F the Preferences Duplicates
page opens, its three rows read the all-on settings, two rows flipped
and OK commit into the session AND `comicrust.toml` on disk.
`commands_probe` re-ran: RESOLVED 77/77. `browserbar_probe`,
`listorder_probe`, `deleteperf_probe` re-ran unchanged and green.

UNKNOWN, stated plainly: the selection-set order is not stable across
dispatches (HashSet iteration), so the probe compares SORTED titles.
The first gate C run failed on exactly that order; the probe
comparison was corrected, not the expectation — the set content was
correct on the failing run.

## Earlier verification record

The navigator side task (2026-09-12) verified green at the time; its
full record is in git history. Tests 4 and 5 in the User tests
section remain open.

`cargo build --release --locked -p cr-app` — the command the release
workflows run — green. `cargo fmt --all --check` and `cargo clippy
--workspace --all-targets -- -D warnings` — green.
`CR_FORMAT_TESTS=1 cargo test --workspace --locked` — 680 passed, 0
failed, unchanged from the pre-change baseline.
`appstreamcli validate` — passes (one pedantic note: the component id
contains uppercase letters, left alone because the desktop file and
the install paths depend on it). YAML parse of all four workflows —
clean.

`statusbar_probe` (release, Xvfb :99, isolated XDG dirs) — gates A
through M all green, including the new M. Re-run after the thumbnail
fix.

UNKNOWN: the `.deb` is NOT verified locally. `dpkg-deb` is absent on
this machine, so `packaging/deb/build.sh` and the new copyright file
have had only a `bash -n` syntax check and a check of the indentation
the machine-readable format needs. The packaging workflow's own
content assertions are their first real test.

### Lesson: a widget gate is not a command gate

`statusbar_probe` gate L asserted the page lamp in full — 16 frames,
hidden at rest, the timer runs only while visible, the click reaches
Tasks — and it passed on code whose Generate Cover Thumbnails command
froze the app. L drives `update_lamps` with SYNTHETIC booleans, so it
never invoked the command and never needed a live main loop.

New gate M invokes the real `win.generate-thumbnails` action and
asserts that the enqueue left the main thread, through a
`library::thumbnail_warmup_spawns` counter. Proven to catch the
defect: with the inline loop restored, M reported `spawned=false`
while L stayed all-true. The timing and heartbeat halves of M cannot
separate the two on the 3-book probe library — `spawned` is the
load-bearing assertion.

The rule: when a feature has a UI surface AND a command that drives
it, gate the COMMAND. A gate that only drives the surface directly
will pass on a broken command.

### Lesson: `cargo test --workspace` is not the release build

The first attempt at the zopfli removal passed fmt, clippy, and 680
workspace tests locally, then FAILED in CI with "unresolved import
`flate2`" inside the zip crate. Two causes, both measured:

1. `deflate-flate2` is a MARKER feature (`= ["_deflate-any"]`). It
   supplies no backend. The backend lives in `flate2/rust_backend`,
   which only the `deflate` meta-feature enabled. Removing `deflate`
   removed the backend along with zopfli.
2. `cr-engine` declared a bare `zip = "2"` DEV-dependency, which took
   zip's default features. Dev-dependencies are feature-unified into
   `cargo test --workspace`, so the workspace test run supplied the
   missing backend and compiled. `cargo build -p cr-app` excludes
   dev-dependencies and failed. The dev-dependency now points at the
   workspace entry, which closes the masking path.

The rule this produces: when a change touches FEATURES or
DEPENDENCIES, run `cargo build --release --locked -p cr-app` as well.
A green `cargo test --workspace` is not evidence about it.

Phases 13, 14, and 15 all passed their user tests on 2026-09-12. Their
records are in `docs/archive/phases/`.

Last commits, `5aea7c3` to `7cb852b` (Phase 15 and the 15a visibility
round):

- The Comic Vine cache is a two-layer SQLite file (ADR-037). MEASURED
  by mock-server gate, in API requests: a closed volume 0, an unchanged
  open volume 1 (the probe only), a changed open volume 2, an unknown
  volume 1 plus its pages.
- The sweep uses `filter=date_last_updated:<start>|<end>`. The API
  reference page renders its per-field filter marks as images, so that
  page does NOT state the filter is allowed; the production
  `update_missing.py` is the evidence.
- The budget default is 200 requests per resource per hour, from the
  user's figure. The API reference page carries NO rate-limit text at
  all, so the figure is NOT verified and the ceiling is the
  `CACHE_RATE_LIMIT` key.
- `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D
  warnings` — green.
- `cargo test --workspace` — 671 pass, 0 fail.
- Probes (release, Xvfb): `scrape_probe` GATE V and A/B/C,
  `statusbar_probe` A-K2, `scanmarker` A-E, `metadatatag` A/B.

The 2026-09-12 side task (ADR-039, ADR-040):

- `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D
  warnings` — green.
- `cargo test --workspace` — 680 pass, 0 fail (671 + 9 new: four for
  `ellipsize_to_width`, two for the `<View>` bridge, three existing
  counts unchanged).
- Probes (release, Xvfb): `statusbar_probe` A-L (L is the new page
  lamp: 16 frames, hidden at rest, the timer runs only while
  visible, the click reaches Tasks); `browserbar_probe` A-G4 (G1-G4
  are the new per-list cycle); `detailresize_probe` A-D;
  `workspace_probe`, `bootview_probe`, `bootreentry_probe`,
  `listorder_probe` — all unchanged and green.
- MEASURED constraint found while gating: the navigator selection is
  debounced 200 ms (`navigator.rs:45`). A view change made inside that
  window is attributed to the OUTGOING list. The first G-gate run
  failed on exactly this; the probe timing was corrected, not the
  expectation.

## User tests

**NEW, from the navigator side task (not yet run):**

4. **Tree drag-and-drop.** Drag a reading list onto a folder: it must
   become a child of that folder. Drag a list onto another list inside
   a folder: it must land ABOVE that list, and the order you build by
   hand must stay. Drag a list to the empty space below the rows: it
   must move to the bottom of the top level. Try to drag a folder into
   one of its own sub-folders: nothing must happen. The Library row
   must not drag at all. Restart the app: every order must be as you
   left it. This test also covers the one thing no probe can gate — a
   plain click must still select a row.
5. **Sort.** Right-click a folder: "Sort" must be there, between
   Rename and Delete. Click it: sub-folders come first, then the lists
   by name ("The Batman" sorts under B). Right-click a LIST: there must
   be no "Sort" row. Restart and confirm the sorted order survived.
6. **Select Worst Duplicates (ADR-044).** Give one series two copies
   that differ (a CBR that is smaller with fewer pages than its CBZ
   twin). Views ▸ Show Duplicates must narrow the list to the
   duplicates. Right-click a book: "Select Worst Duplicates" must sit
   between "Fill Missing Issues…" and "Remove from Library". Click it:
   the worse copy must select, the better one must not. Run Remove
   from Library on the selection and confirm. In Edit ▸ Preferences,
   the new Duplicates page must list the three rules, all on. Uncheck
   "CBR copies are worse than CBZ copies", OK, and run the command
   again on a group where the CBR is LARGER: with the rule off, the
   smaller CBZ must select instead (the score-sum behavior — a
   conflicting pair that ties under the rules must select NOTHING).
   With every rule off the command must select nothing and clear the
   selection. The selection must survive a restart is NOT a
   requirement — the mark is a selection, not a stored flag.

**PASSED 2026-09-12 (user confirmation):**

0. **Generate Cover Thumbnails must not freeze the app.** The first
   run of this test FAILED and stopped the tag. Cause, measured:
   `library::cache_thumbnails` ran its enqueue loop inline on the GTK
   thread, and the per-book key carries the file size and modified
   time, so `front_cover_thumbnail_key` -> `ImageKey::from_file` ->
   `file_stats` -> `std::fs::metadata` made it one `stat()` syscall
   per book plus one contended queue-mutex acquisition per book. A
   blocked main loop serves no redraws and no timers, which is why the
   lamp could not appear: its visibility poll is a
   `glib::timeout_add_local` tick. Rendering was always on worker
   threads, hence the filling cache during the freeze. The loop now
   runs on a "Thumbnail Warmup" worker; only the storage snapshot
   stays on the main thread, because `session()` is a UI thread-local.
   Retested by the user: works.
1. **The thumbnail lamp.** Covered by the same retest: the window
   stayed responsive, the lamp appeared, and the click opened Tasks.

**NOT RUN against the v0.1.0 build:**

2. **Detail-column overflow.** Switch to Details and narrow a column
   that holds long text (Series, or Title). The text must end in an
   ellipsis at the column edge and must NOT paint over the next
   column. Covered by `detailresize_probe` A-D, which is a probe, not
   a user test.
3. **Per-list view settings.** Set list A to Details with a small row
   height, and list B to Thumbnails with a large thumbnail. Switch
   between them: each must come back the way you left it. Then make a
   NEW list and select it — it must show the view you came from, and
   must not change until you change it. Right-click list B, choose
   "Reset View Settings", leave it and come back: it must no longer
   force its own view. Restart the app and confirm all of it
   survived. Covered by `browserbar_probe` G1-G4, which is a probe,
   not a user test.

Both remaining tests exercise code that shipped in v0.1.0. Run them on
the released build; a defect becomes a v0.1.1 fix.

## Blockers

None.

The `0.1.371` concern is CLOSED, and it needed no action. The rolling
release replaces itself: `publish_release.sh` deletes the previous
`rolling` release, its assets, and its tag, then recreates all three at
the current commit. The `0.1.371` artifacts were therefore already
deleted by the next run. MEASURED 2026-09-12 on both remotes: the only
release that exists is `rolling` = `v0.0.372` at commit `6b947592`, and
the mirror carries exactly one release and one tag.

The ordering that follows is clean, with no regression anywhere:

    0.0.372  (published rolling now)
      < 0.1.0    (the stable tag)
        < 0.1.1  (the next rolling build)

The one remaining trace is a MACHINE that installed the `0.1.371`
build while it was up. Nothing can reach back to it, and it outranks
both v0.1.0 and the rolling builds after it, so it would refuse the
upgrade. Check Help ▸ About on any machine used on 2026-09-12; if it
reads `0.1.371`, reinstall once from the release page. There is no
`--version` flag; the About dialog is the only surface.

## Lessons from the Phase 15 user test

Four defects reached the user because no gate watched them. Read these
before starting Phase 16; the same traps are in the dialogs it touches.

1. **A GTK4 window is invisible until `present()` is called.** The
   scrape progress window was built, filled, and closed but never
   presented, from Phase 12 until 2026-09-12. `scrape_probe` GATE V now
   checks it. Add the same check for any new window.
2. **A dialog with `.application(...)` and no `.transient_for(...)`
   lands behind the main window.** Every dialog needs the window as its
   parent. Use `show_report_dialog` or `show_failure_dialog` in
   `browser/shell.rs`, which do it correctly.
3. **A worker thread must never write the UI thread-locals.** It writes
   its own copy. Send over an `mpsc` channel and let the main-thread
   pump write the state. The scan and the Comic Vine cache jobs both
   use this shape.
4. **Long work must show progress and offer a cancel.** A job with no
   indicator reads as a job that did nothing. The status-bar lamp and
   the Tasks window row are the two surfaces; see
   `library::start_cv_job` and the `Comic Vine cache` block in
   `dialogs/tasks.rs`.

One process lesson: when a user reports "nothing happened", read the
presentation path before theorising about the engine.

## Known gaps

Tracked in `docs/backlog.md`; they do not block Phase 16.

- `WebComicProvider` is not ported.
- PDF and DjVu writers are missing.
- The AppStream metainfo has no `<screenshots>`. The element needs
  hosted image URLs, which the project does not have yet.
- `cargo deny check licenses` is not a CI gate: `ring` and
  `webpki-roots` are unresolved exceptions (ADR-041).
- HEIF and AVIF decode is missing.
- "Fill Missing Issues" has no volume picker for an unscraped series.
- The Comic Vine cache jobs have no per-row abort in the Tasks window;
  the lamp menu carries the targeted cancel.
- The per-list `ShowOnlyDuplicates` flag is written to ComicDb.xml by
  cr-core but never read back by cr-ui, so the Views ▸ Show
  Duplicates toggle does not restore per list. (surfaced by the
  duplicate-cleanup task, 2026-09-13).

## Environment notes

- Run UI probes in RELEASE with `Xvfb :99`, `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated `XDG_DATA_HOME` and `XDG_CONFIG_HOME`
  under `/tmp/opencode/`. The probes refuse to run against the real
  library.
- `scanrefresh` gate E stalls mid-scan in a DEBUG build on this
  machine; the unmodified base HEAD fails the same way. Use RELEASE.
- `newbook` and `exportpage` probes reach "PROBE DONE", then their
  internal watchdog fires with `rc=2`. The unmodified base HEAD shows
  the same shape — a pre-existing probe quirk.
- `editor_probe` runs a main loop forever by design; a kill under
  timeout is its normal completion.
