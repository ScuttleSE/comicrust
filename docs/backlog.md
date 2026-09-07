# Backlog

This file collects the open work that no active phase owns. An agent
that picks an entry moves it into the kickoff that will own it.
Locked scope decisions stay in `docs/decisions.md`. Per-phase
omissions stay in that phase's kickoff tracker.

## Policy

- **No scripting host — port natively on demand** (ADR-027,
  recorded here 2026-09-06): comicrust ships no script or plugin
  engine. No Python host, no plugin hooks, no `.crplugin` packages.
  A script or plugin gets support only when a user asks for its
  behavior. Port the behavior into the app natively. Do not add a
  scripting surface. Example: the NewComics.py flow landed natively
  as the "New fileless Book Series…" dialog (Phase 6).

## From Phase 1 (open items)

- **WebComicProvider (`.cbw`)** — the one open reader. The C# spec is
  `WebComic.cs` (853 LOC): a URL template plus a regex PagePart
  engine that parses fetched HTML, compositing, HTTP fetch, and
  `FileCache` interplay. Port it as a standalone task. Verify with a
  local fixture HTTP server (a std `TcpListener`).
- **PDF and DjVu writers** (`cr-io`, T5 tail) — the CBZ/CBT/CB7 and
  folder writers are done. These two formats still have no
  write-back. Low use, as `phase-1-kickoff.md` predicted.
- **HEIF/AVIF/J2K page decode** — the decode chain reports
  UnsupportedFormat for these formats. The codecs need system
  libraries (libheif, openjpeg). Decide at packaging time.

## From port-plan §6

- **Browser dock modes (Fill + Bottom)** — Phase 5.5 T10, moved out
  on 2026-09-05 by user decision (ADR-026). Spec: the reader area
  fills the window. The browser docks to the Bottom inside a
  resizable, collapsible container. F3 toggles the docking. A
  docking-mode button sits on the tab strip. `PanelSize` persists.
  C# spec: `MainForm.cs:679-701` (BrowserDock),
  `MainForm.cs:3629-3716` (dock changed + grip),
  `Views/MainView.cs:199-221` (the alignment button),
  `Config/DisplayWorkspace.cs` (PanelSize). Left/Right stay dropped
  (ADR-024). Pick up when the tab-strip layout work resumes. T14
  persistence carries the mode and the panel size once it lands.
- **Sidebar preview pane (SmallComicPreview)** — Phase 5.5 T11,
  moved out on 2026-09-05 by user decision (ADR-026). Spec: a
  collapsible pane below the navigator. It shows the first selected
  book's cover and caption with the mini toolbar (Open / First /
  Prev / Next / Last / Two Pages / Refresh / Close). Browse ▸ Small
  Preview (Shift+F7) toggles it. Selection debounces for 500 ms.
  C# spec: `Views/SmallComicPreview.cs` + Designer,
  `ComicExplorerView.cs:294-307`. The Browse ▸ Small Preview menu
  item stays a disabled stub until picked up.
- **WikiSearch editor context links** — moved to the backlog on
  2026-09-06, user decision. Spec: the C# `SearchEngines.cs`
  built-in — a Wikipedia `INetSearch` engine. The engine registers
  into the book editor's text-box context menus
  (`ComicBookDialog.cs:137`, `TextBoxContextMenu.AddSearchLinks`)
  and the ListSelectorControls. The scripting NetSearch providers
  die with ADR-027. The native single-engine surface is small and
  optional. Pick up with any book-editor polish work.
- **Remaining `DisplayWorkspace` persistence keys** — Phase 5.5 T14
  leftover. The workspace save already carries the browser view, the
  reader layout, and the display family. These keys wait on their
  owner features:
  - `PanelSize`/`PanelDock` — waits on the dock-modes entry above.
  - `FileView` — the Files browser is unported.
  - `PagesViewConfig` — the Pages panel keeps its defaults.
  - `ComicBookDialogPagesConfig` — the editor pages list keeps its
    defaults.
  - `ScriptOutputBounds` / `PreferencesOutputSize` /
    `ComicBookDialogOutputSize` — dialog geometry. Low value. Pick
    up with any dialog-resize need.
  - `UndockedReaderBounds` / `UndockedReaderState` — the undock
    session state. The C# also treats it as transient. Revisit only
    if users ask.

  The T14 element names are already reserved in
  `cr-core/src/settings/workspace.rs`. The reader is order-tolerant.
  Adding keys is a write-side change only.

## From Phase 7 (deferred 2026-09-06, ADR-028)

