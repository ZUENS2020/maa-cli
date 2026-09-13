#!/usr/bin/env bash
# Re-read the published index inside every downstream job, including failed-job retries.
set -euo pipefail
: "${VERSION:?}" "${GITHUB_REPOSITORY:?}" "${GH_TOKEN:?}"
index=$(curl --fail --silent --show-error \
  --header "Authorization: Bearer $GH_TOKEN" \
  --header 'Accept: application/vnd.github.raw+json' \
  --header 'Cache-Control: no-cache' \
  "${GITHUB_API_URL:-https://api.github.com}/repos/$GITHUB_REPOSITORY/contents/stable.json?ref=version")
if ! jq -e --arg version "$VERSION" --arg tag "${TAG:-v$VERSION}" --arg commit "${COMMIT:-}" \
  '.version == $version and .details.tag == $tag and
   ($commit == "" or .details.commit == $commit)' <<< "$index" > /dev/null; then
  echo 'This is no longer the indexed stable release; refusing to publish downstream.' >&2
  exit 1
fi
