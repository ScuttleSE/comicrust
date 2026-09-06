# Backlog

This file collects the open work that no active phase owns. An agent
that picks an entry moves it into the kickoff that will own it.
Locked scope decisions stay in `docs/decisions.md`. Per-phase
omissions stay in that phase's kickoff tracker.

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
