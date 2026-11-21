#!/usr/bin/env bash
# Text test for the gate workflows (risk, verdict, review, signer, main-open, main-health), run by
# ci.yml job ci-repo: bash .github/scripts/gate-workflows.test.sh
#
# 1. bosun is built from the checked-out base, and a build that fails fails the gate closed with
#    an ::error naming the fix. There is no pinned-release fallback yet: no monorepo release
#    exists, and the old keelstackhq/bosun releases (<= 0.21.0, .bosun/version) cannot read
#    [[folders]] (.github/SETUP.md, "Main does not build"). The step is run with a failing and
#    a passing fake cargo.
# 2. Every Swatinem/rust-cache step in a job that holds the App key sets cache-bin "false" and
#    saves only on main (as ci.yml does).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
wf="$here/../workflows"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() {
  echo "FAIL: $*"
  fail=1
}

# The `run: |` body of every step named "build bosun", one file per step, de-indented.
extract_builds() { # <workflow> <out prefix>
  awk -v pre="$2" '
    /^ *- name: build bosun *$/ { want = 1; next }
    want && /^ *run: \|/ { want = 0; inrun = 1; n++; ind = -1; next }
    want && /^ *run: / { want = 0; n++; f = pre n; sub(/^ *run: /, ""); print > f; close(f); next }
    inrun {
      if ($0 ~ /^ *$/) { print "" > (pre n); next }
      match($0, /^ */)
      if (ind < 0) ind = RLENGTH
      if (RLENGTH < ind) { inrun = 0; close(pre n); next }
      print substr($0, ind + 1) > (pre n)
    }
    END { print n + 0 }
  ' "$1"
}

mkdir -p "$tmp/bin-fail" "$tmp/bin-ok"
printf '#!/bin/sh\necho "error: could not compile" >&2\nexit 101\n' >"$tmp/bin-fail/cargo"
printf '#!/bin/sh\nexit 0\n' >"$tmp/bin-ok/cargo"
chmod +x "$tmp/bin-fail/cargo" "$tmp/bin-ok/cargo"

for name in risk verdict review signer main-open main-health; do
  f="$wf/$name.yml"
  [ -f "$f" ] || { bad "$name.yml is missing"; continue; }
  n="$(extract_builds "$f" "$tmp/$name.build.")"
  [ "$n" -ge 1 ] || bad "$name.yml has no 'build bosun' step"
  builds="$(grep -c 'cargo build --release --locked -p keel-bosun --bin bosun' "$f" || true)"
  [ "$builds" = "$n" ] || bad "$name.yml builds bosun $builds time(s) outside a 'build bosun' step ($n)"
  for i in $(seq 1 "$n"); do
    step="$tmp/$name.build.$i"
    if out="$(PATH="$tmp/bin-fail:$PATH" bash -e "$step" 2>&1)"; then
      bad "$name.yml build step $i passes when cargo build fails (it must fail closed)"
    elif ! grep -q '^::error::bosun does not build at the base commit: this gate cannot judge and fails closed' <<<"$out" ||
      ! grep -q 'bosun:kind/mainfix' <<<"$out" || ! grep -q 'SETUP.md' <<<"$out"; then
      bad "$name.yml build step $i fails without the ::error naming the mainfix fix: $out"
    fi
    PATH="$tmp/bin-ok:$PATH" bash -e "$step" >/dev/null 2>&1 || bad "$name.yml build step $i fails when cargo build passes"
  done
  # No silent fallback: the build step never continues on error, and nothing downloads a release.
  if awk '/^ *- name: build bosun *$/ { s = 1; next } s && /^ *- / { s = 0 } s && /continue-on-error/ { found = 1 } END { exit !found }' "$f"; then
    bad "$name.yml: the build bosun step has continue-on-error"
  fi
  if grep -vE '^[[:space:]]*#' "$f" | grep -vF '::error::' | grep -E 'releases/download|\.bosun/version|gh release download' >/dev/null; then
    bad "$name.yml fetches a pinned release: there is none with [[folders]] support yet"
  fi
  # rust-cache: cache-bin "false" and save-if on main.
  caches="$(grep -c 'uses: Swatinem/rust-cache@' "$f" || true)"
  ok_caches="$(awk '
    /uses: Swatinem\/rust-cache@/ { if (c) done += (bin && save); c = 1; bin = 0; save = 0; next }
    c && /^ *- / { done += (bin && save); c = 0 }
    c && /cache-bin: "false"/ { bin = 1 }
    c && /save-if: \$\{\{ github.ref == .refs\/heads\/main. \}\}/ { save = 1 }
    END { if (c) done += (bin && save); print done + 0 }
  ' "$f")"
  [ "$caches" = "$ok_caches" ] || bad "$name.yml: $ok_caches of $caches rust-cache steps set cache-bin \"false\" and save-if main"
  [ "$fail" = 1 ] || echo "ok: $name.yml ($n build step(s) fail closed, $caches rust-cache step(s))"
done

exit "$fail"
