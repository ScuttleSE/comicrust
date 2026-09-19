# Fix repeated ~11s gap-refresh passes after "Link Series from Cache"

## Handoff status (read this first)

This completed-work record is a handoff for a fresh agent/session.
Repo: `/home/scuttle/Downloads/repo/comicrust`, branch `main`, not committed —
all changes described below are uncommitted working-tree edits. **Do not run
`git commit` unless the user explicitly asks.**

**Already done (round 1 — implemented and verified in this working tree):**
- `crates/cr-ui/src/library.rs`: `project_incoming_external_gaps` rewritten to
  use the `owned_by_identity` single-pass grouping described under "Fix (round
  1)" below. This is live in the file today.
- Two new tests added to `crates/cr-ui/src/library.rs`'s `mod tests`:
  `project_incoming_external_gaps_does_not_cross_contaminate_identities` and
  `project_incoming_external_gaps_completes_well_inside_budget`. Both pass.
  The perf test measured **~535ms** in `--release` for 20,000 books / 2,000
  identities (see "Tests" section for exact numbers).
- Full `cargo test -p cr-ui` (189 passed) and `cargo test -p cr-scrape` (all
  passed) were green after round 1. `cargo clippy -p cr-ui --all-targets` was
  clean.
- **Round 1 was insufficient**: the user re-tested on their real ~21,397-book
  library on a confirmed **release build** and still saw ~11.6s per gap-refresh
  pass (full trace pasted mid-session, summarized below). This led to
  identifying a second, distinct cost (round 2).

**Round 2 status (implemented and locally verified):**
- `CODE-READ` 2a: `cr_scrape::bookdata::series_key_of` now reads only the
  `comicvine_volume` custom value. The two call sites no longer build a full
  `BookData`.
- `CODE-READ` 2b: the gap refresh trace now reports snapshot, volume-id, owned-number, and
  cache/diff phase times.
- `MEASURED`: The `series_key_of` unit test covers linked and unlinked books. It also checks
  the result against `BookData::from_book`.
- `MEASURED`: `cargo fmt --all`, workspace clippy, and workspace tests pass. The targeted
  release timing gate measured 196.7 ms for 20,000 books and 2,000 series.
- `MEASURED` (user, 2026-09-19): the real-library retest works correctly. The
  repeated long refresh and sustained idle-CPU symptom did not recur.
- `UNKNOWN`: exact real-library phase times. The user supplied no trace values.
  The phase traces remain available if the problem returns.

**Unrelated context — do not touch:** this repo had a large set of pre-existing
uncommitted changes at the start of this task (Phase 19 work: `missing_issues_bar.rs`,
`pick_series.rs`, `cache/link.rs`, `missing_perf.rs`, doc updates, etc. — visible
in `git status`/`git diff`). These are the user's own in-progress work, not
related to this gap-refresh investigation. Only touch the files this plan names.

## Context

The user linked ~6 books' series via "Link Series from Cache" and observed sustained
high CPU afterward. `CR_TRACE` output showed a burst of `gap refresh call`
generations (5528-5536) followed by a `gap refresh done` line ~11.4 seconds later,
then one more full pass (~11.5s) before things went idle.

**Update — the first round of fixes below (already implemented and merged into the
working tree) did not resolve the problem.** A follow-up trace, from ~10 books
linked from the same series on a confirmed **release build**, still showed the
gap-refresh worker itself as the dominant cost:

```
[trace t=+ 69.494s] gap refresh call generation=12 active_before=false caller=crates/cr-ui/src/library.rs:4050
[trace t=+ 69.536s] gap refresh call generation=13..22 active_before=true  (bursts from each linked book's apply_edited)
[trace t=+ 81.171s] gap refresh done completed=12 current=22 respawn=true
[trace t=+ 81.171s] gap refresh call generation=23 active_before=false caller=crates/cr-ui/src/library.rs:1642
[trace t=+ 92.753s] gap refresh done completed=23 current=23 respawn=false
```

