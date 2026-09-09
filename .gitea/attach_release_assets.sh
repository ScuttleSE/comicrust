#!/usr/bin/env bash
# Attach additional assets to an EXISTING release on Gitea. Used by
# the packaging workflow (packaging.yaml) to add the source tarball,
# the Arch package, and the Flatpak bundle to the release that
# tagged-release.yaml already created.
#
# Unlike publish_release.sh this NEVER deletes or recreates the
# release (that would drop the previously attached assets) — it fails
# hard when the release is missing, so run tagged-release first.
#
# Usage: attach_release_assets.sh <tag> <file>...
#
# Env needed: TOKEN (API token with repo write), API (server /api/v1),
# REPO (owner/name).
set -euo pipefail

if [ "$#" -lt 1 ]; then
    echo "usage: $0 <tag> <file>..." >&2
    exit 1
fi
tag=$1
shift

for v in TOKEN API REPO; do
    if [ -z "${!v:-}" ]; then
        echo "missing env $v" >&2
        exit 1
    fi
done
if [ "$#" -eq 0 ]; then
    echo "no files to attach" >&2
    exit 1
fi

auth="Authorization: token $TOKEN"

release_id=$(curl -fsS -H "$auth" "$API/repos/$REPO/releases/tags/$tag" | jq -r '.id // empty')
if [ -z "$release_id" ]; then
    echo "no release for tag '$tag' on $REPO - run the tagged release workflow first" >&2
    exit 1
fi

for file in "$@"; do
    if [ ! -f "$file" ]; then
        echo "missing file $file" >&2
        exit 1
    fi
    name=$(basename "$file")
    # Idempotent re-runs: replace an existing asset of the same name.
    asset_id=$(curl -fsS -H "$auth" "$API/repos/$REPO/releases/$release_id/assets" \
        | jq -r --arg name "$name" '.[] | select(.name == $name) | .id' | head -n 1)
    if [ -n "$asset_id" ]; then
        curl -fsS -X DELETE -H "$auth" \
            "$API/repos/$REPO/releases/$release_id/assets/$asset_id" > /dev/null
        echo "replaced existing $name"
    fi
    curl -fsS -X POST -H "$auth" \
        -F "attachment=@${file}" \
        "$API/repos/$REPO/releases/$release_id/assets?name=${name}" > /dev/null
    echo "attached $name (release $release_id, tag $tag)"
done
