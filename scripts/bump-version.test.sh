#!/usr/bin/env bash
# Test for scripts/bump-version.sh. Copies the working tree (tracked and untracked, not
# ignored, so uncommitted changes count) into a temp dir, bumps it to 9.8.7 there and checks the
# result. The repo itself is never modified.
#
#   bash scripts/bump-version.test.sh
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export BUMP_DATE=2031-02-03

fails=0
ok() { echo "ok   $*"; }
fail() {
  echo "FAIL $*" >&2
  fails=$((fails + 1))
}
check() { # <description> <command...>: on failure, the command's output goes to stderr
  local what=$1 log="$tmp/check.log"
  shift
  if "$@" >"$log" 2>&1; then
    ok "$what"
  else
    fail "$what"
    sed 's/^/     /' "$log" >&2
  fi
}

# --- copy the working tree ---------------------------------------------------------------------
copy="$tmp/repo"
mkdir -p "$copy"
(
  cd "$root"
  git ls-files -z -co --exclude-standard | while IFS= read -r -d '' f; do
    if [ -e "$f" ] || [ -L "$f" ]; then printf '%s\0' "$f"; fi
  done | tar --null -T - -c
) | tar -x -C "$copy"

old=$(awk -F'"' '/^\[/ { s = ($0 == "[workspace.package]") } s && /^version *=/ { print $2; exit }' "$copy/Cargo.toml")
new=9.8.7
[ "$old" != "$new" ] || { echo "test needs a repo not at $new" >&2; exit 1; }
# Seed an Unreleased entry in hoard's copy: right after a release the real one is empty.
awk '{ print } $0 == "## Unreleased" && !d { print ""; print "- Test: seeded unreleased entry."; d = 1 }' \
  "$copy/hoard/CHANGELOG.md" >"$tmp/hoard-cl" && cat "$tmp/hoard-cl" >"$copy/hoard/CHANGELOG.md"
snapshot() { (cd "$1" && find . -type f -not -path './target/*' -not -path '*/node_modules/*' -print0 | LC_ALL=C sort -z | xargs -0 sha256sum); }
snapshot "$copy" >"$tmp/before.sum"

# --- bad versions fail and change nothing ------------------------------------------------------
for bad in 1.2 v1.2.3 1.2.3-rc.1 1.2.3.4 01.2.3 "" abc; do
  if bash "$copy/scripts/bump-version.sh" "$bad" >/dev/null 2>&1; then fail "bad version '$bad' is accepted"; else ok "bad version '$bad' fails"; fi
done
if bash "$copy/scripts/bump-version.sh" >/dev/null 2>&1; then fail "no argument is accepted"; else ok "no argument fails"; fi
snapshot "$copy" >"$tmp/after-bad.sum"
check "bad versions change nothing" cmp -s "$tmp/before.sum" "$tmp/after-bad.sum"

# --- bump ----------------------------------------------------------------------------------------
out1="$tmp/run1.txt"
bash "$copy/scripts/bump-version.sh" "$new" >"$out1"
echo "--- files changed by the first run:"
cat "$out1"
echo "---"
if [ -s "$out1" ]; then ok "first run prints the changed files"; else fail "first run printed nothing"; fi

cd "$copy"

# Cargo.toml and every member
v=$(awk -F'"' '/^\[/ { s = ($0 == "[workspace.package]") } s && /^version *=/ { print $2; exit }' Cargo.toml)
if [ "$v" = "$new" ]; then ok "workspace version is $new"; else fail "workspace version is $v"; fi

members=$(awk '/^members *=/ { f = 1 } f { print } f && /\]/ { exit }' Cargo.toml | grep -o '"[^"]*"' | tr -d '"')
crates=()
for m in $members; do
  grep -Eq '^version\.workspace *= *true' "$m/Cargo.toml" || fail "$m/Cargo.toml does not inherit the workspace version"
  crates+=("$(awk '/^\[/ { s = ($0 == "[package]") } s && /^name *=/ { gsub(/^[^"]*"|".*$/, ""); print; exit }' "$m/Cargo.toml")")
done
if [ "${#crates[@]}" -ge 6 ]; then ok "${#crates[@]} workspace crates found"; else fail "fewer than 6 workspace crates: ${crates[*]}"; fi

# Cargo.lock agrees for every workspace crate
for c in "${crates[@]}"; do
  got=$(awk -v n="$c" '$0 == "name = \"" n "\"" { getline; print; exit }' Cargo.lock)
  if [ "$got" = "version = \"$new\"" ]; then ok "Cargo.lock: $c $new"; else fail "Cargo.lock: $c has '$got'"; fi
