#!/usr/bin/env bash
# Publish a release on Gitea. Shared by the two release workflows:
#
#   rolling release (release.yaml):
#     publish_release.sh rolling "$VERSION" true true
#     Deletes the previous "rolling" release AND its tag, so the tag
#     follows main, then recreates the release at the current commit.
#
#   tagged release (tagged-release.yaml):
#     publish_release.sh v0.1.0 "0.1.0" false false
#     Publishes a proper release for an EXISTING tag. The tag is never
#     moved or deleted; a re-run replaces the release (and its assets)
#     but keeps the tag where it is.
#
# Usage: publish_release.sh <tag> <version> <prerelease true|false> <delete-tag true|false>
#
# Env needed: TOKEN (API token with repo write), API (server /api/v1),
# REPO (owner/name), SHA (commit to attach). The tarball
# comicrust-<version>-linux-amd64.tar.gz and its .sha256 file must
# exist in the working directory.
set -euo pipefail

if [ "$#" -ne 4 ]; then
    echo "usage: $0 <tag> <version> <prerelease true|false> <delete-tag true|false>" >&2
    exit 1
fi
tag=$1
version=$2
prerelease=$3
delete_tag=$4

case "$prerelease" in
    true|false) ;;
    *) echo "prerelease must be true or false, got '$prerelease'" >&2; exit 1 ;;
esac
case "$delete_tag" in
    true|false) ;;
    *) echo "delete-tag must be true or false, got '$delete_tag'" >&2; exit 1 ;;
esac

asset="comicrust-${version}-linux-amd64.tar.gz"
checksum="${asset}.sha256"

for v in TOKEN API REPO SHA; do
    if [ -z "${!v:-}" ]; then
        echo "missing env $v" >&2
        exit 1
    fi
done
for f in "$asset" "$checksum"; do
    if [ ! -f "$f" ]; then
        echo "missing file $f" >&2
        exit 1
    fi
done

auth="Authorization: token $TOKEN"

# Drop the previous release, so its assets go with it. For the rolling
# release also drop the tag itself, so the tag can follow main.
release_id=$(curl -fsS -H "$auth" "$API/repos/$REPO/releases/tags/$tag" | jq -r '.id // empty' || true)
if [ -n "$release_id" ]; then
    curl -fsS -X DELETE -H "$auth" "$API/repos/$REPO/releases/$release_id"
fi
if [ "$delete_tag" = "true" ]; then
    curl -fsS -X DELETE -H "$auth" "$API/repos/$REPO/tags/$tag" || true
fi

# Recreate the release. Gitea creates the tag at SHA when it does not
# exist (rolling); for an existing tag (tagged release) it attaches to it.
if [ "$tag" = "rolling" ]; then
    name="Rolling release v${version}"
else
    name="Release v${version}"
fi
short="${SHA:0:10}"
date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
body=$(jq -n \
    --arg sha "$SHA" \
    --arg short "$short" \
    --arg name "$name" \
    --arg tag "$tag" \
    --arg date "$date_utc" \
    --arg checksum "$(cut -d' ' -f1 "$checksum")" \
    --arg asset "$asset" \
    --argjson pre "$prerelease" \
    '{tag_name: $tag,
      target_commitish: $sha,
      name: $name,
      prerelease: $pre,
      body: ($name + " from commit " + $short + " (" + $date + ").\n\nUnpack the tarball and run ./comicrust from the directory that contains assets/.\n\nSHA-256 of " + $asset + ":\n" + $checksum)}')

created=$(curl -fsS -X POST -H "$auth" -H "Content-Type: application/json" \
    -d "$body" "$API/repos/$REPO/releases")
release_id=$(printf '%s' "$created" | jq -r '.id')

curl -fsS -X POST -H "$auth" \
    -F "attachment=@${asset}" \
    "$API/repos/$REPO/releases/$release_id/assets?name=${asset}" > /dev/null
curl -fsS -X POST -H "$auth" \
    -F "attachment=@${checksum}" \
    "$API/repos/$REPO/releases/$release_id/assets?name=${checksum}" > /dev/null

echo "published $asset (release $release_id, tag $tag at $short)"
