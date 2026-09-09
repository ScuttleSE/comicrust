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
| WPD/MTP sync | **deferred to backlog (ADR-028)** — GVFS-mounted MTP volumes collapse the 3,748-LOC Windows WPD interop to a ~150-LOC provider; wireless TCP protocol portable as-is |
| Registry associations / SHFileOperation / recycle | xdg-mime, GIO trash |
| Single instance (WCF named pipe) | GApplication unique mode (D-Bus underneath) |
| uxtheme dark mode, theme color tables | GTK CSS providers + native dark preference |
| IronPython 2.7.4 host | **dropped (ADR-027)** — native modules replace scripts; no Python runtime |
| MSHTML `ObjectForScripting` panels | **dropped (ADR-027)** — the plugin HTML panels die with the scripting host (the News dialog is an ADR-024 omission) |
| `TR.Load()["key"]` XML localization (19 langs) | same XMLs loaded by a Rust `TR` port — reused as-is (**deferred to backlog, ADR-028**) |
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
| 5.5 | UI chrome parity | Menubar, toolbars (reader/browser/navigator/pages), multi-panel status bar, book tabs + context menu, Book Display Settings, About/Zoom/QuickRating/Tasks, bundled CR icons, layout persistence — see `phase-5.5-kickoff.md` (ADR-024; dock modes stay Fill-only per ADR-026) — **COMPLETE, all tasks user-tested (2026-09-06)** | Chrome close to original CR with locked omissions; every task user-tested | 8-10 wk |
| 6 | Native features + de-scripting | Native "New Comic…" fileless flow + "New fileless Book Series…" dialog (the NewComics.py port, ADR-027), `Expression`/plugin matcher parse-compat (not-supported evaluation), Copy Page/Export Page, `cr-script` removal — **COMPLETE, all tasks user-tested (2026-09-06)** | Feature checklist complete with no scripting surface; matcher round-trip stable | 2-3 wk |
| 7 | Platform | D-Bus single instance + the startup file pipeline (re-scoped 2026-09-06, ADR-028: sync, remote, tray, i18n → `docs/backlog.md` with research records) — **COMPLETE, all tasks user-tested (2026-09-06)** | Second-launch handoff parity: focus, files (`newSlot`/`-p`/hide-browser), restart handshake | 1 wk |
| 8 | Polish/ship | Flatpak/.deb/AUR packaging, CI, docs, migration tooling, perf passes | 1.0 | 4-6 wk |
| 9 | Database backend | SQLite canonical store (ADR-029; hot columns + per-book dirty tracking + incremental saves), XML becomes the ComicRack import/export codec, Settings migration + "Export for ComicRack…", backup swap — see `phase-9-kickoff.md` | Migration verified on the real-world fixture; WAL crash-safety + sqlite↔XML round-trip gates green | 3-5 wk |

**Total: ~75-90 weeks (~18-22 months) solo.** Longest-lead items: ItemView behavior parity and dialog volume.

## 4. Sequencing rationale

1. **Data compat first (0-2):** the database is the only unlosable artifact. Proving a byte-stable round-trip before UI means the riskiest compat work happens while the codebase is small.
2. **Reader before browser (3 before 4):** the reader is the emotional core. It validates the GL/cairo rendering strategy. The browser widget is the single largest custom build. It benefits from the reader's widget infrastructure.
3. **Native features at 6 (ADR-027):** the scripting host is dropped; the phase delivers the C#'s native features that the scripts obscured (fileless books) plus the de-scripting cleanup, before platform work (7).
4. **Platform integration last (7):** the single-instance/startup plumbing is isolated and small. Sync and remote defer to the backlog (ADR-028) — sync is an isolated module, and the remote needs a new wire API plus a client before it has any user.

## 5. Kickoff

## 6. Backlog

Deferred ideas and non-urgent findings. Anything phase-scoped lives
in that phase's kickoff tracker instead (`phase-<N>-kickoff.md`,
"Omitted / postponed per task"); locked scope decisions live in
`docs/decisions.md`. An agent picking work from here should move
the entry into the kickoff that will own it.

The collected backlog now lives in **`docs/backlog.md`** (the open
Phase 1 items + the former entries of this section). Phase task
breakdowns with acceptance criteria:

- Phase 0: `phase-0-kickoff.md` — built and validated (see `AGENTS.md` status).
- Phase 1: `phase-1-kickoff.md`.
- Phase 5.5: `phase-5.5-kickoff.md` — the UI-parity phase (ADR-024), inserted between 5 and 6. COMPLETE (2026-09-06).
- Phase 6: `phase-6-kickoff.md` — the re-scoped native-features phase (ADR-027). COMPLETE (2026-09-06, user-tested). The original scripting kickoff survives as a superseded record inside that file.
- Phase 7: `phase-7-kickoff.md` — the platform phase (D-Bus single
  instance + the startup file pipeline, ADR-028 re-scope). COMPLETE (2026-09-06).
- Phase 8: `phase-8-kickoff.md` — polish/ship + the user-reported list. CLOSED (2026-09-08); T1-T6, T10, T11 done + user-tested, T7 → Phase 9, T8 + HEIF/AVIF deferred to the backlog.
- Phase 9: `phase-9-kickoff.md` — the database-backend phase (SQLite
  canonical, ADR-029 gate). Spun out of Phase 8 T7 (2026-09-07);
  DEFERRED to the backlog 2026-09-08 (user decision) — no active
  phase; whoever picks it up re-homes the kickoff first and starts
  at T1 (the spike) → T2 (ADR-029 + user sign-off).