# Phase 13: One unified config file + data tables out of code

## Status

COMPLETE. User-tested 2026-09-12.

## Goal

Replace the three separate configuration files with one TOML file, and move
the Comic Vine Scraper's hardcoded data tables into that file, so a new
imprint needs no recompile.

## Scope

- Consolidate `comicrust.ini` (the ini chain), `Config.xml` (the Settings
  object), and the plugin configuration files into one file.
- Move the imprint-to-publisher table out of the binary into the file.
- Seed every changeable key at its default value.

## Exclusions

- `ComicDb.xml` is untouched. Books, lists, matchers, and watch folders do
  not move.
- There is no migration. The old files stay on disk and are never read
  again.
- `prior_series.json` stays a separate file. It is a runtime cache, not
  configuration.

## Locked decisions

- ADR-033 — one unified TOML file at `~/.config/comicrust/comicrust.toml`.
- Full-seed the built-in data tables. A revision merge adds new built-in
  entries on upgrade without touching a user edit.
- Settings round-trip is TOML equality, not byte-stable XML. The C#
  `Config.xml` and `ComicRack.ini` parity is abandoned by design.
- The ini search chain collapses to the one user file. `-ac` stays a
  parsed-but-inert switch.

## The file shape

```toml
version = 1

[extended]                      # ExtendedSettings keys (argv still wins)
[engine]                        # EngineConfiguration keys
[settings]                      # the Settings fields, C# member names
[settings.CurrentWorkspace]     # the workspace snapshot
[plugins.comic-vine-scraper]    # the scraper Configuration keys
[data.revision]                 # per-table revision markers
[data.imprints]                 # seeded, user-editable
```

Precedence is unchanged: defaults < file < argv switches. The file is read
once at boot. Every save point rewrites the whole file from the session.
Writes are atomic (temporary file, then rename).

Every parameter is documented in `docs/config-reference.md`. Two drift gates
(`cr-core/tests/config_doc.rs` and `cr-scrape/tests/config_doc.rs`) fail when
a key is missing from that document.

## Tasks

- [x] T1 — `cr-core/src/settings/unified.rs`: the `UnifiedDoc` model, load
      and save, the plugin accessors, the data-table accessor, the built-in
      seed, and the revision merge.
- [x] T2 — boot and save rewiring: `paths::config_file`, the unified read in
      `initialize_settings`, the whole-file write in `save_settings`. The
      Settings XML layer and the ini file-chain machinery are deleted.
- [x] T3 — scraper config: the 7 load and save call sites route through
      `library::scraper_config` and `library::store_scraper_config`.
- [x] T4 — imprints out of code: `cv/imprints.rs::find_parent_publisher`
      reads `data_table("imprints")`. The advanced-settings `IMPRINT=`
      overrides still apply on top.
- [x] T5 — gates and docs: `cr-cli migrate` writes `[extended]` keys into the
      unified file; the probes and the scraper config tests are updated;
      ADR-033 and `docs/config-reference.md` are written.

## Verification

Recorded at commit `f451af7` (2026-09-11):

- `cargo test --workspace` — 522 pass.
- fmt and `clippy --workspace --all-targets -- -D warnings` — green.
- Probes green: cache A-G, workspace ALL PASS, scanrefresh A-K (release),
  writeback, startup, pathmigration, scrapeprefs, scrapeconfig, foldersview,
  detailresize, and the remaining chrome probes.
- The environment flakes seen during this phase are recorded in
  `docs/current-status.md`.

## Open issues

None. The phase waits on the user test.

## User test

1. Rebuild with `cargo run -p cr-app --release --`. The first start creates
   `~/.config/comicrust/comicrust.toml` with `[settings]` and
   `[data.imprints]`. The old `Config.xml`, `comicrust.ini`, and the plugin
   `settings.json` stay on disk and are ignored. Delete them by hand at any
   time.
2. Re-enter the preferences you use: the API key on Preferences ▸ Comic Vine
   Scraper, the theme, the quick-open size, and the cache sizes. Press OK,
   close the app, and start it again. Every value persists.
3. The theme toggle (Browse ▸ Dark Mode) and the cache-folder row
   (Preferences ▸ Advanced) still persist across a restart.
4. Add a line such as `"My Imprint" = "DC Comics"` under `[data.imprints]`
   by hand. Restart. Scrape a book whose Comic Vine publisher is
   "My Imprint". The publisher resolves to the parent, and the imprint is
   recorded. No recompile is needed. A changed or removed line applies the
   same way.
5. A scrape run works end to end with the API key from
   `[plugins.comic-vine-scraper]`. `prior_series.json` stays under
   `plugins/comic-vine-scraper/`.
6. The library is untouched. The `ComicDb.xml` bytes and mtime do not
   change.

## Completion record

COMPLETE. User-tested 2026-09-12.

One TOML file at `~/.config/comicrust/comicrust.toml` replaced
`Config.xml`, the `comicrust.ini` chain, and the plugin `settings.json`.
The Comic Vine imprint table moved into `[data.imprints]`, so a new
imprint needs no rebuild. Every parameter is in
`docs/config-reference.md`, held there by two drift gates.

`ComicDb.xml` was untouched, as ADR-033 required.
