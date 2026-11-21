#!/usr/bin/env bash
# Tests for root workflows outside the gates, run by ci.yml job ci-repo:
#   bash .github/scripts/workflows.test.sh
#
# 1. Every actions/checkout that does not push sets persist-credentials: false (the one that
#    pushes: budget-report's `reports` checkout).
# 2. main-health's `report` job (it holds the App key) runs only for refs/heads/main.
# 3. deploy-site: `decide` deploys a version <= 0.21.0 (legacy state, D-024) or one whose tag
#    vX.Y.Z exists, outputs the commit it resolved, and `deploy` checks out exactly that commit;
#    the workflow_run trigger counts only this repository's dispatched release runs.
# 4. release.yml `plan`: only an absent workspace version at the first parent reads as empty; a
#    git error (a missing object) fails the step. The plan checkout keeps no credentials.
# 5. release.yml: the tag deploy key step fails closed without RELEASE_TAG_KEY, pins github.com's
#    host key, and both tag jobs stop the agent in an always() step.
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
ok() { echo "ok: $*"; }
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
# mkcommit <repo> <file> <content> [parent]: a commit (plumbing, loose objects) whose tree holds
# only <file>; prints its sha.
mkcommit() {
  local blob tree
  blob="$(printf '%s' "$3" | git -C "$1" hash-object -w --stdin)"
  tree="$(printf '100644 blob %s\t%s\n' "$blob" "$2" | git -C "$1" mktree)"
  if [ -n "${4:-}" ]; then
    git -C "$1" commit-tree "$tree" -p "$4" -m c
  else
    git -C "$1" commit-tree "$tree" -m c
  fi
}

# job_text <workflow> <job>: the lines of top-level job <job>.
job_text() {
  awk -v j="  $2:" '$0 == j { on = 1; print; next } on && /^  [A-Za-z0-9_-]+:/ { exit } on' "$1"
}
# step_run <workflow> <job> <step name>: the de-indented `run: |` body of that step.
step_run() {
  job_text "$1" "$2" | awk -v n="      - name: $3" '
    $0 == n { want = 1; next }
    want && /^      - / { exit }
    want && /^        run: \|$/ { inrun = 1; want = 0; next }
    inrun {
      if ($0 != "" && $0 !~ /^          /) exit
      print substr($0, 11)
    }'
}

