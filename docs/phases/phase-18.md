# Phase 18: Incoming folders

## Status

IMPLEMENTED, USER TEST PENDING. The user approved the behavior on 2026-09-16.
Implementation and automated gates completed on 2026-09-17. ADR-049 through
ADR-055 record the decisions.

## Goal

Add persistent Incoming folders for books that wait for review. Incoming books stay outside the main library until the user adopts them. The user can also discard them.

## Locked behavior

- Support multiple Incoming folders. The role implies recursive live monitoring.
- Store full incoming records in a separate atomic catalog. Do not change the `ComicDb.xml` schema.
- Add fixed All, Gap Fills, Duplicates, New Series, and Needs Review views.
  Add Library Duplicates and Incoming Duplicates below Duplicates.
- Add persistent user smart lists below Incoming.
- Use shadow Series, Volume, Format, and Language as the series identity.
- Use internal gaps and cached Comic Vine gaps. Do not contact Comic Vine automatically.
- Let a book occur in more than one dynamic view.
- Adopt through a selected Library Organizer Move profile. Remember the last selected profile.
- Return an adopted book to Incoming when the user runs Undo.
- Send discarded files to trash by default. Provide a separate permanent-delete option.
- Transfer existing library records when their folder becomes Incoming.
- Block role removal while the folder has unresolved incoming records.

## Tasks

- [x] Add the Incoming configuration and path-role tests.
- [x] Add the persistent Incoming catalog and classification engine.
- [x] Route scans and watcher events to the correct catalog.
- [x] Add folder-role configuration and safe record conversion.
- [x] Add the Incoming navigator and its dynamic views.
- [x] Add Compare, Preview Adoption, Adopt, Discard, and Comic Vine refresh actions.
- [x] Add Compare resolution actions and Incoming smart lists.
- [x] Add cross-catalog Organizer adoption and undo.
- [x] Add automated tests, a release UI probe, and user tests.

## Implementation record

- Incoming records use `IncomingDb.xml` under the XDG data tree. The writer is
  atomic and keeps complete `ComicBook` records.
- One durable journal serializes adoption, adoption-aware undo, discard,
  Incoming scans, and folder conversion. Startup completes an interrupted
  operation before it creates watchers.
- The navigator contains All, Gap Fills, Duplicates, New Series, and Needs
  Review. Duplicates contains Library Duplicates and Incoming Duplicates.
  Smart Lists contains persistent user queries over Incoming books.
  Classification, smart-list evaluation, and Comic Vine cache projection run
  on workers.
- Adoption accepts Move profiles only. It preserves the complete record and ID.
  A companion manifest keeps the existing `undo.dat` bytes compatible.
- Discard uses trash by default and offers a separate permanent-delete choice.
- Folder conversion preserves saved-list ID references. An unresolved Incoming
  folder cannot lose its role.
- Ordinary Library Organizer runs and Incoming transactions share one operation
  lifecycle. Their file and database landings cannot overlap.
- Replacement stores catalog after-images once in durable sidecar files. Its
  compact journal stores only paths and transaction state. Successful
  replacement filters its exact file paths from watcher events.

## Automated gates

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- Release `incoming_probe`: Gates A-I passed. It covers isolated paths,
  navigator structure, configuration, scan routing, classification, conversion,
  adoption semantics, simulation, operation serialization, and reload.
- The transaction suite covers adoption and undo recovery stages, overwrite,
  discard, conversion, stale-epoch rejection, close barriers, and corrupt or
  ambiguous journals.
- The real-data replacement test completed in 5.36 seconds after dispatch. The
  previous embedded-catalog journal completed in 65.73 seconds. No stale-scan
  popup occurred.

## Verification

The automated gates pass. The user tests in `docs/open-user-tests.md` decide the
final GTK behavior.

The user procedure is in `docs/guides/incoming-folders.md`.
