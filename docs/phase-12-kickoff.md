# Phase 12 — Native modules I: the Comic Vine Scraper

Opened 2026-09-10. The user directive after ADR-027 (the scripting host
dropped): port the *used* plugins natively, keeping them as modular as
possible — they must interfere with the base codebase as little as
possible. Functionality parity is the first priority; look and feel the
second. The first module is the Comic Vine Scraper
(https://github.com/cbanack/comic-vine-scraper, Apache-2.0, Cory
Banack) — the flagship ComicRack plugin (~15k LOC IronPython: a
scrape engine, a ComicVine REST data layer, a matching/matching-support
layer, and a WinForms wizard). ADR-031 records the native-module
pattern this phase establishes; every future plugin reuses it.

## Locked user decisions (2026-09-10)

1. **C#-parity wizard flow**: the non-modal main scrape window (the
   C# `ComicForm`) + modal search/series/issue dialogs with covers +
   welcome + finish dialogs. The dialog flow drives the whole
   interaction model, so it counts as functionality.
2. **Plugin-local settings**: `~/.config/comicrust/plugins/
   comic-vine-scraper/settings.json` (the basic options with the C#
   `settings.dat` key names, plus the verbatim advanced-settings
   string as a field). No Config.xml schema change, no legacy
   Windows-profile import (fresh defaults; the advanced text is
   copyable by hand).
3. **Integration points**: the browser context-menu item + the
   browser-toolbar split button + the `win.scrape-books` action. The
   C#'s File ▸ Automation submenu, editor hook, and F1-F12 shortcuts
   stay out (trivial later additions).
4. **Fileless-book covers display**: the scraper stores the scraped
   cover as a custom thumbnail AND a minimal render path is added so
   the grid actually shows it (the one base-codebase touch; the
   storage field `custom_thumbnail_key` exists, the render path was a
   deferred Phase 5 gap).
5. **Implementation choices** (agent): `format=json` API responses
   (the C# uses XML; same endpoints/fields); `ureq` (blocking, rustls)
   for HTTP — the codebase has no async runtime; the ComicVine API key
   is user-supplied in the config dialog.

## Modularity rules (ADR-031, enforced for every future plugin)

- One new crate per plugin: `crates/cr-scrape`. Pure Rust, no GTK,
  no `Rc`/session access, no cr-ui dependency. Depends on `cr-core`
  (the model + registry) and `cr-image`; nothing in the base crates
  imports it except cr-ui.
- The engine runs on a worker thread. It never touches the app
  session (thread-local `Rc`s) — it mutates **clones** of the books
  it was handed; the UI pump applies results on the main thread via
  `library::apply_edited` (the dirty flag + debounced file write-back
  + Info Writer pipeline are reused as-is).
- Engine ⇄ UI is a message protocol (`ScrapeUi`): request events
  (`SearchTerms`, `PickSeries`, `PickIssue`, progress/status/cover
  events) flow engine→UI over std mpsc; the UI answers over a second
  channel. GTK widgets and callbacks stay on the UI side of the
  channel (Rule 6).
- Config persistence lives in the plugin crate (XDG config root +
  `plugins/comic-vine-scraper/`), NOT in cr-core `Settings`.
- Base-codebase touchpoints (the complete list, for review):
  workspace `Cargo.toml` (+1 member, +`ureq`), `cr-ui/src/dialogs/
  scrape.rs` + `scrape_config.rs`, one context-menu item + one
  action + a toolbar split button in `browser/shell.rs` /
  `browser/browser_toolbar.rs`, and the T8 custom-thumbnail render
  branch in `cr-engine/src/image_pool.rs` + the item-view cover path.

## Source inventory (the port spec, all read 2026-09-10)

| Plugin file (src/py) | LOC | Ported by |
|---|---|---|
| `ComicVineScraper.py` (entry, 2 commands) | 5.9 KB | cr-ui wiring (T7) |
| `scrapeengine.py` (the loop) | 42.6 KB | T5 `engine.rs` |
| `database/comicvine/cvconnection.py` | 10.5 KB | T2 `cv/connection.rs` |
| `database/comicvine/cvdb.py` | 31.8 KB | T2 `cv/queries.rs` |
| `database/comicvine/cvimprints.py` | 3.9 KB | T4 `cv/imprints.rs` |
| `database/db.py` + `dbmodels.py` | 36 KB | T2 `cv/mod.rs` + `cv/models.rs` |
| `book/bookdata.py` + `comicbook.py` + `pluginbookdata.py` | 76 KB | T3 `bookdata.rs` |
| `utils/configuration.py` | 33.7 KB | T1 `config.rs` |
| `utils/fnameparser.py` | 10.1 KB | T1 `fnameparser.rs` |
| `utils/matchscore.py` | 6.7 KB | T4 |
| `utils/automatcher.py` | 6.3 KB | T4 |
| `utils/imagehash.py` | 4.5 KB | T4 `imagehash.rs` |
| `utils/dbutils.py` | 1.8 KB | T4 |
| `utils/utils.py` (helpers) | 15.7 KB | T1/T2/T4 (per use) |
| `gui/forms/*.py` | ~150 KB | T6/T7 (GTK4 rewrite) |
| `tests/test_fnameparser.data` | 218 cases | T1 test fixture |

The plugin's local persistence: `%APPDATA%\Comic Vine Scraper\`
(`settings.dat` = `key : value` lines, `advanced.dat` = the raw
KEY=VALUE text, `series.dat` = prior-series keys). The port stores
the same information as JSON under the plugin dir; the advanced text
stays a verbatim string (user-authored, parse-parity required).

## Tasks

### T1. Crate scaffold + fnameparser + Configuration

- `crates/cr-scrape`: deps `cr-core`, `fancy-regex`, `regex`,
  `serde`, `serde_json`; `[lints] workspace = true`. Library doc
  header carries the Apache-2.0 attribution (credit Cory Banack,
  repo link).
- `fnameparser.rs`: the exact port of `extract()` + `regex()`
  (the user-supplied `ALT_SEARCH_REGEX` path). Python regex
  specifics preserved: `re.match` anchoring (`captures_at(0)`),
  non-overlapping `replace_all` scan-after-match semantics, the two
  fixed-length lookbehinds via fancy-regex, the `V2003` year form,
  bracket-range year extraction (`(2006-9)` → 2006), the 2000AD /
  The Beano / `#<year>` exceptions, rightmost-number-is-the-issue,
  Python float formatting (`21.0000000` → `21.0`), the blank-series
  fallback that returns the whole (pre-strip) name.
- Gate: the plugin's own 218-case test vector file
  (`tests/testdata/fnameparser.data`) drives
  `crates/cr-scrape/tests/fnameparser.rs` — every case must pass.
  (Verified against the real Python module under CPython: 218/218.)
- `config.rs`: `Configuration` (the ~33 basic fields with C#
  defaults; `scrape_in_groups` carried but never persisted — C#
  parity), serde JSON with the C# key names (`apiKey`,
  `updateSeries`, …), and the advanced-settings string parser
  (IGNORE_PUBLISHER, IGNORE_SEARCHTERM alphanumeric-only,
  IGNORE_BEFORE/AFTER_YEAR, NEVER_IGNORE_THRESHOLD, SCRAPE_RATING,
  SHOW_COVERS, WELCOME_DIALOG, ALT_SEARCH_REGEX (compile-checked),
  IGNORE_FOLDERS, FORCE_SERIES_ART, NOTE_SCRAPE_DATE,
  PUBLISHER_ALIAS=A-->B, IMPRINT=I-->P, SCRAPE_DELAY (default 1,
  parsed clamp 2..3600), MAX_SEARCH_RESULTS (clamp 10..5000)).
  `load/save(dir)` + an XDG-derived default dir (pure-core helper).
  (Implementation note: the advanced text persists as a field inside
  `settings.json` instead of a separate `advanced.txt` — one file,
  byte-exact round-trip.)
