#!/usr/bin/env bash
# Test for affected.sh: bash .github/scripts/affected.test.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
fail=0

# expect <changed files> <expected true folders, space-separated> [rust]
expect() {
  local got want
  got="$(GITHUB_OUTPUT='' CHANGED="$1" bash "$here/affected.sh" | { grep -E "^[a-z]+=true$" || true; } | { grep -vE "^(rust|all)=" || true; } | cut -d= -f1 | sort | tr '\n' ' ')"
  want=""; for w in $2; do want+="$w"$'\n'; done; want="$(printf '%s' "$want" | sort | tr '\n' ' ')"
  if [ "$got" != "$want" ]; then
    echo "FAIL: [$1] -> got '$got', want '$want'"
    fail=1
  else
    echo "ok: [$(echo "$1" | tr '\n' ',')] -> $want"
  fi
}

everything="bosun splice hull hoist hoard berth keel decisions"
expect "bosun/src/main.rs" "bosun"
expect "splice/src/lib.rs" "splice hull"
expect "hull/scripts/render.mjs" "hull"
expect $'hoard/action/src/main.ts\nkeel/src/main.rs' "hoard hull keel"
expect "decisions/DECISIONS.md" "decisions"
expect "README.md" ""
expect "MIGRATION.md" ""
for root in Cargo.toml Cargo.lock rust-toolchain.toml deny.toml .cargo/config.toml .github/workflows/ci.yml; do
  expect "$root" "$everything"
done

# rust= is false for decisions-only and hull-only changes.
r="$(GITHUB_OUTPUT='' CHANGED="decisions/x.md" bash "$here/affected.sh" | grep '^rust=')"
[ "$r" = rust=false ] || { echo "FAIL: decisions-only rust: $r"; fail=1; }
r="$(GITHUB_OUTPUT='' CHANGED="berth/src/x.rs" bash "$here/affected.sh" | grep '^rust=')"
[ "$r" = rust=true ] || { echo "FAIL: berth rust: $r"; fail=1; }

# No base: every folder.
got="$(GITHUB_OUTPUT='' PR_BASE='' QUEUE_BASE='' PUSH_BEFORE=0000000000000000000000000000000000000000 bash "$here/affected.sh" | grep -c '=true$')"
[ "$got" = 10 ] || { echo "FAIL: zero SHA should affect all (8 folders + rust + all), got $got"; fail=1; }

exit "$fail"
