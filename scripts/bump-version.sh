#!/usr/bin/env bash
# Bump every lockstep version of the monorepo to X.Y.Z (one version for all tools, D-023).
#
#   bash scripts/bump-version.sh X.Y.Z
#
# Sets, and prints the path of each file it changed (nothing is printed on a no-op run):
#   - Cargo.toml                [workspace.package] version
#   - */Cargo.toml, Cargo.toml  `version = "..."` of path dependencies on workspace crates
#                               (an operator prefix such as `=` is kept)
#   - Cargo.lock                the version of every workspace crate
#   - */npm/*/package.json      "version" and the sibling platform pins in optionalDependencies,
#                               for every tool folder that is a workspace member. Not
#                               cargo-deny* (upstream's own version) and nothing under tests/,
#                               hull/ or node_modules/
#   - */pnpm-lock.yaml          the `specifier:` of those sibling platform pins
#   - splice/action/action.yml  the `version` input default (the Action installs this splice)
#   - hull/template*/pieces/base/files/.github/workflows/system-check.yml
#                               `HOIST_VERSION: "..."` (the hoist a generated repo's weekly
#                               system check installs)
#   - CHANGELOG.md of a folder  a `## X.Y.Z` heading in the file's own style: `## Unreleased`
#                               stays on top, empty, and its entries fall under the new heading;
#                               a `## vA.B.C - unreleased` top heading (hull) is renamed;
#                               otherwise the heading goes above the newest release.
#                               Fragments (bosun/changelog.d) are not folded: run
#                               `bash bosun/scripts/fold-fragments.sh` first.
# Idempotent: a second run with the same version changes nothing. Offline; writes nothing else.
# BUMP_DATE=YYYY-MM-DD overrides today's date (UTC) in new CHANGELOG headings.
set -euo pipefail

die() {
  echo "bump-version: $*" >&2
  exit 1
}

[ "$#" -eq 1 ] || die "usage: bash scripts/bump-version.sh X.Y.Z"
ver=$1
semver='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
[[ $ver =~ $semver ]] || die "'$ver' is not a version: expected exactly X.Y.Z (no v prefix, no suffix)"

