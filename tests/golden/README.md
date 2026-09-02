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
  (ComicDatabase replica), line endings normalized to CRLF. Two
  corrections were applied, both backed by the decompiled source:
  `<Display />` added to every list item (the `Display` property on
  `ComicListItem` is public with no `[XmlIgnore]`; the capture run
  omitted it), and `ExtraSyncInformation` completed to its six bool
  members (`ExtraSyncInformation.cs` declares six; the capture run
  wrote two).

## Emitter assumptions (verify against a real user database in the wild)

The writer reproduces `XmlSerializer.Serialize(Stream)` on .NET
Framework 4.8, which is what `ComicDatabase.SaveXml` uses:

1. Declaration `<?xml version="1.0" encoding="utf-8"?>`, no BOM.
2. Root carries `xmlns:xsi` then `xmlns:xsd`.
3. Two-space indentation, `\r\n` line endings (Windows XmlWriter
   default), no trailing newline.
4. Empty elements as `<Name />`; text elements close inline.
5. Members equal to their `[DefaultValue]` are omitted; the `Pages`,
   `Display`, `Books`, `ComicLists`, `WatchFolders`, `BlackList`
   wrappers are always written (non-null getters in C#).
6. Unknown child elements are captured and re-emitted between `Tags`
   and `Pages` (`[XmlAnyElement]`); unknown attributes are dropped.
7. `float` values use shortest round-trip; scientific notation
   (`1E-05`) for magnitudes at or below 1e-5 or above 7 significant
   digits. Ratings (0..5) always use plain decimal.

If a real-world ComicDb.xml round-trip shows a diff, fix the writer and
record the finding here and in a new ADR if it contradicts one.

## Known deferred items

- Fresh-database default smart lists (`InitializeDefaultLists`) are not
  seeded yet; `open_with_fallback` currently returns an empty database
  for the new-empty path.
- The `.crplugin`/zip backup restore path (`ComicDatabase.Backup`) needs
  zip support from `cr-io` (Phase 1).
- MetronInfo.xml mapping is deferred (in-archive metadata, consumed in
  Phase 1 with the archive providers).
