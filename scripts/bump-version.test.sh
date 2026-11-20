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
check() { # <description> <command...>
  local what=$1
  shift
  if "$@" >/dev/null 2>&1; then ok "$what"; else fail "$what"; fi
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
[ -s "$out1" ] && ok "first run prints the changed files" || fail "first run printed nothing"

cd "$copy"

# Cargo.toml and every member
v=$(awk -F'"' '/^\[/ { s = ($0 == "[workspace.package]") } s && /^version *=/ { print $2; exit }' Cargo.toml)
[ "$v" = "$new" ] && ok "workspace version is $new" || fail "workspace version is $v"

members=$(awk '/^members *=/ { f = 1 } f { print } f && /\]/ { exit }' Cargo.toml | grep -o '"[^"]*"' | tr -d '"')
crates=()
for m in $members; do
  grep -Eq '^version\.workspace *= *true' "$m/Cargo.toml" || fail "$m/Cargo.toml does not inherit the workspace version"
  crates+=("$(awk '/^\[/ { s = ($0 == "[package]") } s && /^name *=/ { gsub(/^[^"]*"|".*$/, ""); print; exit }' "$m/Cargo.toml")")
done
[ "${#crates[@]}" -ge 6 ] && ok "${#crates[@]} workspace crates found" || fail "fewer than 6 workspace crates: ${crates[*]}"

# Cargo.lock agrees for every workspace crate
for c in "${crates[@]}"; do
  got=$(awk -v n="$c" '$0 == "name = \"" n "\"" { getline; print; exit }' Cargo.lock)
  [ "$got" = "version = \"$new\"" ] && ok "Cargo.lock: $c $new" || fail "Cargo.lock: $c has '$got'"
done
# ... and nothing else in the lock moved: same package count, same non-workspace lines
diff <(cd "$root" && sed -E 's/^version = "'"$old"'"$/version = "X"/' Cargo.lock) \
  <(sed -E 's/^version = "'"$new"'"$/version = "X"/' Cargo.lock) >/dev/null &&
  ok "Cargo.lock differs only in the workspace versions" || fail "Cargo.lock changed beyond the workspace versions"

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
npm_files=$(find . -path '*/npm/*/package.json' -mindepth 4 -maxdepth 4 -not -path '*/node_modules/*' -not -path '*/tests/*' -not -path '*/cargo-deny*' | LC_ALL=C sort)
n=0
for f in $npm_files; do
  n=$((n + 1))
  pv=$(node -p "require('./$f').version" 2>/dev/null || sed -n 's/^  "version": "\(.*\)",\?$/\1/p' "$f")
  [ "$pv" = "$new" ] || fail "$f version is $pv"
  # optionalDependencies pins must all be the new version
  pins=$(sed -n '/"optionalDependencies"/,/}/p' "$f" | { grep -o '"@keelstackhq/[^"]*": *"[^"]*"' || true; } | sed 's/.*: *"\(.*\)"/\1/' | sort -u)
  if [ -n "$pins" ] && [ "$pins" != "$new" ]; then fail "$f pins: $pins"; fi
done
[ "$n" -ge 25 ] && ok "$n npm package.json files at $new" || fail "only $n npm package.json files found"
# launcher pins list every platform package
for l in bosun hoist berth keel; do
  p=$(grep -c "\"@keelstackhq/$l-.*\": \"$new\"" "$l/npm/$l/package.json" || true)
  [ "$p" = 5 ] && ok "$l launcher pins 5 platform packages at $new" || fail "$l launcher pins $p platform packages"
done
p=$(grep -c "\"@keelstackhq/splice-.*\": \"$new\"" splice/npm/splice/package.json || true)
[ "$p" = 5 ] && ok "splice launcher pins 5 platform packages at $new" || fail "splice launcher pins $p platform packages"

# not touched: upstream cargo-deny packages, fixtures, hull template, hoard action
cd "$root"
for f in bosun/npm/cargo-deny/package.json bosun/npm/cargo-deny-linux-x64/package.json bosun/npm/cargo-deny-linux-arm64/package.json hoard/action/package.json hull/package.json; do
  [ ! -f "$f" ] || cmp -s "$f" "$copy/$f" && ok "untouched: $f" || fail "$f was changed"
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
  [ "$bad" = 0 ] && ok "bosun/pnpm-lock.yaml specifiers at $new" || fail "bosun/pnpm-lock.yaml has $bad stale specifiers"
fi
grep -q "^    default: $new\$" splice/action/action.yml && ok "splice action pin is $new" || fail "splice action pin is not $new"

