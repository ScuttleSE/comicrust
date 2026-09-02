# Feasibility Analysis: ComicRackCE → Linux-native Rust + GTK4

*Analysis date: 2026-09-02, against ComicRackCE `master` @ `7fd9fd3`.*

## 1. Source inventory

Total: **1,348 .cs files, ~193,000 LOC** (~164k hand-written, ~28k designer-generated). All projects target `net48` with `UseWindowsForms=true`.

| Project | Files | LOC | UI share |
|---|---|---|---|
| ComicRack | 172 | 59,857 | all UI (shell, dialogs, views) |
| ComicRack.Engine | 587 | 46,912 | ~11-15% UI |
| cYo.Common | 326 | 37,448 | ~30% Windows/UI-specific |
| cYo.Common.Windows | 161 | 30,191 | all UI (custom controls) |
| cYo.Common.Presentation | 71 | 8,613 | all UI (renderers, Ceco text engine) |
| ComicRack.Engine.Display.Forms | 10 | 7,660 | all UI (reader controls) |
| ComicRack.Plugins | 21 | 1,946 | scripting host |

**Estimate: ~65-75% of Engine + cYo.Common is UI-free portable logic.** The dominant cost is the WinForms/GDI+/Shell UI shell, which a GTK4 rewrite replaces wholesale anyway.

## 2. Windows-coupling audit

### P/Invoke & COM (148 `DllImport` sites in ~45 files; 48 `ComImport` interfaces)