i.e. ~11.68s for the first pass (69.494 → 81.171) and ~11.58s for the
catch-up pass (81.171 → 92.753) — essentially unchanged from before round 1,
despite round 1 being confirmed live (the respawn caller line number,
`library.rs:1642`, matches the post-round-1 file, since round 1 added ~13
lines above that call site). The user confirmed via `AskUserQuestion` that
they were running a **release build**, which rules out the debug-vs-release
explanation. Since the previous fix's own synthetic benchmark (20,000 books /
2,000 identities) completes in ~535ms in release at a comparable scale, there
is a second, distinct cost in the same code path that round 1 didn't touch:
`incoming_volume_ids_for` builds a full, ~30-field `BookData` (crates/cr-scrape/src/bookdata.rs:187-233)
— 12 `split_list` allocations, ~8 `String` clones, two separate
`custom_values_store` decodes, and a conditional filename re-parse — for every
book in `incoming.iter().chain(library)`, **just to read one field
(`series_key`)**. This is real, uncached, per-call work (unlike `incoming_identity`'s
own filename parsing, which is memoized by file path in `book_view::proposed_cached`
and stays warm across a pass), and it runs across the same book set the O(books)
fix already touches. The section below titled "Fix (round 1)" is the change
already shipped; "Fix (round 2)" is the new change this update adds.

Investigation traced this to `crates/cr-ui/src/library.rs`:

- `refresh_incoming_external_gaps_async` (line 1579) is called once per
  `apply_edited` (line 4037), and "Link Series from Cache"
  (`crates/cr-ui/src/browser/shell.rs:4460`, `link_series_apply`) calls
  `apply_edited` once per matched book in a tight loop — there's no batch-apply
  primitive. This already produces a burst of calls.
- The generation/`active` coalescing scheme already handles that burst correctly:
  calls that arrive while a pass is running just bump a generation counter and
  return; when the running pass finishes, it checks whether the generation moved
  and, if so, runs exactly **one** catch-up pass. For 6 link operations landing
  while an 11s pass is in flight, this is why the trace shows exactly two full
  passes, not nine — the coalescing is not the bug.
