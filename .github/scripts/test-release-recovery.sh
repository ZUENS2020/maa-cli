#!/usr/bin/env bash
# Mock only remote API responses; exercise the actual workflow helper scripts.
set -euo pipefail
scripts=$(cd "$(dirname "$0")" && pwd)
export RUNNER_TEMP
RUNNER_TEMP=$(mktemp -d)
export GITHUB_OUTPUT="$RUNNER_TEMP/outputs"
export GH_REPO=example/repo GITHUB_RUN_ID=123 COMMIT=new TAG=nightly
export GH_CALLS="$RUNNER_TEMP/calls" TEST_CASE=orphan

git() { printf 'old\trefs/tags/%s\n' "$TAG"; }
gh() {
  echo "$*" >> "$GH_CALLS"
  case "$*" in
    *actions/runs/*)
      case "$TEST_CASE" in
        empty) echo '{"artifacts":[]}' ;;
        bundle) echo '{"artifacts":[{"name":"release-plan","expired":false},{"name":"release-bundle","expired":false}]}' ;;
        expired) echo '{"artifacts":[{"name":"release-bundle","expired":true}]}' ;;
        denied) echo 'HTTP 403' >&2; return 1 ;;
      esac
      ;;
    'api --method DELETE '*) ;;
    *releases/tags/nightly*)
      case "$TEST_CASE" in
        orphan) echo 'gh: Not Found (HTTP 404)' >&2; return 1 ;;
        denied) echo 'gh: Forbidden (HTTP 403)' >&2; return 1 ;;
      esac
      ;;
    'release delete nightly --yes --cleanup-tag') ;;
    *) echo "Unexpected gh call: $*" >&2; return 1 ;;
  esac
}
export -f git gh

bash "$scripts/release-tag.sh"
grep -q '^api --method DELETE .*git/refs/tags/nightly$' "$GH_CALLS"
TEST_CASE=exists
: > "$GH_CALLS"
bash "$scripts/release-tag.sh"
grep -qx 'release delete nightly --yes --cleanup-tag' "$GH_CALLS"
TEST_CASE=denied
: > "$GH_CALLS"
if bash "$scripts/release-tag.sh" 2> "$RUNNER_TEMP/error"; then exit 1; fi
if grep -q 'DELETE\|release delete' "$GH_CALLS"; then exit 1; fi
TAG=v0.8.0
if bash "$scripts/release-tag.sh" 2> "$RUNNER_TEMP/error"; then exit 1; fi
COMMIT=old
bash "$scripts/release-tag.sh"

# A failed-job retry must see a bundle uploaded after meta last ran.
TEST_CASE=empty
: > "$GITHUB_OUTPUT"
bash "$scripts/release-artifacts.sh"
grep -qx 'bundle=false' "$GITHUB_OUTPUT"
TEST_CASE=bundle
: > "$GITHUB_OUTPUT"
bash "$scripts/release-artifacts.sh"
grep -qx 'bundle=true' "$GITHUB_OUTPUT"
grep -qx 'plan=true' "$GITHUB_OUTPUT"
for TEST_CASE in expired denied; do
  : > "$GITHUB_OUTPUT"
  if bash "$scripts/release-artifacts.sh" 2> "$RUNNER_TEMP/error"; then exit 1; fi
  [[ ! -s "$GITHUB_OUTPUT" ]]
done
echo "Release recovery checks passed (fixtures: $RUNNER_TEMP)"
