# Architecture Decision Records

Append new ADRs at the end. Never rewrite the decision content of an existing entry. Language-only rewrites (ASD-STE100) are allowed. Status values: `accepted` / `superseded by ADR-nnn`.

---

## ADR-001: Full 1:1 feature parity is the target

- **Status:** accepted (2026-09-02)
- **Context:** Options ranged from minimal reader to full parity. ComicRack's value is the *manager* (library, metadata, smart lists, devices, scripting), not just the reader. A reader-only port would strand existing libraries.
- **Decision:** Target full parity with ComicRackCE, including workspaces, device sync, remote server (new protocol), and scripting. Multi-year effort accepted (~18-22 months solo).
- **Consequences:** No MVP shortcut on scope, but phased delivery keeps every phase shippable. A "core reader first" fallback remains possible if effort/reality diverges — that would be a new ADR.

## ADR-002: ComicDb.xml read/write compatibility is mandatory

- **Status:** accepted (2026-09-02)
- **Context:** Users have years of curated metadata in `ComicDb.xml` (single-XML database with `.bak`/`.restore` rotation — see `DatabaseManager.cs`). The database is the only unlosable artifact. Caches are disposable.
- **Decision:** Read **and write** the existing XML format with exact element/attribute-name fidelity via serde + quick-xml. Golden-file round-trip tests verify this from Phase 0. No SQLite or format change without a new ADR.
- **Consequences:** Every model change must pass validation against golden files. C# `XmlSerializer` quirks (ordering, attribute/element mix, empty-element forms) must be reproduced.

## ADR-003: Preserve the Python plugin ecosystem via PyO3/CPython 3

- **Status:** accepted (2026-09-02)
- **Context:** The plugin ecosystem (ComicVine Scraper, FromDucks, Library Organizer…) is IronPython 2.7 (Python 2 semantics) with `#@Directive` headers and `.crplugin` packages. Dropping it orphans the community. IronPython itself is not embeddable from Rust.
- **Decision:** Embed CPython 3 via PyO3, expose a `ComicRack`/`ComicBook` shim mirroring `IPluginEnvironment` (~40 methods). Keep `#@Directive`, manifest, and `.crplugin` formats unchanged. Ship a 2to3 migration guide. The top-5 community plugins are acceptance tests.
- **Consequences:** Python 2 plugins need migration. Scripts that build WinForms UI (`ComicInfoUI`/`QuickOpenUI` returning controls) are unportable. An HTML/WebKitGTK or GTK panel API replaces them (breaking change, documented).

## ADR-004: Plain GTK4 + custom CSS, no libadwaita

- **Status:** accepted (2026-09-02)
- **Context:** ComicRack is a bespoke, heavily custom-drawn app (ItemView, TabBar, reader overlays, paper textures). libadwaita's opinionated theming restricts custom CSS and would fight the port's fidelity goals.
- **Decision:** Use gtk4-rs directly with our own CSS providers. Adopt libadwaita only if a specific widget need arises (revisit via ADR).
- **Consequences:** We own dark-mode theming and HIG polish ourselves. Some convenience widgets (PreferencesWindow) are off the table.

## ADR-005: Drop WCF/Android remote-protocol compatibility

- **Status:** accepted (2026-09-02)
- **Context:** The C# remote library uses WCF over net.tcp with message-level security + X509 (the legacy Android app's protocol). Reimplementing WCF message security in Rust is very high effort for a legacy client base.
- **Decision:** Do not preserve the wire protocol. A future remote/server feature will define a new HTTP/JSON API (new clients required). UDP-broadcast discovery concepts may be reused (mDNS).
- **Consequences:** The existing Android app will not work against comicrust. The wireless-sync raw TCP protocol (ports 7614/7615/7620+) is *separately* portable if device sync needs it.

## ADR-006: Windows-only metadata/storage replaced by freedesktop equivalents

- **Status:** accepted (2026-09-02)
- **Context:** NTFS Alternate Data Streams store per-file reader settings (`NtfsInfoStorage.cs`). Shell integration (recycle, associations, icons) uses Win32.
- **Decision:** ADS → Linux xattrs (`user.comicrack.*`) with sidecar-file fallback, recycle → GIO trash, associations → xdg-mime, single-instance → D-Bus, sleep inhibit → `org.freedesktop.ScreenSaver`.
- **Consequences:** Windows-side metadata in ADS is not migrated (acceptable, data lives in ComicDb.xml).

## ADR-007: No static-linking of unrar

