#!/usr/bin/env bash
# Exercise real git-cliff/Cargo commands without touching the project checkout.
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
xtask=${XTASK_BIN:-$repo/target/debug/xtask}
files="$repo/.github/scripts/release-files.sh"
test_root=$(mktemp -d)
export RUNNER_TEMP="$test_root/output"
export GITHUB_OUTPUT="$test_root/outputs"
export GIT_CLIFF_OFFLINE=true
mkdir -p "$RUNNER_TEMP" "$test_root/repo/crates/maa-cli/src"
cd "$test_root/repo"
git init -q -b main
git config user.name 'Release Test'
git config user.email 'release@example.invalid'
git config commit.gpgsign false
cp "$repo/cliff.toml" .
printf '[workspace]\nmembers = ["crates/maa-cli"]\nresolver = "2"\n' > Cargo.toml
printf '[package]\nname = "maa-cli"\nversion = "0.7.5"\nedition = "2024"\n' > crates/maa-cli/Cargo.toml
printf 'pub fn example() {}\n' > crates/maa-cli/src/lib.rs
printf '# Release Notes\n\n## Release 0.7.5\n\nPrevious release\n' > CHANGELOG.md
cargo generate-lockfile --offline
git add .
git commit -qm 'chore: initial'
git tag v0.7.5

select_version() {
  : > "$GITHUB_OUTPUT"
  "$xtask" release select-version --version "$1" > "$RUNNER_TEMP/select.log"
  VERSION=$(sed -n 's/^version=//p' "$GITHUB_OUTPUT")
  PREVIOUS_TAG=$(sed -n 's/^previous_tag=//p' "$GITHUB_OUTPUT")
  export VERSION PREVIOUS_TAG
}

git commit -q --allow-empty -m 'docs: clarify usage'
select_version auto
[[ "$VERSION" == 0.7.5 ]]
git commit -q --allow-empty -m 'fix: repair A'
select_version auto
[[ "$VERSION" == 0.7.6 ]]
git checkout -qb release-prep/v0.7.6
bash "$files" prepare
bash "$files" check
git add .
git commit -qm 'chore(release): prepare for v0.7.6'
prepared_sha=$(git rev-parse HEAD)
bash "$files" notes 0.7.6 > "$RUNNER_TEMP/before-tag.md"
git tag v0.7.6
bash "$files" notes 0.7.6 > "$RUNNER_TEMP/after-tag.md"
cmp "$RUNNER_TEMP/before-tag.md" "$RUNNER_TEMP/after-tag.md"
git tag -d v0.7.6 > /dev/null

# A stale PR must be refreshable even after its version files reached main.
git checkout -q main
git commit -q --allow-empty -m 'fix: repair B'
git cherry-pick "$prepared_sha" > /dev/null
if bash "$files" check > "$RUNNER_TEMP/stale.log" 2>&1; then
  echo 'Expected stale release notes to fail' >&2
  exit 1
fi
select_version auto
[[ "$VERSION" == 0.7.6 ]]
bash "$files" prepare
[[ "$(git diff --name-only)" == CHANGELOG.md ]]
bash "$files" check
[[ "$(grep -c '^## Release 0.7.6$' CHANGELOG.md)" == 1 ]]
grep -q 'Repair B' CHANGELOG.md
git add CHANGELOG.md
git commit -qm 'chore(release): prepare for v0.7.6'

# A normal version-only PR can be followed by a changelog-only release PR.
git checkout -qb manual-bump v0.7.5
sed -i.bak 's/0.7.5/0.8.0/' crates/maa-cli/Cargo.toml
mv crates/maa-cli/Cargo.toml.bak "$RUNNER_TEMP/old-manifest"
cargo update -p maa-cli --precise 0.8.0 --offline
git add .
git commit -qm 'chore: bump maa-cli to 0.8.0'
select_version auto
[[ "$VERSION" == 0.8.0 ]]
bash "$files" prepare
[[ "$(git diff --name-only)" == CHANGELOG.md ]]
bash "$files" check
select_version 0.9.0
[[ "$VERSION" == 0.9.0 ]]
select_version patch
[[ "$VERSION" == 0.7.6 ]]

# The beta CLI reads its published index but must not write source files.
mkdir version
git init -q version
git -C version config user.name 'Release Test'
git -C version config user.email 'release@example.invalid'
git -C version config commit.gpgsign false
for channel in stable beta alpha; do
  jq -n --arg channel "$channel" \
    '{version:"0.7.5",details:{tag:"v0.7.5",commit:"old",assets:{}}}' > "version/$channel.json"
done
git -C version add .
git -C version commit -qm 'chore: published index'
git diff > "$RUNNER_TEMP/before-beta.diff"
commit=$(git rev-parse HEAD)
: > "$GITHUB_OUTPUT"
"$xtask" release meta --channel beta --commit "$commit" --version 0.8.0 --publish
grep -qx 'version=0.8.0-beta.1' "$GITHUB_OUTPUT"
git diff > "$RUNNER_TEMP/after-beta.diff"
cmp "$RUNNER_TEMP/before-beta.diff" "$RUNNER_TEMP/after-beta.diff"
git -C version diff --exit-code

# A stable tag may already exist while its version-index job still needs a retry.
git tag v0.8.0
: > "$GITHUB_OUTPUT"
"$xtask" release meta --channel beta --commit "$commit" --version auto --publish
grep -qx 'skip=true' "$GITHUB_OUTPUT"
echo "Release file and beta integration checks passed (fixtures: $test_root)"
