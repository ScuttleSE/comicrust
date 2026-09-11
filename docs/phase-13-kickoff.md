# Phase 13 kickoff — One unified config file + data tables out of code

**Status: IMPLEMENTED, user test pending.**

## Scope (user ask, 2026-09-11)

1. Consolidate the config files into ONE unified file: today
   `comicrust.ini` (the ini chain), `Config.xml` (the Settings
   object), and the plugin configs
   (`plugins/comic-vine-scraper/settings.json` + `prior_series.json`).
2. Move the Comic Vine Scraper's hardcoded data tables (the
   imprint→publisher list) out of the binary into the config file, so
   adding an imprint needs no recompile.
3. The ComicDb.xml is the only sacred artifact (books, lists,
   matchers, watch folders) — untouched by this phase. The old config
   files need no migration (user decision).

User decisions locked (2026-09-11): **TOML** format; **full-seed**
the built-in data tables with a revision merge that adds new built-in
entries on upgrade without touching user edits; **prior_series.json
stays a separate file** (runtime cache, not config); no migration
(fresh defaults; old files left in place, never read again).

## The file

`~/.config/comicrust/comicrust.toml` (ADR-033; `cr_core::paths::
config_file` + `cr_core::settings::unified`):

```toml
version = 1

[extended]                      # ExtendedSettings keys (argv still wins)
[engine]                        # EngineConfiguration keys
[settings]                      # the Settings fields, C# member names
[settings.CurrentWorkspace]     # the workspace snapshot (was <CurrentWorkspace>)
[plugins.comic-vine-scraper]    # the scraper Configuration keys
[data.revision]                 # per-table revision markers
[data.imprints]                 # seeded, user-editable
```

Precedence unchanged: defaults < file < argv switches. The file is
read once at boot; every save point rewrites the whole file from the
session (`library::save_settings`); `save_ini_keys` updates
`[extended]` then saves. The `[extended]`/`[engine]` sections write
back VERBATIM from the loaded session (the C# never writes its ini
either — argv values never leak into the file). Writes are atomic
(tmp + rename, the database save shape).

## Tasks (all done)

- **T1 — cr-core `settings/unified.rs`:** the `UnifiedDoc` model
  (serde), the session section globals (extended/engine/plugins/data),
  `load`/`save_file`/`update_extended_keys`/`merge_extended_keys`
  (the `cr-cli migrate` merge), `get_plugin`/`set_plugin` (typed
  through serde — cr-core stays free of cr-scrape), `data_table`
  (session table → built-in fallback), `ensure_builtin_data` (seed +
  revision merge), the serde helpers (`serde_xml_enum!` over the
  `to_xml`/`from_xml` forms, `CrGuid`, `f32_shortest` for the float
  fields), and the built-in `IMPRINTS` seed (73 verbatim entries +
  `IMPRINTS_REVISION`). Gates: round-trip all sections, seed-on-boot +
  idempotent reseed, corrupt-file fallback + rewrite, revision merge
  (only missing keys; current-revision tables untouched), registry
  name guard, engine special-field texts.
- **T2 — boot/save rewiring:** `paths::config_file` (the old
  `settings_file` + `INI_FILE_NAME`/`ini_default_locations` deleted),
  `initialize_settings` reads the unified file (the ini chain read
  replaced 1:1; argv overlay unchanged), `save_settings` writes the
  whole file, `save_ini_keys` = `[extended]` update + save. The
  Settings XML layer (write_xml/read_elem/save/load) and the ini
  file-chain machinery (`read_files`/`merge_write`) are deleted;
  `IniValues` stays (argv + registry traffic). Settings/WorkspaceState
  gained serde derives mirroring the C# member names (pinned renames:
  `RemoveFilesfromDatabase`, `InformationCover3D`,
  `ThumbCacheSizeMB`/`PageCacheSizeMB`/`InternetCacheSizeMB`/
  `MemoryThumbCacheSizeMB`/`MaximumMemoryMB`).