- The real bug is that each pass costs ~11 seconds on this user's 21,599-book
  library, when it should cost well under a second. `project_incoming_external_gaps`
  (line 1545) contains an O(distinct-identities × library-books) loop at
  lines 1561-1565: for every distinct "Incoming" series identity, it rescans the
  *entire* library and recomputes `incoming_identity()` (mutex-guarded cache
  lookup + string normalization + allocations) for every book, just to collect
  that identity's owned issue numbers. This is the exact complexity shape that
  commit `6b6e9c2` ("Compute Incoming gap identities in one pass to fix idle CPU
  spin") already eliminated from the *sibling* function `incoming_volume_ids_for`
  (lines 1483-1513) a few lines above — but the fix was never applied to this
  second loop in the same function, which has the identical problem. Two full
  11s passes is what a user actually experiences as "CPU pegged for ~23 seconds
  after linking a handful of books."

Confirmed **not** the cause (ruled out during investigation): `cache.issues_of_volume`
(plain indexed SQLite lookup on a long-lived, memoized connection — no network
I/O), `BookData::from_book` (pure in-memory), and `incoming_identity`'s own
per-call cost (cheap; the problem is call *count*, not per-call cost).

The codebase already has the correct pattern to copy: `missing_issues_of_library`
(`crates/cr-scrape/src/cache/missing.rs:146-176`) groups owned issue numbers by
key in one pass before iterating per-series, and `incoming_volume_ids_for`
(`crates/cr-ui/src/library.rs:1483-1513`) already does this for the series-key
side of this same computation.

## Fix (round 1 — already shipped)

In `crates/cr-ui/src/library.rs`, `project_incoming_external_gaps` (lines
1545-1575): replace the per-identity library rescan with a single O(books)
pre-pass that groups owned issue numbers by `IncomingIdentity` into a
`HashMap<IncomingIdentity, Vec<String>>` (it already derives `Hash + Eq + Clone`,
`crates/cr-engine/src/incoming.rs:157-163`, and is already used as a map key
elsewhere in this file), built once before the `for (identity, volume_id) in
volume_ids` loop. Inside the loop, replace the rescan with an O(1) lookup:

```rust
let volume_ids = incoming_volume_ids_for(&identities, incoming, library, config);
// One pass over `library`, grouping owned issue numbers by identity,
// instead of rescanning the whole library once per identity below
// (the same O(identities x books) shape `incoming_volume_ids_for`
// fixes just above).
let mut owned_by_identity: HashMap<cr_engine::incoming::IncomingIdentity, Vec<String>> =
    HashMap::new();
for book in library {
    if let Some(identity) = cr_engine::incoming::incoming_identity(book) {
        owned_by_identity
            .entry(identity)
            .or_default()
            .push(book.info.number.clone());
    }
}
let mut result = cr_engine::incoming::ExternalGapCache::new();
for (identity, volume_id) in volume_ids {
    let Ok(issues) = cache.issues_of_volume(volume_id) else {
        continue;
    };
    let owned = owned_by_identity
        .get(&identity)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let gaps: Vec<_> = cr_scrape::cache::missing::missing_issues(&issues, owned)
        .into_iter()
        .filter_map(|issue| cr_engine::incoming::IssueNumber::parse(&issue.issue_number))
        .collect();
    if !gaps.is_empty() {
        result.insert(identity, gaps);
    }
}
result
```

`missing_issues` already takes `&[String]` (`crates/cr-scrape/src/cache/missing.rs:52`),
so `owned` (now already `&[String]`) is passed without `&`. `HashMap` is already
imported in this file. No signature changes; `project_incoming_external_gaps` is
a private `fn` with no other callers besides `refresh_incoming_external_gaps_async`
and its own test.

No debounce/coalescing change is needed: once a pass costs milliseconds instead
of 11 seconds (consistent with the similarly-shaped `missing_issues_of_library`
benchmark measuring ~66ms over 22k books / 2k series, `crates/cr-scrape/tests/missing_perf.rs`),
the existing "coalesce bursts, run one catch-up pass" behavior stops being
user-visible.

## Fix (round 2 — this update)

**2a. Stop building a whole `BookData` just to read `series_key`.**

`incoming_volume_ids_for` (`crates/cr-ui/src/library.rs:1499`) and
`selected_incoming_has_volume_id` (`crates/cr-ui/src/library.rs:1536-1541`) both
call `cr_scrape::bookdata::BookData::from_book(book, config).series_key` purely
to read the `comicvine_volume` custom value — discarding the other ~29 fields
that call just built. Add a narrow, purpose-built accessor next to the existing
`pub fn set_custom_value` in `crates/cr-scrape/src/bookdata.rs` (keeping
`get_custom_value` itself `pub(crate)` and the `SERIES_KEY_CUSTOM` string
private, matching this file's existing encapsulation):

```rust
/// The Comic Vine volume id a book votes for (empty if unlinked) —
/// the `series_key` read alone, without building a full `BookData`.
pub fn series_key_of(book: &ComicBook) -> String {
    get_custom_value(book, SERIES_KEY_CUSTOM)
}
```

Then in `crates/cr-ui/src/library.rs`:

- Line 1499: replace
  `let series_key = cr_scrape::bookdata::BookData::from_book(book, config).series_key;`
  with `let series_key = cr_scrape::bookdata::series_key_of(book);` (the `config`
  parameter becomes unused in `incoming_volume_ids_for` and can be dropped from
  its signature — check `selected_incoming_volume_ids`, its one caller, doesn't
  need it either).
- Lines 1534-1543 (`selected_incoming_has_volume_id`): replace the body with
  `selected.iter().any(|book| cr_scrape::bookdata::series_key_of(book).trim().parse::<i64>().is_ok_and(|id| id > 0))`,
  dropping the now-unused `scraper_config()` call.

This removes, per book per pass: 12 `Vec<String>` allocations (`split_list` for
crossovers/characters/teams/locations/writers/pencillers/inkers/colorists/
letterers/cover_artists/editors/tags), ~8 `String` clones, a second
`custom_values_store` decode (`issue_key`, never used here), and the conditional
`parse_extra_details_from_path` filename re-parse — across up to
`incoming.len() + library.len()` calls per pass.

**2b. Add phase timing so a persistent slowdown is measured, not guessed at.**

Two rounds of fixes based on complexity-class reasoning have already landed
without a real profile of the user's actual data. Rather than guess a third
time, add cheap `Instant`-based timing around each phase of
`refresh_incoming_external_gaps_async` / `project_incoming_external_gaps`
(`crates/cr-ui/src/library.rs`), traced with the existing
`crate::trace::trace(format!(...))` call used by the neighboring "gap refresh
call/done" lines:

- the two upfront snapshots (`incoming_books_snapshot()` +
  `session().borrow().database().books.clone()`, `library.rs:1614-1615`),
- the `incoming_volume_ids_for` call,
- the new `owned_by_identity` grouping pass,
- the per-identity `cache.issues_of_volume` + `missing_issues` loop.

If CPU is still high after 2a, the next `CR_TRACE` capture will show which of
these four phases dominates on the real 21,397-book library, instead of
requiring another round of hypothesis agents against a codebase we can't
profile directly. This mirrors how the original 2026-09-18 idle-CPU incident
was diagnosed (`docs/current-status.md:107-109`: `perf`/`gdb` located the exact
function before it was fixed).

## Tests

Round 1 (already shipped and passing):
- `project_incoming_external_gaps_does_not_cross_contaminate_identities` —
  asserts two distinct series' gap sets don't leak into each other.
- `project_incoming_external_gaps_completes_well_inside_budget` — synthetic
  20,000 books / 2,000 identities perf-budget test (`crates/cr-ui/src/library.rs`,
  `mod tests`), currently ~535ms in release.

Round 2 additions:
- Add a unit test for the new `cr_scrape::bookdata::series_key_of` asserting it
  returns the same value `BookData::from_book(..).series_key` would for a book
  with a `comicvine_volume` custom value set, and empty for a book without one
  (next to `BookData::from_book`'s existing tests in `crates/cr-scrape/src/bookdata.rs`
  or `crates/cr-scrape/tests/bookdata.rs`).
- No new perf test is needed for 2a: the existing
  `project_incoming_external_gaps_completes_well_inside_budget` test already
  exercises `incoming_volume_ids_for` (and therefore this call site) end to
  end; rerun it after the change and note the new timing in this plan's
  verification step, since it should drop further from ~535ms.

## Verification

1. `cargo test -p cr-ui` and `cargo test -p cr-scrape` — existing suite plus the
   new/extended tests above.
2. `cargo test -p cr-ui --release project_incoming_external_gaps_completes_well_inside_budget -- --nocapture`
   to confirm the synthetic timing drops further after removing the `BookData`
   construction.
3. Manual check with the real library: run the release build, enable
   `CR_TRACE`, perform "Link Series from Cache" on several books in a row as
   before, and read the new phase-timing trace lines (2b) alongside
   `gap refresh call/done`.
   - If the pass is now fast (sub-second to low seconds), the fix worked —
     confirm idle CPU returns to zero promptly afterward.
   - If it's still slow, the phase timings tell us which of the four phases
     (snapshot/clone, `incoming_volume_ids_for`, `owned_by_identity` build, or
     the per-identity cache-lookup loop) to investigate next, rather than
     guessing a third round of fixes.

`MEASURED` local verification on 2026-09-19:

- `cargo test -p cr-scrape`: passed.
- `cargo test -p cr-ui`: passed.
- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- Release timing gate: 196.7 ms for 20,000 books and 2,000 series. The handoff
  recorded approximately 535 ms before round 2.
- `MEASURED` (user, 2026-09-19): the real-library retest works correctly. The
  repeated long refresh and sustained idle-CPU symptom did not recur.
- `UNKNOWN`: exact real-library phase times. The user supplied no trace values.

## Handoff

Round 2 is complete. No implementation work remains for this issue.

Do not investigate the gap-refresh path again unless the symptom returns. If
it returns, collect `CR_TRACE` output first. Compare the `snapshots`,
`volume_ids`, `owned_by_identity`, and `cache_and_missing` phase times before
you design another change.

The working tree still contains the larger uncommitted Phase 19 change set.
Do not separate, discard, or overwrite those changes as part of this issue.
