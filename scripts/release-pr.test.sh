#!/usr/bin/env bash
# Test for scripts/release-pr.sh: runs it on a throwaway git copy of the working tree.
#   bash scripts/release-pr.test.sh
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export BUMP_DATE=2031-02-03 GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
fails=0
ok() { echo "ok   $*"; }
fail() { echo "FAIL $*" >&2; fails=$((fails + 1)); }

copy="$tmp/repo"
mkdir -p "$copy"
(
  cd "$root"
  git ls-files -z -co --exclude-standard | while IFS= read -r -d '' f; do
    if [ -e "$f" ] || [ -L "$f" ]; then printf '%s\0' "$f"; fi
  done | tar --null -T - -c
) | tar -x -C "$copy"
cd "$copy"
git init -q -b main
printf 'target/\n' >.gitignore
mkdir -p bosun/changelog.d bosun/docs/progress.d
echo "- Test: a fragment." >bosun/changelog.d/test-frag.md
echo "- 2031-01-01 · t · a task · PR 1" >bosun/docs/progress.d/test-frag.md
git add -A
git commit -q -m base
new=9.8.7

for bad in "" 1.2 v1.2.3 0.0.1; do
  if bash scripts/release-pr.sh "$bad" >/dev/null 2>&1; then fail "version '$bad' accepted"; else ok "version '$bad' refused"; fi
done
echo dirty >>README.md
if bash scripts/release-pr.sh "$new" >/dev/null 2>&1; then fail "dirty tree accepted"; else ok "dirty tree refused"; fi
git checkout -q -- README.md

# A detached HEAD is refused before anything changes.
git checkout -q --detach
if bash scripts/release-pr.sh "$new" >/dev/null 2>&1; then fail "detached HEAD accepted"; else ok "detached HEAD refused"; fi
git checkout -q main

# A failure after the branch was made leaves nothing behind.
echo "not a fragment" >"bosun/changelog.d/bad name.md"
git add -A; git commit -q -m "bad fragment"
if bash scripts/release-pr.sh "$new" >/dev/null 2>&1; then fail "bad fragment accepted"; else ok "a failing run is refused"; fi
[ "$(git rev-parse --abbrev-ref HEAD)" = main ] && ok "back on main after a failure" || fail "left on $(git rev-parse --abbrev-ref HEAD)"
[ -z "$(git branch --list "release/v$new")" ] && ok "no half-made release branch" || fail "release branch left behind"
[ -z "$(git status --porcelain)" ] && ok "tree clean after the failure" || fail "dirty after failure: $(git status --porcelain)"
git rm -q "bosun/changelog.d/bad name.md"; git commit -q -m "drop bad fragment"

if bash scripts/release-pr.sh "$new" >"$tmp/out" 2>"$tmp/err"; then ok "release-pr runs"; else fail "release-pr failed: $(cat "$tmp/err")"; fi
[ "$(git rev-parse --abbrev-ref HEAD)" = "release/v$new" ] && ok "on branch release/v$new" || fail "branch is $(git rev-parse --abbrev-ref HEAD)"
[ "$(git log -1 --format=%s)" = "chore: release $new" ] && ok "one commit" || fail "commit: $(git log -1 --format=%s)"
[ -z "$(git status --porcelain)" ] && ok "tree clean after commit" || fail "dirty: $(git status --porcelain)"
grep -q "^version = \"$new\"" Cargo.toml && ok "Cargo.toml bumped" || fail "Cargo.toml not bumped"
grep -A1 '^name = "keel-hoist"' Cargo.lock | grep -q "\"$new\"" && ok "Cargo.lock bumped" || fail "Cargo.lock not bumped"
grep -q "\"version\": \"$new\"" hoist/npm/hoist/package.json && ok "npm launcher bumped" || fail "npm launcher not bumped"
[ -z "$(ls bosun/changelog.d | grep -v README.md)" ] && ok "fragments folded away" || fail "fragments left: $(ls bosun/changelog.d)"
awk -v v="$new" '/^## /{ if ($2 == v) f = 1 } f && /Test: a fragment/ { ok = 1 } END { exit !ok }' bosun/CHANGELOG.md && ok "fragment is under $new in bosun/CHANGELOG.md" || fail "fragment not under $new"
grep -q "git push -u origin release/v$new" "$tmp/out" && ok "prints the PR command" || fail "no PR command"
if bash scripts/release-pr.sh "$new" >/dev/null 2>&1; then fail "second run accepted"; else ok "second run refused"; fi

[ "$fails" = 0 ] || { echo "$fails failure(s)" >&2; exit 1; }
echo "all ok"
