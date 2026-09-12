# Open user tests

Every test here waits for a person. A build that passes is not proof
that the user interface behaves.

**There is no open user test right now.** Every test through Phase 15
passed on 2026-09-12.

`docs/current-status.md` names which tests are open. This file holds
their steps.

## How to add one

Add a numbered section per test, and add its title to the list in
`docs/current-status.md`. Delete both when the test passes.

Give each step three things: what to do, what to expect, and what to
report.

```markdown
## N. Title (the phase or the commit)

1. Do this.
   *Expect:* this happens.
   *Report:* this number.
```

## Before any test

Build once and use that binary for the whole test.

```sh
cd /home/scuttle/Downloads/repo/comicrust
cargo build -p cr-app --release
./target/release/comicrust
```

"Restart" means: close the window, wait for the process to end, then
start the binary again.

| What | Where |
|---|---|
| Configuration | `~/.config/comicrust/comicrust.toml` |
| Library database | `~/.local/share/comicrust/ComicDb/ComicDb.xml` |
| Comic Vine cache | `~/.local/share/comicrust/plugins/comic-vine-scraper/cvcache.sqlite` |
| Scrape history | `~/.config/comicrust/plugins/comic-vine-scraper/prior_series.json` |

Back up `ComicDb.xml` before a test that scans or writes the library.

```sh
cp ~/.local/share/comicrust/ComicDb/ComicDb.xml ~/ComicDb.xml.backup
```
