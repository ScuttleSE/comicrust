# comicrust

comicrust is a native Linux port of [ComicRack Community Edition](https://github.com/maforget/ComicRackCE), the Windows comic library manager and reader. It reads and writes the same library file and the same comic metadata as ComicRack CE. The goal is full feature parity with the original.

## Status

The core application is complete and ready for daily use. Work continues on polish, packaging, and documentation. Expect occasional bugs. comicrust keeps a backup copy of the library database and can recover it. Still, make your own backups of `ComicDb.xml`.

## Features

**Library**

- Add comics from folders. The scanner finds new, moved, and removed files. Watch folders rescan automatically.
- Browse in thumbnail, tile, or detail views. Group, sort, filter, and quick search.
- Organize with smart lists, folders, and reading lists. Import `.cbl` reading lists made with ComicRack.
- Find duplicate books.
- Edit the details of a book. Bulk-edit many books at once.
- Track reading state: current page, read percentage, open count, rating, and tags.
- The Quick Open view shows your recent and favorite lists at startup.

**Reader**

- Single page, double page, adaptive double page, and continuous scroll layouts.
- Manga mode (right-to-left). Page rotation, zoom, and pan.
- Page turn transitions, paper textures, and background colors or images.
- Bookmarks and a magnifier lens. Full screen mode. Undock the reader into its own window.
- The full keyboard shortcut set of the original.

**Files and metadata**

- Read and write `ComicInfo.xml`, `MetronInfo.xml`, and ComicRack `ComicBook.xml` metadata.
- comicrust stores embedded metadata as Linux file attributes (xattrs) where ComicRack used NTFS streams.
- Export comics to CBZ or CBT.
- Create and restore backups of the library.

## Supported formats

| Comic format | Read | Write metadata back |
|---|---|---|
| CBZ | yes | yes |
| CBT | yes | yes |
| CB7 | yes (needs 7z) | yes (needs 7z) |
| CBR, RAR | yes (needs 7z) | yes (needs the `rar` CLI) |
| PDF | yes (needs pdfium) | no |
| DjVu | yes (needs djvulibre) | no |
| Folders of images | yes | yes (sidecar files) |

Page images: JPEG, PNG, GIF, TIFF, WebP, and JPEG XL. HEIF, AVIF, and JPEG 2000 pages do not decode yet.

Export produces CBZ or CBT archives. Export to PDF, DjVu, or CB7 is not available.

To convert a comic to another format (for example `.cbr` to `.cbz`),
use "Export…" with Target = "Replace source": the book re-points to
the new file and the old one moves to the trash. "Delete original
files after export" and "Add exported files to the library" cover the
other conversions.

Writing metadata into CBR/RAR archives uses the RARLAB `rar` command
(not included; install it from your package repository or
[rarlab.com](https://www.rarlab.com/download.htm), or point `CR_RAR`
at the binary). Old RAR4 archives keep their format when updated.
Without `rar`, edits stay in the library database and "Update Book
File(s)" reports the error.

## Install

### Arch package

Download `comicrust-<version>-source.tar.gz` and `PKGBUILD` from the [Releases page](https://github.com/ScuttleSE/comicrust/releases), put both in one folder, and build:

```sh
makepkg -f
sudo pacman -U comicrust-<version>-*-x86_64.pkg.tar.zst
```

The package installs `/usr/bin/comicrust` with a desktop entry and app icon. Optional packages: `p7zip` (CB7/CBR reading), `djvulibre` (DjVu), `rar` (RAR write-back).

### Flatpak bundle

Download `comicrust-<version>.flatpak` from the [Releases page](https://github.com/ScuttleSE/comicrust/releases). Install it from the file (the freedesktop runtime comes from Flathub):

```sh
flatpak remote-add --if-not-exists --user flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install --user ./comicrust-<version>.flatpak
flatpak run io.github.ScuttleSE.comicrust
```

### Portable tarball

Download `comicrust-<version>-linux-amd64.tar.gz` from the [Releases page](https://github.com/ScuttleSE/comicrust/releases). Then:

```sh
mkdir comicrust
tar xzf comicrust-*-linux-amd64.tar.gz -C comicrust
./comicrust/comicrust
```

Keep the `assets` folder next to the `comicrust` binary. It holds the icons, paper textures, and backgrounds.

### From source

You need the Rust toolchain and GTK 4.6 development packages.

```sh
git clone https://github.com/ScuttleSE/comicrust.git
cd comicrust
cargo run -p cr-app --release
```

Run from the repository root. The build then finds its assets in the source tree.## Requirements

- Linux with GTK 4.6 or newer (`libgtk-4-1`)
- Optional: `7z` (p7zip) for CB7, CBR, and RAR archives
- Optional: the pdfium library (`libpdfium.so`) for PDF files
- Optional: the djvulibre tools for DjVu files

Debian or Ubuntu example:

```sh
sudo apt install libgtk-4-1 p7zip-full
```

## Command line

```sh
comicrust                  # open the library browser
comicrust comic.cbz        # open a comic
comicrust list.cbl         # import a reading list
```

comicrust runs as a single instance. A second start sends its files to the running app.

## Your data

| Path | Contents |
|---|---|
| `~/.config/comicrust/` | settings (`Config.xml`, `comicrust.ini`) |
| `~/.local/share/comicrust/ComicDb/ComicDb.xml` | the library database |
| `~/.local/share/comicrust/Cache/` | thumbnail and image caches. Safe to delete. |

comicrust also writes a `ComicDb.xml.bak` copy next to the database and can recover from it after a problem.

## Migrate your library from Windows ComicRack

The database format is the same. Your comic files stay where they are.

**The easy way** — run the migration helper from the release tarball (or your build):

```sh
cr-cli migrate /path/to/ComicRackCE-profile
```

Point it at your ComicRack profile folder (on Windows `%APPDATA%\cYo\ComicRack Community Edition`; copy it over if you run comicrust on another machine). The tool verifies the database, copies `ComicDb.xml` into `~/.local/share/comicrust/ComicDb/`, and maps the settings from `ComicRack.ini` that comicrust consumes. Use `--dry-run` to preview, `--out` for a custom target, `--force` to replace an existing database (the old file is kept as `ComicDb.xml.premigrate.bak`).

**The manual way** — copy `ComicDb.xml` from `%APPDATA%\cYo\ComicRack Community Edition\ComicDb\` to `~/.local/share/comicrust/ComicDb/`.

**Windows paths.** A migrated database points at Windows locations (`C:\...`, `\\server\...`). comicrust detects them at startup and offers the migration dialog: pick the Linux folder each Windows root maps to, and the app re-homes every found book (missing ones become fileless entries that keep their metadata). The same dialog is available any time under File ▸ Migrate Windows Paths…. If the comic files themselves moved to different names, add the comic folders to the library instead — the scanner re-links moved books by file name and size.

## Differences from ComicRack CE

- Python plugins do not run. Some popular script features exist as built-in commands instead (for example "New Comic…" and "New fileless Book Series…").
- No remote server and no Android app sync.
- English only. The translation files of the original are not loaded yet.
- Web comics (`.cbw`) are not supported yet.
- You cannot export to PDF, DjVu, or CB7.

## For developers

See [AGENTS.md](AGENTS.md) for the project status and the [docs](docs/) folder for the port plan, the decision records, and the phase notes.

## Credits and license

ComicRack was created by Markus Eisenstöck (cYo) and continued as [Community Edition](https://github.com/maforget/ComicRackCE) by maforget and contributors. comicrust uses the CE source as its behavioral specification. All credit for the original design belongs there.

The project has no license yet. The decision is deliberate and planned (see `docs/decisions.md`, ADR-009).
