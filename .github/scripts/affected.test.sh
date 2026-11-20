#!/usr/bin/env bash
# Test for affected.sh: bash .github/scripts/affected.test.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
fail=0
# CI runs this test on push events too: every case names its event.
export GITHUB_EVENT_NAME=pull_request
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

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
expect "bosun/src/main.rs" "bosun keel"
expect "splice/src/lib.rs" "splice hull keel"
expect "hoist/src/lib.rs" "hoist keel"
expect "keel/src/main.rs" "keel"
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

# Every folder runs on a push to main, a merge queue run, a schedule and a manual run: main health
# reads each folder's latest real result, and a skipped job is no result (bosun spec §9).
for ev in push merge_group schedule workflow_dispatch; do
  got="$(GITHUB_EVENT_NAME=$ev GITHUB_OUTPUT='' CHANGED="bosun/src/main.rs" bash "$here/affected.sh" | grep -c '=true$')"
  [ "$got" = 10 ] || { echo "FAIL: $ev should affect every folder, got $got =true lines"; fail=1; }
  got="$(GITHUB_EVENT_NAME=$ev GITHUB_OUTPUT='' PR_BASE='' QUEUE_BASE='' PUSH_BEFORE=HEAD bash "$here/affected.sh" | grep -c '=true$')"
  [ "$got" = 10 ] || { echo "FAIL: $ev with a base should affect every folder, got $got"; fail=1; }
done
echo "ok: push/merge_group/schedule/workflow_dispatch -> every folder"

# depends_on comes from .bosun/bosun.toml (the one source): hull depends on splice and hoard,
# keel calls bosun, splice and hoist.
for want in 'depends_on = ["splice", "hoard"]' 'depends_on = ["bosun", "splice", "hoist"]'; do
  grep -qxF "$want" "$here/../../.bosun/bosun.toml" ||
    { echo "FAIL: .bosun/bosun.toml should hold: $want"; fail=1; }
done
expect ".bosun/bosun.toml" "$everything"

# A fixture config: a transitive chain (keel -> berth -> bosun) and a comment are read.
fixture() {
  for d in bosun splice hull hoist hoard berth keel decisions; do
    printf '[[folders]]\npath = "%s"\n' "$d"
    case "$d" in
      berth) echo 'depends_on = ["bosun"] # comment' ;;
      keel) echo 'depends_on = ["berth",]' ;;
    esac
    echo
  done
}
{ echo 'project = "x"'; echo '[review]'; echo 'mode = "subagent"'; fixture; } >"$tmp/ok.toml"
got="$(BOSUN_TOML="$tmp/ok.toml" GITHUB_OUTPUT='' CHANGED="bosun/a" bash "$here/affected.sh" | grep '=true$' | grep -vE '^(rust|all)=' | cut -d= -f1 | sort | tr '\n' ' ')"
[ "$got" = "berth bosun keel " ] || { echo "FAIL: transitive depends_on: got '$got'"; fail=1; }
echo "ok: fixture depends_on (transitive) -> $got"

# Fail closed: an unreadable, unparsable or drifted bosun.toml fails the run with ::error.
refuse() {
  local name="$1" err
  if err="$(BOSUN_TOML="$tmp/$name.toml" GITHUB_OUTPUT='' CHANGED="bosun/a" bash "$here/affected.sh" 2>&1 >/dev/null)"; then
    echo "FAIL: $name should fail"; fail=1
  elif ! grep -q '::error::affected.sh' <<<"$err"; then
    echo "FAIL: $name: no ::error: $err"; fail=1
  else
    echo "ok: $name fails closed"
  fi
}
fixture | sed 's/^depends_on = \["bosun"\].*/depends_on = [/' >"$tmp/multiline.toml"
fixture | sed 's/^path = "decisions"/path = "decisionz"/' >"$tmp/drift.toml"
fixture | sed 's/^depends_on = \["bosun"\].*/depends_on = ["nope"]/' >"$tmp/unknown.toml"
fixture | sed 's/^depends_on = \["bosun"\].*/depends_on = "bosun"/' >"$tmp/scalar.toml"
fixture | sed 's/^depends_on = \["bosun"\].*/extra = 1/' >"$tmp/key.toml"
for name in missing multiline drift unknown scalar key; do refuse "$name"; done

exit "$fail"
