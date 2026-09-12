# Architecture Decision Records

Append new ADRs at the end. Never rewrite the decision content of an existing entry. Language-only rewrites (ASD-STE100) are allowed. Status values: `accepted` / `superseded by ADR-nnn`.

---

## Index

This index is navigation only. The entry below each ADR is the decision.

| ADR | Title | Status | Superseded by |
|---|---|---|---|
| ADR-001 | Full 1:1 feature parity is the target | accepted | — |
| ADR-002 | ComicDb.xml read/write compatibility is mandatory | accepted | — |
| ADR-003 | Preserve the Python plugin ecosystem via PyO3/CPython 3 | superseded | ADR-027 |
| ADR-004 | Plain GTK4 + custom CSS, no libadwaita | accepted | — |
| ADR-005 | Drop WCF/Android remote-protocol compatibility | accepted | — |
| ADR-006 | Windows-only metadata/storage replaced by freedesktop equivalents | accepted | — |
| ADR-007 | No static-linking of unrar | accepted | — |
| ADR-008 | Cairo-first rendering, GL second | accepted | — |
| ADR-009 | License deferred | accepted | — |
| ADR-010 | Reuse upstream translation XMLs verbatim | accepted | — |
| ADR-011 | The ComicDb.xml layer is a hand-rolled writer, not serde | accepted | — |
| ADR-012 | Metadata write-back rewrites zip/tar natively, not through 7z | accepted | — |
| ADR-013 | The matcher engine keeps decompiled C# quirks as behavior | accepted | — |
| ADR-014 | Queue identity, priorities, and the Rust threading model | accepted | — |
| ADR-015 | Smart-list random selection uses the .NET Framework Random | accepted | — |
| ADR-016 | The regex operator uses the regex/fancy-regex crates | accepted | — |
| ADR-017 | The reader is one virtual image driven by the part machinery | accepted | — |
| ADR-018 | gtk4-rs stays on the GTK 4.0-era API surface for now | accepted | — |
| ADR-019 | Reader page loads ride the ImagePool queues | accepted | — |
| ADR-020 | CI and release builds run in a container; one rolling release with commit-count versioning | accepted | — |
| ADR-021 | Two release tracks: a rolling prerelease and tagged stable releases | accepted | — |
| ADR-022 | The library session and the default database location | accepted | — |
| ADR-023 | XDG layout — configuration in ~/.config, data and caches in ~/.local/share | accepted | — |
| ADR-024 | A UI-parity phase (5.5) with a locked chrome scope | accepted | — |
| ADR-025 | The dark/light toggle — theme-following UI, boot-switch compatibility | accepted | — |
| ADR-026 | The browser dock modes and the sidebar preview pane move to the backlog | accepted | — |
| ADR-027 | No scripting host — native modules replace the Python plugin ecosystem | accepted | — |
| ADR-028 | Phase 7 re-scoped — sync, remote, tray, and i18n defer to the backlog | accepted | — |
| ADR-030 | CBR/RAR in-archive write-back through the user-installed `rar` CLI | accepted | — |
| ADR-031 | Native modules — one crate per plugin behind a thin UI seam | accepted | — |
| ADR-032 | The Book Scanner scans a clone of the book storage, not a take | accepted | — |
| ADR-033 | One unified config file (`comicrust.toml`) | accepted | — |
| ADR-034 | Archive readers are chosen by content, and every open is bounded | accepted | — |
| ADR-035 | A scan never waits for a person: per-file verdicts in `CustomValuesStore` | accepted | — |
| ADR-036 | An explicit scan is one request with a one-shot forced retry | accepted | — |
| ADR-037 | The Comic Vine cache is a plugin-local SQLite file with two layers | accepted | — |
| ADR-038 | The MCL interchange format and the incremental Comic Vine sweep | accepted | — |

ADR-029 is reserved for the deferred Phase 9 (SQLite) decision. It is not written yet.

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

- **Status:** superseded by ADR-027 (2026-09-06)
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
- **Consequences:** All layout decisions are pure functions with unit tests (fit modes, part grid, spread rules, anchors). The GL renderer (ADR-008) replaces only the draw call behind the same geometry. Widget lifecycle pitfalls (RefCell re-entrancy, glib channel absence) are recorded in `docs/guides/gtk-and-ui.md`.

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

## ADR-023: XDG layout — configuration in ~/.config, data and caches in ~/.local/share

