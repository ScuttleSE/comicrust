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
