# Phase 11 — Packaging (Arch + Flatpak)

Re-homed from `docs/backlog.md` ("From Phase 8", the deferred T8) on
2026-09-09. The user decisions that shape it:

- **Arch:** an in-repo PKGBUILD; CI builds the package in an
  `archlinux:base-devel` container and attaches the `.pkg.tar.zst` to
  the release. AUR submission stays a manual follow-up (no CI secrets).
- **Flatpak:** a self-hosted bundle — DROPPED 2026-09-09 mid-phase
  (user decision) after two runtime walls; replaced by the **Debian
  .deb**. The full record lives in the omissions section.
- **Trigger:** the new workflow runs on manual dispatch with a tag
  input (the `tagged-release.yaml` shape), gated by fmt/clippy/test.
  The rolling tarball track is untouched.
- Priorities: Arch first, .deb second. RPM stays in the backlog.

Existing tracks stay as-is: `release.yaml` (rolling tarball) and
`tagged-release.yaml` (stable tarball). This phase adds
`packaging.yaml` and package definitions; it does not modify the two
release workflows.

## Design

### Source tarball (the shared input)

Both packagers build from a CI-produced source tarball:

- `comicrust-<version>-source.tar.gz` = the checkout contents
  (`git archive HEAD`) PLUS a `vendor/` directory
  (`cargo vendor`) and a `.cargo/config.toml` selecting the vendored
  sources. Vendored = offline builds everywhere: makepkg does not run
  `cargo fetch`, and flatpak-builder's no-network sandbox builds.
- Attached to the SAME release (Gitea + GitHub mirror), so a user
  running the PKGBUILD pulls the exact bytes CI validated. The
  sha256 travels in the release assets; the workflow passes it
  between jobs as an artifact.
- `VERSION=<version>` is exported for every build (the `cr-ui`
  `build.rs` stamp), so the About version equals the package version.

### T1. App install paths + desktop integration

- **Asset lookup.** Today the roots are CWD-relative only
  (`assets/icons` + the dev fallback `crates/cr-ui/assets/icons` in
  `icon.rs`; the papers/backgrounds pair in `page_view.rs`). A system
  install puts the binary in `/usr/bin` and assets in
  `/usr/share/comicrust/assets`, so the lookup gains (in order):
  1. the existing CWD-relative pair (portable tarball + dev, first —
     current behavior preserved),
  2. `current_exe()/../share/comicrust/assets` (covers /usr/bin AND
     the flatpak /app/bin prefix),
  3. `$XDG_DATA_HOME/comicrust/assets`,
  4. every `$XDG_DATA_DIRS` entry + `/comicrust/assets`
     (default /usr/local/share:/usr/share).
  New `cr-ui/src/assets.rs` shared by `icon.rs` and `page_view.rs`.
  The env-reading part stays a thin wrapper over a pure
  `find_with(roots, rel)` core that carries the unit tests (no env
  mutation in tests — cargo runs them in parallel threads).
- **`packaging/` tree** (shared by PKGBUILD and the flatpak manifest):
  - `io.github.ScuttleSE.comicrust.desktop` (app id
    `io.github.ScuttleSE.comicrust`; `Exec=comicrust %U`; the comic
    MIME types),
  - hicolor app icons `128x128` + `256x256` (PLACEHOLDER art generated
    programmatically — replace when real art exists),
  - `io.github.ScuttleSE.comicrust.metainfo.xml` (appstream; the
    release CI seds the version+date of the built release in, the repo
    copy keeps a generic dev entry).
- Gate: unit tests on the resolution order; the packaged launches
  (user test) prove the icons/papers/backgrounds load from
  `/usr/share`.

### T2. PKGBUILD

`packaging/arch/PKGBUILD`:

- `source=("comicrust-$pkgver-source.tar.gz::<release url>")` with
  sha256sums; CI seds `pkgver` + the checksum and drops the artifact
  tarball next to it (so CI validates exactly what a user runs —
  checksums included).
- `depends=(gtk4)`; `optdepends` for p7zip (CB7/CBR reading), djvulibre
  (DjVu), rar (RAR write-back, from Phase 10).
- `build()`: `export VERSION="$pkgver"`; `cargo build --release
  --locked --offline -p cr-app`.
- `package()`: binary → `/usr/bin/comicrust`; assets →
  `/usr/share/comicrust/assets/{papers,backgrounds,icons}`; desktop
  file → `/usr/share/applications`; icons →
  `/usr/share/icons/hicolor/{128x128,256x256}/apps`; metainfo →
  `/usr/share/metainfo`.
- OPEN GAP (user decision pending): no LICENSE file in the repo, so
  `license=` and the metainfo `<project_license>` carry placeholders.
- The tarball layout note: the portable tarball ships `assets/` NEXT
  to the binary; packages install them under `/usr/share/comicrust/`.
  Both resolve through T1's lookup.

### T3. Debian package (the .deb pivot, 2026-09-09)