date=${BUMP_DATE:-$(date -u +%F)}
[[ $date =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || die "BUMP_DATE '$date' is not YYYY-MM-DD"

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
[ -f Cargo.toml ] || die "no Cargo.toml in $root"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# apply <file> <awk|sed ...>: run the filter on <file> into a temp file; replace the file and
# print its path only when the content changed. A filter that fails fails the script.
apply() {
  local file=$1 out="$tmp/out"
  shift
  [ -f "$file" ] || die "missing file: $file"
  "$@" <"$file" >"$out" || die "failed to edit $file"
  if ! cmp -s "$file" "$out"; then
    cat "$out" >"$file"
    echo "$file"
  fi
}

# --- workspace members and their crate names ---------------------------------------------------
members=()
while IFS= read -r m; do members+=("$m"); done < <(
  awk '/^members[ \t]*=/ { f = 1 } f { print } f && /\]/ { exit }' Cargo.toml | grep -o '"[^"]*"' | tr -d '"'
)
[ "${#members[@]}" -gt 0 ] || die "no [workspace] members in Cargo.toml"
# Member names reach sed expressions below: refuse anything but plain path characters.
for m in "${members[@]}"; do
  [[ $m =~ ^[A-Za-z0-9_./-]+$ ]] || die "invalid workspace member name '$m' (allowed: A-Z a-z 0-9 _ . / -)"
done

crates=()
for m in "${members[@]}"; do
  [ -f "$m/Cargo.toml" ] || die "member $m has no Cargo.toml"
  name=$(awk '/^\[/ { s = ($0 == "[package]") } s && /^name[ \t]*=/ { gsub(/^[^"]*"|".*$/, ""); print; exit }' "$m/Cargo.toml")
  [ -n "$name" ] || die "$m/Cargo.toml has no [package] name"
  crates+=("$name")
done

# --- 1. [workspace.package] version ------------------------------------------------------------
set_workspace_version() {
  awk -v v="$ver" '
    /^\[/ { s = ($0 == "[workspace.package]") }
    s && !done && /^version[ \t]*=/ { print "version = \"" v "\""; done = 1; next }
    { print }
    END { if (!done) exit 1 }
  '
}
apply Cargo.toml set_workspace_version

# --- 2. versions of path dependencies on workspace crates --------------------------------------
# Inline form `name = { path = "..", version = "=1.2.3" }`, and table form
# `[dependencies.name]` with `path = ".."` and `version = ".."` lines. Only tables that carry a
# `path` are touched, so registry dependencies keep their pins.
set_path_dep_versions() {
  awk -v v="$ver" '
    function bump(line,    head, tail) {
      if (match(line, /version[ \t]*=[ \t]*"[=^~<>]*/)) {
        head = substr(line, 1, RSTART + RLENGTH - 1)
        tail = substr(line, RSTART + RLENGTH)
        sub(/^[^"]*/, "", tail)
        return head v tail
      }
      return line
    }
    { L[NR] = $0 }
    END {
      sec = 0
      for (i = 1; i <= NR; i++) {
        if (L[i] ~ /^\[/) {
          sec = i
          istable[sec] = (L[i] ~ /dependencies\.[^.]+\][ \t]*$/)
        } else if (sec && istable[sec] && L[i] ~ /^path[ \t]*=/) {
          haspath[sec] = 1
        }
        owner[i] = sec
      }
      for (i = 1; i <= NR; i++) {
        line = L[i]
        s = owner[i]
        if (line ~ /^[A-Za-z0-9_-]+[ \t]*=[ \t]*\{.*path[ \t]*=[ \t]*"/ && line ~ /version[ \t]*=[ \t]*"/) {
          line = bump(line)
        } else if (s && istable[s] && haspath[s] && line ~ /^version[ \t]*=[ \t]*"/) {
          line = bump(line)
        }
        print line
      }
    }
  '
}
apply Cargo.toml set_path_dep_versions
for m in "${members[@]}"; do
  apply "$m/Cargo.toml" set_path_dep_versions
done

# --- 3. Cargo.lock: the workspace crates -------------------------------------------------------
if [ -f Cargo.lock ]; then
  names=$(IFS=,; echo "${crates[*]}")
  set_lock_versions() {
    awk -v v="$ver" -v names="$names" '
      BEGIN { n = split(names, a, ","); for (i = 1; i <= n; i++) ws[a[i]] = 1 }
      /^name = "/ { nm = $0; sub(/^name = "/, "", nm); sub(/"$/, "", nm); hit = (nm in ws); print; next }
      hit && /^version = "/ { print "version = \"" v "\""; hit = 0; next }
      /^\[\[package\]\]/ { hit = 0 }
      { print }
    '
  }
  apply Cargo.lock set_lock_versions
fi

# --- 4. npm launcher and platform packages -----------------------------------------------------
set_package_version() { # <tool>
  local tool=$1
  awk -v v="$ver" '
    !done && /^  "version"[ \t]*:/ { sub(/"version"[ \t]*:[ \t]*"[^"]*"/, "\"version\": \"" v "\""); done = 1 }
    { print }
    END { if (!done) exit 1 }
  ' | sed -E "s#(\"@keelstackhq/${tool}-[a-z0-9-]+\"[[:space:]]*:[[:space:]]*\")[^\"]*\"#\1${ver}\"#"
}
set_pnpm_specifiers() { # <tool>
  awk -v v="$ver" -v pre="'@keelstackhq/$1-" '
    index($0, pre) > 0 && /:[ \t]*$/ { pin = 1; print; next }
    pin && /^[ ]+specifier:/ { sub(/specifier:.*/, "specifier: " v); pin = 0; print; next }
    { pin = 0; print }
  '
}
for m in "${members[@]}"; do
  [ -d "$m/npm" ] || continue
  while IFS= read -r pkg; do
    apply "$pkg" set_package_version "$m"
  done < <(find "$m/npm" -mindepth 2 -maxdepth 2 -name package.json \
    -not -path '*/node_modules/*' -not -path '*/cargo-deny*' | LC_ALL=C sort)
  [ ! -f "$m/pnpm-lock.yaml" ] || apply "$m/pnpm-lock.yaml" set_pnpm_specifiers "$m"
done

# --- 5. the splice Action's pinned splice ------------------------------------------------------
set_action_pin() {
  awk -v v="$ver" '
    /^  version:[ \t]*$/ { inv = 1; print; next }
    inv && /^    default:/ { print "    default: " v; inv = 0; next }
    /^  [A-Za-z]/ { inv = 0 }
    { print }
  '
}
[ ! -f splice/action/action.yml ] || apply splice/action/action.yml set_action_pin

# --- 5b. the hoist the hull templates' weekly system-check.yml installs ----------------------
set_hoist_pin() {
  sed -E "s/^([[:space:]]*HOIST_VERSION:[[:space:]]*)\"[0-9]+\.[0-9]+\.[0-9]+\"[[:space:]]*\$/\1\"$ver\"/"
}
for t in template template-rust; do
  f=hull/$t/pieces/base/files/.github/workflows/system-check.yml
  [ ! -f "$f" ] || apply "$f" set_hoist_pin
done

# --- 6. CHANGELOG.md of each folder ------------------------------------------------------------
set_changelog() {
  awk -v ver="$ver" -v date="$date" '
    { L[NR] = $0 }
    END {
      # Style of the first versioned heading: optional v, optional [..], "(date)" or " - " / em dash.
      pfx = ""; br = 0; pa = 0; sep = " - "
      for (i = 1; i <= NR; i++) {
        if (L[i] ~ /^## / && L[i] ~ /[0-9]+\.[0-9]+\.[0-9]+/) {
          h = substr(L[i], 4)
          if (h ~ /^v/) pfx = "v"
          br = (h ~ /^v?\[/)
          pa = (h ~ /[0-9] \(/)
          if (index(h, " \342\200\224 ")) sep = " \342\200\224 "
          break
        }
      }
      core = br ? "[" pfx ver "]" : pfx ver
      heading = "## " core (pa ? " (" date ")" : sep date)

      found = 0; unrel = 0; vunrel = 0; firsth = 0
      for (i = 1; i <= NR; i++) {
        if (L[i] !~ /^## /) continue
        if (!firsth) firsth = i
        h = substr(L[i], 4); gsub(/^\[/, "", h); gsub(/^v/, "", h)
        if (index(h, ver) == 1 && (length(h) == length(ver) || substr(h, length(ver) + 1, 1) !~ /[0-9A-Za-z.+-]/)) {
          if (!found) found = i
        } else if (tolower(L[i]) ~ /^## \[?unreleased\]?[ \t]*$/) {
          if (!unrel) unrel = i
        } else if (tolower(L[i]) ~ /unreleased/ && L[i] ~ /[0-9]+\.[0-9]+\.[0-9]+/) {
          if (!vunrel) vunrel = i
        }
      }
      if (found) {
        if (tolower(L[found]) ~ /unreleased/) L[found] = heading
        for (i = 1; i <= NR; i++) print L[i]
      } else if (unrel) {
        for (i = 1; i <= NR; i++) { print L[i]; if (i == unrel) { print ""; print heading } }
      } else if (vunrel) {
        L[vunrel] = heading
        for (i = 1; i <= NR; i++) print L[i]
      } else if (firsth) {
        for (i = 1; i <= NR; i++) { if (i == firsth) { print heading; print "" } print L[i] }
      } else {
        for (i = 1; i <= NR; i++) print L[i]
        if (NR > 0) print ""
        print heading
      }
    }
  '
}
for m in "${members[@]}" hull; do
  [ ! -f "$m/CHANGELOG.md" ] || apply "$m/CHANGELOG.md" set_changelog
done

# --- 6b. splice docs/PROGRESS.md Status names the lockstep version (splice/tests/docs.rs) ---------
set_progress_status() {
  sed -E "s/^splice [0-9]+\.[0-9]+\.[0-9]+([^ ]*)? is the workspace lockstep version/splice $ver is the workspace lockstep version/"
}
[ ! -f splice/docs/PROGRESS.md ] || apply splice/docs/PROGRESS.md set_progress_status

# --- 6c. hull docs/adopt.md pins the hosted assess script to the release (hull/tests/adopt-doc) --
# Its check-before-run line carries the sha256 of the ready.mjs this release ships.
assess_src=hull/template/pieces/base/files/scripts/ready.mjs
assess_sha=
if [ -f "$assess_src" ]; then
  assess_sha=$( (sha256sum "$assess_src" 2>/dev/null || shasum -a 256 "$assess_src") | cut -d' ' -f1)
fi
set_adopt_pin() {
  local sha_expr=()
  [ -z "$assess_sha" ] || sha_expr=(-e "s#^echo \"[0-9a-f]{64}  /tmp/assess\.mjs\"#echo \"$assess_sha  /tmp/assess.mjs\"#")
  sed -E -e "s#keelstack\.dev/assess/v[0-9]+\.[0-9]+\.[0-9]+\.mjs#keelstack.dev/assess/v$ver.mjs#g" \
    -e "s#hull v[0-9]+\.[0-9]+\.[0-9]+'s #hull v$ver's #g" "${sha_expr[@]}"
}
[ ! -f hull/docs/adopt.md ] || apply hull/docs/adopt.md set_adopt_pin

# --- 7. check: the lock agrees with the manifests ----------------------------------------------
if [ -f Cargo.lock ]; then
  for c in "${crates[@]}"; do
    got=$(awk -v n="$c" '$0 == "name = \"" n "\"" { getline; print; exit }' Cargo.lock)
    [ "$got" = "version = \"$ver\"" ] || die "Cargo.lock has no '$c' at $ver (got: ${got:-nothing}); is the crate missing from the lock?"
  done
  if command -v cargo >/dev/null 2>&1; then
    # --locked: fail instead of rewriting the lock. A registry that is not cached (offline) is
    # not a problem of the bump.
    if ! out=$(cargo metadata --locked --offline --format-version 1 2>&1 >/dev/null); then
      # `if grep`, not `case`: the agents' guard refuses a glob in a case pattern.
      if grep -qE 'needs to be updated|lock file' <<<"$out"; then
        die "Cargo.lock is out of date after the bump: $out"
      else
        echo "bump-version: note: cargo metadata could not run offline here ($(head -n1 <<<"$out"))" >&2
      fi
    fi
  fi
fi
