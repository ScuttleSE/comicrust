# Phase 6 Kickoff — Scripting (the PyO3 plugin host)

Goal: the IronPython scripting host from `ComicRack.Plugins` ported
to a PyO3/CPython 3 host — the `#@Directive` script model, the hook
wiring (the Automation menus, ParseComicPath, Startup/Shutdown,
BookOpened, ReaderResized, NetSearch, thumbnail overlays), the
script preferences, the `.crplugin` package manager, and the
Expression/Plugin smartlist matchers. Exit gate: **the shipped
sample scripts run against the port's host API, and a
ComicVine-class plugin (NetSearch + config) operates** (the port
plan's Phase 6 row).

Target crate: `cr-script` (empty — only the Cargo.toml + lib stub
exist). The host lands in `cr-script`. The UI wiring lands in
`cr-ui`, the matcher extensions in `cr-engine`.

## The C# source map (the spec)

| File | LOC | Holds |
|---|---|---|
| `ComicRack.Plugins/PluginEngine.cs` | 244 | The 17 `ValidHooks` table (hook id → menu description), the two-initializer scan over the scripts folder, Key dedup, the `ConfigScript` pairing (a config script attaches to the command with the same Key), the `CommandStates` string (`+key,-key,...` — `Settings.PluginsStates`), the F1-F12 shortcut assignment (first 12 enabled commands get `0x30000 \| (112+n)`), `Invoke(hook, data)`. |
| `ComicRack.Plugins/Command.cs` | 254 | The abstract command: Hook, PCount, Key, Name, Description, Image, Enabled, ShortCutKeys + the XML attributes (the `.config`/manifest surface), Initialize/Invoke/PreCompile, config load/save (the per-command `<script>-<method>.config` file for Python). |
| `ComicRack.Plugins/PythonPluginInitializer.cs` | 66 | The `#@` scan: `#\s*@(?<name>[A-Za-z][\w_]*)\s+(?<value>.*)` comment lines set properties by NAME via reflection (unknown names swallow), each `def\s+(?<function>...)` line closes one command (one command per def; defaults Key=Method, Name=Method via `MakeDefaults`). Only `.py` files. |
| `ComicRack.Plugins/PythonCommand.cs` | 423 | The IronPython engine (one per command scope, the mtime re-check recompiles), the hook-signature table (the delegate types below), `scope.ComicRack = environment` + `scope.ScriptPath = <script dir>`, search paths = the script's own folder + `LibraryPaths`, `CompileExpression<T>` (the one-liner → delegate helper the Expression matcher uses), the error logging (`Syntax error at [line, column]`). |
| `ComicRack.Plugins/PluginEnvironment.cs` | 112 | The environment the scripts see as `ComicRack`: App (IApplication), Browser (IBrowser), OpenBooks (IOpenBooksManager), ComicDisplay, MainWindow, Theme, LibraryPaths, `Localize(resourceKey, nameKey, text)` (the TR lookup with the `Script.` fallback), `ReadDatabaseBooks(file)`. |
| `ComicRack.Plugins/Automation/IApplication.cs` | 52 | The ~20 host methods: ProductVersion, Restart, SynchronizeDevices, ScanFolders, ReadDatabaseBooks, GetLibraryBooks, AddNewBook, RemoveBook, SetCustomBookThumbnail, GetBook, GetComicPage, GetComicThumbnail, GetComicFields, the four icon getters, ReadInternet, AskQuestion, ShowComicInfo. |
| `ComicRack.Plugins/Automation/IBrowser.cs` + `IOpenBooksManager.cs` | 29 | OpenNext/OpenPrev/OpenRandom/SelectComics; Open/OpenFile/IsOpen. |
| `ComicRack.Plugins/ComicBookExpressionMatcher.cs` | ~150 | The `Expression` smartlist matcher: the value compiles as a Python one-liner over `__book` (+ `__bookStats` when used), ops "is True/is False", a parse-error cache. |
| `ComicRack.Plugins/ComicBookPluginMatcher.cs` | ~90 | The plugin list matcher: the value names a `CreateBookList` command (Key), the command's books ARE the result set. |
| `ComicRack/ScriptUtility.cs` | 286 | The app wiring: Initialize (two scan paths — install `Output/Scripts` + the data `Scripts` folder; `CommandStates` from settings; the ParseComicPath + NetSearch registrations), the Automation menu items (`ScriptTypeLibrary`), the NewBooks items into the File menu, the Startup invoke after load, the thumbnail-overlay event hookup, the config-dialog invoke, `CreateToolItems` (name/image/tooltip localization + the click path with the undo marker `Automation '{name}'`). |
| `ComicRack.Engine/PackageManager.cs` | 459 | `.crplugin` packages (zip with `package.ini` + the scripts), install to the pending folder + commit on next start, uninstall, the package list. |
| `ComicRack/Output/Scripts/*.py` | — | The shipped samples: Sample.py (ParseComicPath, BookHasBeenOpened, GetBooksWith, RenameBookFiles, SaveCSVList, AutoadjustTwoPageMode), Autonumber.py, CommitProposed.py, RefreshView.py, SearchAndReplace.py (NetSearch sample), NewComics.py (a WinForms dialog — the `NewBooks` hook), OtherScripts.py + `Package.ini` (`Name = Built in`). |

The hook-signature table (`PythonCommand.hookTypes` — the shape
every hook calls with):

| Hook | Signature |
|---|---|
| Library / Books / NewBooks | `Action<ComicBook[]>` |
| ParseComicPath | `Action<string, ComicNameInfo>` |
| BookOpened | `Action<ComicBook>` |
| CreateBookList | `Func<ComicBook[], string, string, IEnumerable<ComicBook>>` |
| ReaderResized | `Action<int, int>` |
| NetSearch | `Func<string, string, int, Dictionary<string, string>>` |
| ConfigScript / Startup | `Action` |
| Shutdown | `Func<bool, bool>` |
| ComicInfoHtml / QuickOpenHtml | `Func<ComicBook[], string>` |
| ComicInfoUI / QuickOpenUI | `Func<Control>` |
| DrawThumbnailOverlay | `Action<ComicBook, Graphics, Rectangle, int>` |

## Locked decisions that bind this phase

- **PyO3/CPython 3, not IronPython** (the compatibility gotcha):
  scripts keep IronPython 2.7 semantics, so the migration guide
  ships with the phase (the 2to3 pass, `print` statements, the
  `clr` imports die). The HOST API (the `ComicRack` object, ~40
  methods across IPluginEnvironment/IApplication/IBrowser/
  IOpenBooksManager) is the compat surface — keep the method NAMES
  and argument shapes so migrated scripts work.
- The host API methods the port cannot honor keep working with a
  recorded no-op/degraded behavior (device sync, icon getters) —
  never a crash.
- ADR-024: the Automation MENU itself was "omitted (Phase 6)" —
  meaning deferred here, not dropped. `Settings.Scripting` (the
  global kill switch) and `Settings.PluginsStates` (the enabled
  set) already exist in the settings port.
- The `#@Directive` comment model and one-command-per-def are the
  on-disk compat format (invariant #3) — parse them exactly.
- `ComicBookExpressionMatcher`/`ComicBookPluginMatcher` extend the
  smartlist engine — the saved queries with `Expression`/plugin
  matchers from real databases must keep parsing (the tokenizer
  already accepts them; the evaluation side lands here).
- The Expression matcher compiles UNTRUSTED code only when a saved
  list uses it — same trust model as the C# (local files).

## Task list

### T0. Kickoff (this doc)

### T1. The command model + the `#@Directive` parser (`cr-script`)
- The `Command` model port: Hook, PCount, Key, Name, Description,
  Image, Enabled, ShortCutKeys + the defaults (`MakeDefaults`:
  Key=Method, Name=Method).
- The Python initializer port: the two regexes as scans (the
  comment property SETS swallow unknown names; every `def` closes
  the current command), only `.py`.
- The `PluginEngine` port: the 17-hook ValidHooks table with the
  menu descriptions, the recursive folder scan, the Key dedup, the
  ConfigScript pairing (last config wins — the C# list + attach),
  the `CommandStates` string parse/format, the F1-F12 assignment
  for the first 12 enabled commands.
- Tests: parse every `Output/Scripts/*.py` fixture (copied under
  `cr-script/tests/fixtures/`): the command count, the hook, the
  Key/Name defaults, the PCount/Enabled directives; the
  CommandStates round-trip; the shortcut assignment order.

### T2. The PyO3 host (`cr-script`)
- One Python engine (a `PyO3` interpreter handle). GIL rules: every
  script call takes the GIL, never on a GTK draw path directly —
  route through glib idle/threads like the pool queues.
- The scope bootstrap: `ComicRack` (the environment shim) +
  `ScriptPath` (the script's folder). The search path = the script
  folder (+ the data scripts folder).
- The environment shim: the IApplication/IBrowser/IOpenBooksManager
  methods as a Python-visible object backed by `cr-ui`-owned
  closures (the app injects the real implementations; `cr-script`
  stays UI-free with a trait). ComicBook exposure: a shim object
  over `cr_core::ComicBook` with the property-registry getters AND
  setters (the scripts mutate books — the registry is the surface,
  Phase 0 T1).
- The hook-call validation: call the method with the signature
  table's argument count; a wrong shape logs and disables (the C#
  throws + `HandleException`).
- The mtime re-check (recompile on file change), the error log
  shape (`Syntax error at [line, column]`), the per-command config
  file (`<script>-<method>.config` load/save).
- `compile_expression` (the `CompileExpression` port) for T6.
- Tests: a std fixture script round-trips (parse → execute → the
  shim observed the call), a syntax error logs the position, the
  config file round-trips.

### T3. The shell wiring (`cr-ui`)
- The scan roots: the bundled `Output/Scripts` equivalents + the
  data `Scripts` folder (`cr_core::paths` already carries both).
- The Automation submenu (File ▸ Automation): the `Library` hook
  commands as dynamic menu items (the T4 `MenuNode::Dyn` machinery
  — icon via the directive or the resx name, the localized
  name/description fallbacks).
- The `NewBooks` items into the File menu (before the New Comic
  position — `CreateToolItems` inserts after `miNewComic`).
- The invoke plumbing: Startup (after the window load),
  Shutdown (on exit — the `Func<bool,bool>` cancel check),
  BookOpened (the reader open hook), ReaderResized (the reader
  size change).
- ParseComicPath: feed `ComicNameInfo`'s proposed values (the C#
  event shape — the pipeline runs BEFORE the proposed fallback
  locks in; find the exact call point in
  `ComicBook.ParseFilePath`).
- The Automation undo marker (`Automation '{name}'` — the undo
  system is an ADR-024 omission; record the marker as a no-op or
  re-home).
- The F1-F12 accelerators on the menu items.
- `Settings.PluginsStates` persistence (the CommandStates string
  written at the exit save; enable/disable through the
  Preferences page T4).
- Gate: a headless probe loads Sample.py, shows the Automation
  items, dispatches one, and observes the book mutation through
  the shim.

### T4. The Preferences Scripts page (`cr-ui/src/settings`)
- The page the T1 settings port reserved: the scripting master
  switch, the script list (name, hook, enabled check), the
  shortcut display, the per-script Configure invoke (the
  ConfigScript pairing) when present.
- Gate: toggling a script off survives the restart and the menu
  item disappears.

### T5. NetSearch + thumbnail overlays
- NetSearch (`ScriptTypeSearch`): the browser search box's scope
  menu gains the script providers (the C# `SearchEngines.Engines`
  + `ScriptSearch`); results land as the `Dictionary<string,
  string>` row set (the C# opens the result URL list — port the
  shape the search-browser UI omitted; check the C# handler
  `ScriptUtility.cs:53` first).
- DrawThumbnailOverlay: a cairo pass after the thumbnail draw in
  the ItemView (the C# `CoverViewItem.DrawCustomThumbnailOverlay`
  — comic + cairo context + bounds + flags).
- Gate: SearchAndReplace.py's provider shows in the scope menu;
  an overlay script draws on the thumbnails (a probe screenshot).

### T6. The package manager + the HTML panels
- `.crplugin` install/uninstall (`PackageManager` port: the zip +
  `package.ini`, the pending folder + the commit-on-start flow)
  — reuses `cr-io` zip machinery.
- The ComicInfoHtml/QuickOpenHtml panels: the C# hosts a WebKit
  control; decide WebKitGTK vs the recorded reduction (ADR-024
  adjacent) BEFORE building — the scripts return an HTML STRING,
  the display surface is the only question. ComicInfoUI/
  QuickOpenUI (the WinForms control hooks) degrade to the HTML
  path (a recorded deviation — the C# UI scripting cannot map to
  GTK).
- Gate: a package installs, its script registers, the uninstall
  removes it.

### T7. The smartlist matcher extensions + the migration guide
- `Expression` (the compiled one-liner over `__book`/`__bookStats`
  through `compile_expression`) and the plugin-list matcher (the
  `CreateBookList` Key → the command's books) in the
  `cr-engine` registry + eval.
- The real-world DB gate: any saved query using Expression/plugin
  matchers evaluates (extend `tests/realworld_query.rs` if the
  fixture carries one).
- The 2to3 migration guide (`docs/scripting-migration.md`): the
  `clr` import removal, the .NET type mappings (System.Drawing →
  nothing / PIL for overlays), `print`, the changed samples, the
  host-API surface table.
- The shipped samples run: port the sample scripts to Python 3
  under the new host (they ship with the app — the Output/Scripts
  equivalents) and gate them in a probe.
- Gate: the user test — the acceptance scripts from the port plan
  (ComicVine-class: a NetSearch provider + config dialog).

## Risks / known traps

- The GIL + the GTK main thread: script invokes that touch books
  or the UI must run on the main loop (glib idle) or hold the
  library borrow carefully — the Phase 4/5 borrow lessons apply
  double (a script can re-enter ANY shell hook).
- The `MakeDefaults` order matters for the Key dedup and the
  ConfigScript pairing — port the C# sequence, not the intent.
- The scripts folder scan is RECURSIVE (packages carry
  subfolders); the C# swallows every per-file error — keep that
  (one bad script never kills the scan).
- The ComicBook shim: the property registry is the getter/setter
  surface (Phase 0 T1) — do NOT hand-map properties in the shim;
  registry-driven keeps the matcher/columns parity.
- The shortcuts: F1-F12 collide with nothing in the current port
  (checked T1's table — no plain-F shell accels); keep the C#
  assignment order (enabled-command insertion order).
- The C# `.config` files are the per-command settings blobs —
  opaque strings the config script owns; store/load them
  byte-transparently.
- Untrusted-code trust model: the C# runs whatever sits in the
  scripts folders; keep that model (local files = trusted) and
  document it — no sandboxing work.
- The sample scripts use WinForms dialogs (NewComics.py) — the
  port's samples need GTK equivalents or the native-port
  candidates from the BACKLOG (`NewComics.py` is already listed
  there as a native-port candidate; review the rest at T7).

## Verification

```sh
cargo test -p cr-script          # the directive parser + the engine tests
cargo run -p cr-ui --example scripts_probe   # the wiring probe (T3+)
cargo run -p cr-cli -- info <comic-file>     # unchanged regressions
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

Headless probes: Xvfb + an isolated XDG pair (the standing
lesson). The user-test protocol stays mandatory per task
(`cargo run -p cr-app --release --`, then PAUSE with the written
test).
