#!/usr/bin/env bash
# Publish (replace) the "rolling" release on Gitea.
#
# Env needed: TOKEN (API token with repo write), API (server /api/v1),
# REPO (owner/name), SHA (commit to tag). $1 = version. The tarball and
# its .sha256 file must exist in the working directory.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <version>" >&2
    exit 1
fi
version=$1
tag=rolling
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

# Drop the previous rolling release and its tag, so the tag follows
# main. Deleting the release removes its assets too.
release_id=$(curl -fsS -H "$auth" "$API/repos/$REPO/releases/tags/$tag" | jq -r '.id // empty' || true)
if [ -n "$release_id" ]; then
    curl -fsS -X DELETE -H "$auth" "$API/repos/$REPO/releases/$release_id"
fi
curl -fsS -X DELETE -H "$auth" "$API/repos/$REPO/tags/$tag" || true

# Recreate the release at the current commit; Gitea creates the tag.
short="${SHA:0:10}"
date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
body=$(jq -n \
    --arg sha "$SHA" \
    --arg short "$short" \
    --arg version "$version" \
    --arg date "$date_utc" \
    --arg checksum "$(cut -d' ' -f1 "$checksum")" \
    --arg asset "$asset" \
    '{tag_name: "rolling",
      target_commitish: $sha,
      name: ("Rolling release v" + $version),
      prerelease: true,
      body: ("Automated build v" + $version + " from commit " + $short + " (" + $date + ").\n\nUnpack the tarball and run ./comicrust from the directory that contains assets/.\n\nSHA-256 of " + $asset + ":\n" + $checksum)}')

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
