#!/usr/bin/env bash
# Build the comicrust Debian package from a source tree that was
# already built (target/release/cr-app present). Run from the source
# tree root:
#
#   VERSION=0.0.279 packaging/deb/build.sh [output-dir]
#
# The layout mirrors the PKGBUILD:
#   /usr/bin/comicrust
#   /usr/share/comicrust/assets/{icons,papers,backgrounds}
#   /usr/share/applications + /usr/share/icons/hicolor + metainfo
#
# Depends floor: the binary is built on Debian trixie (glibc 2.41), so
# the deb targets Debian 13 and equivalent-glibc derivatives; older
# distros use the portable tarball.
set -euo pipefail

if [ -z "${VERSION:-}" ]; then
    echo "VERSION env required" >&2
    exit 1
fi
out_dir=${1:-.}
if [ ! -x target/release/cr-app ]; then
    echo "build first: cargo build --release --locked --offline -p cr-app" >&2
    exit 1
fi

deb_name="comicrust_${VERSION}-1_amd64"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

install -d "$stage/DEBIAN" "$stage/usr/bin" "$stage/usr/share/comicrust/assets" \
    "$stage/usr/share/applications" "$stage/usr/share/metainfo" \
    "$stage/usr/share/icons/hicolor/128x128/apps" \
    "$stage/usr/share/icons/hicolor/256x256/apps"

install -m755 target/release/cr-app "$stage/usr/bin/comicrust"
for kind in icons papers backgrounds scan pages; do
    cp -r "crates/cr-ui/assets/$kind" "$stage/usr/share/comicrust/assets/$kind"
done
install -m644 packaging/io.github.ScuttleSE.comicrust.desktop \
    "$stage/usr/share/applications/io.github.ScuttleSE.comicrust.desktop"
install -m644 packaging/icons/io.github.ScuttleSE.comicrust.128.png \
    "$stage/usr/share/icons/hicolor/128x128/apps/io.github.ScuttleSE.comicrust.png"
install -m644 packaging/icons/io.github.ScuttleSE.comicrust.256.png \
    "$stage/usr/share/icons/hicolor/256x256/apps/io.github.ScuttleSE.comicrust.png"
install -m644 packaging/io.github.ScuttleSE.comicrust.metainfo.xml \
    "$stage/usr/share/metainfo/io.github.ScuttleSE.comicrust.metainfo.xml"

cat > "$stage/DEBIAN/control" <<EOF
Package: comicrust
Version: ${VERSION}-1
Architecture: amd64
Maintainer: ScuttleSE <https://github.com/ScuttleSE>
Depends: libc6 (>= 2.41), libgtk-4-1 (>= 4.6)
Recommends: p7zip-full, djvulibre-bin
Section: graphics
Priority: optional
Homepage: https://github.com/ScuttleSE/comicrust
Description: Comic library manager and reader (ComicRack port)
 ComicRust organizes digital comic archives (CBZ, CBR, CBT, CB7,
 folders, PDF, DjVu) in a library with smart lists, reads them in a
 configurable reader, and reads and writes embedded ComicInfo.xml
 metadata without damaging the archives.
 Linux-native port of ComicRack Community Edition.
EOF

cat > "$stage/DEBIAN/postinst" <<'EOF'
#!/bin/sh
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -qf /usr/share/icons/hicolor || true
fi
EOF
chmod 755 "$stage/DEBIAN/postinst"

dpkg-deb --build --root-owner-group "$stage" "$out_dir/$deb_name.deb"
echo "built $out_dir/$deb_name.deb"
