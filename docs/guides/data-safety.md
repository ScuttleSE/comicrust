# Guide: data safety

The database is the one artifact users cannot lose. Caches are disposable.

## Never commit user library data

- `tests/testfiles/` is the home for user-supplied test comics. It is
  git-ignored. NEVER commit its contents.
- Golden test files must be synthesized or anonymized fixtures under
  `tests/golden/`.
- The real-world database under `tests/realworld/` is committed with the
  user's permission. See `tests/realworld/README.md`. Remove it before the
  repository becomes public.

History: one 42 MB comic was committed by accident in Phase 3. The history
was rewritten the same day with a `filter-branch` index-filter, a force-push,
and a local `gc`. The blob is gone from the remote. The Gitea server can hold
the old pack objects until its own GC runs.

## Protect the database

- `ComicDb.xml` element names, attribute names, casing, and structure must
  match the C# `XmlSerializer` output exactly.
- Golden round-trip tests verify this. Do not weaken them.
- Database writes are atomic: write a temporary file, then rename.
- A probe or a test must never write to the user's real database.

## Isolate test and probe state

- Set isolated XDG paths in tests and probes. Do not read or write the real
  `~/.config/comicrust/` or `~/.local/share/comicrust/`.
- A probe must not change the user's preferences.

## Destructive commands

- Never pass an empty path or an unset variable to a delete command. Check
  the value first.
- Never run a recursive delete built from a variable you did not print.

## Metadata

`ComicInfo.xml`, ComicRack's `ComicBook.xml`, and `MetronInfo.xml` are read
and written in the archive. Preserve the schema. A write-back that corrupts
an archive loses user data.
