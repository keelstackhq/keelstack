#!/usr/bin/env bash
# One command from "release X.Y.Z" to a committed release branch (lockstep, D-023).
#
#   bash scripts/release-pr.sh X.Y.Z
#
# On a clean checkout of main (or any base) it:
#   1. creates the branch release/vX.Y.Z,
#   2. folds every folder's changelog and progress fragments (<folder>/changelog.d,
#      <folder>/docs/progress.d) with the folder's fold-fragments.sh,
#   3. runs scripts/bump-version.sh X.Y.Z (Cargo.toml, path deps, Cargo.lock, npm launchers and
#      platform packages, pnpm lock specifiers, the splice Action default, every CHANGELOG),
#   4. commits it as `chore: release X.Y.Z`,
# then prints the command that opens the PR. It never pushes, tags or publishes: merging the PR
# is what releases (.github/workflows/release.yml, docs/releasing.md).
# RELEASE_PR_NO_BRANCH=1 stays on the current branch (tests).
set -euo pipefail

die() {
  echo "release-pr: $*" >&2
  exit 1
}

[ "$#" -eq 1 ] || die "usage: bash scripts/release-pr.sh X.Y.Z"
ver=$1
[[ $ver =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || die "'$ver' is not a version: expected exactly X.Y.Z"

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

cur=$(awk -F'"' '/^\[/ { s = ($0 == "[workspace.package]") } s && /^version *=/ { print $2; exit }' Cargo.toml)
[ -n "$cur" ] || die "no [workspace.package] version in Cargo.toml"
[ "$cur" != "$ver" ] || die "the workspace is already at $ver"
newest=$(printf '%s\n%s\n' "$cur" "$ver" | sort -V | tail -n1)
[ "$newest" = "$ver" ] || die "$ver is older than the current $cur"

[ -z "$(git status --porcelain)" ] || die "the working tree is not clean: commit or stash first"
branch="release/v$ver"
if [ "${RELEASE_PR_NO_BRANCH:-}" != 1 ]; then
  if git rev-parse --verify --quiet "refs/heads/$branch" >/dev/null; then
    die "branch $branch already exists"
  fi
  orig=$(git rev-parse --abbrev-ref HEAD)
  git checkout -q -b "$branch"
  # The tree was clean, so a failure before the commit is undone completely: back on the
  # original branch, the half-made release branch gone.
  committed=0
  cleanup() {
    [ "$committed" = 1 ] && return 0
    echo "release-pr: failed; going back to $orig and removing $branch" >&2
    git reset -q --hard
    git clean -fdq
    git checkout -q "$orig"
    git branch -q -D "$branch"
  }
  trap cleanup EXIT
fi

# Fold fragments of every folder that has them (only bosun does today).
for d in */; do
  d=${d%/}
  if [ -d "$d/changelog.d" ] || [ -d "$d/docs/progress.d" ]; then
    script="$d/scripts/fold-fragments.sh"
    [ -f "$script" ] || die "$d has fragments but no $script"
    echo "folding fragments of $d"
    bash "$script" "$root/$d"
  fi
done

bash scripts/bump-version.sh "$ver"

git add -A
git commit -q -m "chore: release $ver" \
  -m "Lockstep version bump to $ver (D-023). Merging this PR starts release.yml."
committed=1
echo
echo "release $ver committed on $(git rev-parse --abbrev-ref HEAD) ($(git rev-parse --short HEAD))."
echo "Open the PR (the version-bump merge starts the release; the owner taps the release environment):"
echo "  git push -u origin $(git rev-parse --abbrev-ref HEAD)"
echo "  gh pr create --base main --title 'chore: release $ver' --body 'Lockstep release $ver. Merge starts release.yml (docs/releasing.md).'"
