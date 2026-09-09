#!/usr/bin/env bash
# Attach additional assets to an EXISTING release on the GitHub mirror
# (ScuttleSE/comicrust). Used by the packaging workflow
# (packaging.yaml) right after attach_release_assets.sh.
#
# Skips with a notice when GH_TOKEN is unset (the
# MIRROR_RELEASE_TOKEN shape), so the workflow stays green until the
# secret is configured on Gitea.
#
# Usage: attach_github_release_assets.sh <tag> <file>...
#
# Env needed: GH_TOKEN (a GitHub PAT with Contents read/write on
# GH_REPO), GH_REPO (owner/name).
set -euo pipefail

if [ "$#" -lt 1 ]; then
    echo "usage: $0 <tag> <file>..." >&2
    exit 1
fi
tag=$1
shift

if [ -z "${GH_TOKEN:-}" ]; then
    echo "GH_TOKEN not set - skipping the GitHub mirror attach (add the MIRROR_RELEASE_TOKEN secret on Gitea to enable it)"
    exit 0
fi
for v in GH_REPO; do
    if [ -z "${!v:-}" ]; then
        echo "missing env $v" >&2
        exit 1
    fi
done
if [ "$#" -eq 0 ]; then
    echo "no files to attach" >&2
    exit 1
fi

API="https://api.github.com"
UPLOAD="https://uploads.github.com/repos/$GH_REPO/releases"
auth="Authorization: Bearer $GH_TOKEN"
stdhdr=(-H "$auth" -H "Accept: application/vnd.github+json" -H "X-GitHub-Api-Version: 2022-11-28")

release_id=$(curl -fsS "${stdhdr[@]}" "$API/repos/$GH_REPO/releases/tags/$tag" | jq -r '.id // empty')
if [ -z "$release_id" ]; then
    echo "no release for tag '$tag' on $GH_REPO - run the tagged release workflow first" >&2
    exit 1
fi

for file in "$@"; do
    if [ ! -f "$file" ]; then
        echo "missing file $file" >&2
        exit 1
    fi
    name=$(basename "$file")
    # GitHub asset upload takes the RAW body on uploads.github.com
    # (multipart -F is a Gitea-ism and fails here).
    curl -fsS -X POST "${stdhdr[@]}" -H "Content-Type: application/octet-stream" \
        --data-binary "@${file}" \
        "$UPLOAD/$release_id/assets?name=${name}" > /dev/null
    echo "attached $name on GitHub (release $release_id, tag $tag)"
done
