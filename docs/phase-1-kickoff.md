# Phase 1 Kickoff — IO Providers, Image Pipeline, Metadata Write-Back

Goal: comicrust opens every format ComicRack opens, extracts pages, generates thumbnails, and writes metadata back into archives without damage. Exit gate: **`cr-cli` opens, lists, and extracts pages for every supported format; ComicInfo.xml write-back verified against originals; thumbnails generated for all formats** — all headless, zero UI.

Phase 0 remains the reference for doc style: see `phase-0-kickoff.md`. The Phase 0 gate (byte-stable ComicDb.xml round-trip, including the real-world database in `tests/realworld/`) is met and must stay green; do not regress it.

## Scope from the roadmap

Port plan Phase 1 "IO + images": all readers (zip/tar/7z/rar/pdf/folder/web), image pipeline + decode chain, resize/adjust filters, ImagePool/DiskCache caches, thumbnail generation, ComicInfo.xml write-back into archives. Estimated 8-10 weeks solo.

## Task list

### T1. Provider framework + archive readers (`cr-io`)

The C# spec lives in `ComicRack.Engine/IO/Provider/` (`Readers/`, `ProviderFactory.cs`, `IComicAccessor.cs`, `ComicProvider.cs`, `ImageProvider.cs`, `FileFormat.cs` + `FileFormatAttribute`).

- [ ] Provider abstraction: accessor trait, provider factory, file-format registry keyed by extension and header sniffing (`KnownFileFormats.cs`, `FileFormatExtensions.cs`)
- [ ] CBZ reader (zip) and CBT reader (tar) — pure Rust crates; page enumeration, page-order rules, in-memory and streaming access
- [ ] CB7/CBR/RAR5 readers via 7z or libarchive **subprocess** (ADR-007: never static-link unrar); subprocess pooling and error mapping
- [ ] Directory (folder-of-images) provider (`FileProviderBase.cs`)
- [ ] PDF provider via `pdfium-render`; page raster limited like the C# (≤1920×2540 normalization, `Pdfium.cs`)
- [ ] Dynamic-image providers: DjVu (djvulibre subprocess, `DjVuImage.cs`), WebP/JXL/HEIF/AVIF (`WebpImage.cs`, `JpegXLImage.cs`, `HeifAvifImage.cs`, `Jpeg2000Image.cs` for j2k)
- [ ] `cr-cli` extraction smoke fixtures per format under `tests/fixtures/` (see test data policy below)

### T2. In-archive metadata (`cr-io` + `cr-core`)

The C# spec lives in `IO/Provider/XmlInfo/` (`ComicInfoProvider.cs`, `ComicBookProvider.cs`, `MetronInfoProvider.cs`, `XmlInfoProviderFactory.cs`) and `IInfoStorage.cs`.

- [ ] ComicInfo.xml write: `ComicInfo::write_xml` root serialization already exists in `cr-core`; verify the root form against a ComicRack-written ComicInfo.xml (the `ComicRack Introduction.djvu.xml` artifact shows: declaration without `encoding`, `xsd` first, 2-space indent)
- [ ] ComicBook.xml sidecar: port `ComicBook.Serialize` (ComicBook.cs:2743) — clone and strip the file-derived fields (`Id`, `FilePath`, `FileModifiedTime`, `FileCreationTime`, `FileSize`, `LastOpenedFromListId`, `CustomThumbnailKey`, dirty flags, `FileInfoRetrieved`/`FileIsMissing`, `ExtraSyncInformation`, `NewPages`, `IsDynamicSource`, `EnableDynamicUpdate`) before writing. Note the C# also has `SerializeFull` (ComicBook.cs:2777) which writes everything — port both.
- [ ] MetronInfo mapping (deferred from Phase 0; scope in `tests/golden/README.md`): map the generated schema (`MetronInfo.cs`, 1,789 LOC) to `cr-core` structs with the same emitter rules
- [ ] Provider priority + merge logic: which of ComicInfo.xml / ComicBook.xml / MetronInfo.xml wins per field (`InfoLoadingMethod.cs`, `XmlInfoProviders.cs`)
- [ ] `IInfoStorage` port: NTFS ADS → Linux xattrs `user.comicrack.*` with sidecar fallback (ADR-006; `NtfsInfoStorage.cs`)

### T3. Image pipeline (`cr-image`)

The C# spec lives in `cYo.Common/Drawing/` (`ImageProcessing.cs` 1,557 LOC, `BitmapExtensions.cs`) and `IO/Provider/*Image*.cs`.

