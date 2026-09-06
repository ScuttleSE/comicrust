#!/usr/bin/env bash
# Publish a release on the GitHub mirror (ScuttleSE/comicrust). Shared by
# the two release workflows, called right after the Gitea publish step:
#
#   rolling release (release.yaml):
#     publish_github_release.sh rolling "$VERSION" true true
#
#   tagged release (tagged-release.yaml):
#     publish_github_release.sh v0.1.0 "0.1.0" false false
#
# The build happens once on Gitea; the same tarball + sha256 are attached
# to a GitHub release with the same tag and body.
#
# Usage: publish_github_release.sh <tag> <version> <prerelease true|false> <delete-tag true|false>
#
# Env needed: GH_TOKEN (a GitHub PAT with Contents read/write on GH_REPO),
# GH_REPO (owner/name), SHA (commit to attach). The tarball
# comicrust-<version>-linux-amd64.tar.gz and its .sha256 file must
# exist in the working directory.
#
# Skips with a notice when GH_TOKEN is unset, so the workflow stays
# green until the secret is configured on Gitea.
set -euo pipefail

if [ "$#" -ne 4 ]; then
    echo "usage: $0 <tag> <version> <prerelease true|false> <delete-tag true|false>" >&2
    exit 1
fi
tag=$1
version=$2
prerelease=$3
delete_tag=$4

if [ -z "${GH_TOKEN:-}" ]; then
    echo "GH_TOKEN not set - skipping the GitHub mirror publish (add the GITHUB_RELEASE_TOKEN secret on Gitea to enable it)"
    exit 0
fi
for v in GH_REPO SHA; do
    if [ -z "${!v:-}" ]; then
        echo "missing env $v" >&2
        exit 1
    fi
done
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
for f in "$asset" "$checksum"; do
    if [ ! -f "$f" ]; then
        echo "missing file $f" >&2
        exit 1
    fi
done

API="https://api.github.com"
UPLOAD="https://uploads.github.com/repos/$GH_REPO/releases"
auth="Authorization: Bearer $GH_TOKEN"
stdhdr=(-H "$auth" -H "Accept: application/vnd.github+json" -H "X-GitHub-Api-Version: 2022-11-28")

# Drop the previous release, so its assets go with it. For the rolling
# release also drop the tag ref, so the tag can follow main.
release_id=$(curl -fsS "${stdhdr[@]}" "$API/repos/$GH_REPO/releases/tags/$tag" | jq -r '.id // empty' || true)
if [ -n "$release_id" ]; then
    curl -fsS -X DELETE "${stdhdr[@]}" "$API/repos/$GH_REPO/releases/$release_id"
fi
if [ "$delete_tag" = "true" ]; then
    curl -fsS -X DELETE "${stdhdr[@]}" "$API/repos/$GH_REPO/git/refs/tags/$tag" || true
fi

# Create the release. GitHub creates the tag at SHA when it does not
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

created=$(curl -fsS -X POST "${stdhdr[@]}" -H "Content-Type: application/json" \
    -d "$body" "$API/repos/$GH_REPO/releases")
release_id=$(printf '%s' "$created" | jq -r '.id')

# GitHub asset upload takes the RAW body on uploads.github.com
# (multipart -F is a Gitea-ism and fails here).
curl -fsS -X POST "${stdhdr[@]}" -H "Content-Type: application/octet-stream" \
    --data-binary "@${asset}" \
    "$UPLOAD/$release_id/assets?name=${asset}" > /dev/null
curl -fsS -X POST "${stdhdr[@]}" -H "Content-Type: application/octet-stream" \
    --data-binary "@${checksum}" \
    "$UPLOAD/$release_id/assets?name=${checksum}" > /dev/null

echo "published $asset on GitHub (release $release_id, tag $tag at $short)"