done
# ... and nothing else in the lock moved: same package count, same non-workspace lines
if diff <(cd "$root" && sed -E 's/^version = "'"$old"'"$/version = "X"/' Cargo.lock) \
  <(sed -E 's/^version = "'"$new"'"$/version = "X"/' Cargo.lock) >/dev/null; then
  ok "Cargo.lock differs only in the workspace versions"
else
  fail "Cargo.lock changed beyond the workspace versions"
fi

if command -v cargo >/dev/null 2>&1; then
  if err=$(cargo metadata --locked --offline --format-version 1 2>&1 >/dev/null); then
    ok "cargo metadata --locked --offline"
  elif grep -q "failed to download\|offline" <<<"$err"; then
    echo "skip cargo metadata --locked --offline: registry not cached here"
    check "cargo metadata --locked --offline --no-deps" cargo metadata --locked --offline --no-deps --format-version 1
  else
    fail "cargo metadata --locked --offline: $err"
  fi
fi

# npm: every lockstep package.json is at the new version, none at the old one
mapfile -t npm_files < <(find . -path '*/npm/*/package.json' -mindepth 4 -maxdepth 4 -not -path '*/node_modules/*' -not -path '*/tests/*' -not -path '*/cargo-deny*' | LC_ALL=C sort)
n=0
for f in "${npm_files[@]}"; do
  n=$((n + 1))
  pv=$(node -p "require('./$f').version" 2>/dev/null || sed -n 's/^  "version": "\(.*\)",\?$/\1/p' "$f")
  [ "$pv" = "$new" ] || fail "$f version is $pv"
  # optionalDependencies pins must all be the new version
  pins=$(sed -n '/"optionalDependencies"/,/}/p' "$f" | { grep -o '"@keelstackhq/[^"]*": *"[^"]*"' || true; } | sed 's/.*: *"\(.*\)"/\1/' | sort -u)
  if [ -n "$pins" ] && [ "$pins" != "$new" ]; then fail "$f pins: $pins"; fi
done
if [ "$n" -ge 25 ]; then ok "$n npm package.json files at $new"; else fail "only $n npm package.json files found"; fi
# launcher pins list every platform package
for l in bosun hoist berth keel; do
  p=$(grep -c "\"@keelstackhq/$l-.*\": \"$new\"" "$l/npm/$l/package.json" || true)
  if [ "$p" = 5 ]; then ok "$l launcher pins 5 platform packages at $new"; else fail "$l launcher pins $p platform packages"; fi
done
p=$(grep -c "\"@keelstackhq/splice-.*\": \"$new\"" splice/npm/splice/package.json || true)
if [ "$p" = 5 ]; then ok "splice launcher pins 5 platform packages at $new"; else fail "splice launcher pins $p platform packages"; fi

# not touched: upstream cargo-deny packages, fixtures, hull template, hoard action
cd "$root"
for f in bosun/npm/cargo-deny/package.json bosun/npm/cargo-deny-linux-x64/package.json bosun/npm/cargo-deny-linux-arm64/package.json hoard/action/package.json hull/package.json; do
  if [ ! -f "$f" ] || cmp -s "$f" "$copy/$f"; then ok "untouched: $f"; else fail "$f was changed"; fi
done
while IFS= read -r f; do
  [ -f "$copy/$f" ] || continue # ignored files (node_modules) are not copied
  cmp -s "$f" "$copy/$f" || fail "$f (fixture, template or node_modules) was changed"
done < <(find . \( -path '*/tests/*' -o -path '*/template*/*' -o -path '*/node_modules/*' \) -name package.json -not -path './target/*')
ok "fixtures and templates untouched"
cd "$copy"

# pnpm lock specifiers, the splice Action's pin
if [ -f bosun/pnpm-lock.yaml ]; then
  bad=$(grep -A1 "'@keelstackhq/bosun-" bosun/pnpm-lock.yaml | grep specifier | grep -vc "specifier: $new" || true)
  if [ "$bad" = 0 ]; then ok "bosun/pnpm-lock.yaml specifiers at $new"; else fail "bosun/pnpm-lock.yaml has $bad stale specifiers"; fi
fi
if grep -q "^    default: $new\$" splice/action/action.yml; then ok "splice action pin is $new"; else fail "splice action pin is not $new"; fi