# No old version left in the lockstep fields: Cargo.toml, Cargo.lock workspace crates, npm files
if grep -n "\"$old\"" Cargo.toml >/dev/null 2>&1; then fail "$old remains in Cargo.toml"; else ok "no $old in Cargo.toml"; fi
if grep -l "\"$old\"" $npm_files >/dev/null 2>&1; then fail "$old remains in: $(grep -l "\"$old\"" $npm_files | tr '\n' ' ')"; else ok "no $old in npm package.json files"; fi

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
  [ -n "$u" ] && [ -n "$h" ] && [ "$u" -lt "$h" ] && ok "$f: Unreleased above $new" || fail "$f: Unreleased/$new order ($u/$h)"
done
if grep -qi '^## .*unreleased' hull/CHANGELOG.md; then fail "hull/CHANGELOG.md still has an unreleased heading"; else ok "hull: unreleased heading renamed"; fi
# the entries that were under Unreleased follow the new heading (hoard has one)
awk -v h="## $new — 2031-02-03" '$0 == h { f = 1; next } /^## / { f = 0 } f && /^- Docs: version pins/ { found = 1 } END { exit !found }' hoard/CHANGELOG.md &&
  ok "hoard: Unreleased entries moved under $new" || fail "hoard: Unreleased entry not under $new"
for f in berth keel splice; do
  [ ! -e "$f/CHANGELOG.md" ] && ok "$f has no CHANGELOG.md (none created)" || fail "$f/CHANGELOG.md exists"
done

# --- nothing else was written ------------------------------------------------------------------
cd "$tmp"
snapshot "$copy" >"$tmp/after.sum"
changed=$(diff <(awk '{print $2}' "$tmp/before.sum") <(awk '{print $2}' "$tmp/after.sum") || true)
[ -z "$changed" ] && ok "no file added or removed" || fail "files added or removed: $changed"
actual=$({ diff "$tmp/before.sum" "$tmp/after.sum" || true; } | sed -n 's/^> [0-9a-f]*  \.\///p' | LC_ALL=C sort)
reported=$(LC_ALL=C sort "$out1")
[ "$actual" = "$reported" ] && ok "printed list equals the files that changed" || {
  fail "printed list differs from the changed files"
  diff <(echo "$actual") <(echo "$reported") >&2 || true
}

# --- second run: a no-op -------------------------------------------------------------------------
out2="$tmp/run2.txt"
bash "$copy/scripts/bump-version.sh" "$new" >"$out2"
[ ! -s "$out2" ] && ok "second run prints nothing" || fail "second run changed: $(cat "$out2")"
snapshot "$copy" >"$tmp/after2.sum"
check "second run changes no file" cmp -s "$tmp/after.sum" "$tmp/after2.sum"

# --- a third version moves everything again (headings stack up, newest first) ---------------------
bash "$copy/scripts/bump-version.sh" 9.8.8 >/dev/null
grep -q '^## 9.8.8 (2031-02-03)$' "$copy/bosun/CHANGELOG.md" && grep -q "^## $new (2031-02-03)\$" "$copy/bosun/CHANGELOG.md" &&
  ok "a later bump adds its own heading and keeps $new" || fail "later bump headings"
[ "$(awk -F'"' '/^\[/ { s = ($0 == "[workspace.package]") } s && /^version *=/ { print $2; exit }' "$copy/Cargo.toml")" = 9.8.8 ] &&
  ok "a later bump moves the workspace version" || fail "later bump workspace version"

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
grep -q 'syn-b = { path = "../b", version = "=2.0.0" }' "$syn/a/Cargo.toml" && ok "inline path dependency version (operator kept)" || fail "inline path dependency not bumped"
grep -q 'serde = { version = "=1.0.229" }' "$syn/a/Cargo.toml" && ok "registry dependency untouched" || fail "registry dependency was changed"
grep -q '^version = "^2.0.0"$' "$syn/a/Cargo.toml" && ok "table-form path dependency version" || fail "table-form path dependency not bumped"
grep -q '^version = "=1.13.1"$' "$syn/a/Cargo.toml" && ok "table-form registry dependency untouched" || fail "table-form registry dependency was changed"
[ "$(grep -c '^version = "2.0.0"$' "$syn/Cargo.lock")" = 2 ] && grep -q '^version = "1.0.229"$' "$syn/Cargo.lock" && ok "synthetic Cargo.lock" || fail "synthetic Cargo.lock"
[ -z "$(bash "$syn/scripts/bump-version.sh" 2.0.0)" ] && ok "synthetic second run is a no-op" || fail "synthetic second run changed files"

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
