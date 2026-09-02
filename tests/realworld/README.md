# Real-world regression fixture

`ComicDb.xml` is a genuine ComicRack CE database (255 books, default
smart lists, one watch folder). The user supplied it and approved its
commit (2026-09-02). It is the primary byte-compatibility regression
fixture: `crates/cr-core/tests/golden_roundtrip.rs` re-loads and
re-serializes it on every test run and requires byte identity.

Rules:

- Do not edit, re-save, or reformat this file. Byte identity is the
  test. Any edit must come from the user.
- Do not copy user library data anywhere else in the repo.
- If the repo ever becomes public, remove this file first.

## What this file proved (2026-09-02 validation run)

The first real-world round-trip found five writer defects that all
synthetic fixtures missed. All are fixed and covered by tests now:

1. Declaration is `<?xml version="1.0"?>` — no `encoding` attribute.
   The earlier `encoding="utf-8"` form came from a .NET 8 replica run,
   not from ComicRack.
2. Root namespace order is `xmlns:xsd` first, then `xmlns:xsi`.
3. The book time elements are `FileModifiedTime` and
   `FileCreationTime` (property names), not the shorter names an
   earlier analysis note suggested.
4. `System.Drawing.Size` adds no `<Size>` wrapper: `<ThumbnailSize>`
   and `<TileSize>` contain `Width`/`Height` directly.
5. An element whose text is the empty string serializes self-closing
   (`<CacheStorage />`, `<MatchValue />`), because `XmlTextWriter`
   treats `WriteString("")` as "no content".

Also confirmed: `\n` inside text content stays `\n` while structural
lines use `\r\n`, and 7-digit fractional seconds round-trip.