- **T3 — scraper config:** the 7 load/save call sites route through
  `library::scraper_config`/`library::store_scraper_config`
  (`[plugins.comic-vine-scraper]`); the cr-scrape file store
  (`load(dir)`/`save(dir)`) is deleted; `default_config_dir` survives
  only for `prior_series.json`. The `advanced` reparse moved to the
  accessor (serde skips it — the old file-load shape).
- **T4 — imprints out of code:** `cv/imprints.rs::find_parent_publisher`
  reads `data_table("imprints")` (exact trimmed key; unknown →
  input). The advanced-settings `IMPRINT=` overrides stay on top
  (`bookdata::convert_publishers` unchanged). Builtin-table fallback
  keeps the cv_mock/bookdata tests headless.
- **T5 — gates + docs:** `cr-cli migrate` writes `[extended]` keys
  into the unified file (`merge_extended_keys`); the cli test, the
  workspace/bootreentry/writeback/startup/pathmigration/foldersview/
  deleteperf/detailresize probes and the cr-scrape config tests
  updated; README config section; ADR-033; this kickoff.

## Verification record (2026-09-11)

- `cargo test --workspace` — 514 passed (the count grew from 506:
  +11 unified-config unit tests, +3 settings/workspace TOML
  round-trip suites, +1 plugin-section round trip; the XML golden
  tests went with the deleted layer).
- fmt + `clippy --workspace --all-targets -- -D warnings` green.
- Probes (release, Xvfb): cache A-G (the cache-path override rides
  `[extended]` now), workspace ALL PASS (the `[settings.
  CurrentWorkspace]` TOML shape + the close-path save), bootreentry
  COMPLETE, scanrefresh A-K (RELEASE; gate E in a debug build stalls
  mid-scan on this machine right now — the unmodified base HEAD
  fails the same gate identically, so it is a debug-timing
  environment flake, not this phase's regression; the release run is
  the reference), statusbar J/J2, browserbar D/D2/D3, navpages,
  tabstrip, displaysettings (5 gates), smalldialogs, writeback
  (RESULT lines), startup (init 557 µs / shell 23.5 ms), menubar,
  commands (73/73), pathmigration ALL GATES, scrapeprefs, scrapeconfig,
  importlist, listorder, dynmenus, toolbar, menubarvis, foldersview,
  deleteperf, detailresize, icons. newbook/exportpage/contextmenu
  reach "PROBE DONE" then their internal watchdog fires — the same
  rc=2 shape on the unmodified base HEAD (pre-existing probe quirk).
  editor_probe runs a main loop forever by design (kill under
  timeout is its normal completion).

## Deviations recorded (ADR-033)

- The C# `Config.xml`/`ComicRack.ini` parity is abandoned BY DESIGN
  (user decision). Linux-native single file.
- The ini search chain (exe dir / `/etc/comicrust`) collapses to the
  one user file. `-ac` stays a parsed-but-inert switch as before.
- Settings round-trip is TOML equality, not byte-stable XML.
- `data_revision` from the plan landed as the per-table
  `[data.revision]` map (extensible to new tables).

## User test

1. Rebuild (`cargo run -p cr-app --release --`). On first start the
   app creates `~/.config/comicrust/comicrust.toml` with
   `[settings]` + `[data.imprints]` (the old `Config.xml`,
   `comicrust.ini` and the plugin `settings.json` stay on disk,
   ignored — clean them up manually whenever).
2. Re-enter the preferences you care about (API key on
   Preferences ▸ Comic Vine Scraper; theme, quick-open size, cache
   sizes…), OK, close the app, start again — everything persists.
3. The theme toggle (Browse ▸ Dark Mode) and the cache-folder row
   (Preferences ▸ Advanced) still persist across a restart.
4. Hand-edit `[data.imprints]` in the file: add a line like
   `"My Imprint" = "DC Comics"`, restart, scrape a book whose Comic
   Vine publisher is "My Imprint" → the book's publisher resolves to
   the parent, the imprint recorded — no recompile. Changing/removing
   a line applies the same way.
5. A scrape run still works end to end (API key from
   `[plugins.comic-vine-scraper]`); `prior_series.json` keeps living
   under `plugins/comic-vine-scraper/`.
6. The library is untouched: `ComicDb.xml` bytes/mtime unchanged by
   any of this.