| Dependency | What it provides | Linux replacement |
|---|---|---|
| user32 (27 sites) | message-based control skinning, layered splash windows, DPI awareness, drag cursors | not needed (GTK) |
| kernel32 (25) | LoadLibrary for 7z/jxl/libheif, long-path enumeration, **NTFS ADS metadata**, prevent-sleep, auto-restart | std / xattrs / `org.freedesktop.ScreenSaver` / n/a |
| shell32 (16) | file icons, shell-namespace tree, recycle-bin delete, "reveal in Explorer" | GIO/GVfs, xdg portals |
| gdi32 (18) | font measurement, custom text rendering (Ceco) | Pango/cairo |
| uxtheme/dwmapi (8) | **dark-mode engine** (undocumented ordinals #133/#135/#136) | GTK CSS + libadwaita-style pref |
| credui (3) | credential prompts for remote access | GTK dialog |
| 7z.dll COM (19 interfaces) | all archive read/write via COM `IInArchive` | zip/tar crates, 7z subprocess |
| WPD COM (8 classes) | MTP device sync | libmtp |
| IFileOperation/IShellFolder | shell copy/delete/trash | GIO trash |
| OLE IDataObject | drag-drop of virtual in-archive files | GTK DnD with content providers |
| MSHTML WebBrowser | plugin HTML panels, News dialog | WebKitGTK |
| **WCF net.tcp** | remote library server (Android protocol), wireless sync, single-instance | new HTTP API (compat dropped); zbus for single-instance |

### Bundled native codecs (paths hard-baked, `SetDllDirectory` loading)

libwebp (14 sites), jxl (23 sites, full decoder+encoder wrapper), libheif+libde265+libx265+aom (via LibHeifSharp 3.2.0), pdfium (PDFiumSharpV2), djvulibre (via `ddjvu.exe`/`c44.exe` subprocess), 7z.exe console fallback. **Every one has a first-class Linux equivalent.**

### Other platform APIs

Registry (file associations, Ghostscript discovery), SystemEvents (display/power/theme), WMI (memory), WindowsAPICodePack (taskbar/jumplist), Windows7.Multitouch (reader gestures), `AddMessageFilter` mouse-wheel forwarding, `IWin32Window` leaking into plugin APIs (59 uses).

**Notably absent:** no WPF/XAML, no WIC, no global message hooks, no DPAPI, no WinRT projection, no .NET DLL plugin loading, no AppDomain sandboxes.

## 3. Engine portability (ComicRack.Engine + cYo.Common)

| Area | Reference | Rust assessment |
|---|---|---|
| Archives | `IComicAccessor` + `ArchiveComicProvider` + 3 engine backends (7z COM / SharpCompress / SharpZipLib) | **Easy** — zip/tar crates; RAR via 7z/libarchive subprocess (unrar licensing) |
| Image decode | normalize-to-JPEG chain: DjVu (exe), WebP/JXL/HEIF/AVIF (P/Invoke), JPEG2000 (CSJ2K managed), PDF (pdfium ≤1920×2540) | **Moderate** — image crate + zune-jpeg/image-webp/jxl-oxide/libheif-rs/pdfium-render; djvulibre subprocess (already how it works) |
| Image processing | `ImageProcessing.cs` 1,557 LOC unsafe resizers (nearest/bilinear/bicubic/HQ box), `BitmapAdjustment` brightness/contrast/gamma (the reader color filter), convolution, histogram | **Moderate** — direct port of ~2k LOC of pixel loops |
| Page/thumb caches | `ImagePool` (5 queues), `ImageManagerBase` LRU + `DiskCache` (BinaryFormatter `cache.idx`) | **Easy** — channels+LRU; cache format is disposable, no compat needed |
| Database | single `ComicDb.xml` (XmlSerializer), `.bak`/`.restore` rotation, corrupt-file quarantine, zip backup; optional MySQL/MSSQL shared library (XML blobs + change counters) | **Easy** — serde + quick-xml with exact name fidelity; sqlx for shared mode |
| Data model | `ComicInfo` ~40 fields, `ComicBook` +60 state fields (3,076 LOC), `ComicPageInfo`, `MetronInfo` (1,789 LOC), `ValuesStore` custom values | **Moderate** — mechanical serde; reflection-by-name needs a registry |
| Smart lists | custom query tokenizer, 76 matchers / 73 comparers / 65 groupers / 24 series matchers, `MatcherSet` AND/OR/NOT, dependency-tracked caches | **Moderate** — self-contained parser port, no external deps |
| Filename parsing | `ComicNameInfo.FromFilePath` regexes → proposed metadata | **Moderate** — port regexes + tests |
| Remote server | WCF net.tcp + message security + X509, UDP broadcast discovery (BZip2 XML, 10s ping) | **Hard if protocol-compat; Easy if new HTTP/JSON** (compat dropped — see decisions) |
| Device sync | wireless raw TCP protocol (ports 7614/7615/7620+, SSL pinned certs), USB copy, MTP (WPD COM) | **Moderate** — TCP protocol portable; libmtp for MTP |
| Job model | `ProcessingQueue` (460 LOC dedicated-thread queues) × QueueManager (5 queues), ComicScanner thread, no async/await | **Easy** — crossbeam channels + worker threads |
| Scripting | IronPython 2.7.4 DLR host, `#@Directive` discovery, ~40-method host API, hot-reload, `.crplugin` packages | **Moderate** — PyO3 + CPython 3 shim; Python 2→3 migration burden on scripts |

## 4. UI surface (GTK4 target)

| Subsystem | LOC | GTK4 approach |
|---|---|---|
| Reader rendering (`ComicDisplayControl`/`ImageDisplayControl`/`ComicDisplay`) | ~9,500 | custom `GtkWidget` + `GtkGLArea` (glow), `Gtk.EventController*` for pan/zoom/gestures, frame-clock animations, cairo pattern MULTIPLY for paper texture; cairo fallback first |
| Browser list (`ItemView` + `CoverViewItem` + renderers) | ~9,000 | **custom widget** — virtualized thumbnail/tile/detail modes, grouping, stacking, column machinery; nothing in GTK4 comes close |
| Main shell (`MainForm`/`MainView`/`TabBar`/auto-hide/undock) | ~11,000 | `GtkApplicationWindow` + `GtkPaned` + custom tab strip + `Gtk.Revealer`; behavior matrix is the work |
| Dialogs (~50: book editor, bulk edit, preferences, smart-list editor, export, devices…) | ~25,000 | `GtkDialog` + builder; reflection options panels need a serde-driven builder |
| Workspace/layout persistence | ~2,500 | JSON/GSettings + restore module |
| Theming/dark mode | ~3,500 | GTK CSS providers, native dark preference |
| Localization | small code, ~1,269 XMLs × 19 langs | port `TR` loader over existing XMLs as-is |

Designer files are only 15% of UI code; the three flagship surfaces are 80-90% hand-written behavior — a Builder file will not shortcut any of it.

## 5. Plugin/scripting architecture

- **No native DLL plugins exist.** The entire extensibility is file-based: `.py` (IronPython 2.7) with `#@` comment directives, `.xml` manifests, `.crplugin` packages (zip + `package.ini`).
- Hooks: `Startup`, `Shutdown`, `BookOpened`, `ReaderResized`, `ParseComicPath`, `NetSearch`, `NewBooks`/`Books`/`Library`/`Editor` commands, `ComicInfoHtml/ComicInfoUI`, `QuickOpenHtml/QuickOpenUI`, `DrawThumbnailOverlay`, `ConfigScript`, `CreateBookList`.
- Host API = `IPluginEnvironment` (~40 methods) injected as global `ComicRack`; scripts mutate live `ComicBook` objects in-process.
- **Unportable as-is:** scripts that build WinForms controls (`ComicInfoUI`/`QuickOpenUI`) and GDI+ overlay drawing — need a WebKitGTK/HTML or GTK replacement API.
- Expression matchers in smart lists compile Python one-liners — preserved via PyO3 with a per-book shim object (or a Rust expr-language with saved-list compat risk).

## 6. Verdict

**Feasible, no architectural blockers.** Full 1:1 parity is a **~75-90 working-week (~18-22 month) solo effort** across 9 phases. The engine core is cleanly separable (Phases 0-2 validate data compat headlessly); the risk concentrates in the two flagship custom widgets (reader, browser), the ~50-dialog surface, and Python plugin migration. See `port-plan.md` and `risk-register.md`.