The four deferred Phase 7 areas, each with its research record so a
future agent does not re-study the C# source.

### Device sync (all of it)

The full sync subsystem: the engine, the disk/MTP providers, the
wireless protocol, and the device dialogs. Research record
(2026-09-06):

- **Engine** — `ComicRack.Engine/Sync/`, 2,301 LOC / 13 files.
  `StorageSync.Synchronize` (`StorageSync.cs:80-237`): read the
  device's `*.cbp` books → pull back the dirty fields per the six
  `ExtraSyncInformation` flags (reading state, rating/manga/review,
  bookmarks, page types, checked, info) → per-list `LimitList`
  (sort key, unread-only + the `SyncKeepReadComics` read-prolog,
  count/MB/GB caps, round-robin interleaved across series groups,
  the global `BookSyncLimit`) → remove + add → `sync_information.xml`
  + the `comicrack.ini` marker rewrite. A `.cbp` book is a CBZ of
  converted pages (JPEG; WebP only if the device advertises it) with
  a full-`ComicBook` `<name>.cbp.xml` sidecar
  (`SyncProviderBase.GetPortableFormat`, `:480-513`; "optimized"
  mode: max height 1500, JPEG quality 65, optional sharpen,
  thumbnails). The port's cr-io export engine already covers the
  conversion/packing.
- **Queues** — one sync job per device on `DeviceSyncQueue` (lowest
  priority, `QueueManager.cs:344`); inside the provider a write
  queue with back-pressure 50 (`SyncQueueLength` ini key) and a
  device-access lock. The ProcessingQueue port exists.
- **Providers** — `DiskDriveSyncProvider` (84 LOC, plain file IO)
  ports as-is. The 3,748-LOC Windows WPD COM interop is NOT needed:
  on Linux an MTP phone mounts through GVFS at
  `/run/user/$UID/gvfs/mtp:host=...`, so the provider is "find the
  mounted volume containing `comicrack.ini` (max 10 levels deep) +
  POSIX IO" — ~150 LOC in the DiskDrive provider's shape. No libmtp.
  `WirelessSyncProvider` (624 LOC) is the raw-TCP Android protocol:
  the PC connects to the device on port 7614, 14 single-byte
  commands, big-endian int32 framing, length-prefixed UTF-8/blobs;
  UDP discovery group `224.34.123.90:7615` (`"ComicRack:<key>[:Sync]"`
  messages); a TCP control listener from port 7620; pairing-key
  validation (`AndroidKey`/`AndroidDebugKey`). Fully portable, but
  only useful against the old ComicRack Android app.
- **UI** — `DevicesEditDialog` (210 + 217 Designer), the per-device
  `DeviceEditControl` (490 + 328: the shared-lists checkbox tree +
  per-list options group), `DeviceSelectDialog` (71 + 118: the
  discovery list); the navigator "Sync with {device}…" context item;
  the status-bar sync lamp; Preferences: the extra Wi-Fi addresses +
  Test (`PreferencesDialog.cs:215-225`).
