#!/usr/bin/env bash
set -euo pipefail
: "${TAG:?}" "${COMMIT:?}" "${GH_REPO:?}" "${RUNNER_TEMP:?}"
refs=$(git ls-remote --tags origin "refs/tags/$TAG" "refs/tags/$TAG^{}")
existing=$(awk '/\^\{\}$/ { peeled=$1 } { direct=$1 } END { print peeled ? peeled : direct }' <<< "$refs")
if [[ -z "$existing" || "$existing" == "$COMMIT" ]]; then
  exit 0
fi
if [[ "$TAG" != nightly ]]; then
  echo "Tag $TAG targets $existing instead of $COMMIT" >&2
  exit 1
fi

if gh api "repos/$GH_REPO/releases/tags/nightly" --silent 2> "$RUNNER_TEMP/nightly-error"; then
  gh release delete nightly --yes --cleanup-tag
elif grep -q 'HTTP 404' "$RUNNER_TEMP/nightly-error"; then
  # The tag can survive a failed release creation or manual deletion of the release.
  gh api --method DELETE "repos/$GH_REPO/git/refs/tags/nightly"
else
  cat "$RUNNER_TEMP/nightly-error" >&2
  exit 1
fi