# No old version left in the lockstep fields: Cargo.toml, Cargo.lock workspace crates, npm files
if grep -n "\"$old\"" Cargo.toml >/dev/null 2>&1; then fail "$old remains in Cargo.toml"; else ok "no $old in Cargo.toml"; fi
if grep -l "\"$old\"" "${npm_files[@]}" >/dev/null 2>&1; then fail "$old remains in: $(grep -l "\"$old\"" "${npm_files[@]}" | tr '\n' ' ')"; else ok "no $old in npm package.json files"; fi

# splice docs/PROGRESS.md Status names the new version (splice/tests/docs.rs pins it)
if grep -q "^splice $new is the workspace lockstep version" splice/docs/PROGRESS.md; then
  ok "splice PROGRESS status names $new"
else
  fail "splice/docs/PROGRESS.md status does not name $new"
fi

if grep -q "keelstack.dev/assess/v$new.mjs" hull/docs/adopt.md && grep -q "hull v$new's" hull/docs/adopt.md; then
  ok "hull adopt.md pins v$new"
else
  fail "hull/docs/adopt.md does not pin v$new"
fi

# CHANGELOG headings, each in the file's own style
chk_heading() { # <file> <exact heading line>
  if grep -qxF -- "$2" "$1"; then ok "$1: $2"; else fail "$1 lacks heading '$2'"; fi
}
chk_heading bosun/CHANGELOG.md "## $new (2031-02-03)"
chk_heading hoist/CHANGELOG.md "## $new (2031-02-03)"
chk_heading hoard/CHANGELOG.md "## $new — 2031-02-03"
chk_heading hull/CHANGELOG.md "## v$new — 2031-02-03"
# Unreleased stays on top for bosun and hoard, above the new heading; hull's unreleased is renamed
for f in bosun hoard; do
  u=$(grep -n '^## Unreleased$' "$f/CHANGELOG.md" | head -1 | cut -d: -f1)
  h=$(grep -n "^## $new" "$f/CHANGELOG.md" | head -1 | cut -d: -f1)
  if [ -n "$u" ] && [ -n "$h" ] && [ "$u" -lt "$h" ]; then ok "$f: Unreleased above $new"; else fail "$f: Unreleased/$new order ($u/$h)"; fi
done
if grep -qi '^## .*unreleased' hull/CHANGELOG.md; then fail "hull/CHANGELOG.md still has an unreleased heading"; else ok "hull: unreleased heading renamed"; fi
# the entries that were under Unreleased follow the new heading (hoard has the seeded one)
if awk -v h="## $new — 2031-02-03" '$0 == h { f = 1; next } /^## / { f = 0 } f && /^- Test: seeded unreleased entry\./ { found = 1 } END { exit !found }' hoard/CHANGELOG.md; then
  ok "hoard: Unreleased entries moved under $new"
else
  fail "hoard: Unreleased entry not under $new"
fi
for f in berth keel splice; do
  if [ ! -e "$f/CHANGELOG.md" ]; then ok "$f has no CHANGELOG.md (none created)"; else fail "$f/CHANGELOG.md exists"; fi
done

# --- nothing else was written ------------------------------------------------------------------
cd "$tmp"
snapshot "$copy" >"$tmp/after.sum"
changed=$(diff <(awk '{print $2}' "$tmp/before.sum") <(awk '{print $2}' "$tmp/after.sum") || true)
if [ -z "$changed" ]; then ok "no file added or removed"; else fail "files added or removed: $changed"; fi
actual=$({ diff "$tmp/before.sum" "$tmp/after.sum" || true; } | sed -n 's/^> [0-9a-f]*  \.\///p' | LC_ALL=C sort)
reported=$(LC_ALL=C sort "$out1")
if [ "$actual" = "$reported" ]; then
  ok "printed list equals the files that changed"
else
  fail "printed list differs from the changed files"
  diff <(echo "$actual") <(echo "$reported") >&2 || true
fi

# --- second run: a no-op -------------------------------------------------------------------------
out2="$tmp/run2.txt"
bash "$copy/scripts/bump-version.sh" "$new" >"$out2"
if [ ! -s "$out2" ]; then ok "second run prints nothing"; else fail "second run changed: $(cat "$out2")"; fi
snapshot "$copy" >"$tmp/after2.sum"
check "second run changes no file" cmp -s "$tmp/after.sum" "$tmp/after2.sum"