- [ ] Image currency type: the `System.Drawing.Bitmap` stand-in (pixel format, disposal, EXIF orientation handling)
- [ ] Decode chain: zune-jpeg, image-webp, png/gif/tiff via `image`, jxl-oxide, libheif-rs, j2k, pdfium-render, djvulibre subprocess — with the normalize-to-JPEG chain from `feasibility.md`
- [ ] **Preserve the 32-bit JPEG EXIF quirk** from `BitmapExtensions.BitmapFromBytes` (see Critical gotchas in `AGENTS.md`)
- [ ] Resize filters: port the exact filter set and defaults from `ImageProcessing.cs` (scale-to-fit, thumbnail modes)
- [ ] Color adjust: apply `BitmapAdjustment` (saturation/contrast/brightness/gamma/sharpening) like `ImageProcessing.cs`; differential-test against C# reference outputs
- [ ] Thumbnail rendering (`IO/ThumbnailImage.cs`): page pick, aspect, caption-free sizing

### T4. Page and thumbnail caches (`cr-image`)

The C# spec lives in `IO/Cache/` (`ImagePool.cs`, `ImageManager.cs`, `ThumbnailManager.cs`, `ImageDiskCache.cs`, `ThumbnailDiskCache.cs`, `FileCache.cs`).

- [ ] Memory pools with the C# eviction shape (`ImagePool.cs`, `IPagePool`, `IThumbnailPool`)
- [ ] Disk caches: **fresh format** — the C# `cache.idx` is BinaryFormatter and has NO compat requirement (invariant #5). Design a simple index+data layout; document it in the module
- [ ] Cache keys: port `IO/PageKey.cs` and `IO/ThumbnailKey.cs` semantics (they feed the engine's caches in Phase 2)

### T5. Write-back and export (`cr-io`)

The C# spec lives in `IO/Provider/Writers/` (`CbzStorageProvider.cs`, `CbtStorageProvider.cs`, `Cb7StorageProvider.cs`, `FolderStorageProvider.cs`, `XmlInfoStorageProvider.cs`) and `IStorageProvider.cs`.

- [ ] Zip write-back (CBZ): replace/add ComicInfo.xml entry, preserve all other entries byte-for-byte, preserve entry order and compression where possible
- [ ] Tar/7z/folder write-back; PDF and DjVu writers are last (lowest use)
- [ ] Failure semantics: temp-file + atomic replace, `WriteErrorException` mapping
- [ ] Export pipeline skeleton (`ExportSetting.cs`, `ExportImageContainer.cs`) — the dialogs come in Phase 5

### T6. `cr-cli` verification tools

- [ ] `cr-cli pages <file>` — enumerate pages with index/type/size per page
- [ ] `cr-cli extract <file> [page]` — decode and write a page image (verifies the full decode chain)
- [ ] `cr-cli thumb <file>` — generate a thumbnail image
- [ ] `cr-cli rewrite <file>` — read metadata, write it back, verify archive integrity (entry hashes before/after, only metadata entries differ)
- [ ] `cr-cli metron <file>` — print the MetronInfo mapping for a file (once T2 lands it)

## Test data policy

- Never commit user library data (invariant in `AGENTS.md`). Synthetic fixtures only: build tiny cbz/cbt archives in tests from generated PNGs.
- For subprocess formats (7z, rar, pdf, djvu), prefer tiny generated samples committed as fixtures if licensing allows; otherwise gate those tests behind a `CR_FORMAT_TESTS` env var and document which are local-only.
- Keep one real-world smoke file per format out of the repo (user-provided, like the Phase 0 real-world database flow) for manual `cr-cli` verification.

## Acceptance criteria (phase exit)

1. `cargo test` green; Phase 0 golden tests still byte-identical (regression guard).
2. `cr-cli pages`/`extract`/`thumb` work on fixtures for every supported format: cbz, cbt, cb7, cbr/rar5, pdf, folder, web URL.
3. `cr-cli rewrite` on a ComicRack-written archive changes only the metadata entries; ComicRack (or `ComicInfoProvider` semantics) still reads the result.
4. ComicInfo.xml, ComicBook.xml, and MetronInfo.xml round-trip: read from a ComicRack-written file → write → byte-compare (modulo documented diffs in a README, like Phase 0).
5. Thumbnail output for the same input matches the C# filter behavior on reference cases (documented tolerance for platform differences).
6. CI green; ADRs updated with anything learned that contradicts planning.

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
