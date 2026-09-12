# comicrust

comicrust is a native Linux port of [ComicRack Community Edition](https://github.com/maforget/ComicRackCE), the Windows comic library manager and reader. It reads and writes the same library database and the same comic metadata as ComicRack CE. The goal is to preserve ComicRack's core library, metadata, and reader behavior on Linux.

## Status

comicrust is a usable pre-release. The main workflows work, and validation continues. Expect occasional bugs. comicrust keeps a backup copy of the library database and can recover it. Still, make your own backups of `ComicDb.xml`.

## Features

**Library**

- Add comics from folders. The scanner finds new, moved, and removed files. Watch folders rescan automatically.
- Browse in thumbnail, tile, or detail views. Group, sort, filter, and quick search.
- Organize with smart lists, folders, and reading lists. Import `.cbl` reading lists made with ComicRack.
- Find duplicate books.
- Edit the details of a book. Bulk-edit many books at once.
- Track reading state: current page, read percentage, open count, rating, and tags.
- The Quick Open view shows your recent lists at startup.

**Reader**

- Single page, double page, adaptive double page, and continuous scroll layouts.
- Manga mode (right-to-left). Page rotation, zoom, and pan.
- Page turn transitions, paper textures, and background colors or images.
- Bookmarks and a magnifier lens. Full screen mode. Undock the reader into its own window.
- ComicRack-style keyboard shortcuts.

**Files and metadata**

- Read and write `ComicInfo.xml`, `MetronInfo.xml`, and ComicRack `ComicBook.xml` metadata.
- ComicRack's NTFS stream metadata maps to Linux file attributes (xattrs), with sidecar files as fallback.
- Export comics to CBZ or CBT.
- Create and restore backups of the library.

**Scraping**

- The Comic Vine Scraper is built in. Set it up under File ▸ Comic Vine Scraper Settings…: register at comicvine.gamespot.com/api for a free API key and paste it in.

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

To convert a comic to another format (for example `.cbr` to `.cbz`), use "Export…" with Target = "Replace source".

Writing metadata into CBR/RAR archives needs the RARLAB `rar` command (see Requirements). Without it, edits stay in the library database.

## Install

### Arch package

Download `comicrust-<version>-source.tar.gz` and `PKGBUILD` from the [Releases page](https://github.com/ScuttleSE/comicrust/releases), put both in one folder, and build:

```sh
makepkg -f
sudo pacman -U comicrust-<version>-*-x86_64.pkg.tar.zst
```

The package installs `/usr/bin/comicrust` with a desktop entry and app icon.

### Debian package

Download `comicrust_<version>-1_amd64.deb` from the [Releases page](https://github.com/ScuttleSE/comicrust/releases). Then:

```sh
sudo apt install ./comicrust_<version>-1_amd64.deb
```

The deb is built on Debian 13 (glibc 2.41). Older distros use the portable tarball below.

### Portable tarball

Download `comicrust-<version>-linux-amd64.tar.gz` from the [Releases page](https://github.com/ScuttleSE/comicrust/releases). Then:

```sh
mkdir comicrust
tar xzf comicrust-*-linux-amd64.tar.gz -C comicrust
./comicrust/comicrust
```

Keep the `assets` folder next to the `comicrust` binary.

### From source

You need the Rust toolchain and GTK 4.6 development packages.

```sh
git clone https://github.com/ScuttleSE/comicrust.git
cd comicrust
cargo run -p cr-app --release
```

Run from the repository root. The build then finds its assets in the source tree.

## Requirements

- Linux with GTK 4.6 or newer (`libgtk-4-1`)
- Optional: `7z` (p7zip) for CB7 and CBR archives
- Optional: the pdfium library (`libpdfium.so`) for PDF files
- Optional: the djvulibre tools for DjVu files
- Optional: the RARLAB `rar` command for metadata write-back into CBR/RAR

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
| `~/.config/comicrust/comicrust.toml` | the config file: settings, engine/extended options, plugin settings, and the editable data tables |
| `~/.local/share/comicrust/ComicDb/ComicDb.xml` | the library database |
| `~/.local/share/comicrust/Cache/` | thumbnail and image caches. Safe to delete. |

comicrust also writes a `ComicDb.xml.bak` copy next to the database and can recover from it after a problem.

### The config file

All configuration lives in one TOML file: `~/.config/comicrust/comicrust.toml`. The app seeds every changeable key at its default. Hand edits apply at the next start. Every parameter is documented in [docs/config-reference.md](docs/config-reference.md).

## Migrate your library from Windows ComicRack

Put your ComicRack CE `ComicDb.xml` into `~/.local/share/comicrust/ComicDb/` before you start comicrust. Your comic files stay where they are.

## Added in comicrust

Features that ComicRack and ComicRack CE do not have:

- A dark mode toggle at runtime (Browse ▸ Dark Mode). The reader follows the theme.
- A cache-folder chooser under Preferences ▸ Advanced.
- An option to turn off automatic thumbnail generation (Preferences ▸ Advanced). File ▸ Generate Cover Thumbnails fills the gaps later.
- Cover markers. A "?" marks books without metadata, a red "!" marks unreadable files, and an amber "≠" marks format mismatches. The tooltip gives the reason.
- Scans that never stall on one file: a per-file time limit, a "Skip current file" command, one summary at the end, and problem verdicts you can list with smart lists.
- Metadata write-back into CBR and RAR archives (needs the `rar` command).
- Vertical column lines in Detail view.
- One hand-editable configuration file with editable data tables (the Comic Vine imprint mappings).
- A Windows-path migration dialog and a `cr-cli migrate` helper for ComicRack CE profiles.
- The Comic Vine Scraper, ported natively from the add-on.

## Not ported from ComicRack CE

- Python plugins and scripts. Popular script features exist as built-in commands instead (for example "New Comic…" and "New fileless Book Series…").
- The remote library server and the Android app sync.
- Device sync.
- Translations. The interface is English only.
- Web comics (`.cbw`).
- Export to PDF, DjVu, or CB7.
- HEIF, AVIF, and JPEG 2000 pages.
- The system tray icon.
- The bottom-docked browser layout and the sidebar preview pane.

Other deferred differences are tracked in [docs/backlog.md](docs/backlog.md).

## For developers

See [docs/current-status.md](docs/current-status.md) for the project status, [AGENTS.md](AGENTS.md) for the contribution rules, and the [docs](docs/) folder for the port plan, the decision records, and the phase notes.

## Credits and license

ComicRack was created by Markus Eisenstöck (cYo) and continued as [Community Edition](https://github.com/maforget/ComicRackCE) by maforget and contributors. comicrust uses the CE source as its behavioral specification. All credit for the original design belongs there.

The Comic Vine Scraper port is based on the add-on by Cory Banack (`https://github.com/cbanack/comic-vine-scraper`), Apache 2.0.

comicrust bundles artwork from ComicRack CE without changes: the toolbar and menu icons, the reader paper and background textures, and the page-activity animation frames. All of it stays under the license below.

## License

comicrust is licensed **GPL-2.0-only**, the same license as ComicRack CE. The full text is in [LICENSE](LICENSE). See ADR-041 in [docs/decisions.md](docs/decisions.md) for the reasoning and for one recorded open question about the Apache-2.0 Comic Vine Scraper port.