The Flatpak attempt is DROPPED (user decision: "seems very
cumbersome") after two measured walls in one day: the 24.08 rust
extension is EOL-frozen at rustc 1.89 (< the crates' 1.92), and the
26.08 freedesktop runtime has NO GTK4 at all (only gtk3 — paged
through the freedesktop-sdk components tree); the next step would
have been the GNOME runtime + its extension wiring. The full record
lives in the omissions section. The replacement:

- `packaging/deb/build.sh` — a hand-rolled `dpkg-deb --build` script
  (zero extra tooling; dpkg-deb ships in the Debian CI image), run
  from the built source tree: `VERSION=<v> packaging/deb/build.sh
  [out-dir]`. Same layout as the PKGBUILD: `/usr/bin/comicrust`,
  `/usr/share/comicrust/assets/{icons,papers,backgrounds}`, the
  desktop file, the hicolor icons, the metainfo.
- Control: `Depends: libc6 (>= 2.41), libgtk-4-1 (>= 4.6)` — the
  binary is built on Debian trixie, so the deb targets Debian 13 and
  equivalent-glibc derivatives (older distros: the portable tarball);
  `Recommends: p7zip-full, djvulibre-bin`. A `postinst` refreshes the
  desktop-database and icon cache (guarded `|| true`).
- CI (the `deb` job): fetches + sha-verifies the release source
  tarball, extracts it, builds offline (`--locked --offline`, the
  vendored config), runs the script, sanity-checks with
  `dpkg-deb --info/--contents`, attaches `comicrust_<v>-1_amd64.deb`
  + sha256 to both hosts.

### T4. CI: `packaging.yaml` + attach scripts

- **The existing publish scripts cannot be reused:** both
  `publish_release.sh` and `publish_github_release.sh` DELETE and
  recreate the release, which would drop any previously attached
  assets. New scripts:
  - `.gitea/attach_release_assets.sh <tag> <file>...` — finds the
    release by tag on Gitea, POSTs each file as an additional asset;
    fails hard when the release does not exist (run
    `tagged-release.yaml` first).
  - `.gitea/attach_github_release_assets.sh <tag> <file>...` — same on
    the mirror (uploads.github.com raw-body upload, the
    `publish_github_release.sh` lesson). Skips with a notice when
    `GH_TOKEN` is unset (the MIRROR_RELEASE_TOKEN shape).
- **Workflow** `.gitea/workflows/packaging.yaml` — manual dispatch,
  `tag` input (validated in the `tagged-release.yaml` form):
  1. `checks` (comicrust-ci image): fmt, clippy, test — the same gate
     as the tagged release.
  2. `source` (comicrust-ci, needs checks): checkout at the tag,
     version step, assemble the vendored source tarball + sha256 +
     a `meta.env` (version, sha256), upload as workflow artifacts.
  3. `arch` (archlinux:base-devel, needs source): restore artifacts,
     create a build user (makepkg refuses root), install rust,
     sed the PKGBUILD, `makepkg -f`, attach the `.pkg.tar.zst` +
     sha256 to Gitea AND GitHub.
  4. `deb` (comicrust-ci, needs source): fetch + verify the source
     tarball, extract, build offline, `dpkg-deb --build`, attach the
     `.deb` + sha256 to both.
- Tokens: `RELEASE_TOKEN` (Gitea) and `MIRROR_RELEASE_TOKEN` (GitHub),
  the existing secrets.

## Gate

- fmt/clippy/`cargo test --workspace` green; the new asset-lookup
  unit tests green.
- The workflow files are validated by CI itself on first dispatch
  (this machine cannot run makepkg/flatpak — the containers decide).
- USER TEST (after a tagged release exists — e.g. `v0.1.0` — and the
  workflow ran):
  1. Download `comicrust-<v>-source.tar.gz` from the release; on an
     Arch machine (or container): `makepkg -f` in the extracted
     PKGBUILD dir; `pacman -U` the package.
  2. Launch from the desktop menu (the icon shows) AND from a
     terminal in an unrelated directory; confirm icons/papers/
     backgrounds render (the reader Display dialog lists 4 papers +
     14 backgrounds).
  3. `pacman -R comicrust` removes cleanly.
  4. Download `comicrust_<v>-1_amd64.deb` from the release; on a
     Debian 13 machine (or container): `sudo apt install ./...deb`;
     launch from the desktop menu (the icon shows) AND from a terminal
     in an unrelated directory; same asset checks; `sudo apt remove
     comicrust` removes cleanly.
  5. Confirm the portable tarball still runs as before (CWD-relative
     assets win).

## Omissions / postponed

- **Flatpak — DROPPED 2026-09-09 (user decision)** after the measured
  wall chain: (1) the runner's docker profile blocks bwrap namespaces
  (needed the runner-host `container.privileged: true`), (2) the
  24.08 rust-stable extension is EOL-frozen at rustc 1.89 while the
  locked gtk-rs crates need 1.92+, (3) the 26.08 freedesktop runtime
  has NO GTK4 at all (only gtk3 — freedesktop-sdk components tree),
  so a GNOME-runtime manifest would have been the next round. The
  manifest was deleted with the pivot to the .deb; the pieces remain
  in git history (`packaging/flatpak/` up to commit 1cf2d37) if it is
  ever revisited.
- AUR publishing (needs an AUR account + SSH deploy key secret) — the
  PKGBUILD is AUR-ready by construction; submit by hand later.
- RPM — backlog.
- Real app-icon art — placeholder generated; replace in `packaging/`.
- LICENSE file — open user decision; blocks nothing mechanically.