- **Settings** — `Settings.Devices` (the `DeviceSyncSettings` list,
  a Config.xml element) + `ExtraWifiDeviceAddresses`; ini keys
  `Sync*`/`WifiSync*`/`FreeDeviceMemoryMB`
  (`EngineConfiguration.cs:462-592` — none in the C# Preferences UI).
- **Port path** — a `cr-engine` sync module (~2,200 LOC portable) +
  a `cr-ui` device dialog set; own kickoff when picked up.

### HTTP remote library

ADR-005 stands: the WCF net.tcp wire protocol (net.tcp framing,
message security, embedded X509, `DataContract object` payloads) is
not preserved — any port is a NEW API with NEW clients. Research
record (2026-09-06):

- **Live surface** (`ComicLibraryServer.cs`, 571 LOC): 5 read + 1
  write operations — `GetLibraryData` (the whole filtered
  `ComicLibrary` as BZip2'd .NET XmlSerializer bytes — the cr-core
  model + Emitter can produce it byte-stably), `GetImageCount`,
  `GetImage` (JPEG bytes, quality scales q75), `GetThumbnailImage`
  (the already-ported `ThumbnailImage` serialization), `UpdateComic`
  (a reflection property-setter through the registry — the port has
  the registry), `IsValid`; plus the unsecured `Info` endpoint
  (Id/Name/Description/Options). Default port 7612 (`-isp`/`-psp`
  already parsed in the port's ExtendedSettings), path `/Share`,
  `/Share2`… per share. One shared password per share (the username
  is the constant `"ComicRack"`); no password = open. A
  private-network guard (`OnlyPrivateConnections`).
- **Discovery** — UDP broadcast port 7613 (`cYo.Common/Net/
  Broadcaster.cs`, 188 LOC): BZip2-compressed XML
  `{BroadcastType, ServerName, ServerPort}`, types
  Client/Server × Started/Stopped; servers re-ping every 10 s;
  clients resolve the answer through the Info endpoint. Linux
  replacement: mDNS (AVahi) or keep the broadcast format.
- **Client** — `ComicLibraryClient` (234 LOC) +
  `RemoteComicBookProvider` (74) + the UI (`RemoteConnectionView`
  255, `OpenRemoteDialog` 207, the Preferences `ServerEditControl`
  123, the Tasks-dialog Network stats tab — an ADR-024 omission).
  Remote books carry `FileLocation = "REMOTE:{libId}\\{path}"` and a
  provider that fetches pages over the connection.
- **Dead weight** — the entire public-internet listing
  (`ServerRegistration.cs`, the `IsInternet` flag) is commented out
  or hard-disabled in CE.
- **Port path** — a small HTTP/JSON server in `cr-engine` (library
  XML + page/thumb bytes), share config in `Settings.Shares`, mDNS
  discovery; a client (mobile/web) is the actual product question —
  without one there is nothing to talk to.

### Tray icon

- C# (`MainForm.cs:4057-4127` + `-hidden`, `MinimizeToTray`,
  `CloseMinimizesToTray`): a NotifyIcon with a context menu,
  left-click restore. `ExtendedSettings.start_hidden` is already
  parsed in the port (inert today — recorded in the T1 deviations).
- Linux: a StatusNotifierItem implementation (`ksni` or
  `tray-item`); GNOME shows SNI icons only with an extension — gate
  the feature on availability. Pick up with any platform task.

### i18n — the TR port (machine + the string sweep)

The Preferences language page (the Phase 5 T1 deferral) and the
localized operator lists (`cr-engine/src/matcher/spec.rs` note) wait
on this. Research record (2026-09-06):

- **Machinery** (`cYo.Common/Localize/`, ~700 LOC across 5 files):
  a TR context is one named string map; `TR.Load(name)` caches by
  name process-wide (first load wins — language change is
  restart-only, port the same); lookup `tr["key", "default"]` —
  missing or EMPTY text → the inline English default. Well-known
  contexts: `Default` and `Messages`.
- **Files** — `<ResourceFolder>/<CultureName>/<ContextName>.xml`:
  root `<TR Name CultureName>`, one `<Texts>` wrapper of
  `<Text Key Text Comment/>` (Comment carries the English source for
  completion tooling). Two-level culture merge: the BASE culture
  loads first (`pt-BR` → `pt`), the specific overrides by key.
  Escape decode at load: `\n` → `\r\n`, `\r` deleted, `\t` → tab.
  Parse failures degrade to English (never crash). Entries sort by
  Key on save. No plurals (manual `ComicSingle`/`ComicMulti` pairs);
  `GetStrings(key, array, sep)` pipe lists with a count guard.
  Optional zip-pack overlay (`PackedLocalize`) over the loose files.
- **Metadata** — per-language `LanguageInfo.xml` (`TRInfo`):
  Author/Notes/Language/RightToLeft. `InstalledLanguages` scans
  `Languages/*/LanguageInfo.xml` and computes completion % against
  the FRENCH pack (the C# quirk — port as-is).
- **Key conventions** (`LocalizeUtility.cs:50-112`): control-tree
  localization by widget Name (`.Tooltip` suffix when the designer
  tooltip differed; ListView columns key `"col" + header text`;
  combo items `Name + ".Item" + i`; UserControl children skip — they
  self-localize). Enum translation: key = member name, default =
  the Description attribute `PascalToSpaced` (contexts
  `ComicPageType`, `ItemViewMode`, `ImageRotation`,
  `ComicPagePosition`…). ~430 manual lookup sites + 42 forms in the
  C#; the port's options builder already mirrors
  `FillPanelWithOptions` (`tr[p.Name, p.Description]`).
- **Selection** — `Settings.CultureName` (Config.xml) overridden by
  `ExtendedSettings.Language` (`-l`; ini:false). Applied at boot;
  restart-only.
- **Packs** — 19 languages ship in `ComicRack/Output/Languages/`
  (cs-CZ, de, el-GR, es, fi, fr, hr, hu, it, ja, nl-BE, pl, pt-BR,
  ru, sk-SK, tr, zh, zh-CN, zh-Hans), ~70 contexts, 37-85 XML files
  each. Reused AS-IS: bundle as `cr-ui/assets/languages/` and add
  the folder to both release workflows (the papers pattern).
- **Port path** — the load/merge/escape core (cr-core or
  `cr-ui/src/tr.rs`), then the sweep: (a) shell chrome (menubar
  table, toolbars, status bar, dialogs, context menus), (b) engine
  captions (columns, groupers, matchers, page types, Messages).

### Auto-update check + news feed

- C# startup: `CheckForUpdateAsync` (`MainForm.cs:4530-4574`) —
  GitHub compare API `repos/maforget/ComicRackCE/compare/<sha>...nightly`,
  status `ahead` → the Download/Zip/No prompt + never-again; skipped
  for dirty builds. The `NewsFeeds.xml` feed + `Settings.NewsStartup`
  → the News dialog (an MSHTML panel — dead with ADR-024/027; a
  native feed reader would replace it). The "Check For Update…"
  menu item is an ADR-024 omission.
- Linux packaging handles updates; port only if the user asks.

### Crash watchdog dialog

- C# (`CrashWatchDog.cs` + `CrashDialog.cs`): unhandled-exception →
  a report dialog (program info + all thread stacks) with
  Retry/Restart/Quit; Retry breaks a UI-freeze watchdog the port
  does not have.
- Rust: `std::panic::set_hook` → log + a best-effort report dialog
  (a panicked GTK main loop usually cannot recover — exit is
  acceptable). Pick up with Phase 8 polish if wanted.

## User requests (2026-09-07)

### Komga server connection

- Add the ability to connect to a remote Komga server. When
  connected, the Komga server appears as a second Library node in
  the Library view (the navigator tree), next to the local Library
  node.
- This is a NEW feature (no C# counterpart). The Komga REST API
  (OpenAPI-documented: `/api/v1/libraries`, `/api/v1/books`,
  `/api/v1/books/{id}/file/...`, API-key auth) maps to a new
  provider side: the node lists Komga libraries/series as books,
  pages fetch over HTTP, and read-progress round-trips through
  Komga's `markReadProgress` endpoints. Scope the read-state and
  metadata write-back when picked up.
- Needs the navigator's Library-node model to allow a second
  source (today the tree hardcodes the single local Library).

### Usenet support (NZB indexer + SABnzbd)

- Add usenet download support: take NZBs (from an indexer) and
  hand them to SABnzbd for download; downloaded comics land in the
  library (the watch-folder/scan path covers the ingestion side
  today).
- NEW feature (no C# counterpart). Likely shape: an SABnzbd API
  client (`add`, queue/status commands — SABnzbd exposes a simple
  HTTP JSON API with an api-key), NZB file association/import, and
  a download queue surface (a Tasks-dialog section or a
  navigator node). Scope the indexer side (NZB search/browsing)
  separately when picked up.

## From Phase 8 (deferred 2026-09-07)

### C#-parity per-book proposed cache (plan B)

The Phase 8 perf work (T3 + the view-side sweep) computes the
ComicNameInfo parse per OPERATION (per import, per rebuild, per
matcher evaluation) with the `needs_prop` gate
(`cr-engine/src/matcher/book_view.rs`). The C# caches `Proposed` on
the ComicBook instance (`OnParseFilePath`): one parse per book per
SESSION, reused by every consumer. Plan B would mirror that with a
process-wide per-book cache (a `HashMap<CrGuid, ComicNameInfo>` in
the Library session, or a serde-skipped `ComicBook` field), dropping
the remaining per-rebuild parse cost (the `MatchContext` lazy parse
still pays one parse per parse-needing book per evaluation — the
10k-library quick-search keystroke case).

- **Why deferred**: after the per-operation precomputes, the measured
  hot paths are scalar (sort 5000 books 4.5 ms, group pass 78 µs,
  duplicates 1000 books 5.9 ms, CBL import 0.069 s at 2886×255 —
  release). No remaining user-visible wait justifies the invalidation
  risk: a stale cached parse after a file edit/rename/scan would
  silently break series matching and sorting. Revisit only if a
  measured rebuild cost reappears (a 10k+ library with
  parse-needing books).
- **Invalidation surface if picked up**: `refresh_file_info`, the
  scanner (add/move/recover), `apply_edited`/editor commits, the
  write-back path, the Windows-path migration (T11), `file_path`
  changes of any kind. The cache key must cover the file path + the
  file modified time, or the invalidation must ride those sites.