- Gate: unit tests for every advanced key, clamping, quote stripping,
  and a JSON round-trip.

### T2. ComicVine data layer (`cv/`)

- `models.rs`: `SeriesRef` (key, name, volume year, publisher,
  issue count, thumb url), `IssueRef` (issue number, key, title,
  thumb url), `Issue` (the full scraped payload + image url list).
- `connection.rs`: blocking HTTP (ureq, rustls) with the C#
  behaviors: a global 1100 ms query throttle, one retry after 2.5 s,
  the `[ComicVineScraper, version …]` user agent, `client=cvscraper`,
  `format=json`, the invalid-XML-char strip equivalent (JSON needs
  no strip; keep the response-shape guards: `status_code==1` or
  `DatabaseConnectionError`-style typed errors).
- `queries.rs`: the four endpoints (search volumes paged at 100 with
  the `resources=volume` field list; volume details; issues for a
  volume paged; issue by number with the leading-zero strip + the
  `½/¼/¾` alternate-number retry ladder; issue details), the
  `4050-/4000-` URL→ref decoder, `cleanup_search_terms` (incl.
  number-word conversion — the `utils.convert_number_words` port),
  the per-session series-details cache.
- Gate: unit tests against a canned-response mock server (std
  TcpListener) covering pagination, retries, alternate issue
  numbers, URL decode; optional live gate `CV_API_TESTS=1` that hits
  the real API (never in CI).

### T3. BookData + the update rules (`bookdata.rs`)

