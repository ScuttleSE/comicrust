# Golden fixtures — ComicDb.xml compatibility

These files are the byte-compatibility reference for the comicrust
ComicDb.xml writer. Rule: never commit real user library data. Every
fixture here is synthetic or anonymized.

## Fixtures

- `db-small.xml` — hand-written minimal library (one book, one list).
- `db-large.xml` — snapshot of the `large_db()` constructor in
  `crates/cr-core/tests/golden_roundtrip.rs`; covers the full surface
  (pages, color adjustment, custom values, unparsed elements, nested
  lists, matchers, watch folders, blacklist). Re-bless with
  `CR_BLESS=1 cargo test -p cr-core --test golden_roundtrip`.
- `db-net-reference.xml` — captured .NET `XmlSerializer` output
  (ComicDatabase replica), line endings normalized to CRLF, header
  normalized to the real ComicRack form (see the assumptions below;
  the .NET 8 replica used a different declaration form). Two
  corrections were applied, both backed by the decompiled source:
  `<Display />` added to every list item (the `Display` property on
  `ComicListItem` is public with no `[XmlIgnore]`; the capture run
  omitted it), and `ExtraSyncInformation` completed to its six bool
  members (`ExtraSyncInformation.cs` declares six; the capture run
  wrote two).

## Emitter assumptions (validated against a real user database)

The writer reproduces the ComicRack CE (net48) `XmlSerializer` output
form. Every rule below is confirmed byte-for-byte by the round-trip on
`tests/realworld/ComicDb.xml` (255 books, 2026-09-02):

1. Declaration `<?xml version="1.0"?>` — **no** `encoding` attribute.
   (An earlier .NET 8 replica produced `encoding="utf-8"`; the real
   ComicRack output does not.)
2. Root carries `xmlns:xsd` first, then `xmlns:xsi`.
3. Two-space indentation, `\r\n` line endings (Windows XmlWriter
   default), no trailing newline. `\n` inside text content stays `\n`.
4. Empty elements as `<Name />`; text elements close inline. An
   element whose text is the empty string serializes self-closing
   (`WriteString("")` writes no content).
5. Members equal to their `[DefaultValue]` are omitted; the `Pages`,
   `Display`, `Books`, `ComicLists`, `WatchFolders`, `BlackList`
   wrappers are always written (non-null getters in C#).
6. Unknown child elements are captured and re-emitted between `Tags`
   and `Pages` (`[XmlAnyElement]`); unknown attributes are dropped.
   Real-world capture check: lowercase `writer` and `Issue` elements
   round-trip verbatim.
7. `float` values use shortest round-trip; scientific notation
   (`1E-05`) for magnitudes at or below 1e-5 or above 7 significant
   digits. Ratings (0..5) always use plain decimal.
8. 7-digit fractional seconds round-trip exactly (`2026-09-02T17:51:16.2915449Z`).

If a future real-world ComicDb.xml round-trip shows a diff, fix the
writer and record the finding here and in a new ADR if it contradicts
one.

## Known deferred items

- Fresh-database default smart lists (`InitializeDefaultLists`) are not
  seeded yet; `open_with_fallback` currently returns an empty database
  for the new-empty path.
- The `.crplugin`/zip backup restore path (`ComicDatabase.Backup`): zip
  support now exists in `cr-io` (Phase 1); the restore flow itself is
  still open (Phase 2 backup-manager work).
- MetronInfo.xml: done in Phase 1 (schema, serializer, parser, and the
  `MetronInfo` → `ComicInfo` mapping in `cr-core/model/metron_info.rs`).
