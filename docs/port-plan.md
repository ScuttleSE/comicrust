# Port Plan: ComicRackCE → Rust + GTK4

Companion to `feasibility.md` (evidence) and `decisions.md` (locked scope). Estimates assume one experienced developer working ~full-time. Divide durations for teams.

## 1. Architecture — cargo workspace

```
comicrust/
├── crates/
│   ├── cr-core      # Data model, ComicDb.xml serde, settings, filename parsing, property registry
│   ├── cr-io        # Comic providers (zip/tar/7z/rar/pdf/folder/web), ComicInfo write-back
│   ├── cr-image     # Image currency type, codecs, resize/adjust filters, page/thumb caches
│   ├── cr-engine    # Smart-list parser + matchers, queue manager, scanner, watch folders, backup, sync, remote
│   ├── cr-script    # PyO3 plugin host, #@Directive loader, .crplugin packages, host API shim
│   ├── cr-ui        # GTK4: reader (GtkGLArea), ItemView browser, shell, dialogs, theming, i18n
│   ├── cr-cli       # Headless verification binary (info, db-dump, scan) — the port's test harness
│   └── cr-app       # Main binary: D-Bus single instance, i18n wiring, packaging
```

The crate split mirrors the C# project boundaries. Porting stays mechanically traceable, file by file. `cr-cli` enables headless differential testing against real ComicRack data **before any UI exists**.

## 2. Technology mapping

| C# / Windows | Rust / Linux |
|---|---|
| WinForms + GDI+ chrome | gtk4-rs + cairo, custom CSS (no libadwaita — ADR-004) |
| Tao.OpenGL reader renderer (wgl-on-HWND, tiled textures) | `GtkGLArea` + `glow` on GL 3.2 core, cairo fallback first |
| `System.Drawing.Bitmap` (universal image currency) | `image` crate types, ~2k LOC unsafe filter port |
| Codecs: GDI+ (jpg/png/gif/tiff), libwebp, jxl, libheif, CSJ2K, pdfium, ddjvu | zune-jpeg, image-webp, jxl-oxide, libheif-rs, j2k crate, pdfium-render, djvulibre subprocess |
| 7z.dll COM + 7z.exe | zip/tar crates, 7z subprocess, RAR5 via 7z/libarchive subprocess (ADR: no unrar static link) |
| ComicDb.xml (XmlSerializer) | serde + quick-xml, exact element/attribute fidelity, golden-file tests |
| `ProcessingQueue` / ThreadPool | crossbeam channels + scoped worker threads |
| WCF net.tcp remote + UDP broadcast | **not protocol-compatible** (ADR-005), future HTTP/JSON API + mDNS if wanted |
| NTFS ADS metadata (`NtfsInfoStorage`) | xattrs `user.comicrack.*` + sidecar fallback |
| WPD/MTP sync | libmtp, USB copy via std, wireless TCP protocol portable as-is |
| Registry associations / SHFileOperation / recycle | xdg-mime, GIO trash |
| Single instance (WCF named pipe) | zbus / D-Bus |
| uxtheme dark mode, theme color tables | GTK CSS providers + native dark preference |
| IronPython 2.7.4 host | PyO3 + CPython 3, `ComicRack`/`ComicBook` shim, same `#@Directive` + `.crplugin` formats |
| MSHTML `ObjectForScripting` panels | WebKitGTK `messageHandlers` |
| `TR.Load()["key"]` XML localization (19 langs) | same XMLs loaded by a Rust `TR` port — reused as-is |
| BinaryFormatter `cache.idx` | fresh format (caches are disposable, no compat) |

## 3. Roadmap

Every phase ends shippable and testable. Phases 0-2 are fully headless. They de-risk data compatibility with zero UI exposure.

