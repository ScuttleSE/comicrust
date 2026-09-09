# Phase 11 — Packaging (Arch + Flatpak)

Re-homed from `docs/backlog.md` ("From Phase 8", the deferred T8) on
2026-09-09. The user decisions that shape it:

- **Arch:** an in-repo PKGBUILD; CI builds the package in an
  `archlinux:base-devel` container and attaches the `.pkg.tar.zst` to
  the release. AUR submission stays a manual follow-up (no CI secrets).
- **Flatpak:** a self-hosted bundle — the manifest lives in the repo;
  CI builds a single-file `.flatpak` and attaches it to the release.
  Users install with `flatpak install ./comicrust.flatpak`. Flathub is
  NOT the target (a follow-up if asked).
- **Trigger:** the new workflow runs on manual dispatch with a tag
  input (the `tagged-release.yaml` shape), gated by fmt/clippy/test.
  The rolling tarball track is untouched.
- Priorities: Arch first, Flatpak second. `.deb`/RPM stay in the
  backlog.

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

### T3. Flatpak manifest

`packaging/flatpak/io.github.ScuttleSE.comicrust.yml`:

- `org.freedesktop.Platform` / `Sdk` 24.08 with the
  `org.freedesktop.Sdk.Extension.rust-stable` sdk-extension
  (the canonical Rust flatpak shape; GTK4 is in the runtime).
- `finish-args`: `--socket=fallback-x11 --socket=wayland
  --share=ipc --device=dri --filesystem=home` (comics live anywhere in
  home; the library DB stays in the sandboxed app data — no extra
  grants). GApplication single-instance rides the session bus under
  the own name (granted by default).
- `buildsystem: simple`: cargo `--offline --locked` against the
  vendored tarball; installs binary + assets + desktop integration
  under `/app`.
- The repo manifest carries placeholder source url+sha256; the CI job
  seds in the release tarball's values at build time (a per-release
  field cannot live in a committed manifest). The header documents the
  local-build recipe.

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
  4. `flatpak` (comicrust-ci, needs source): apt-install
     flatpak/flatpak-builder, add flathub, install the 24.08
     runtime/sdk/extension, sed the manifest, `flatpak-builder` +
     `build-bundle`, attach the `.flatpak` + sha256 to both.
  - Known cost: act_runner containers are ephemeral, so the flatpak
    runtime re-downloads each run (~1-2 GB). An actions/cache step is
    the follow-up if it hurts.
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
  4. `flatpak install ./comicrust-<v>.flatpak` then
     `flatpak run io.github.ScuttleSE.comicrust`; same asset checks
     on a fresh sandbox; `flatpak uninstall` clean.
  5. Confirm the portable tarball still runs as before (CWD-relative
     assets win).

## Omissions / postponed

- AUR publishing (needs an AUR account + SSH deploy key secret) — the
  PKGBUILD is AUR-ready by construction; submit by hand later.
- Flathub submission — the manifest follows Flathub conventions but
  Flathub hosts its own manifest repo + review.
- `.deb`/RPM — backlog.
- Real app-icon art — placeholder generated; replace in `packaging/`.
- LICENSE file — open user decision; blocks nothing mechanically.
