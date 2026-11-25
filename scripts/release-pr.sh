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
#   5. makes the landing site follow X.Y.Z (site/apps/site): installs site/ dependencies if
#      missing, copies hull's ready.mjs as the hosted assess/vX.Y.Z (apps/site/scripts/adopt.mjs,
#      pinned to the commit of step 4, which already holds hull/docs/adopt.md at X.Y.Z), and
#      regenerates src/pages/docs, site/agents and adopt.json (gen-docs.ts); commits that as
#      `chore: site follows X.Y.Z`,
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
  [ "$orig" != HEAD ] || die "HEAD is detached: check out main (or another branch) first"
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

# The site's pinned copies (ci-site checks them against this commit): adopt.mjs reads hull/ at HEAD,
# so it runs after the bump commit; its output is a second commit.
if [ ! -d site/node_modules ]; then
  command -v pnpm >/dev/null || die "pnpm not found: install pnpm (see site/package.json packageManager), then rerun"
  pnpm --dir site install --frozen-lockfile --prefer-offline >&2 ||
    die "'pnpm --dir site install --frozen-lockfile' failed: fix it, then rerun"
fi
(cd site/apps/site && node scripts/adopt.mjs HEAD "v$ver") >&2 ||
  die "site/apps/site/scripts/adopt.mjs failed: run 'pnpm --dir site install' and rerun"
git add -A
git commit -q -m "chore: site follows $ver" \
  -m "Hosted assess v$ver, adopt.json and the regenerated docs pages (apps/site/scripts/adopt.mjs)."
committed=1
echo
echo "release $ver committed on $(git rev-parse --abbrev-ref HEAD) ($(git rev-parse --short HEAD))."
echo "Open the PR (the version-bump merge starts the release; the owner taps the release environment):"
echo "  git push -u origin $(git rev-parse --abbrev-ref HEAD)"
echo "  gh pr create --base main --title 'chore: release $ver' --body 'Lockstep release $ver. Merge starts release.yml (docs/releasing.md).'"