| # | Phase | Key work | Gate / exit criteria | Est. |
|---|---|---|---|---|
| 0 | Core model | Workspace scaffold + CI, serde model of ComicBook/ComicInfo/MetronInfo/PageInfo mapped field-by-field against `ComicInfo.cs`/`ComicBook.cs`, ComicDb.xml load/save with `.bak`/`.restore` semantics, settings/ini, `ComicNameInfo` filename parsing, property registry skeleton | Round-trip a real `ComicDb.xml` byte-stable (golden tests), `cr-cli info` / `db-dump` work | 6-8 wk |
| 1 | IO + images | All readers (zip/tar/7z/rar/pdf/folder/web), image pipeline + decode chain, resize/adjust filters, ImagePool/DiskCache caches, thumbnail generation, ComicInfo.xml write-back into archives | `cr-cli` opens every supported format, extracts pages, generates thumbs, write-back verified against originals | 8-10 wk |
| 2 | Engine | Query tokenizer + 76 matchers + comparers/groupers, smart-list persistence, QueueManager (5 queues), scanner, watch folders, backup manager | Smart lists from a migrated library evaluate identically (fixture tests), scanner runs unattended | 8-10 wk |
| 3 | Reader UI | GTK4 shell skeleton, GL renderer port: single/double/adaptive/continuous layouts, fit modes, zoom/pan/rotation, transitions, magnifier, paper texture, gestures, fullscreen/undock, tabs | Comfortable daily-driver reading session | 10-12 wk |
| 4 | Browser | ItemView port (thumbnail/tile/detail, grouping, stacking, columns, sort, rubber-band, drag-drop), library tree, search popover, QuickOpen, PagesView | Library browse/manage replaces C# browser for common flows | 10-12 wk |
| 5 | Dialogs | All ~50: book editor, bulk edit, preferences (+ serde-driven options builder), smart-list/matcher editors, export, devices, workspace save/switch | Feature-complete for local-library workflows | 12-14 wk |
| 5.5 | UI chrome parity | Menubar, toolbars (reader/browser/navigator/pages), multi-panel status bar, book tabs + context menu, browser dock modes (Fill/Bottom), sidebar preview, Book Display Settings, About/Zoom/QuickRating/Tasks, bundled CR icons, layout persistence — see `phase-5.5-kickoff.md` (ADR-024) | Chrome close to original CR with locked omissions; every task user-tested | 8-10 wk |
| 6 | Scripting | PyO3 host, hook wiring (Automation menus, NetSearch, overlays, info panels), package manager, WebKitGTK panel bridge, 2to3 migration guide + top-5 plugin acceptance tests | Shipped sample scripts + ComicVine-class plugin operational | 6-8 wk |
| 7 | Platform | D-Bus single instance, MTP/wireless sync, HTTP remote server, full i18n wiring, workspace persistence (the dark/light toggle + theme-following views landed in 5.5 — ADR-025) | Feature checklist from C# complete | 8-10 wk |
| 8 | Polish/ship | Flatpak/.deb/AUR packaging, CI, docs, migration tooling, perf passes | 1.0 | 4-6 wk |

**Total: ~75-90 weeks (~18-22 months) solo.** Longest-lead items: ItemView behavior parity and dialog volume.

## 4. Sequencing rationale

1. **Data compat first (0-2):** the database is the only unlosable artifact. Proving a byte-stable round-trip before UI means the riskiest compat work happens while the codebase is small.
2. **Reader before browser (3 before 4):** the reader is the emotional core. It validates the GL/cairo rendering strategy. The browser widget is the single largest custom build. It benefits from the reader's widget infrastructure.
3. **Scripting late but before polish (6):** the plugin API shape stabilizes only after the app surface exists. It precedes polish because ecosystem compat may force host-API changes.
4. **Platform integration last (7):** sync and remote are isolated modules. Deferring them avoids coupling their APIs to an unstable engine.

## 5. Kickoff

## 6. Backlog

Deferred ideas and non-urgent findings. Anything phase-scoped lives
in that phase's kickoff tracker instead (`phase-<N>-kickoff.md`,
"Omitted / postponed per task"); locked scope decisions live in
`docs/decisions.md`. An agent picking work from here should move
the entry into the kickoff that will own it.

- **Build selected bundled scripts into the app natively** (Phase 5.5
  T3 finding, 2026-09-05): some of the C#'s menu items are actually
  IronPython scripts under `ComicRack/Output/Scripts/` — e.g. File ▸
  Automation ▸ "New fileless Book Series..." is `NewComics.py`
  (`#@Hook NewBooks`). Candidates worth a native Rust port instead of
  Python-plugin machinery: NewComics.py (fileless series/entries),
  and any other high-traffic sample scripts reviewed at Phase 6. The
  rest keep the Phase 6 plugin host.

Phase task breakdowns with acceptance criteria:

- Phase 0: `phase-0-kickoff.md` — built and validated (see `AGENTS.md` status).
- Phase 1: `phase-1-kickoff.md`.
- Phase 5.5: `phase-5.5-kickoff.md` — the UI-parity phase (ADR-024), inserted between 5 and 6.