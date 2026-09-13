#!/usr/bin/env bash
# Query on every attempt, including retries that did not rerun the meta job.
set -euo pipefail
: "${GH_REPO:?}" "${GITHUB_RUN_ID:?}" "${GITHUB_OUTPUT:?}"
artifacts=$(gh api --paginate "repos/$GH_REPO/actions/runs/$GITHUB_RUN_ID/artifacts" |
  jq -s '[.[].artifacts[] | select(.name == "release-plan" or .name == "release-bundle")]')
if jq -e 'any(.[]; .expired)' <<< "$artifacts" > /dev/null; then
  echo 'Release artifacts expired; do not recalculate or rebuild an existing release.' >&2
  exit 1
fi
for name in plan bundle; do
  exists=$(jq --arg name "release-$name" 'any(.[]; .name == $name)' <<< "$artifacts")
  echo "$name=$exists" >> "$GITHUB_OUTPUT"
done
