# Phase 1 Kickoff — IO Providers, Image Pipeline, Metadata Write-Back

Goal: comicrust opens every format ComicRack opens, extracts pages, generates thumbnails, and writes metadata back into archives without damage. Exit gate: **`cr-cli` opens, lists, and extracts pages for every supported format; ComicInfo.xml write-back verified against originals; thumbnails generated for all formats** — all headless, zero UI.

Phase 0 remains the reference for doc style: see `phase-0.md`. The Phase 0 gate (byte-stable ComicDb.xml round-trip, including the real-world database in `tests/realworld/`) is met and must stay green; do not regress it.

## Status (2026-09-02, end of phase work)

T1-T6 are built and verified; 92 tests green. The open items (the
WebComicProvider `.cbw` tail, the PDF/DjVu writers, the HEIF/AVIF/J2K
decode decision) moved to `docs/backlog.md`. Details in the task list
below and in `AGENTS.md`.

## Scope from the roadmap

Port plan Phase 1 "IO + images": all readers (zip/tar/7z/rar/pdf/folder/web), image pipeline + decode chain, resize/adjust filters, ImagePool/DiskCache caches, thumbnail generation, ComicInfo.xml write-back into archives. Estimated 8-10 weeks solo.

## Task list

### T1. Provider framework + archive readers (`cr-io`) — done, except `.cbw`

The C# spec lives in `ComicRack.Engine/IO/Provider/` (`Readers/`, `ProviderFactory.cs`, `IComicAccessor.cs`, `ComicProvider.cs`, `ImageProvider.cs`, `FileFormat.cs` + `FileFormatAttribute`).

- [x] Provider abstraction: accessor trait, provider factory, file-format registry keyed by extension and header sniffing (`KnownFileFormats.cs`, `FileFormatExtensions.cs`) — deterministic registration order; `.cbr`/`.rar` map to CBR first, both route to the same 7z accessor
- [x] CBZ reader (zip) and CBT reader (tar) — pure Rust crates; page enumeration, page-order rules (the `ExtendedStringComparer` IgnoreCase natural-sort port defines page order)
- [x] CB7/CBR/RAR5 readers via 7z **subprocess** (ADR-007); one process per operation like the C# exe mode (pooling still open as an optimization if needed)
- [x] Directory (folder-of-images) provider (FOLDER id 100, recursive walk)
- [x] PDF provider via `pdfium-render`; `CalculateSize` port verified (612x792pt page renders 1920x2484)
- [x] Dynamic-image providers: DjVu (djvulibre subprocess, PPM instead of TIFF intermediate); WebP/JXL decode in `cr-image` — HEIF/AVIF/J2K report UnsupportedFormat (need libheif/openjpeg, decide at packaging time)
- [x] `cr-cli` extraction smoke fixtures per format — synthetic (zip/tar built in tests), `7z`-created CB7, hand-built PDF fixture; gated behind `CR_FORMAT_TESTS`/`CR_PDFIUM`/djvulibre-on-PATH
- [ ] WebComicProvider (`.cbw`) — the one open reader. Moved to
  `docs/backlog.md` (the port recipe travels with the entry).

### T2. In-archive metadata (`cr-io` + `cr-core`) — done

The C# spec lives in `IO/Provider/XmlInfo/` (`ComicInfoProvider.cs`, `ComicBookProvider.cs`, `MetronInfoProvider.cs`, `XmlInfoProviderFactory.cs`) and `IInfoStorage.cs`.

- [x] ComicInfo.xml write: `ComicInfo::serialize_bytes` (root form verified against the ComicRack artifacts: declaration without `encoding`, `xsd` first, 2-space indent)
- [x] ComicBook.xml sidecar: `ComicBook::serialize_bytes` (stripped form) + `serialize_full_bytes`; `ComicBook::parse_root` for the `<ComicBook>` root
- [x] MetronInfo mapping: schema + serializer + parser + `to_comic_info` in `cr-core/model/metron_info.rs`; byte-stable round-trip tested; `MetronInfoProvider.ToXml` ported including the RoleValues substring quirks and LocalizeEnum English defaults
- [x] Provider priority + merge logic: load chain in `cr-io/info.rs` — xattrs → sidecar (`<file>.xml`, then extension-swapped) → in-archive (ComicInfo.xml order 0, MetronInfo.xml order 1 mapped, ComicBook.xml for books); `InfoLoadingMethod` Fast/Slow
- [x] `IInfoStorage` port: xattrs `user.comicrack.ComicRackInfo`/`ComicRackBook` (ADR-006) with sidecar fallback and skip-on-same-content (`is_same_content` chains ported to `cr-core`)

### T3. Image pipeline (`cr-image`) — done

The C# spec lives in `cYo.Common/Drawing/` (`ImageProcessing.cs` 1,557 LOC, `BitmapExtensions.cs`) and `IO/Provider/*Image*.cs`.

