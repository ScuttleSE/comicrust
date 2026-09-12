#!/usr/bin/env bash
# Build the comicrust Debian package from a source tree that was
# already built (target/release/cr-app present). Run from the source
# tree root:
#
#   VERSION=0.0.279 packaging/deb/build.sh [output-dir]
#
# The layout mirrors the PKGBUILD:
#   /usr/bin/comicrust
#   /usr/share/comicrust/assets/{icons,papers,backgrounds,scan,pages}
#   /usr/share/applications + /usr/share/icons/hicolor + metainfo
#   /usr/share/doc/comicrust/copyright (Debian policy 12.5)
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
    "$stage/usr/share/doc/comicrust" \
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

# Debian policy 12.5 requires a copyright file. ADR-041: GPL-2.0-only.
# The machine-readable format needs the full license text indented,
# so the root LICENSE is appended with a leading space on each line
# and a "." standing in for every blank line.
{
    cat <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: comicrust
Upstream-Contact: ScuttleSE <https://github.com/ScuttleSE>
Source: https://github.com/ScuttleSE/comicrust

Files: *
Copyright: 2026 ScuttleSE and the comicrust contributors
License: GPL-2.0-only

Files: crates/cr-ui/assets/icons/* crates/cr-ui/assets/papers/*
 crates/cr-ui/assets/backgrounds/* crates/cr-ui/assets/scan/*
 crates/cr-ui/assets/pages/*
Copyright: Markus Eisenstoeck (cYo) and the ComicRack Community Edition
 contributors
License: GPL-2.0-only
Comment: Redistributed without change from ComicRack Community Edition
 (https://github.com/maforget/ComicRackCE). See ADR-041.

Files: crates/cr-scrape/*
Copyright: Cory Banack
License: Apache-2.0 and GPL-2.0-only
Comment: A port of the Comic Vine Scraper add-on
 (https://github.com/cbanack/comic-vine-scraper), which is licensed
 Apache-2.0. ADR-041 records an unresolved compatibility question
 between that license and GPL-2.0-only.

License: GPL-2.0-only
EOF
    sed -e 's/^/ /' -e 's/^ $/ ./' LICENSE
} > "$stage/usr/share/doc/comicrust/copyright"
chmod 644 "$stage/usr/share/doc/comicrust/copyright"

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
