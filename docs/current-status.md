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

**Release preparation for v0.1.0.** The code work is done and gated.
The tag is NOT cut yet: it waits on the three open user tests below.

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
4. **The rolling version moves to `0.1.<commit count>` (ADR-042).** At
   `0.0.<count>` every rolling build after the tag would sort BELOW
   0.1.0 for pacman, dpkg, and AppStream, and rolling users would stop
   being offered upgrades. The prefix is a manual bump at each stable
   tag.
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

Phase 16 T1 (add `cover_date` to `IssueRef` and the issue queries) is
unchanged and NOT STARTED. It does not block the tag.

## Open licence question (inherited by v0.1.0 knowingly)

Apache-2.0 is incompatible with GPL-2.0-only. Three dependencies sit
on that line: the `cr-scrape` port of Cory Banack's Apache-2.0 Comic
Vine Scraper, plus `ring` and `webpki-roots`. ADR-041 records the two
exits and takes neither. The cheaper one is to ask maforget to elect
"GPL-2.0-or-later" for ComicRack CE; comicrust could then move to
GPL-3.0-or-later, under which Apache-2.0 is compatible. No claim is
made that the present combination is permissible.

## Verification record

`cargo build --release --locked -p cr-app` — the command the release
workflows run — green. `cargo fmt --all --check` and `cargo clippy
--workspace --all-targets -- -D warnings` — green.
`CR_FORMAT_TESTS=1 cargo test --workspace --locked` — 680 passed, 0
failed, unchanged from the pre-change baseline.
`appstreamcli validate` — passes (one pedantic note: the component id
contains uppercase letters, left alone because the desktop file and
the install paths depend on it). YAML parse of all four workflows —
clean.

UNKNOWN: the `.deb` is NOT verified locally. `dpkg-deb` is absent on
this machine, so `packaging/deb/build.sh` and the new copyright file
have had only a `bash -n` syntax check and a check of the indentation
the machine-readable format needs. The packaging workflow's own
content assertions are their first real test.

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

## Open user tests

Three, all from the 2026-09-12 side task. A passing probe is not a
passing user test.

1. **The thumbnail lamp.** Run Generate Thumbnails on a large list.
   A small animated icon must appear in the status bar while the work
   runs and disappear when it ends. A click on it must open Tasks.
   Run this one against a BUILT TARBALL, not the dev tree. The dev
   tree finds the frames through its own asset root and passes either
   way, which is exactly how the missing `assets/pages` copy stayed
   hidden.
2. **Detail-column overflow.** Switch to Details and narrow a column
   that holds long text (Series, or Title). The text must end in an
   ellipsis at the column edge and must NOT paint over the next
   column.
3. **Per-list view settings.** Set list A to Details with a small row
   height, and list B to Thumbnails with a large thumbnail. Switch
   between them: each must come back the way you left it. Then make a
   NEW list and select it — it must show the view you came from, and
   must not change until you change it. Right-click list B, choose
   "Reset View Settings", leave it and come back: it must no longer
   force its own view. Restart the app and confirm all of it
   survived.

## Blockers

None.

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