# --- 1. persist-credentials ---------------------------------------------------------------
for f in "$wf"/*.yml; do
  name="$(basename "$f")"
  # One record per checkout step: "<line> <persist value> <path>".
  while read -r line persist path; do
    if [ "$name" = budget-report.yml ] && [ "$path" = reports ]; then
      [ "$persist" = true ] || bad "$name:$line: the reports checkout pushes (persist-credentials: true)"
      continue
    fi
    [ "$persist" = false ] || bad "$name:$line: checkout without persist-credentials: false (got '${persist:-unset}')"
  done < <(awk '
    function flush() { if (at) print at, (p == "" ? "-" : p), (d == "" ? "-" : d); at = 0 }
    /uses: actions\/checkout@/ { flush(); at = NR; p = ""; d = ""; ind = index($0, "uses:"); next }
    at && /^ *- / { flush() }
    at && /^ *[a-z-]+:/ && index($0, $1) < ind && !/^ *with:/ { flush() }
    at && /persist-credentials:/ { p = $2 }
    at && /^ *path:/ { d = $2 }
    END { flush() }
  ' "$f")
done
[ "$fail" = 1 ] || ok "every checkout that does not push sets persist-credentials: false"

# --- 2. main-health report on main only -----------------------------------------------------
report="$(job_text "$wf/main-health.yml" report)"
grep -qF "github.ref == 'refs/heads/main'" <<<"$(awk '/^    if: /,/^    runs-on:/' <<<"$report")" ||
  bad "main-health.yml: job report (App key) has no github.ref == 'refs/heads/main' guard in its if:"
ok "main-health report is guarded to refs/heads/main"

# --- 3. deploy-site ------------------------------------------------------------------------
ds="$wf/deploy-site.yml"
decide_if="$(job_text "$ds" decide | awk '/^    if: /,/^    runs-on:/')"
for want in "github.event.workflow_run.event == 'workflow_dispatch'" \
  "github.event.workflow_run.head_repository.full_name == github.repository" \
  "github.event.workflow_run.conclusion == 'success'"; do
  grep -qF "$want" <<<"$decide_if" || bad "deploy-site.yml decide if: lacks $want"
done
grep -qF 'sha: ${{ steps.tag.outputs.sha }}' <<<"$(job_text "$ds" decide)" || bad "deploy-site.yml: decide has no sha output"
deploy="$(job_text "$ds" deploy)"
grep -qF 'ref: ${{ needs.decide.outputs.sha }}' <<<"$deploy" || bad "deploy-site.yml: deploy does not check out decide's sha"
! grep -q 'SITE_REF\|github.sha\|default_branch' <<<"$deploy" || bad "deploy-site.yml: deploy resolves the ref a second time"
grep -q '^# ci-minutes: allow concurrency [a-z]' "$ds" || bad "deploy-site.yml: no ci-minutes allow comment with a reason"

decide="$(step_run "$ds" decide 'Is the version released')"
[ -n "$decide" ] || bad "deploy-site.yml: step 'Is the version released' not found"
mkdir -p "$tmp/ghbin"
# A fake gh: prints $FAKE_REFS for matching-refs, logs its calls, fails when FAKE_GH_FAIL is set.
cat >"$tmp/ghbin/gh" <<'EOF'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_GH_LOG"
[ -z "${FAKE_GH_FAIL:-}" ] || { echo "HTTP 500" >&2; exit 1; }
case "$*" in */commits/*) printf '%s' "${FAKE_SHA:-}" ;; *) printf '%s' "$FAKE_REFS" ;; esac
EOF
chmod +x "$tmp/ghbin/gh"
# decide_case <version> <tag refs> <want deploy|error>
decide_case() {
  local d="$tmp/decide-$1-$RANDOM" out want="$3" got wantsha tagsha="${TAG_SHA-}"
  mkdir -p "$d"
  git -C "$d" init -q
  git -C "$d" update-ref HEAD "$(mkcommit "$d" Cargo.toml "$(printf '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "%s"\n' "$1")")"
  git -C "$d" checkout -q -f HEAD -- .
  out="$d.out"
  : >"$out"
  if (cd "$d" && PATH="$tmp/ghbin:$PATH" FAKE_GH_LOG="$d.log" FAKE_REFS="$2" FAKE_SHA="$tagsha" FAKE_GH_FAIL="${FAKE_GH_FAIL:-}" \
    GITHUB_OUTPUT="$out" GITHUB_REPOSITORY=o/r GH_TOKEN=x bash -e -c "$decide") >"$d.stdout" 2>&1; then
    got="$(sed -n 's/^deploy=//p' "$out")"
    [ "$got" = "$want" ] || bad "decide $1 [$2]: deploy=$got, want $want: $(cat "$d.stdout")"
    # A released version (> 0.21.0) builds the tag's commit; a legacy one the resolved HEAD.
    wantsha="$(git -C "$d" rev-parse HEAD)"
    if [ "$want" = true ] && [ -n "$tagsha" ]; then wantsha="$tagsha"; fi
    [ "$(sed -n 's/^sha=//p' "$out" | tail -n 1)" = "$wantsha" ] || bad "decide $1: sha output is not $wantsha"
  else
    [ "$want" = error ] || bad "decide $1 [$2] failed: $(cat "$d.stdout")"
  fi
  if [ "${NO_GH:-}" = 1 ] && [ -s "$d.log" ]; then bad "decide $1 called gh: $(cat "$d.log")"; fi
}
NO_GH=1 decide_case 0.21.0 "" true
NO_GH=1 decide_case 0.20.9 "" true
NO_GH=1 decide_case 0.3.1 "" true
NO_GH=1 FAKE_GH_FAIL=1 decide_case 0.21.0 "" true
TAG_SHA=1111111111111111111111111111111111111111 decide_case 0.22.0 "refs/tags/v0.22.0" true
TAG_SHA=2222222222222222222222222222222222222222 decide_case 0.21.1 $'refs/tags/v0.21.10\nrefs/tags/v0.21.1' true
TAG_SHA=notasha decide_case 0.22.0 "refs/tags/v0.22.0" error
decide_case 0.22.0 "refs/tags/v0.22.0-rc.1" false
decide_case 0.22.0 "" false
decide_case 1.0.0 "" false
FAKE_GH_FAIL=1 decide_case 0.22.0 "" error
decide_case 0.22 "" error
decide_case 0.22.0-rc.1 "" error
[ "$fail" = 1 ] || ok "deploy-site decide: <= 0.21.0 or tagged deploys, sha pinned, guards"

# --- 4. release.yml plan: the first parent's version --------------------------------------
rel="$wf/release.yml"
plan="$(step_run "$rel" plan 'Did the workspace version change?')"
[ -n "$plan" ] || bad "release.yml: plan step not found"
job_text "$rel" plan | grep -qF 'persist-credentials: false' || bad "release.yml: plan checkout persists credentials"
# plan_case <name> <parent Cargo.toml or "-" for none> <break: "" | blob> <want release=|error>
plan_case() {
  local d="$tmp/plan-$1" out
  mkdir -p "$d"
  git -C "$d" init -q
  local parent blob
  if [ "$2" = - ]; then
    parent="$(mkcommit "$d" README x)"
  else
    parent="$(mkcommit "$d" Cargo.toml "$2")"
  fi
  if [ "$3" = blob ]; then
    # The parent's Cargo.toml blob is gone (an incomplete clone): git show must fail.
    blob="$(git -C "$d" rev-parse "$parent:Cargo.toml")"
    rm -f "$d/.git/objects/${blob:0:2}/${blob:2}"
  fi
  git -C "$d" update-ref HEAD "$(mkcommit "$d" Cargo.toml $'[workspace.package]\nversion = "0.22.0"\n' "$parent")"
  out="$d.out"
  : >"$out"
  if (cd "$d" && EVENT=pull_request REF=refs/pull/1/merge SHA=HEAD INPUT_VERSION='' GH_TOKEN=x GH_REPO=o/r \
    GITHUB_OUTPUT="$out" bash -e -c "$plan") >"$d.stdout" 2>&1; then
    [ "$4" != error ] || { bad "plan $1: passed, want a failure: $(cat "$d.stdout")"; return; }
    [ "$(sed -n 's/^release=//p' "$out")" = "$4" ] || bad "plan $1: $(grep '^release=' "$out"), want release=$4: $(cat "$d.stdout")"
  else
    [ "$4" = error ] || { bad "plan $1 failed: $(cat "$d.stdout")"; return; }
    grep -q '^::error::cannot read Cargo.toml at the first parent' "$d.stdout" || bad "plan $1: no ::error naming the git failure: $(cat "$d.stdout")"
  fi
}
plan_case no-cargo-toml - "" false
plan_case no-workspace-version $'[package]\nname = "x"\nversion = "0.21.0"\n' "" false
plan_case same-version $'[workspace.package]\nversion = "0.22.0"\n' "" false
plan_case bumped $'[workspace.package]\nversion = "0.21.5"\n' "" true
plan_case missing-blob $'[workspace.package]\nversion = "0.21.5"\n' blob error
[ "$fail" = 1 ] || ok "release plan: an absent version reads empty, a git failure fails"

# --- 5. release.yml: the tag deploy key -------------------------------------------------------
for j in release major; do
  key="$(step_run "$rel" "$j" 'Load the tag deploy key')"
  [ -n "$key" ] || { bad "release.yml $j: no 'Load the tag deploy key' step"; continue; }
  if out="$(RELEASE_TAG_KEY='' RUNNER_TEMP="$tmp" GITHUB_ENV="$tmp/env" bash -e -c "$key" 2>&1)"; then
    bad "release.yml $j: the key step passes without RELEASE_TAG_KEY"
  else
    grep -q '^::error::secret RELEASE_TAG_KEY of the environment release is missing' <<<"$out" ||
      bad "release.yml $j: no clear ::error without RELEASE_TAG_KEY: $out"
  fi
  grep -qF 'github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl' <<<"$key" ||
    bad "release.yml $j: github.com's ed25519 host key is not pinned"
  grep -qF 'SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU' <<<"$key" || bad "release.yml $j: no fingerprint check"
  grep -qF 'StrictHostKeyChecking=yes' <<<"$key" || bad "release.yml $j: host key checking is not strict"
  ! grep -q 'StrictHostKeyChecking=no\|accept-new\|ssh-keyscan' <<<"$key" || bad "release.yml $j: host key not pinned"
  ! grep -qE '>[^&]*(id_|\.key|private)' <<<"$key" || bad "release.yml $j: the key is written to a file"
  job_text "$rel" "$j" | grep -qF 'RELEASE_TAG_KEY: ${{ secrets.RELEASE_TAG_KEY }}' || bad "release.yml $j: no RELEASE_TAG_KEY env"
  stop="$(job_text "$rel" "$j" | awk '/^      - name: Stop the ssh-agent$/ { s = 1; next } s && /^      - / { exit } s')"
  grep -qx '        if: always()' <<<"$stop" || bad "release.yml $j: no always() step stops the ssh-agent"
  job_text "$rel" "$j" | grep -qx '    environment: release' || bad "release.yml $j: not in environment release"
done
[ "$(step_run "$rel" release 'Load the tag deploy key')" = "$(step_run "$rel" major 'Load the tag deploy key')" ] ||
  bad "release.yml: the key steps of release and major differ"
n="$(grep -vE '^[[:space:]]*#' "$rel" | grep -cE 'git/refs|extraheader' || true)"
[ "$n" = 0 ] || bad "release.yml: a tag is still written through the API or an HTTPS token"
[ "$fail" = 1 ] || ok "release.yml: tag deploy key fails closed, host key pinned, agent stopped"

exit "$fail"
