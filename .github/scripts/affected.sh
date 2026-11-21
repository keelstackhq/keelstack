#!/usr/bin/env bash
# Which monorepo folders a change touches (ci.yml, job detect).
#
# Input (env): PR_BASE / QUEUE_BASE / PUSH_BEFORE (the first set one is the base), or CHANGED (a
# newline-separated file list, for tests). Output: `<folder>=true|false` lines, `rust=`, `all=`
# and `base=`, to $GITHUB_OUTPUT (stdout when unset).
#
# Rules: a push to main, a merge queue run, a schedule or a manual run (GITHUB_EVENT_NAME push,
# merge_group, schedule, workflow_dispatch) affects every folder: main health reads every
# folder's latest real result on the main head, and a skipped job is no result (bosun spec §9;
# main pushes are few, D-007). Else, for a PR: a path under <folder>/ affects that folder. The
# shared root files (Cargo.toml, Cargo.lock, rust-toolchain.toml, deny.toml, .cargo/**,
# .github/**, .bosun/bosun.toml) affect every folder. No base (first push, zero SHA) or a failed
# diff affects every folder. Then the `depends_on` of `[[folders]]` in .bosun/bosun.toml (the one
# source, also read by bosun) adds dependents: a folder is affected when a folder it depends on
# is. A bosun.toml this parser cannot read, or whose folders differ from FOLDERS, fails the run.
set -euo pipefail

FOLDERS=(bosun splice hull hoist hoard berth keel decisions site)
RUST=(bosun splice hoist hoard berth keel)
toml="${BOSUN_TOML:-$(cd "$(dirname "$0")/../.." && pwd)/.bosun/bosun.toml}"

out="${GITHUB_OUTPUT:-/dev/stdout}"
all=false
base=""

die() {
  echo "::error::affected.sh: $1 ($toml). Fix .bosun/bosun.toml: each [[folders]] has one-line path = \"<dir>\" and optional one-line depends_on = [\"<dir>\", ...]." >&2
  exit 1
}

# DEPENDS_ON: "<folder>:<folder it depends on>" from the [[folders]] tables (one-line values).
[ -r "$toml" ] || die "cannot read the file"
if ! parsed="$(awk '
  function flush() { if (inf) { if (path == "") bad = 1; print "P " path; for (i = 1; i <= nd; i++) print "D " path ":" dep[i] } }
  /^[[:space:]]*(#|$)/ { next }
  /^[[:space:]]*\[/ { flush(); inf = ($0 ~ /^[[:space:]]*\[\[folders\]\][[:space:]]*(#.*)?$/); path = ""; nd = 0; next }
  !inf { next }
  /^[[:space:]]*path[[:space:]]*=/ {
    if (match($0, /"[^"]*"/) == 0) { bad = 1; next }
    path = substr($0, RSTART + 1, RLENGTH - 2); next
  }
  /^[[:space:]]*depends_on[[:space:]]*=/ {
    v = $0; sub(/^[^=]*=[[:space:]]*/, "", v); sub(/[[:space:]]*(#.*)?$/, "", v)
    if (v !~ /^\[[^]]*\]$/) { bad = 1; next }
    gsub(/[][[:space:]]/, "", v); n = split(v, parts, ",")
    for (i = 1; i <= n; i++) {
      if (parts[i] == "" && i == n) continue
      if (parts[i] !~ /^"[^"]+"$/) { bad = 1; continue }
      dep[++nd] = substr(parts[i], 2, length(parts[i]) - 2)
    }
    next
  }
  { bad = 1 }
  END { flush(); if (bad) exit 1 }
' "$toml")"; then
  die "cannot parse [[folders]]"
fi
listed="$(printf '%s\n' "$parsed" | sed -n 's/^P //p' | sort)"
[ "$listed" = "$(printf '%s\n' "${FOLDERS[@]}" | sort)" ] ||
  die "[[folders]] paths ($(echo "$listed" | tr '\n' ' ')) differ from FOLDERS (${FOLDERS[*]})"
DEPENDS_ON=()
while IFS= read -r d; do
  [ -n "$d" ] || continue
  case " ${FOLDERS[*]} " in *" ${d#*:} "*) ;; *) die "depends_on names unknown folder ${d#*:}" ;; esac
  DEPENDS_ON+=("$d")
done < <(printf '%s\n' "$parsed" | sed -n 's/^D //p')

case "${GITHUB_EVENT_NAME:-}" in
  push | merge_group | schedule | workflow_dispatch) all=true ;;
esac

if [ -n "${CHANGED+x}" ]; then
  changed="$CHANGED"
else
  base="${PR_BASE:-${QUEUE_BASE:-${PUSH_BEFORE:-}}}"
  case "$base" in "" | 0000000000000000000000000000000000000000) base="" ;; esac
  if [ -z "$base" ]; then
    all=true
    changed=""
  elif ! changed="$(git diff --name-only "$base...HEAD" 2>/dev/null)"; then
    echo "::warning::git diff against $base failed; treating every folder as affected" >&2
    all=true
    changed=""
  fi
fi

declare -A hit=()
while IFS= read -r f; do
  [ -n "$f" ] || continue
  case "$f" in
    Cargo.toml | Cargo.lock | rust-toolchain.toml | deny.toml | .cargo/* | .github/* | .bosun/bosun.toml) all=true ;;
    */*) hit["${f%%/*}"]=1 ;;
  esac
done <<<"$changed"

if [ "$all" = true ]; then
  for d in "${FOLDERS[@]}"; do hit[$d]=1; done
fi
# Transitive: repeat until nothing changes.
changed_dep=true
while [ "$changed_dep" = true ]; do
  changed_dep=false
  for dep in "${DEPENDS_ON[@]}"; do
    if [ -n "${hit[${dep#*:}]:-}" ] && [ -z "${hit[${dep%%:*}]:-}" ]; then
      hit[${dep%%:*}]=1
      changed_dep=true
    fi
  done
done

rust=false
for d in "${RUST[@]}"; do [ -n "${hit[$d]:-}" ] && rust=true; done
{
  for d in "${FOLDERS[@]}"; do
    if [ -n "${hit[$d]:-}" ]; then echo "$d=true"; else echo "$d=false"; fi
  done
  echo "rust=$rust"
  echo "all=$all"
  echo "base=$base"
} >>"$out"
if [ "$out" != /dev/stdout ]; then cat "$out" >&2; fi