# --- a third version moves everything again (headings stack up, newest first) ---------------------
bash "$copy/scripts/bump-version.sh" 9.8.8 >/dev/null
if grep -q '^## 9.8.8 (2031-02-03)$' "$copy/bosun/CHANGELOG.md" && grep -q "^## $new (2031-02-03)\$" "$copy/bosun/CHANGELOG.md"; then
  ok "a later bump adds its own heading and keeps $new"
else
  fail "later bump headings"
fi
if [ "$(awk -F'"' '/^\[/ { s = ($0 == "[workspace.package]") } s && /^version *=/ { print $2; exit }' "$copy/Cargo.toml")" = 9.8.8 ]; then
  ok "a later bump moves the workspace version"
else
  fail "later bump workspace version"
fi

# --- path dependencies on workspace crates (none exist yet): synthetic workspace -----------------------
syn="$tmp/syn"
mkdir -p "$syn/scripts" "$syn/a" "$syn/b"
cp "$root/scripts/bump-version.sh" "$syn/scripts/"
cat >"$syn/Cargo.toml" <<'EOF'
[workspace]
members = ["a", "b"]

[workspace.package]
version = "1.0.0"
EOF
cat >"$syn/a/Cargo.toml" <<'EOF'
[package]
name = "syn-a"
version.workspace = true

[dependencies]
syn-b = { path = "../b", version = "=1.0.0" }
serde = { version = "=1.0.229" }

[dev-dependencies.syn-b]
path = "../b"
version = "^1.0.0"

[dependencies.regex]
version = "=1.13.1"
EOF
cat >"$syn/b/Cargo.toml" <<'EOF'
[package]
name = "syn-b"
version.workspace = true
EOF
cat >"$syn/Cargo.lock" <<'EOF'
version = 4

[[package]]
name = "serde"
version = "1.0.229"

[[package]]
name = "syn-a"
version = "1.0.0"
dependencies = [
 "syn-b",
]

[[package]]
name = "syn-b"
version = "1.0.0"
EOF
bash "$syn/scripts/bump-version.sh" 2.0.0 >/dev/null
if grep -q 'syn-b = { path = "../b", version = "=2.0.0" }' "$syn/a/Cargo.toml"; then ok "inline path dependency version (operator kept)"; else fail "inline path dependency not bumped"; fi
if grep -q 'serde = { version = "=1.0.229" }' "$syn/a/Cargo.toml"; then ok "registry dependency untouched"; else fail "registry dependency was changed"; fi
if grep -q '^version = "^2.0.0"$' "$syn/a/Cargo.toml"; then ok "table-form path dependency version"; else fail "table-form path dependency not bumped"; fi
if grep -q '^version = "=1.13.1"$' "$syn/a/Cargo.toml"; then ok "table-form registry dependency untouched"; else fail "table-form registry dependency was changed"; fi
if [ "$(grep -c '^version = "2.0.0"$' "$syn/Cargo.lock")" = 2 ] && grep -q '^version = "1.0.229"$' "$syn/Cargo.lock"; then ok "synthetic Cargo.lock"; else fail "synthetic Cargo.lock"; fi
if [ -z "$(bash "$syn/scripts/bump-version.sh" 2.0.0)" ]; then ok "synthetic second run is a no-op"; else fail "synthetic second run changed files"; fi

# A member name with sed metacharacters is rejected before any file is touched.
bad="$tmp/bad"
mkdir -p "$bad/scripts" "$bad/a"
cp "$root/scripts/bump-version.sh" "$bad/scripts/"
printf '[workspace]\nmembers = ["a#b"]\n\n[workspace.package]\nversion = "1.0.0"\n' >"$bad/Cargo.toml"
if out=$(bash "$bad/scripts/bump-version.sh" 2.0.0 2>&1); then
  fail "member name with '#' was accepted"
elif grep -q 'invalid workspace member' <<<"$out"; then
  ok "member name with '#' is rejected"
else
  fail "member name rejected without the expected message: $out"
fi

# --- shell hygiene -------------------------------------------------------------------------------
for s in scripts/bump-version.sh scripts/bump-version.test.sh; do
  check "bash -n $s" bash -n "$root/$s"
  if command -v shellcheck >/dev/null 2>&1; then
    check "shellcheck $s" shellcheck -x "$root/$s"
  else
    echo "skip shellcheck $s: not installed"
  fi
done

if [ "$fails" -gt 0 ]; then
  echo "$fails check(s) failed" >&2
  exit 1
fi
echo "all checks passed"