- [x] Image currency type: 8-bit RGBA `Image` (the 32bpp ARGB stand-in)
- [x] Decode chain: zune-jpeg (with the 32-bit EXIF `RemoveExif` retry preserved), png/gif/tiff/bmp/webp via `image`, jxl via jxl-oxide; `normalize_to_jpeg` (the `RetrieveSourceByteImage` conversion chain) wired into `ComicProvider::read_page`; JPEG encode q75; HEIF/AVIF/J2K unsupported (system libs)
- [x] **Preserve the 32-bit JPEG EXIF quirk** from `BitmapExtensions.BitmapFromBytes`
- [x] Resize filters: fit-to-box scale (`GetScale` semantics, scales UP too), filter mapping (Triangle ≈ bilinear, CatmullRom ≈ bicubic) — pixel-level tolerance documented in the crate docs
- [x] Color adjust: `ApplyAdjustment` port (histogram black/white points, color matrix in the C# row-vector convention — the custom 5x5 matrices sit transposed relative to GDI+ ColorMatrix, gamma LUT, sharpen convolution with border preservation)
- [x] Thumbnail rendering (`IO/ThumbnailImage.cs`): MaxHeight 512, JPEG q60, FastBilinear, size+data serialization

### T4. Page and thumbnail caches (`cr-image`) — done

The C# spec lives in `IO/Cache/` (`ImagePool.cs`, `ImageManager.cs`, `ThumbnailManager.cs`, `ImageDiskCache.cs`, `ThumbnailDiskCache.cs`, `FileCache.cs`).

- [x] Memory pools with the C# eviction shape (LRU, item + byte budgets; C# defaults 5 pages / 20 thumbs + 5 MB)
- [x] Disk caches: fresh format (one file per entry, FNV-1a name, header with key text for verification, atomic writes, index rebuilt by scan)
- [x] Cache keys: `ImageKey`/`PageKey`/`ThumbnailKey` with `IsSameFile` and the `type:\\resource` locator parse
- The `ProcessingQueue` machinery is NOT ported — it belongs to Phase 2's QueueManager

### T5. Write-back and export (`cr-io`) — done, except PDF/DjVu writers

The C# spec lives in `IO/Provider/Writers/` (`CbzStorageProvider.cs`, `CbtStorageProvider.cs`, `Cb7StorageProvider.cs`, `FolderStorageProvider.cs`, `XmlInfoStorageProvider.cs`) and `IStorageProvider.cs`.

- [x] Zip write-back (CBZ): native full rewrite — same entry order, page content byte-identical, only metadata entries change; temp file + atomic rename. (Note: the C# CE shells out to `7z u` even for zip/tar; we preserve the behavior, not the mechanism.)
- [x] Tar/7z/folder write-back (CB7 via `7z u`, per `UpdateComicInfos`; folder writes plain files). **PDF and DjVu writers remain open — moved to `docs/backlog.md`**
- [x] Failure semantics: errors surface to the caller (no silent `false`); `cr-cli rewrite` never writes when no metadata was found (writing defaults would destroy file metadata)
- [x] Export pipeline skeleton (`export.rs`: `ExportImageContainer`, compression levels, page-order CBZ packing) — the parallel/spill/progress parts wait for the Phase 5 dialogs

### T6. `cr-cli` verification tools — done

- [x] `cr-cli pages <file>` — index/type/size per page in provider page order, plus format and cache hash
- [x] `cr-cli extract <file> [page] [-o out] [--decode]` — raw bytes or full decode → JPEG
- [x] `cr-cli thumb <file>` — page-0 thumbnail (512px, JPEG q60)
- [x] `cr-cli rewrite <file>` — read metadata, write back, verify entry content hashes (only metadata entries may differ)
- [x] `cr-cli metron <file>` — print the MetronInfo mapping for a file

## Test data policy

- Never commit user library data (invariant in `AGENTS.md`). Synthetic fixtures only: build tiny cbz/cbt archives in tests from generated PNGs.
- For subprocess formats (7z, rar, pdf, djvu), prefer tiny generated samples committed as fixtures if licensing allows; otherwise gate those tests behind a `CR_FORMAT_TESTS` env var and document which are local-only. — As implemented: 7z suite runs when `CR_FORMAT_TESTS=1` and `7z` exists; PDF needs `CR_PDFIUM=<libpdfium.so>`; DjVu needs the djvulibre tools on `PATH`. Binaries are discovered on `PATH` with env overrides (`CR_SEVENZIP`, `CR_PDFIUM`, `CR_DJVULIBRE`).
- Keep one real-world smoke file per format out of the repo (user-provided, like the Phase 0 real-world database flow) for manual `cr-cli` verification.

## Acceptance criteria (phase exit)

1. [x] `cargo test` green; Phase 0 golden tests still byte-identical (regression guard). 92 tests pass.
2. [~] `cr-cli pages`/`extract`/`thumb` work on fixtures for every supported format: cbz, cbt, cb7, cbr/rar5, pdf, folder, web URL — all done except the web URL (`.cbw` provider open, see T1).
3. [x] `cr-cli rewrite` on a ComicRack-written archive changes only the metadata entries; verified via entry content hashes before/after.
4. [x] ComicInfo.xml, ComicBook.xml, and MetronInfo.xml round-trip: read → write → byte-compare (cr-core tests, ComicRack writer form).
5. [x] Thumbnail output follows the C# filter behavior (MaxHeight 512, q60, FastBilinear mapping; platform pixel tolerance documented in the crate docs).
6. [x] CI green; no planning contradictions requiring a new ADR (xattr naming per ADR-006, 7z subprocess per ADR-007 both held).

## Explicitly deferred (not Phase 1)

- Reader rendering, layouts, zoom/pan (Phase 3, ADR-008 cairo-first).
- Smart-list evaluation, scanner, watch folders, queues (Phase 2).
- Export dialogs and UI surfaces (Phase 5) — only the pipeline skeleton here.
- Device sync, wireless sync, remote server (Phase 7).
- xattr metadata migration from real Windows ADS data (accepted loss, ADR-006).

## Dependencies and sequencing notes

1. T1 first: everything else consumes the provider abstraction.
2. T2 can start after T1's CBZ reader — it needs only zip read+write and the Phase 0 emitter.
3. T3 decode chain and T4 caches can run in parallel with T2; they share the image currency type, so land that early in T3.
4. T5 depends on T1 (write access) and T2 (metadata model). T6 grows alongside each task; keep the commands working as formats land.
