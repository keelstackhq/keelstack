#!/usr/bin/env bash
# Which monorepo folders a change touches (ci.yml, job detect).
#
# Input (env): PR_BASE / QUEUE_BASE / PUSH_BEFORE (the first set one is the base), or CHANGED (a
# newline-separated file list, for tests). Output: `<folder>=true|false` lines, `rust=`, `all=`
# and `base=`, to $GITHUB_OUTPUT (stdout when unset).
#
# Rules: a path under <folder>/ affects that folder. The shared root files (Cargo.toml,
# Cargo.lock, rust-toolchain.toml, deny.toml, .cargo/**, .github/**) affect every folder. No base
# (first push, zero SHA) or a failed diff affects every folder. Then DEPENDS_ON adds dependents:
# a folder is affected when a folder it depends on is.
set -euo pipefail

FOLDERS=(bosun splice hull hoist hoard berth keel decisions)
RUST=(bosun splice hoist hoard berth keel)
# <folder>:<folder it depends on> (hull renders with splice; its dogfood step runs ./hoard).
DEPENDS_ON=("hull:splice" "hull:hoard")

out="${GITHUB_OUTPUT:-/dev/stdout}"
all=false
base=""

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
    Cargo.toml | Cargo.lock | rust-toolchain.toml | deny.toml | .cargo/* | .github/*) all=true ;;
    */*) hit["${f%%/*}"]=1 ;;
  esac
done <<<"$changed"

if [ "$all" = true ]; then
  for d in "${FOLDERS[@]}"; do hit[$d]=1; done
fi
for dep in "${DEPENDS_ON[@]}"; do
  if [ -n "${hit[${dep#*:}]:-}" ]; then hit[${dep%%:*}]=1; fi
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