The parity heart. Read side: build the scraped field set from a
`cr_core::model::ComicBook` (series/number/year from the book, falling
back to `fnameparser` on the filename when missing — the C#
`__parse_extra_details_from_path`; volume/format from the Shadow
(proposed) values; lists split on `,` with comma-cleanup on write;
custom values `comicvine_issue`/`comicvine_volume`).
Write side (`update(issue)`): every field through the massage rules —
`update_<field>` flag ∧ (overwrite_existing ∨ old blank) ∧ ¬(ignore_
blanks ∧ new blank), series/number always ignore-blanks; publisher/
imprint conversions (cvimprints table, user imprints, publisher
aliases, convert-imprints-off, self-imprint nullify); Tags key tag
replace-or-append (`CVDB<n>` / `CVDBSKIP`); Notes key-note replace
(`Scraped metadata from ComicVine [CVDB<n>] on <date>.`); ReleasedTime
written only when year+month+day all present, Year/Month/Day
progressive; CommunityRating 0..5; custom values; cover-url.
- Gate: pure unit tests mutating real `ComicBook`s — every massage
  rule, the key-tag replace paths, date partials, imprint chains.

### T4. Matching

- `matchscore.rs` (word-overlap namescore, prior-series file score,
  mirror-publisher penalties, ±100 bookscore, yearscore, recency,
  `record_choice`), `imagehash.rs` (8×8 grayscale average hash,
  64-bit, hamming similarity — on cr-image RGBA), `strip_back_cover`
  (1.2–1.5 pixel-ratio right-half crop), `automatcher.rs` (threshold
  0.87, first-issue too-similar bail-out at −0.10, associated-image
  fallbacks), `filter_series_refs`, the imprints table.
- Gate: pure unit tests + synthetic-image hash tests.

### T5. Engine loop (`engine.rs`)

Book statuses (SCRAPED/SKIPPED/UNSCRAPED/DELAYED), fast-rescrape via
the issue key, skip-tag short-circuit, magic `cvinfo.txt` file,
series/issue caches keyed by unique-series, autoscrape, delayed-book
requeue to the end, fast-rescrape-first sort (series + padded issue
number order), per-book scrape delay (min 2 s, cancellable), the
`ScrapeUi` request/response protocol, cancellation, and the
scrape-cache semantics (the C# `__scrape_book` subtleties: the
series-cache delete on wrong-series, the SHOW-forces-issue-dialog
rule, the ambiguity count rule).
- Gate: engine test with a scripted fake UI + the T2 mock server;
  cancellation and delay timing gated.

### T6. cr-ui config dialog

API key entry + the checkbox grid (per-field updates, behavior
flags) + the advanced text view (saved verbatim). Store/load through
T1. Gate: `scrapeconfig_probe`.

### T7. cr-ui scrape wizard + shell wiring

The non-modal main window (per-book status list: scraped/skipped/
pending, the C# `ComicForm` parity), welcome dialog, search-terms
dialog, series dialog (score-sorted, cover thumbs, "Show Issues"),
issue dialog (covers, alt-cover browse, the C# result set: ok/back/
skip/permskip/cancel), finish summary dialog. Worker thread + mpsc +
pump; `apply_edited` per scraped book; batch error reporting like the
"Update Book File(s)" arms. Wiring: context-menu "Scrape from Comic
Vine…", toolbar split button, `win.scrape-books` (enabled on
selection), `win.scrape-config`.
- Gate: `scrape_probe` (mock server) + **user test**.

### T8. Fileless covers (the one base touch)

The scraper downloads the issue cover and stores it as the book's
custom thumbnail; the render path learns `type://` custom
thumbnails: `image_pool.rs` gains the custom-thumbnail branch
(`paths.custom_thumbnail_path` holds the file; `custom_thumbnail_key`
carries the locator) and the item-view/tab-strip cover path resolves
it. File-backed books are NOT touched (the C# only sets custom
thumbnails for fileless books either way).
- Gate: probe + **user test** (a scraped fileless book shows its cover).

### T9. Attribution + docs + close-out

Apache-2.0 attribution in the crate (already started in T1), README
section (API key, what gets scraped), kickoff close-out record.

## Ordering, gates, and the user-test protocol

Task order T1 → T2 → T3 → T4 → T5 → T6 → T7 → T8 → T9. T2-T5 are
headless-testable end to end (mock server + fake UI) — the UI tasks
(T6-T8) consume a finished engine. Every task: fmt/clippy/
`cargo test --workspace` green, commit, push, then pause with a
written user test where the table says so. Phases 10/11 user tests
remain open in parallel.

## Omissions / postponed (recorded up front)

- Legacy `settings.dat` import — declined (user decision, 2026-09-10).
- File ▸ Automation, the editor hook, F1-F12 auto-assign — out of the
  first slice (user decision).
- The C# `scrape_in_groups_b` setting is carried (never persisted,
  C# parity) and only matters once grouping is wired; a no-op in T1.
- GEOMETRY_FILE (dialog positions persistence) — recorded, not ported
  in the first slice; revisit with T7 if the user misses it.
- The C# `__str__` config debug dump and the `log.py` machinery —
  replaced by `log`-crate-style debug envs if needed, not ported.
- Rating scraping (SCRAPE_RATING) is slow-by-design in the C# (an
  extra query per issue); port verbatim behind the same advanced flag.