#!/usr/bin/env bash
# Test for scripts/release-result.sh.   bash scripts/release-result.test.sh
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fails=0
ok() { echo "ok   $*"; }
fail() { echo "FAIL $*" >&2; fails=$((fails + 1)); }

run() { # <json> <code> -> sets $rc, $tmp/out
  printf '%s' "$1" >"$tmp/r.json"
  : >"$tmp/out"
  set +e
  GITHUB_OUTPUT="$tmp/out" GITHUB_STEP_SUMMARY="$tmp/sum" bash "$root/scripts/release-result.sh" "$tmp/r.json" "$2" npm >"$tmp/log" 2>&1
  rc=$?
  set -e
}

run '{"items":[{"action":"ok","key":"a@1.0.0"}]}' 0
if [ "$rc" = 0 ] && [ "$(cat "$tmp/out")" = "skipped=" ]; then ok "exit 0: nothing skipped"; else fail "exit 0 ($rc): $(cat "$tmp/out")"; fi

run '{"items":[{"action":"ok","key":"a@1.0.0"},{"action":"pending","key":"@k/new-linux-x64@1.2.3"},{"action":"pending","key":"@k/new@1.2.3"},{"action":"pending","key":"crate-x@1.2.3"}]}' 2
if [ "$rc" = 0 ] && [ "$(cat "$tmp/out")" = "skipped=@k/new @k/new-linux-x64 crate-x" ] && grep -q "hoist release bootstrap" "$tmp/log"; then
  ok "exit 2: pending names are skipped, job succeeds"
else
  fail "exit 2 ($rc): $(cat "$tmp/out") / $(cat "$tmp/log")"
fi

run '{"items":[]}' 2
if [ "$rc" = 2 ]; then ok "exit 2 without a pending item fails"; else fail "exit 2 empty gave $rc"; fi

run '{"items":[]}' 1
if [ "$rc" = 1 ]; then ok "exit 1 fails with 1"; else fail "exit 1 gave $rc"; fi
run '' 3
if [ "$rc" = 3 ]; then ok "exit 3 fails with 3 (result unreadable is fine)"; else fail "exit 3 gave $rc"; fi

[ "$fails" = 0 ] || { echo "$fails failure(s)" >&2; exit 1; }
echo "all ok"