- **Status:** accepted (2026-09-04, user directive)
- **Context:** The C# stores everything under the roaming `%APPDATA%\cYo\ComicRack Community Edition\` (Config.xml, ComicDb, NewsFeeds.xml, Scripts) and the disk caches under the non-roaming `%LOCALAPPDATA%\...\Cache\` (`SystemPaths`). Linux has no roaming-profile split; the user directive for comicrust is two buckets: configuration files in `~/.config/comicrust`, data files in `~/.local/share/comicrust`.
- **Decision:** `cr-core::paths` maps: config root = `${XDG_CONFIG_HOME:-~/.config}/comicrust` (holds `Config.xml` — the C# `defaultSettingsFile` — and `comicrust.ini`, the `IniFile.Default` chain: entry dir → `/etc/comicrust` → user config, later files override); data root stays `~/.local/share/comicrust` (ADR-022: ComicDb, NewsFeeds.xml, Scripts) and also hosts the C# `Cache` subtree (`Thumbnails`/`Images`/`Files`/`CustomThumbnails`) — the C# puts the caches under LocalApplicationData, a data location, so the user's data bucket covers them. `XDG_CACHE_HOME` was considered and rejected: the user's rule names only the two buckets.
- **Consequences:** A Windows migration copies `Config.xml` (+ optionally `ComicRack.ini`, renamed `comicrust.ini`) into `~/.config/comicrust/`. No `.local/share` path changes for existing users.

## ADR-024: A UI-parity phase (5.5) with a locked chrome scope

- **Status:** accepted (2026-09-04, user directive)
- **Context:** Phases 3-5 ported behavior (reader, browser, dialogs) but not the chrome: no menubar, no toolbars, a one-label status bar, bare book tabs, no dock modes, no bundled icons. Original ComicRack is chrome-heavy. A dedicated phase between 5 and 6 closes the gap before the scripting host (Phase 6) builds on the shell.
- **Decision:** New phase 5.5, spec in `docs/archive/phases/phase-5.5.md` (full C# chrome inventory with file:line refs, tasks T1-T14, per-task user acceptance tests). Locked scope: browser dock modes Fill + Bottom only (Left/Right dropped); the Detail column chooser is IN; CR's Info Panel is OUT (Properties editor + sidebar preview cover it); the C# PNG icon set (`ComicRack/Resources/*.png`, 183 files) is bundled into `cr-ui/assets/icons/` (papers precedent); named workspace presets are OUT (only automatic layout persistence). Omitted: undo/redo, tray icon, remote library UI, device sync, News/update/help links, Automation menu (Phase 6), splash, crash dialog, search-browser matcher panel, web-comics item.
- **Consequences:** The shell grows a command/action layer (T1) before any menu work — GTK menubar accelerators need real Gio actions. The dock-mode task (T10) reshapes the shell; Fill must stay bit-identical and is user-tested before/after. Deviations from CR are recorded per task in the kickoff doc.

## ADR-025: The dark/light toggle — theme-following UI, boot-switch compatibility

- **Status:** accepted (2026-09-05, user tested; user directive for the theme-following reader)
- **Context:** The C# theme is boot-only: `ExtendedSettings.UseDarkMode` (the `-dark` switch) forces `Themes.Dark` over the stored `Theme` value; both ride the ini chain; there is NO menu command and the app never writes the ini. The C# `Themes.Default` renders the light WinForms look regardless of the Windows dark setting. The reader is darker still: `ImageDisplayControl.InitializeComponent` sets `BackColor = Color.Black` unconditionally and no DarkControlDefinition overrides it — the C# reader surround never follows any theme. The user asked for a runtime toggle and, after two test rounds, for the whole app (reader included) to flip with it.
- **Decision:** The port adds `win.dark-mode` (Browse ▸ _Dark Mode): a stateful bool action, iconless, no accelerator (the C# table has none to port). The handler flips `ExtendedSettings::global_mut().theme` (Dark/Default), clears `use_dark_mode` (the explicit toggle must not let a stale `-dark` re-darken the next boot), applies `theme::set_dark` (the GTK `prefer-dark` flag re-styles instantly), and persists `Theme` + `UseDarkMode=False` into the LAST ini-chain file through the new `IniValues::merge_write`/`library::save_ini_keys` — the C#-shaped storage, but the ini write-back is a deviation. `ExtendedSettings::effective_theme` ports the C# `Theme` getter; `Themes::Default` renders LIGHT (C# parity) — the app starts light on a fresh or pre-toggle config; `-dark` and `-theme Dark` keep working. CORRECTION (2026-09-06): the earlier observation "the app starts light on an EXISTING Dark config" was the `init_global` boot bug (a `OnceLock::set` silently dropped the parsed ini after an early `global()` reader froze the defaults) — with the write-through fix an existing `Theme=Dark` boots dark, the C# behavior. The cairo-drawn views (ItemView, Pages panel, the reader surround) resolve the GTK named colors per draw call (`theme::Palette` — the `SystemColors` parity) and re-draw on the prefer-dark notify. The reader surround follows the theme in Color/Texture modes — a recorded deviation from the C# `BackColor = Black` (the user chose theme-following so the whole app flips); Auto mode keeps the C# page-corner sampling.
- **Consequences:** The ExtendedSettings global moved `OnceLock` → `RwLock` (the C# static is mutable). GTK does not invalidate custom cairo draws on a theme flip — every drawn view needs the `redraw_on_theme_change` hook plus per-draw palette resolution (no caches). With a light surround, white pages blend into it edge-to-edge (the look the C# Auto mode gives on white comics). The dead `window.reader-window`/`.reader-page-area` CSS rules are gone. A hand-edited `UseDarkMode=True` ini still forces dark until the user toggles (the toggle clears it).

## ADR-026: The browser dock modes and the sidebar preview pane move to the backlog

- **Status:** accepted (2026-09-05, user directive)
- **Context:** Phase 5.5 carried T10 (browser dock modes Fill + Bottom) and T11 (the SmallComicPreview sidebar pane) as tasks. With the tab strip (T9) landed, the user judged the second dock mode and the preview pane low-value next to the remaining chrome tasks and moved both out of the phase.
- **Decision:** T10 and T11 are BACKLOG items (`docs/port-plan.md` §6, with the C# refs); they are no longer Phase 5.5 tasks and do not block Phase 6. The port stays Fill-only (the C# default); F3 keeps toggling the browser within the Fill stack; the Browse ▸ Small Preview menu item stays a disabled stub. The T14 persistence scope drops the dock-mode and preview-pane keys until the backlog items land; it gains the T12 display-options persistence instead (the C# `DisplayWorkspace` display family).
- **Consequences:** The ADR-024 scope line ("Fill + Bottom only") stays for the eventual pickup — Left/Right remain dropped. The kickoff's T10/T11 sections record the move and keep the C# specs for whoever picks the work up.

## ADR-027: No scripting host — native modules replace the Python plugin ecosystem

- **Status:** accepted (2026-09-06, user directive). Supersedes ADR-003.
- **Context:** Phase 6 planned a PyO3/CPython 3 port of the IronPython host (ADR-003). A feasibility review (2026-09-06) tested that plan against the real plugin surface:
  - The flagship plugin (Comic Vine Scraper, ~12k lines IronPython) fails at `import clr` on CPython. Its hook use (`ConfigScript` + `Books, Editor`) maps 1:1 to the planned host, and its engine layer (~55% of the code) migrates after a 2to3 pass. Its UI layer (~40%) is WinForms: no shim of acceptable size maps Form/DataGridView/GDI+ onto GTK.
  - The bundled sample scripts split the same way: pure-logic hooks port after 2to3; every script that builds a dialog (NewComics.py, Autonumber.py, SearchAndReplace.py) hits the WinForms wall.
  - The user community is small, and the scripts people actually use are a handful. The C# features behind them are small native tasks: fileless book creation is a NATIVE C# command (`MainForm.AddNewBook`, MainForm.cs:1879), not a script; the built-in NetSearch table (`SearchEngines.cs`) holds one Wikipedia search source.
  - The only scripting surface inside user data — the smartlist `Expression` and plugin-list matchers — shows zero use in the real-world database fixture.
- **Decision:** Drop the scripting host. No PyO3/CPython dependency, no `#@Directive` loader, no `.crplugin` packages, no Automation menu, no plugin hooks, no 2to3 guide. The `cr-script` crate is removed from the workspace. Extensibility is native features added release by release. From the old Phase 6 scope the port keeps:
  - The native "New Comic…" fileless-book flow (`MainForm.cs:1879` parity) and a native "New fileless Book Series…" dialog (the NewComics.py port).
  - Smartlist `Expression` and plugin-list matchers keep parsing and rendering byte-stably; evaluation returns an explicit not-supported result (never a crash).
  - The settings parser keeps accepting the `Scripting` and `PluginsStates` keys from existing Config.xml files (they are ignored).
- **Consequences:** Users with script workflows migrate to built-in features; new requests land as native work ("small features as we go along" is the maintenance model). The compat invariant on plugin file formats is retired. A future scripting host stays possible — a new ADR would revive it, and the C# hook-table record at the end of `docs/archive/phases/phase-6.md` remains the reference.

## ADR-028: Phase 7 re-scoped — sync, remote, tray, and i18n defer to the backlog

- **Status:** accepted (2026-09-06, user directive)
- **Context:** Phase 7 as planned (`docs/port-plan.md`: D-Bus single instance, MTP/wireless sync, HTTP remote server, full i18n) estimated 8-10 weeks. The user uses none of sync/remote/tray today, and the i18n port is a large mechanical sweep (the TR machinery is small, but instrumenting every surface is not) with low immediate value while the UI keeps evolving. All four areas were researched against the C# source in the same session (2026-09-06) so the findings survive the deferral.
- **Decision:** Phase 7 delivers the GApplication unique mode + the startup file pipeline only (the C# second-instance handoff, `OpenSupportedFile` semantics, the restart handshake). Device sync (engine + GVFS MTP provider + wireless protocol), the HTTP remote library (ADR-005 stands), the tray icon, and the i18n (TR) port move to `docs/backlog.md` — each entry carries its research record. The auto-update check, the news feed, and the crash-watchdog dialog record as backlog entries too. Deferred items return through their own kickoff when picked up.
- **Consequences:** comicrust stays single-user and English-only for now; the Preferences language page stays deferred. The `-p`/`-il`/`-hidden` switches parse but several stay inert until their owners land (recorded in the Phase 7 kickoff deviations). The Phase 8 estimate unchanged; the Phase 7 estimate drops to ~1 week.

## ADR-030: CBR/RAR in-archive write-back through the user-installed `rar` CLI

- **Status:** accepted (2026-09-09, user decision; record: `docs/archive/phases/phase-10.md`)
- **Context:** The C# never writes into RAR archives — `CbrComicProvider`/`Rar5ComicProvider` carry no `FileFormatAttribute.EnableUpdate` (only CBZ/CBT/CB7/CBW are updatable), and `ComicProvider.StoreInfo` instead persists edits for every format into the NTFS ADS stream. On Linux the ADS equivalent is the xattr store (ADR-006); it was offered as the parity fix and the user DECLINED it ("not really interested in the xattr thing, more interested in writing info into the actual files"). A real in-archive RAR write requires a RAR compressor, which does not exist as free software: 7-Zip and libarchive decode only, unrar is extract-only and GPL-incompatible. The only writer is RARLAB's `rar` CLI (closed-source freeware).
- **Decision:** `cr-io` gains a `rar` subprocess writer (the `7z` precedent, ADR-007): `find_rar()` resolves `CR_RAR` then PATH (never `unrar`), and `store_info_scoped` routes CBR/RAR5 through one `rar a -y <archive> <files>` with cwd at the staging directory (root-level bare-name entries) and stdin null (a password-protected target then fails its prompt immediately — measured exit 12 — instead of hanging). Measured: rar 7.12 updates both RAR5 and existing RAR4 archives (format preserved, pages intact); it can no longer CREATE RAR4 archives (no `-ma4` in 7.x) — RAR4 write-back is proven on real fixtures via the `CBR_RAR4_FIXTURE` gate. `supports_update` stays false for RAR in the format registry (C# parity: not unconditionally updatable). The xattr fallback in `ComicProvider::store_info` stays as-is for the CLI path; the app's `update_book_file` reports "rar executable not found" through the existing error dialog.
- **Consequences:** The binary is never bundled, linked, or distributed (license hygiene; the tarball and CI ship nothing RAR-related, so the gated tests skip in CI). Users install `rar` themselves (Arch AUR, Debian `non-free`, Fedora RPM Fusion, or the RARLAB static tarball). Without it, CBR writes surface a clear error and edits persist DB-only — the manual command is no longer a silent no-op. `rar` exit 1 (warning) still counts as failure for us. ADR-029 stays reserved for the deferred Phase 9 (SQLite) decision.

## ADR-031: Native modules — one crate per plugin behind a thin UI seam

- **Status:** accepted (2026-09-10, user directive; record: `docs/archive/phases/phase-12.md`)
- **Context:** ADR-027 dropped the scripting host; the used plugins must return as native features. The user's directive for the first one (Comic Vine Scraper): keep it as modular as possible, minimal interference with the base codebase, functionality parity first, look and feel second.
- **Decision:** One new crate per plugin (`crates/cr-scrape` first): pure Rust, no GTK, depends only on cr-core (model + registry) and cr-image plus its own external deps (ureq/rustls, serde_json). The engine runs on a worker thread, never touches the thread-local app session, and mutates clones of the books it is handed; the UI side (the only consumer, cr-ui) drives it through a request/response message protocol over std mpsc + a `timeout_add_local` pump and applies results on the main thread through the existing `library::apply_edited` pipeline. Plugin configuration lives plugin-locally under `~/.config/comicrust/plugins/<plugin>/` — never in the cr-core Settings schema. Base-codebase touchpoints are enumerated in the phase kickoff and stay minimal (workspace member, one dialog module, a few shell wiring lines, plus at most one declared engine/render branch when a feature genuinely needs it — Phase 12 T8, the fileless custom-thumbnail render path).
- **Consequences:** Plugins are independently testable headless (mock servers, fake UIs). The C# hook-table integration points (context menus, toolbars) get wired per-feature in cr-ui instead of through a generic plugin loader; there is no plugin discovery, no `.crplugin`, no host — each native module is app code that happens to live in its own crate. Future modules (Metron scrapers, autonumbering, …) follow the same shape.

## ADR-032: The Book Scanner scans a clone of the book storage, not a take

- **Status:** accepted (2026-09-11, user approval; record: commit `d9262a4`, and the scan-liveness history in git)
- **Context:** The scan worker `std::mem::take`d the whole book storage out of the database for the duration of a scan (the ADR-019-era take-and-return shape). A re-scan fires `on_new` only for NEW files, so zero batches flowed and the database held ZERO books for the entire scan; every list evaluation (smart lists, the navigator select after a watch landing, F5) read 0 books and wiped the view, and the search results blanked until a restart. The user approved the architecture change explicitly.
- **Decision:** `start_scan_worker` gives the worker a CLONE (`db.books.clone()`); the database keeps the full library mid-scan and grows by the pump's batch appends as before. The landing merge (`merge_scan_storage`, pure + unit-tested) reconciles the worker's storage with what the main thread did WHILE the scan ran: database-only books stay (mid-scan adds), touched ids keep the database copy (edits, reading state, page sizes, write-back results — recorded by `record_scan_touch` at the mutation sites), removed ids drop everywhere (`record_scan_removal` at the remove sites). The per-tick scan hook no longer falls back to a full refresh for non-Library views (the landing hook does the one refresh), and the watch poll holds pending roots while a scan runs instead of stacking a rescan per second.
- **Consequences:** One library clone per scan (transient; the C# scans the live collection, the same class). A book edited mid-scan whose file write completed still wins through the touched record (the scan's file-info refresh for that book is discarded until the next scan). A scan with `remove_missing` (not used by any UI path today) would resurrect scanner-removed books unless they are also recorded; noted for whoever wires it.

## ADR-033: One unified config file (`comicrust.toml`)

- **Status:** accepted (2026-09-11, user decision; record: `docs/archive/phases/phase-13.md`)
- **Context:** The config state sat in three stores: the `comicrust.ini` search chain (`ExtendedSettings` + `EngineConfiguration` keys), `Config.xml` (the ~120-field `Settings` object, ADR-023), and the plugin-local `settings.json` (ADR-031). The user asked for ONE file and for the scraper's hardcoded data tables (the imprint→publisher list) to leave the binary: "if I want to add another imprint, I don't have to recompile the whole app". The user confirmed ComicDb.xml stays sacred (books, lists, matchers, watch folders — untouched) and that migrating the old config files is not an issue.
- **Decision:** ONE TOML file, `~/.config/comicrust/comicrust.toml` (serde, hand-editable). Sections: `[extended]` (the `ExtendedSettings` keys, stored/applied verbatim through the field registry — argv still overrides at boot and never writes back), `[engine]` (the `EngineConfiguration` keys; the Size/Color converter fields keep the .NET text forms), `[settings]` (the Settings fields under their C# member names, serde round-tripped), `[plugins.<name>]` (opaque plugin tables behind typed accessors — cr-core never depends on cr-scrape), and `[data]` — the user-editable data tables seeded from the built-ins on first boot with a per-table revision marker that merges ONLY missing keys on upgrade (user edits survive; a user-deleted built-in entry stays deleted until a future revision bump re-offers it). The Comic Vine imprints table moved from `cr-scrape/src/cv/imprints.rs` into the seed (`IMPRINTS` in cr-core `settings/unified.rs`); `find_parent_publisher` reads the session table and falls back to the built-in seed when uninitialized. Precedence unchanged: defaults < file < argv. The ini search chain (exe dir / `/etc`) collapses to the one file (recorded simplification); `save_ini_keys` becomes the `[extended]` update + whole-file save; the old `Config.xml` writer/reader and the ini file-chain machinery are deleted. supersessions: ADR-023's `Config.xml`/`comicrust.ini` layout, ADR-031's plugin-local `settings.json` storage (the plugin-local DIRECTORY survives only for the `prior_series.json` scrape cache, which is state, not config).
- **Consequences:** The Settings layer is no longer byte-stable XML (Config.xml was never the sacred artifact — only ComicDb.xml is); round-trip equality tests replace the XML golden tests, and a registry-name guard test pins the C# member spellings (the pinned renames: `RemoveFilesfromDatabase`, `InformationCover3D`, the `*MB` cache fields). Hand-added TOML comments do not survive an app rewrite (values always do). The file is read once at boot — hand edits apply at the next start (the Config.xml model). Old files (`Config.xml`, `comicrust.ini`, `plugins/comic-vine-scraper/settings.json`) are left on disk, never read or written; users re-enter their preferences (per user decision). f32 fields serialize through the shortest f32 text to keep hand-edited values clean.

## ADR-034: Archive readers are chosen by content, and every open is bounded

- **Status:** accepted (2026-09-11, user directive; record: this commit, and the live-scan measurements in the session)
- **Context:** A user scan stalled for minutes per file on 18-67 MB books and for hours on a 2.8 GB one. Measured on the live process (thread "Book Scanner", TID 1436857): the thread sat in `folio_wait_bit_common`, CIFS reported ONE SMB request in flight owned by that thread, and the open file offsets walked BACKWARDS at about 0.49 MB/s. Two independent causes were proven. (1) `ComicProvider::open` picked the accessor from the file EXTENSION only and never called the `ComicAccessor::is_format` check it already implemented, so a RAR archive named `.cbz` ("Blacksad (2016) Volume 01 Issue 004.cbz", confirmed RAR by `7z`) drove the zip reader. (2) `zip::ZipArchive::new` (`ArchiveOffset::Detect`) resolves prepended junk with UNBOUNDED backward searches in 2045-byte windows, so a file with no usable central directory ("The Boys 064 (2012).cbz": one local entry plus 17.4 MB of trailing data, no EOCD) was scanned end to end. The earlier fix (commit `c8ef15d`) made this worse than believed: its `EOCD - cd_size - cd_offset` arithmetic is only valid when the EOCD directly follows the central directory, which is FALSE for every ZIP64 file, because the ZIP64 record (56 bytes) and its locator (20 bytes) sit in between. On the 2.8 GB omnibus the arithmetic produced 76 for a file whose real offset is 0; the mandatory guess then missed and the crate fell back to the full-file scan.
- **Decision:** The extension still picks the candidate reader, then the accessor's own signature check runs, and a failure routes the source through `formats::detect_format` — one bounded head read (265 bytes, enough for the tar `ustar` magic at 257) that maps the signature to a reader. This is the port's form of the C# `ImageProviderFactory.CreateSourceProvider` fallback, which asks the other providers for a `FastFormatCheck` hit (ImageProviderFactory.cs:18-27). Separately, `open_zip_archive` resolves the archive offset inside ONE tail read (64 KiB + 22): it locates the EOCD, reads the ZIP64 record from the same buffer when present, builds the small candidate set (0, the ZIP64-derived offset, the zip32 arithmetic), and VERIFIES each candidate against the central-directory signature with a single 4-byte read. A file that fails every candidate is rejected immediately and never handed to the crate's detection, because that is the unbounded path. `ComicProvider::open_with_report` returns what happened (mismatch, entry error) so callers can record it.
- **Consequences:** Measured after the change, on the same CIFS mount with no contention: the 2.8 GB ZIP64 omnibus 0.664 s / 850 pages (was hours), the 33 MB RAR-named-`.cbz` 0.513 s / 57 pages through the RAR reader (was minutes), the 18 MB directory-less file 0.014 s / rejected (was minutes), a 67 MB CBZ 0.053 s / 37 pages. A mislabeled archive now reads through the correct reader instead of failing, so some books that never imported will import. A zip whose EOCD sits further than 64 KiB from the end of the file is rejected rather than searched for; that file is not a valid zip. Regression fixtures pin all four shapes, including ZIP64 with and without prepended bytes.

## ADR-035: A scan never waits for a person: per-file verdicts in `CustomValuesStore`

- **Status:** accepted (2026-09-11, user directive; record: this commit)
- **Context:** The user's requirement was explicit: a scan of tens of thousands of files must not be found hours later stuck on file two offering a "skip this file" button, and failed, corrupt, or mislabeled files must be findable and manageable AFTER the scan. The C# has no per-file deadline — `Scanner.Stop` waits 10 s and then calls `Thread.Abort` (ComicScanner.cs:95-99), which Rust has no safe equivalent for. The port also had no place to record a per-file outcome, and the abort flag was only checked BETWEEN files, so nothing could leave a file in flight.
- **Decision:** Every file's provider work runs under a per-file deadline (`ScanFileTimeoutSeconds`, default 120 s — the slowest measured healthy open in the sample is 9.0 s under load, so this keeps more than a ten-fold margin) on its own thread, plus a "Skip Current File" control that consumes one request and abandons one file. Both outcomes record a verdict and CONTINUE; neither asks the user anything. The verdict lives in `CustomValuesStore` under the `comicrust.scan.` prefix (`status`, `error`, `detected-format`, `expected-format`, `checked`, `fingerprint`) with the status texts "Unreadable", "Timed out", "Skipped", "Format mismatch". That store already round-trips through `ComicDb.xml`, the smart-list engine already has `ComicBookCustomValuesMatcher`, and the Properties editor already lists custom values — so the schema does not change, ComicRack still reads the file, and the user's own `Tags` field stays untouched. The thumbnail chip family gains a red "!" (failure) and an amber "≠" (format mismatch) beside the existing "?" chip, with a tooltip carrying the stored reason. A `size:mtime` fingerprint lets a rescan skip a known-bad unchanged file (`ScanRetryFailedFiles` forces the re-read); a clean read clears the whole key group, so a repaired file loses its marker with no user action. `ScanResult` carries the per-run counts, and the shell reports ONE summary after the last queued scan lands.
- **Consequences:** An abandoned per-file thread is DETACHED, because Rust cannot safely kill a thread; it ends when its blocked read returns, which the `soft` CIFS mount guarantees, and its result is dropped. This is a backstop, not the main defence — ADR-034 removes the case that produced the stalls. A book that failed its scan is still IN the library (visible, queryable, fixable) instead of silently absent. Users list the problem books with a smart-list query. The matcher has NO "is not empty" operator (`ComicBookCustomValuesMatcher` uses the string operator list, and `contains ""` matches EVERY book through the C# empty-needle rule), so the "any marker" query is the regex form: `Match [Custom Value] regex "comicrust.scan.status" "."`. One verdict at a time uses `equals`. The working queries are pinned by `cr-engine/tests/scan_status_queries.rs`. The status texts are part of the user's saved queries and must stay stable.

## ADR-036: An explicit scan is one request with a one-shot forced retry

- **Status:** accepted (2026-09-12, user directive; record: `docs/archive/phases/phase-14.md`)
- **Context:** The user asked for two actions the C# does not offer in this form. (1) A right-click rescan of individual books that timed out: the C# "Refresh" scans selected paths (ComicBrowserControl.cs:2594-2618) but has no per-file deadline and no failure retry — Ctrl only forces the metadata re-read. (2) A right-click scan of one list's contents: the C# list-tree "Refresh" row exists but is UNBOUND and hidden for local libraries (ComicListLibraryBrowser.cs:308-356; `miRefresh` has no command registration), so no such action works there. Meanwhile the normal rescan skips a file whose stored failure verdict and its `size:mtime` are unchanged (ADR-035), so a plain folder scan never re-reads the books the user wants repaired.
- **Decision:** The UI scan queue entry carries explicit items + limits, not one folder string. `scan_files(paths, label, force_retry, done)` builds ONE request of one-file `ScanItem`s (no folder walk), deduplicated, empty paths dropped. The book menu "Rescan Book File(s)" scans the selected books' linked paths; the navigator "Scan List Contents" (smart lists and reading lists only, per the user's scope choice) evaluates the list through the same evaluation path the browser fills and scans the distinct non-empty paths. BOTH commands force `retry_failed` for that one request only — the global `ScanRetryFailedFiles` default false keeps its meaning for folder scans. The book command reads the selection that the C# right-click rule produced (`UpdateSelectionFromMouse`, ItemView.cs:3855-3900: an unselected target replaces the selection, a selected target keeps the multi-selection); the old "selection plus clicked target" union is deleted, because the selection is already correct when the menu opens.
- **Consequences:** A forced rescan re-pays the read cost of every selected known-bad file ON PURPOSE; a still-bad file re-marks with a fresh verdict and the summary reports it. The list command can queue a large scan when the user right-clicks a broad smart list — the same contract as a folder scan. The pre-existing LIFO scan-queue order and the multi-root early summary are NOT fixed here (out of scope; each new action submits one request). The Files grid gains the C# right-click selection rule as a side effect of the shared `emit_context` fix; its menu still reads the selection.

## ADR-037: The Comic Vine cache is a plugin-local SQLite file with two layers

- **Status:** accepted (2026-09-12, user directive; record: `docs/archive/phases/phase-15.md`)
- **Context:** The Comic Vine API has a low rate limit. The user states the limit as 200 requests per resource per hour; the API reference page at `https://comicvine.gamespot.com/api/documentation` carries NO rate-limit text at all (measured: no "200", no "per resource", no "quota", no "throttle", no HTTP 420, and a `status_code` table that ends at 105), so the figure comes from a Comic Vine statement elsewhere and is not verified here. The port cached almost nothing: `SessionCaches` (`cv/queries.rs:31-54`) lives for one scrape run and holds exactly ONE series' issue list (`:37`), `CvClient.series_details_cache` (`cv/connection.rs:49`) is also per-run, `query_image` (`connection.rs:199-235`) re-downloads every cover every time, and the only disk artifact is `prior_series.json` (a set of chosen series keys). The single defence against the limit is one fixed inter-query delay (`connection.rs:16, 44, 89-99`). Re-scraping a 500-issue series therefore re-pays 5 paged `/issues` requests plus every cover download, and nothing counts what was spent. The user's requirement: cache much more aggressively, never re-fetch a series that has ended, and track the per-resource budget closely.
- **Decision:** One SQLite file (`rusqlite`, bundled) at `$XDG_DATA_HOME/comicrust/plugins/comic-vine-scraper/cvcache.sqlite`, behind a `CvCache` trait with an in-memory test implementation. NOT `~/.cache`: a mirror built under a hard rate limit must survive a cache clean. NOT `~/.config`: ADR-033 reserves that for hand-editable configuration. The cache holds TWO layers, because they have different costs and different lifetimes. The SKELETON layer (`volume`, `issue_skeleton`: volume id, issue id, issue number) is complete, cheap, and seedable offline from an MCL file (ADR-038). The DETAIL layer (`issue_detail`, `image_blob`, `search_result`) is per-issue, expensive, and filled on demand. Freshness reads EVIDENCE, not a clock: a volume is CLOSED when its cached `count_of_issues` equals its cached issue count and its last issue's cover date is older than a configurable horizon, and a closed volume serves from the cache until the user asks for a refresh; an open volume revalidates with ONE request (`/volumes?filter=id:N&field_list=id,count_of_issues,date_last_updated`) instead of re-paging its issue list. Every request passes ONE chokepoint that writes `(resource, timestamp)` to `request_log`, so the budget survives a restart; the ceiling is a per-resource configurable key (default 200 per hour, from the user's figure) and the client slows, then BLOCKS with a visible "budget spent, resuming at HH:MM" state rather than stalling silently or failing in a burst. Only four resources are ever touched (`/search`, `/volumes`, `/issues`, `/issue`), so the budget is a four-bucket problem, not a 42-bucket one.
- **Consequences:** A new runtime dependency (`rusqlite`, bundled SQLite) enters `cr-scrape` only. This does NOT pre-empt ADR-029, which stays reserved for the LIBRARY database decision: this file is disposable and rebuildable, so the "caches are disposable, the database is not" invariant covers it, and deleting it costs only API budget. The cache can serve stale data for a volume that the closed rule misjudges (a series that resumes after a long gap); the explicit refresh is the escape, and the horizon is configurable. The budget ceiling is a GUESS until measured against the live API; the ADR records this openly and the key exists so a wrong default is a configuration change, not a rebuild. `prior_series.json` stays where it is and is not migrated.

**Correction (2026-09-12, at implementation).** The decision text above names
`/volumes?filter=id:N&field_list=id,count_of_issues,date_last_updated` as the
one-request revalidation probe. The IMPLEMENTATION uses `/volume/4050-<id>/`
instead — the singular detail resource the scraper already queries for series
details (`cv/queries.rs`). Reason: the API reference page renders its per-field
Sort and Filter marks as images, so the page text does NOT state that
`/volumes` accepts a filter on `id`, and no measurement confirmed it. The
detail resource is proven working in this codebase. The cost is the same one
request. The budget bucket is `volume`, not `volumes`.

## ADR-038: The MCL interchange format and the incremental Comic Vine sweep

- **Status:** accepted (2026-09-12, user directive; record: `docs/archive/phases/phase-15.md`)
- **Context:** The user asked to import "MCL lists" and pointed at the `Update Missing` add-on (`https://gitea.baerentsen.space/FrederikBaerentsen/ComicRack_Scripts`, `update_missing.py`). Reading that script produced two findings. (1) It pages `/issues` with `limit=100&offset=N&field_list=id,issue_number,volume&filter=date_last_updated:<start>|<end>&sort=id`. The API reference page renders its per-field Sort and Filter marks as images, so the text of the page does NOT state that `date_last_updated` is filterable; this working script is the evidence that it is. One such sweep returns every issue changed in a date window across the whole of Comic Vine, which is far cheaper than one revalidation per volume. (2) The `.mcl` file is a full snapshot of the volume-to-issue map, so importing one seeds the skeleton layer at ZERO API requests.
- **Decision:** Support the MCL format on read and on write. The format is `Missing;<date>` on line 1, then one line per volume: `<volume_id>;<issue_id>,<issue_id>,...;<issue_num>,<issue_num>,...`. Volumes are sorted; issues are sorted by issue ID, and the number list is positionally aligned to the id list. The reader must accept the source writer's REAL output, which differs from that writer's own docstring: the number list always ends with a TRAILING COMMA (only the id list is trimmed); issue numbers carry the escapes `.&@1` for `,` and `.&@2` for `;` and the source writer NEVER reverses them, so the reader unescapes; the docstring's rule "wrap the list in double quotes when it contains a space" is never emitted by that writer, so the reader accepts the quoted form and the writer does not produce it. Fixtures pin all four shapes plus volume 77901, whose issue number is `1,5`. The user SUPPLIES the file through a configuration key and a menu import; comicrust never downloads a snapshot from a third-party host. The incremental sweep uses the query above, records its progress in `sweep_state`, runs on a worker thread (Rule 9), and resumes after a restart.
- **Consequences:** The skeleton can be complete and current for a cost the rate limit permits, which is what makes the "Fill Missing Issues" command possible: the gap between the owned issue numbers of a series and the skeleton is computable offline. The skeleton carries NO titles, cover dates, images, or credits, so a scrape still needs the detail layer. The port depends on no third-party host: the `Update Missing` repository's raw endpoint currently answers `302` to `https://127.0.0.1:444` (measured 2026-09-12), which is a further reason not to fetch from it. A library series carries a Comic Vine volume id only after a scrape, so the fill command must ask the user to pick the volume when the id is unknown.

## ADR-039: View settings belong to the list, and an absent `<View>` means inherit

- **Status:** accepted (2026-09-12, user directive)
- **Context:** The user asked for the view mode (thumbnails, tiles, details) and its settings (thumbnail size, columns) to be unique to each list. ComicRack already works this way. `ComicListItem.Display` is never null (`ComicListItem.cs:223-232`), so every `<Item>` writes a `<Display>` element, and `DisplayListConfig.View` (`cYo.Common.Windows/Forms/ItemViewConfig.cs`) carries `ItemViewMode`, `SortKey`, `GrouperId`, `ItemSortOrder`, `Columns` (id, visible, width), `ThumbnailSize`, `TileSize` and `ItemRowHeight`. `ComicBrowserControl.RegisterBookList` (`:1926-1954`) applies that config when a list becomes current, and `UpdateViewConfig` (`:3355-3390`) writes the live config back when a list leaves. The port kept full serde parity for that whole subtree from the start (`cr-core/src/database/display_config.rs`) but no UI code ever read or wrote it. Instead T14 persisted ONE global workspace (`Settings.CurrentWorkspace`), which made the view application-wide: the settings did not follow the list, and a list switch discarded the sort on purpose. That was the recorded "T14 per-list sort deviation".
- **Decision:** The list owns its view settings. `ListItemBase.display.view` becomes live in both directions. When a list becomes current the port applies its `<View>` (the `RegisterBookList` equivalent); when a list leaves the port writes the live config into it (the `UpdateViewConfig` equivalent). The write is GATED on a dirty flag that every view-changing handler sets: the view mode, the item size (menu, status-bar slider and Ctrl+wheel), the column visibility, the column width drag, the header auto-size, the sort column, the sort direction and the grouper. Without the gate, merely visiting a list would freeze the current view onto it and no list would ever inherit again. `Display.View == None` therefore means "this list has no settings of its own". A list in that state changes nothing when it becomes current: the browser keeps the view it already shows. That is the C# behavior for a null config, and it is the user's explicit choice between the two candidate rules. A new command, "Reset View Settings" on the navigator context menu, clears `Display.View` and returns the list to inheriting; the row is hidden for a list that has nothing to reset. The global `WorkspaceState.view` keeps its old job unchanged: it restores the browser at startup and is saved at exit, so it is the view a fresh, never-touched list inherits.
- **Consequences:** The recorded T14 per-list sort deviation is closed. A list switch no longer clears the sort; `browserbar_probe` gate E2 asserted that clearing and its expectation changed with the behavior, not to make a failing gate pass. No ComicDb.xml schema change was needed, and no golden fixture moved: the port already wrote this subtree byte-compatibly, and it now carries real values instead of defaults. A list with no settings of its own inherits the view of whichever list the user came from, so the same list can look different depending on the navigation path; the user chose this over inheriting from the Library specifically. The navigator selection is debounced by 200 ms (`navigator.rs:45`), so a view change made inside that window is attributed to the OUTGOING list; this is inherent to the debounce and is a measured probe-timing constraint, not a defect. `StackerId`, `GroupsStatus`, `FormatId` and `LastTimeVisible` are written at their defaults — the port has no stacker, no persisted group-collapse state, no per-column format picker and no column-visibility history.

## ADR-040: The page and thumbnail activity lamp

- **Status:** accepted (2026-09-12, user directive)
- **Context:** The user asked for an animated status-bar icon for thumbnail generation, like the one the library scan already shows. This is a parity gap, not a new feature. The C# status strip carries `tsPageActivity` with `ReadPagesAnimation.gif` (`MainForm.Designer.cs:1903`), made visible once a second by `UpdateActivityTimerTick` from `Program.ImagePool.IsWorking` (`MainForm.cs:3975`), and a click calls `ShowPendingTasks` (`MainForm.cs:3517`). The port built four lamps (scan, write, export, Comic Vine) and omitted this one, so a Generate Thumbnails run showed no indicator at all.
- **Decision:** Port `tsPageActivity`. `ImagePool::is_working()` is the OR of `is_active()` over the five queues, exactly as the C# property is. The lamp animates the coalesced frames of `ReadPagesAnimation.gif`, bundled as `assets/pages/frame-N.png` on the same precedent as the existing `assets/scan/` frames, and the click opens the Tasks window, which already lists the three thumbnail queues. The per-lamp animation machinery in the status bar is refactored into one `AnimLamp` type shared by the scan lamp and this one, so the frame timer runs only while its lamp is visible.
- **Consequences:** The lamp reports page decoding as well as thumbnail creation, because `IsWorking` covers all five queues; that matches the C# name and behavior. It gives no count and no percentage — the Tasks window carries the pending rows and the "Abort Cover Generation" action. The packaging scripts gained `pages` in their asset-kind list; an asset directory missing at runtime degrades to the static `ThumbView.png` with no animation rather than failing.
