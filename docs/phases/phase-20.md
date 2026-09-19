# Phase 20: Comic Vine cache manager

## Status

Implemented. User test pending.

## Goal

Add a dialog that lets the user inspect and update one Comic Vine volume in
the local cache. The dialog opens from the File menu and searches by Comic
Vine volume ID.

## Scope

- Show and edit the cached volume name, publisher, and start year.
- Show the cached issue IDs and issue numbers.
- **Update from API** fetches all volume fields and the complete issue-number
  map. It does not fetch each issue detail resource.
- **Complete Update from API** also fetches the detail resource for every
  issue.
- A complete update keeps completed issue details after an interruption. A
  later run resumes the unfinished issue IDs.
- Store image URLs. Do not download image files during either update.

## Exclusions

- Do not change library books or ComicDb.xml.
- Do not edit individual issue records in the first version.
- Do not download cover images during an update.

## Locked decisions

- ADR-064 defines the two update modes, cache storage, and resume behavior.
- All cache and network work runs on a worker thread.
- An API update replaces manual volume values.

## Tasks

- [x] T1: Add schema version 2 and cache operations for volume detail,
      authoritative issue replacement, lookup, and durable detail progress.
- [x] T2: Add forced volume and issue API updates with mock-server tests.
- [x] T3: Add the File-menu command and cache-manager dialog.
- [x] T4: Add automated UI coverage and the user test.

## Verification

- Run the required workspace format, lint, and test commands.
- Run a release UI probe with isolated XDG paths.
- Test a real MCL volume with both update modes.

## Open issues

- `UNKNOWN`: Open user test 26 has not run against a real Comic Vine volume.

## Completion record

Implemented on 2026-09-19. SQLite schema version 2 stores complete volume JSON
and a durable issue-detail queue. The summary update replaces volume metadata
and issue membership. The complete update stores one full issue response at a
time and resumes unfinished work. The cache manager opens from the File menu.

`MEASURED`: Mock-server tests cover summary replacement, preservation of
retained issue detail, complete-update cancellation, and resume. The schema
migration test upgrades a version 1 cache without data loss. The release GTK
probe opens the dialog, finds series ID 806, shows its issues, and saves manual
metadata. The required workspace verification passes.
