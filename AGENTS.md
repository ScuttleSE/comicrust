# AGENTS.md — Agent Onboarding

You are working on **comicrust**: a from-scratch port of **ComicRack Community Edition** (a Windows C# WinForms comic library manager/reader) to a **Linux-native Rust + GTK4 application**, targeting **full 1:1 feature parity**.

Read this file first, then `docs/port-plan.md` (architecture + roadmap) and `docs/decisions.md` (locked decisions — do not relitigate them without explicit user sign-off).

---

## Agent working rules (hard limits)

These rules are absolute. Break none of them. If you break them, you waste the user's time and tokens.

### Rule 1: No loops
- Do not guess in a loop. Do not repeat actions that give no new information.
- If the cause of a problem is not clear after two or three file reads, stop.
- Ask the user one question that targets the problem. Then wait for the answer.
- Do not chain guesses. Do not say "let me check one more thing" again and again.
- If you start to loop, or if you repeat searches without a clear answer, stop immediately.
- Do not wait until you notice the loop. Do not start a loop.
- You can continue after a loop only if you ask the user first. There is no other way.

### Rule 2: Short answers first
- Give the short answer first. Then stop. Do not write a long block of text.
- When you ask the user to do a task, write only the task. Then wait for the result.
- Do not add a plan, a hypothesis, or a "what I am looking for" section to a request for action.
- Do not restate the plan after each step. The user reads the plan one time.
- Keep each reply short. Add detail only if the user asks for it.

### Rule 3: Evidence before claims
- Find the true cause before you state a cause.
- Do not blame or clear a change without evidence. Get a measurement first.
- Do not assume the user's environment. The user runs the binary on a different machine.
- Local disk state, tools, and timing do not transfer to that machine.
- Trust the trace over the theory. If strace or gdb data conflicts with your reading of the code, the data wins.
- A "window did not appear" symptom means the main thread blocks. Find the main-thread stall (the futex or syscall gap). Do not look only at background workers.
- Confirm that the fix solves the measured problem. Do not stop at "it builds".

### Rule 4: Commit and push
- After you complete a change, commit all changes. Then push.
- Do not leave work uncommitted. A push starts CI.

### Rule 5: Language
- Write all communication and documentation in Simplified Technical English (ASD-STE100).
- Use the asd-ste100 skill for new text and for rewrites.

---

## Current status (KEEP UPDATED)

Update this section at the **end of every work session** so the next agent knows exactly where things stand.

- **Current phase:** Phase 0 — not started
- **Completed:** feasibility analysis, port plan, agent docs, agent working rules, ASD-STE100 rewrite of all docs (planning stage only, no code)
- **In progress:** —
- **Next up:** workspace scaffold per `docs/phase-0-kickoff.md`
- **Blockers / open questions:** —

---

## Reference codebase (THE SPEC)

- **Local checkout:** `/home/scuttle/Downloads/repo/ComicRackCE` (if this path is stale, locate the checkout and update this file)
- **Upstream:** https://github.com/maforget/ComicRackCE (branch `master`)
- **Golden rule:** the C# source is the specification. Before implementing any behavior, **find and read the corresponding C# code**. Never guess from names, screenshots, or memory of "how ComicRack works".
- **Decompiled caveat:** the reference was produced by decompilation. Expect dead `using`s, odd names, swallowed exceptions, and dead code (e.g. `UseWPF=true` is vestigial — there is no WPF). The target is *behavior*, not style.

### Source project map

| C# project | LOC (.cs) | Role | Key files to know |
|---|---|---|---|
| `ComicRack` | ~59,900 | Main WinForms app: shell, ~50 dialogs, views | `MainForm.cs` (4,576 — the orchestrator), `ScriptUtility.cs`, `Config/DisplayWorkspace.cs`, `Dialogs/ComicBookDialog.cs`, `Dialogs/PreferencesDialog.cs`, `PackageManager.cs` |
| `ComicRack.Engine` | ~46,900 | Core engine (≈85-90% UI-free): providers, DB, matchers, caches, sync, remote | `ComicBook.cs` (3,076), `ComicInfo.cs` (1,594), `Database/ComicDatabase.cs`, `Database/ComicLibrary.cs`, `DatabaseManager.cs`, `IO/Provider/*` (readers/storage), `Metadata/ComicBook/Matcher/*` (76 matchers), `ComicNameInfo.cs` (filename parsing), `QueueManager.cs`, `IO/Cache/ImagePool.cs`, `IO/DiskCache.cs` lives in cYo.Common, `IO/Network/ComicLibraryServer.cs` |
| `cYo.Common` | ~37,400 | Base lib: imaging (GDI+), threading, IO, text, XML, Win32 interop | `Drawing/ImageProcessing.cs` (1,557), `Drawing/BitmapExtensions.cs`, `Threading/ProcessingQueue.cs` (460), `IO/DiskCache.cs` (623), `Xml/XmlUtility.cs`, `Compression/SevenZip/*` (COM interop), `Text/Tokenizer.cs` |
| `cYo.Common.Windows` | ~30,200 | The custom WinForms control toolkit (~50 controls) | `Forms/ItemView.cs` (4,770 — the browser list), `Forms/TabBar.cs` (1,926), `Forms/ScrollControl.cs`, `Forms/SizableContainer.cs`, `FormUtility.cs` (reflection-driven options UI) |
| `cYo.Common.Presentation` | ~8,600 | Renderer abstraction (GDI/OpenGL), Ceco XHTML text engine, panels/overlays | `Tao/ControlOpenGlRenderer.cs`, `Tao/TextureManager.cs`, `Ceco/*` |
| `ComicRack.Engine.Display.Forms` | ~7,700 | The book reader controls | `ImageDisplayControl.cs` (2,632), `ComicDisplayControl.cs` (3,514), Engine `Display/ComicDisplay.cs` (2,018) |
| `ComicRack.Plugins` | ~1,900 | IronPython 2.7 scripting host | `PluginEngine.cs` (244), `PythonCommand.cs`, `PythonPluginInitializer.cs`, `PluginEnvironment.cs` (the `ComicRack` object scripts see) |

### Key formats/locations in the reference

- Library database: single XML at `%APPDATA%\cYo\ComicRack Community Edition\ComicDb\ComicDb.xml` — see `Engine/SystemPaths.cs`, `DatabaseManager.cs` (`.bak`/`.restore` rotation, corruption fallback)
- Localization: `ComicRack/Output/Languages/<lang>/*.xml` — **19 languages, reused as-is**; lookup pattern `TR.Load("FormName")["Key", "Default"]`
- Sample scripts: `ComicRack/Output/Scripts/*.py`
- Reader paper textures: `ComicRack/Output/Resources/Textures/Papers`

---

## This repo

Crate layout (to be scaffolded in Phase 0 — see `docs/port-plan.md`):

| Crate | Contents |
|---|---|
| `crates/cr-core` | Data model (ComicBook/ComicInfo/MetronInfo/PageInfo), ComicDb.xml serde, settings, filename parsing |
| `crates/cr-io` | Comic providers (zip/tar/7z/rar/pdf/folder/web), ComicInfo.xml read/write-back, archives |
| `crates/cr-image` | Image currency type, decode/encode pipeline, resize/adjust filters, page/thumbnail caches |
| `crates/cr-engine` | Smart-list parser + matchers, queue manager, scanner, watch folders, backup, sync, remote server |
| `crates/cr-script` | PyO3 plugin host, `#@Directive` loader, `.crplugin` packages |
| `crates/cr-ui` | GTK4: reader (GtkGLArea), ItemView browser, shell, dialogs, theming, i18n |
| `crates/cr-cli` | Headless verification tooling (`info`, `db-dump`, round-trip) |
| `crates/cr-app` | Main binary: D-Bus single instance, app wiring, packaging |

---

## Compatibility invariants (DO NOT BREAK)

1. **ComicDb.xml read/write.** Element/attribute names and structure must match the C# `XmlSerializer` output exactly (attribute/element names, casing, the ComicLists tree, custom values store). Verified by golden-file round-trip tests. The database is the one thing users cannot lose.
2. **Metadata schema compat:** `ComicInfo.xml` (Anansi standard), ComicRack's `ComicBook.xml`, `MetronInfo.xml` — read AND write in-archive.
3. **Plugin file formats:** `.py` scripts with `#@Name/#@Hook/#@Key/#@Description/#@PCount/#@Enabled/#@Image` comment directives + one command per `def` (see `PythonPluginInitializer.cs`), XML manifests, `.crplugin` = zip with `package.ini`.
4. **Smart-list query language** must parse and match identically (saved lists contain these queries; see `ComicSmartListItem.cs`, `ComicBookGroupMatcher.cs`).
5. **Caches are disposable; the database is not.** Thumbnail/image caches (`DiskCache` `cache.idx`, BinaryFormatter-serialized) have NO compat requirement — design fresh formats freely.

---

## Critical gotchas

- **IronPython 2.7 = Python 2 semantics** (scripts use `print '...'` statements). We target PyO3/CPython 3: existing ecosystem scripts need a 2to3 pass. The host API (`IPluginEnvironment`, ~40 methods) is the shim surface.
- **unrar license is GPL-incompatible**: use subprocess/7z or libarchive for RAR, never static-link unrar.
- **WCF net.tcp remote protocol is NOT being preserved** (see decisions.md) — the Android app protocol compat was explicitly dropped.
- **NTFS ADS metadata → Linux xattrs** (`user.comicrack.*`) with sidecar fallback.
- **Reflection-based property access by string name** is load-bearing in the C# (matchers, columns, remote `UpdateComic`, `FormUtility` options panels). In Rust this needs an explicit property registry — plan for it early in `cr-core`.
- **Windows paths are baked into user data** (workspace paper textures point at install paths); be lenient when loading.
- **Tao.OpenGL is legacy GL**; the reader targets GL 3.2 core via `glow`, with a cairo fallback first (the C# app itself falls back to GDI+).
- **32-bit JPEG EXIF quirk** in decode path (`BitmapExtensions.BitmapFromBytes`) — preserve the fix.
- Localization is data-driven per-widget-name; port the `TR` lookup, don't gettext-ify.

---

## Verification workflow

- `cargo fmt --check` and `cargo clippy -- -D warnings` must pass before every commit.
- `cargo test` — golden-file round-trip tests for ComicDb.xml are the phase gate for Phases 0-2 (see `docs/phase-0-kickoff.md`).
- `cr-cli` subcommands (`info <file>`, `db-dump <ComicDb.xml>`) are the manual verification tools against real data.
- **Never commit user library data.** Golden test files must be synthesized or anonymized fixtures under `tests/golden/`.

## Conventions

- Commits: imperative mood, concise subject (`Add ComicDb.xml round-trip test`).
- Add decisions to `docs/decisions.md` as new ADRs. Append only. Language-only rewrites (ASD-STE100) are allowed.
- Commit and push all changes after each completed task (see Agent working rules).
- Update the **Current status** section at the top of this file every session.
