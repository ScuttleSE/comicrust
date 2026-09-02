# AGENTS.md — Agent Onboarding

You are working on **comicrust**. This project is a from-scratch port of **ComicRack Community Edition** (a Windows C# WinForms comic library manager/reader). The target is a **Linux-native Rust + GTK4 application** with **full 1:1 feature parity**.

Read this file first. Then read `docs/port-plan.md` (architecture and roadmap) and `docs/decisions.md` (locked decisions). Do not challenge a locked decision without explicit user approval.

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

Update this section at the **end of every work session**. The next agent must know the exact state of the work.

### State summary

- **Phase:** 0 (core model and data compatibility). Tasks T0-T4 are built. The phase exit review is not done.
- **State:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` are green. 30 tests pass.
- **Gate status:** byte-stable ComicDb.xml round-trip works on all three fixtures. Acceptance criterion #2 (summary of a real user database) waits for a user-provided ComicDb.xml. Do not commit user data.

### What exists (cr-core module map)

| Path | Contents |
|---|---|
| `crates/cr-core/src/xml/mod.rs` | `Emitter` — hand-rolled writer that reproduces net48 `XmlSerializer.Serialize(Stream)` byte for byte. Rules in `tests/golden/README.md`. |
| `crates/cr-core/src/xml/reader.rs` | `XmlReader` — token reader over quick-xml. Order-tolerant. Captures unknown elements raw. |
| `crates/cr-core/src/xml/scalar.rs` | `CrGuid` (lowercase "d" form), `CrDateTime` (.NET kind suffixes), `net_f32` (.NET float text). |
| `crates/cr-core/src/model/` | `comic_info.rs`, `comic_book.rs` (+ `values_store` codec), `comic_page_info.rs`, `enums.rs` (macro-generated, exact member names), `bitmap_adjustment.rs`, `comic_name_info.rs`. |
| `crates/cr-core/src/database/` | `comic_database.rs` (load, save with `.bak` rotation, `open_with_fallback` with `.restore` → main → `.bak` → quarantine chain), `list_items.rs` (ComicLists tree, matchers with `xsi:type` passthrough), `display_config.rs` (the `<Display>` subtree). |
| `crates/cr-core/src/registry.rs` | Property registry: C# property name → typed getter/setter on `ComicBook`. Entry point for matchers, columns, remote updates. |
| `crates/cr-cli/src/main.rs` | `info` (filename proposal or ComicInfo.xml), `db-dump` (JSON summary), `db-roundtrip` (byte-diff report). |

Tests: `crates/cr-core/tests/golden_roundtrip.rs`, `crates/cr-cli/tests/cli.rs`. Fixtures: `tests/golden/` (read `tests/golden/README.md` before you touch the XML layer).

### How to verify

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p cr-cli -- db-roundtrip <ComicDb.xml>
cargo run -p cr-cli -- db-dump <ComicDb.xml>
cargo run -p cr-cli -- info <comic-file>
```

Re-bless the `db-large.xml` snapshot after a deliberate model change: `CR_BLESS=1 cargo test -p cr-core --test golden_roundtrip`. Re-blessing changes fixture bytes. Review the diff before you commit it.

### Remaining Phase 0 work (in order)

1. **Settings port (T2 tail).** Port `IniFile` (`cYo.Common/Runtime/IniFile.cs`), `EngineConfiguration` (`ComicRack.Engine/EngineConfiguration.cs`), and `SystemPaths` (`ComicRack.Engine/SystemPaths.cs`) into `cr-core`. Add settings tests. Note: `ComicNameInfo` currently hard-codes `OfValues = "of,von,de"` and the legacy-parser flag; wire these to `EngineConfiguration` when it lands.
2. **Fresh-DB default lists.** Port `ComicLibrary.InitializeDefaultLists` (`ComicRack.Engine/Database/ComicLibrary.cs:254`). This needs the localized names (English defaults are acceptable first) and the matcher type names for `xsi:type` (`ComicBookRatingMatcher`, `ComicBookReadPercentageMatcher`, `ComicBookModifiedInfoMatcher`, and the default lists in the same file). `create_new()` in `database/comic_database.rs` is the entry point.
3. **MetronInfo mapping (T1 remainder).** Deferred by agreement. Rationale and scope in `tests/golden/README.md`. Needed before Phase 1 (in-archive read/write).
4. **Real-world validation (acceptance #2).** Blocked on the user. See Blockers.
5. **Phase 0 exit review.** Confirm all acceptance criteria in `docs/phase-0-kickoff.md`. Record anything learned in `docs/decisions.md`.

### Lessons from Phase 0 (do not re-learn these)

- `chrono::NaiveDateTime::MIN` is not .NET `DateTime.MinValue`. Build the min value from year 1 (see `CrDateTime::min_value`).
- fancy-regex rejects variable-length lookbehind (`LookBehindNotConst`). `ComicNameInfo` emulates those with prefix guards (see `last_match_guard` in `model/comic_name_info.rs`).
- .NET `RegexOptions.RightToLeft` means "take the last match". `ComicNameInfo` emulates this with `last_match`.
- The reader treats whitespace-only text as indentation. A whitespace-only element value does not survive a round-trip. This is a documented tolerance.
- The captured .NET reference output had two errors against the C# source: no `<Display />` in list items, and an `ExtraSyncInformation` with 2 of 6 members. The C# source wins. The fixture was corrected; details in `tests/golden/README.md`.
- Git normalizes CRLF to LF in the upstream repo (`* text=auto`). Never trust checked-out line endings as format evidence. Read the blob or reason from the writer.
- `cr-cli` panics on `Broken pipe` when output goes through `head`. Cosmetic. Fix when you touch the CLI.

### Blockers / open questions

Acceptance criterion #2 needs a ComicDb.xml from a real ComicRack install. The user must provide it. Not committed to the repo.

---

## Reference codebase (THE SPEC)

- **Local checkout:** `/home/scuttle/Downloads/repo/ComicRackCE` (if this path is stale, locate the checkout and update this file)
- **Upstream:** https://github.com/maforget/ComicRackCE (branch `master`)
- **Golden rule:** the C# source is the specification. Before you implement any behavior, **find and read the corresponding C# code**. Never guess from names, screenshots, or memory of "how ComicRack works".
- **Decompiled caveat:** decompilation produced the reference. Expect dead `using`s, odd names, swallowed exceptions, and dead code. Example: `UseWPF=true` is vestigial. There is no WPF. The target is *behavior*, not style.

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

- Library database: single XML at `%APPDATA%\cYo\ComicRack Community Edition\ComicDb\ComicDb.xml`. See `Engine/SystemPaths.cs` and `DatabaseManager.cs` (`.bak`/`.restore` rotation, corruption fallback).
- Localization: `ComicRack/Output/Languages/<lang>/*.xml`. **19 languages, reused as-is.** Lookup pattern: `TR.Load("FormName")["Key", "Default"]`.
- Sample scripts: `ComicRack/Output/Scripts/*.py`
- Reader paper textures: `ComicRack/Output/Resources/Textures/Papers`

---

## This repo

Crate layout (all eight crates exist. `cr-core` and `cr-cli` are active. The rest are empty stubs — see `docs/port-plan.md`):

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

1. **ComicDb.xml read/write.** Element and attribute names, casing, and structure must match the C# `XmlSerializer` output exactly (the ComicLists tree, the custom values store). Golden-file round-trip tests verify this. The database is the one artifact users cannot lose.
2. **Metadata schema compat:** `ComicInfo.xml` (Anansi standard), ComicRack's `ComicBook.xml`, and `MetronInfo.xml` — read AND write in-archive.
3. **Plugin file formats:** `.py` scripts with `#@Name/#@Hook/#@Key/#@Description/#@PCount/#@Enabled/#@Image` comment directives + one command per `def` (see `PythonPluginInitializer.cs`), XML manifests, `.crplugin` = zip with `package.ini`.
4. **Smart-list query language** must parse and match identically (saved lists contain these queries — see `ComicSmartListItem.cs` and `ComicBookGroupMatcher.cs`).
5. **Caches are disposable. The database is not.** Thumbnail/image caches (`DiskCache` `cache.idx`, BinaryFormatter-serialized) have NO compat requirement. Design fresh formats freely.

---

## Critical gotchas

- **IronPython 2.7 = Python 2 semantics** (scripts use `print '...'` statements). We target PyO3/CPython 3, so existing ecosystem scripts need a 2to3 pass. The host API (`IPluginEnvironment`, ~40 methods) is the shim surface.
- **unrar license is GPL-incompatible.** Use subprocess/7z or libarchive for RAR. Never static-link unrar.
- **The WCF net.tcp remote protocol is NOT preserved** (see `docs/decisions.md`). Android app protocol compat was explicitly dropped.
- **NTFS ADS metadata → Linux xattrs** (`user.comicrack.*`) with sidecar fallback.
- **Reflection-based property access by string name** is load-bearing in the C# (matchers, columns, remote `UpdateComic`, `FormUtility` options panels). Rust needs an explicit property registry for this. Plan for it early in `cr-core`.
- **Windows paths are baked into user data** (workspace paper textures point at install paths). Be lenient when you load.
- **Tao.OpenGL is legacy GL.** The reader targets GL 3.2 core via `glow`, with a cairo fallback first (the C# app itself falls back to GDI+).
- **32-bit JPEG EXIF quirk** in the decode path (`BitmapExtensions.BitmapFromBytes`). Preserve the fix.
- Localization is data-driven per widget name. Port the `TR` lookup. Do not gettext-ify.

---

## Verification workflow

- CI: Gitea Actions on the hemmalab runner (label `debian-go`, host has Rust + GTK4 dev libs, no sudo). See `.gitea/workflows/ci.yaml`. It runs fmt, clippy, and tests on every push to main.
- `cargo fmt --check` and `cargo clippy -- -D warnings` must pass before every commit.
- `cargo test` — golden-file round-trip tests for ComicDb.xml are the phase gate for Phases 0-2 (see `docs/phase-0-kickoff.md`).
- `cr-cli` subcommands (`info <file>`, `db-dump <ComicDb.xml>`) are the manual verification tools against real data.
- **Never commit user library data.** Golden test files must be synthesized or anonymized fixtures under `tests/golden/`.

## Conventions

- Commits: imperative mood, concise subject (`Add ComicDb.xml round-trip test`).
- Add decisions to `docs/decisions.md` as new ADRs. Append only. Language-only rewrites (ASD-STE100) are allowed.
- Commit and push all changes after each completed task (see Agent working rules).
- Update the **Current status** section at the top of this file every session.
