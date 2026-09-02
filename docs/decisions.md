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