- **Status:** accepted (2026-09-02)
- **Context:** RAR5 decode in C# comes from SharpCompress/7z. The `unrar` library's license is GPL-incompatible with several plausible project licensing options (license itself deferred, ADR-008).
- **Decision:** RAR/RAR5 via 7z or libarchive **subprocess** (the C# app itself shells to 7z.exe), or pure-Rust readers if available. Never statically link unrar.
- **Consequences:** A subprocess adds slight startup cost per RAR open. A persistent extraction process can mitigate this if needed.

## ADR-008: Cairo-first rendering, GL second

- **Status:** accepted (2026-09-02)
- **Context:** The C# reader tries OpenGL (Tao) and falls back to GDI+. Porting the full GL texture pipeline immediately would put animations/transitions/3D-spine work on the critical path.
- **Decision:** Implement the reader widget with cairo first (covers layouts, zoom/pan, fit modes, paper texture via MULTIPLY pattern). Then add the `GtkGLArea`/glow path for transitions, magnifier, and large-image performance — mirroring the C# fallback architecture.
- **Consequences:** Phase 3 ships cairo. GL is an optimization within the same widget contract.

## ADR-009: License deferred

- **Status:** accepted (2026-09-02)
- **Context:** Upstream ComicRack CE has a nonstandard provenance (decompiled commercial app, revived with author-approval caveats — see upstream README). Third-party constraints: unrar (worked around, ADR-007), embedded CPython (PSF, fine), gtk4-rs (MIT, fine).
- **Decision:** No LICENSE file yet. This must be resolved before any binary distribution. A future ADR will pick the license after a review of upstream obligations.
- **Consequences:** The repo stays private-planning until licensing lands. Keep a third-party dependency license inventory as we add crates.

## ADR-010: Reuse upstream translation XMLs verbatim

- **Status:** accepted (2026-09-02)
- **Context:** Localization is already data-driven: 19 languages × ~72 XML files keyed by form/widget name, loaded via `TR.Load("Form")["Key", "Default"]`.
- **Decision:** Port the `TR` loader and consume the existing XML files unchanged. Do not convert to gettext.
- **Consequences:** This gives instant 19-language support. Widget names in cr-ui should mirror the C# control names where translations must hit.

## ADR-011: The ComicDb.xml layer is a hand-rolled writer, not serde

- **Status:** accepted (2026-09-02)
- **Context:** ADR-002 planned "serde + quick-xml". Implementation showed that serde cannot express the net48 `XmlSerializer` behaviors the byte-compat gate needs: per-member default-value suppression, base-class-first member order, `[XmlAnyElement]` raw passthrough at a fixed position, `xsi:type` passthrough for matcher types the code does not know, and the exact empty-element and indentation forms. Decorated derive structs would fight every one of these rules.
- **Decision:** Keep quick-xml for reading (event-based `XmlReader`, order-tolerant). Write XML with the hand-rolled `Emitter` in `cr-core/src/xml/mod.rs`. It reproduces `XmlSerializer.Serialize(Stream)` on .NET Framework 4.8 directly. Model types are plain structs. Each type has an explicit `write_xml` and an explicit read function. The emission rules live in `tests/golden/README.md`. Golden round-trip tests enforce them.
- **Consequences:** A new serialized member needs a manual write+read pair. The round-trip tests catch a missing pair. serde stays available for other uses (JSON output in cr-cli, future settings files). The goal of ADR-002 (exact format fidelity) is unchanged.
## ADR-012: Metadata write-back rewrites zip/tar natively, not through 7z

- **Status:** accepted (2026-09-02)
- **Context:** ComicRack CE writes `ComicInfo.xml`/`ComicBook.xml` into zip/tar archives through `7z u` subprocesses (`SevenZipEngine.UpdateComicInfos`, even when the reader engine is SharpZipLib). Phase 1 T5 asked for a native rewrite that preserves entry order and page content byte-for-byte.
- **Decision:** CBZ/CBT write-back is a native full rewrite (same entry order, same decompressed content, temp file + atomic rename). CB7 stays a `7z u` subprocess (C# parity, ADR-007). The observable behavior matches ComicRack: only the metadata entries change.
- **Consequences:** No `7z` dependency for CBZ/CBT metadata updates. Page content hashes are verified in `cr-cli rewrite`. Compression methods are preserved only where the original entries used Stored/Deflated; other methods re-encode to Deflate. A crash between temp write and rename leaves the original untouched plus a `.rewrite-<pid>` leftover that the next run overwrites.

## ADR-013: The matcher engine keeps decompiled C# quirks as behavior

- **Status:** accepted (2026-09-03)
- **Context:** Porting the query language and matchers (Phase 2 T1/T2) surfaced several places where the decompiled C# code produces surprising behavior that nevertheless IS the observable behavior of shipped ComicRack.
- **Decision:** Port the quirks verbatim, each with a comment in the source:
  1. The duplicate comparer's ternary chain compiles to `yearCond ? yearEq : (monthCond ? monthEq : (dayCond ? dayEq : bwEq))` — when either book has a year, only the year is compared (black-and-white is dropped).
  2. The series comparer's `IgnoreArticles` compares the SKIPPED-PREFIX lengths first; "The Batman" always sorts after "Batman", never reaching the number compare.
  3. `List contains` builds the regex from `MatchValue`, so the BOOK value is the list and the match value is the member.
  4. The `.restore` file is `ComicDb.restore` (no `.xml`), while `.bak` is `ComicDb.xml.bak`; `open_with_fallback` now matches.
  5. Group headers with zero matchers render as bare `MATCH` and do not re-parse (C# parity).
  6. The duplicate/grouping article lists use the English defaults shipped in ComicRack.ini (`the, der, die, das, le, la, les, l'`); the C# would throw on a fresh install because the ini value is unset.
- **Consequences:** When the C# source is ambiguous, the decompiled artifact decides. A future C# fix (upstream) can be ported then, with a fixture test pinning the new behavior.

## ADR-014: Queue identity, priorities, and the Rust threading model

- **Status:** accepted (2026-09-03)
- **Context:** `ProcessingQueue<K>` de-duplicates by `K` equality. The C# queues carry `ComicBook` instances (reference equality) and `ImageKey`s (field equality). The C# also has a scan-then-claim window where two workers could claim one item, and it sets `ThreadPriority` on Windows.
- **Decision:** Generic `ProcessingQueue<K: Eq + Hash + Clone + Send>`; ComicBook queues use a pointer-identity `BookRef(Arc<ComicBook>)`. The claimed item is marked running INSIDE the lock (fixes the C# claim race; documented). Thread priorities are stored but not applied — Linux has no portable user-space mapping; the C# defaults are BelowNormal/Lowest. Stop joins workers; there are no thread aborts, so the current item always finishes (the C# flags are checked at the same points).
- **Consequences:** Queue semantics are deterministic. The UI threads will run at normal priority until Phase 8 decides whether nice values are worth a libc dependency.

## ADR-015: Smart-list random selection uses the .NET Framework Random

- **Status:** accepted (2026-09-03)
- **Context:** `ComicSmartListItem` persists `LimitRandomSeed` and shuffles with `Random(seed)` after an `OrderBy(Id)`. A different PRNG would select different books from the same database.
- **Decision:** Port the .NET Framework subtractive generator (`CompatPrng`) and the `Guid.CompareTo` order (LE u32 / LE u16 / LE u16 / bytes) so a persisted seed reproduces the C# selection exactly.
- **Consequences:** `cr-engine/src/sort.rs` hosts `DotNetRandom` and `guid_compare`; test vectors are pinned against an independent transliteration of the dotnet/runtime source.

## ADR-016: The regex operator uses the regex/fancy-regex crates

- **Status:** accepted (2026-09-03)
- **Context:** The string matcher's `regex` operator compiles user-supplied .NET regex. A .NET-regex engine is not available in Rust.
- **Decision:** Use `fancy-regex`; on compile error the matcher reports no match (C# stores null and does the same). Lookbehind-only differences remain a documented tolerance.
- **Consequences:** Existing .NET patterns with simple syntax match identically; exotic features (e.g. variable-length lookbehind) degrade to no-match instead of failing the load.

## ADR-017: The reader is one virtual image driven by the part machinery

- **Status:** accepted (2026-09-03)
- **Context:** The C# reader nests `ComicDisplayControl` (page management, spreads, continuous strip) inside `ImageDisplayControl` (one image, fit/zoom/pan/rotation, part grid). Porting two widget layers would duplicate the geometry. The decompiled C# also reveals non-obvious mechanics: the part transform is part-local (source rectangles shift by the part window origin), and continuous mode keeps the whole scroll in part 0's offset (`GetClampedPartOffset` clamps against the full image, not the grid row).
- **Decision:** One widget (`cr-ui/src/reader/page_view.rs`) renders one *virtual image* through the part machinery from `reader/display.rs`. The comic layer composes pages into that virtual image: a single page, a two-page spread (`compose_spread`, pure and unit-tested), or the continuous strip (`reader/continuous.rs`, the `ContinuousPageLayout` port). The widget never subclasses a GObject; state lives in `Rc<RefCell<ViewState>>` captured by GTK closures (main-thread only). Pages decode on a background worker (latest-wins mailbox + std mpsc + a `timeout_add_local` pump) so the logical page advances per press while images trail, matching the C# book/display split. Cairo renders via `gdk`-independent `ImageSurface` + the `DisplayOutput` matrix (GDI+ element order maps 1:1 onto `cairo::Matrix::new`).
- **Consequences:** All layout decisions are pure functions with unit tests (fit modes, part grid, spread rules, anchors). The GL renderer (ADR-008) replaces only the draw call behind the same geometry. Widget lifecycle pitfalls (RefCell re-entrancy, glib channel absence) are recorded in the AGENTS.md lessons.

## ADR-018: gtk4-rs stays on the GTK 4.0-era API surface for now

- **Status:** accepted (2026-09-03)
- **Context:** The CI runner (Debian, `debian-go`) has "GTK4 dev libraries" of unknown version. gtk4-rs 0.11 gates newer APIs (FileDialog, `CssProvider::load_from_string`, `Picture::content-fit`) behind version features; enabling them risks link failures against an older system GTK. The dev machine runs GTK 4.22.
- **Decision:** Use only the GTK 4.0-era API surface: `FileChooserNative` (not `FileDialog`), `CssProvider::load_from_data` (not `load_from_string`), no `v4_*` cargo features. Revisit when the CI runner's GTK version is confirmed (or the settings port decides the minimum supported GTK); then enable the matching feature flags in one commit.
- **Consequences:** Some GTK 4.10+ conveniences are off the table for now. Behavior differences between the dev machine (4.22) and CI are possible at runtime; the headless CI cannot run GTK apps anyway (no display), so UI smoke tests stay manual/Xvfb-based.

## ADR-019: Reader page loads ride the ImagePool queues

- **Status:** accepted (2026-09-03). Supersedes ADR-017's background-worker description on this point only.
- **Context:** ADR-017 shipped a per-widget decode worker (latest-wins mailbox). The C# reader never decodes privately: `ComicDisplayControl.GetImage` checks the pool's memory cache on the UI thread (`onlyMemory`) and otherwise queues the page (`ImagePool.CachePage` → `AddPageToQueue`, fast queue when the disk cache can serve, slow otherwise; `AddToTop` for demanded pages, bottom for backward prefetch; `AsyncCallback` reports completion).
- **Decision:** The reader enqueues every wanted page through `ImagePool::add_page_to_queue` (the Phase 2 ProcessingQueue port: fast/slow split, AddToTop moves, dedup). The queue callback renders via `ImagePool::render_page` and ships the result (page, rotation, `Option<Image>`, comic source) over std mpsc; a `timeout_add_local` pump on the UI thread turns results into cairo surfaces. A failed decode delivers `image: None` and the reader renders the error page — the C# pool caches `CreateErrorPage` at the same point. Completion payloads carry the comic source string so stale results from a previous open are dropped.
- **Consequences:** No private decode threads; prefetch and priority ordering match the C# queues; the same queues later serve the browser's thumbnails. The worker/mailbox code from ADR-017 was removed.

## ADR-020: CI and release builds run in a container; one rolling release with commit-count versioning

- **Status:** accepted (2026-09-03)
- **Context:** CI ran directly on the `debian-go` runner host (no containers), so toolchain and GTK versions were whatever the host had and could not be updated freely. Packaged distributions (deb, flatpak) are a later phase.
- **Decision:** All CI and release workloads move to the `docker-runner-amd64` runner and execute inside the `comicrust-ci` image built from `.gitea/container/Dockerfile` (debian:trixie + rustup stable + libgtk-4-dev + 7z + djvulibre; rebuild the image to refresh the toolchain). A second workflow ("Rolling release") builds `cr-app` in release mode on every push to main and republishes the single prerelease tagged `rolling`: version `0.0.<total commits on main>` (bump major/minor by editing the workflow's Version step), assets `comicrust-<version>-linux-amd64.tar.gz` + `.sha256`. The tarball holds the binary renamed `comicrust` plus `assets/papers/` (the papers load at runtime relative to the binary's working directory; the error assets are compiled in). Publishing talks to the Gitea release API with `secrets.RELEASE_TOKEN || secrets.GITHUB_TOKEN`.
- **Consequences:** Host toolchains no longer matter, but every push until the new runner is registered and the image built will sit "waiting" for `docker-runner-amd64`. Older rolling assets disappear on each publish (by design). PDF format tests stay off in CI (the image has no libpdfium.so). deb/flatpak jobs can later join the same workflow as extra jobs producing extra assets.

## ADR-021: Two release tracks: a rolling prerelease and tagged stable releases

- **Status:** accepted (2026-09-03)
- **Context:** ADR-020 defined one release path. Every push to main republished the single `rolling` prerelease. There was no path to publish a stable release for a fixed tag.
- **Decision:** The repo has three workflows. `ci.yaml` runs fmt, clippy, and tests on every push to main. `release.yaml` keeps the ADR-020 rolling behavior. `tagged-release.yaml` runs on manual dispatch only, with a required `tag` input (for example `v0.1.0`). The tag must exist before the dispatch. The `checks` job runs fmt, clippy, and tests on the tag's code. The `release` job then builds `cr-app` and publishes a stable release for that tag. The version is the tag without its leading `v` (v0.1.0 → 0.1.0), and the version must match `X.Y.Z` digits. A re-run for the same tag replaces the release and its assets. The tag itself is never moved or deleted. Both release workflows publish through one shared script: `.gitea/publish_release.sh <tag> <version> <prerelease> <delete-tag>` (it replaces `publish_rolling.sh`). Rolling passes `delete-tag=true`, so the `rolling` tag follows main. The tagged call passes `delete-tag=false`.
- **Consequences:** A dispatch with a missing or malformed tag fails early, in the checkout or Version step. The release build checks out the tag, so the publish step derives the commit with `git rev-parse HEAD`. `github.sha` names the dispatching branch head, not the tag. `publish_release.sh` was checked offline with a stub `curl` only. The first real proof is a dispatch on the Gitea server. The image recipe now uses `--no-install-recommends`. The first image build failed on the invalid `--no-recommends` spelling. The second build failed on the rustup `--component` flag: rustup-init takes it as a comma-separated list, not space-separated arguments. Both fixes are verified against the current tool sources (apt-get 3.0.3 man page, a sandboxed rustup-init run). The third image gotcha: the runner execs JavaScript actions (`actions/checkout` runs under node20) with the job container's own node, so the image ships Debian's nodejs 20.x.

## ADR-022: The library session and the default database location

- **Status:** accepted (2026-09-03)
- **Context:** Phase 4 T1 wires the ComicDb.xml lifecycle into the app (the C# `Program.DatabaseManager`). The C# layout is `%APPDATA%\cYo\ComicRack Community Edition\ComicDb\ComicDb.xml` (`SystemPaths`, Company="cYo", Product="ComicRack Community Edition"); the settings port (`IniFile`/`EngineConfiguration`/`SystemPaths`) is still an open Phase 0 tail, but the session needs the database location now.
- **Decision:** `cr-core::paths` is a minimal `SystemPaths` slice: the data root is `${XDG_DATA_HOME:-~/.local/share}/comicrust` (the XDG analog of the C# roaming APPDATA; the Company/Product nesting flattens to the product name), the database path is `.../comicrust/ComicDb`, the file is `.../ComicDb/ComicDb.xml`. Directories are created on construction (the C# `MakeApplicationPath` parity). The session itself is `cr_engine::library::Library` (UI-free): open through the fallback chain, save on exit, dirty tracking, scans, watch events, and the `QueueManager` instance. The C# distinction between a missing database file (silent `CreateNew()`, no message) and a corrupt one ("There was a problem opening the Database...") is a new `OpenStatus::FreshEmpty` variant. The exit save writes even when clean; the background save (`DatabaseBackgroundSaving` 600 s) only when dirty. Smart-list `CacheStorage` round-trips as loaded — comicrust evaluates lists on demand and does not commit caches on save (the C# exit save runs `StoreCache`; the in-memory cache machinery was never ported). ComicRack's scanner runs on a low-priority worker thread; the port runs one scan synchronously (a 255-book scan is fast).
- **Consequences:** A user migrates by copying their Windows `ComicDb.xml` into `~/.local/share/comicrust/ComicDb/` (or re-adding folders — the scanner's same-name+size recovery preserves reading state). The full settings port will replace the hard-coded locations and the `TRACK_CURRENT_PAGE` constant. The temporary (non-library) books opened from the command line keep session-only reading state — the C# `AddToTemporary` parity (`AddToLibraryOnOpen` defaults to false). Correction (same day, first user test): the scan does NOT run synchronously — a real folder froze the UI thread. The scan runs on a "Book Scanner" worker thread (the C# thread name) with the book storage moved to the worker and back over mpsc + a main-loop pump; requests arriving mid-scan queue in arrival order (the C# scan queue), and the exit/background saves wait out or skip an in-flight scan (a mid-scan save would write the taken, empty book list — the C# joins the scanner before `DatabaseManager.Dispose`).
