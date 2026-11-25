#!/usr/bin/env bash
# Tests for root workflows outside the gates, run by ci.yml job ci-repo:
#   bash .github/scripts/workflows.test.sh
#
# 1. Every actions/checkout that does not push sets persist-credentials: false (the one that
#    pushes: budget-report's `reports` checkout).
# 2. main-health's `report` job (it holds the App key) runs only for refs/heads/main.
# 3. deploy-site: every push to main deploys main's site; `decide` resolves the commit (the
#    release run's head_sha after a release), the latest release tag vX.Y.Z reachable from it and
#    that tag plus every lower one (`candidates`). `build` (no secrets) asks npm which candidate is
#    fully published ('Published release' sets RELEASED_VERSION; none = legacy), regenerates the
#    docs for it, installs and checks, `deploy` (environment site) runs wrangler on the sha256-verified
#    artifact only. The workflow_run trigger counts only this repository's dispatched release runs.
# 4. release.yml `plan`: only an absent workspace version at the first parent reads as empty; a
#    git error (a missing object) fails the step. The plan checkout keeps no credentials.
# 5. release.yml: the tag deploy key step fails closed without RELEASE_TAG_KEY, pins github.com's
#    host key, and both tag jobs stop the agent in an always() step.
# 6. release.yml crates and npm steps: the release job runs `hoist release publish --only <registry>` (a
#    package not published yet is skipped with a notice, see scripts/release-result.sh) and
#    major waits for skipped packages.
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
# Every reader that may stop early (awk `exit`, `grep -q`) reads a variable through a
# here-string, never a pipe: under `pipefail` a writer cut off by SIGPIPE fails the line (141).
step_run() {
  local jt
  jt="$(job_text "$1" "$2")"
  awk -v n="      - name: $3" '
    $0 == n { want = 1; next }
    want && /^      - / { exit }
    want && /^        run: \|$/ { inrun = 1; want = 0; next }
    inrun {
      if ($0 != "" && $0 !~ /^          /) exit
      print substr($0, 11)
    }' <<<"$jt"
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
decide_job="$(job_text "$ds" decide)"
build_job="$(job_text "$ds" build)"
deploy="$(job_text "$ds" deploy)"
# deploy-site.yml has no workflow_run trigger (bosun forbids naming a workflow_run head or
# downloading an artifact there); deploy-site-release.yml dispatches it after a release, for this
# repository's dispatched release runs that succeeded or failed.
! grep -q 'workflow_run' "$ds" || bad "deploy-site.yml: names workflow_run (bosun forbids head_sha and artifacts there)"
grep -qF 'SITE_SHA: ${{ github.sha }}' <<<"$decide_job" || bad "deploy-site.yml: decide does not resolve github.sha"
! grep -q 'default_branch' "$ds" || bad "deploy-site.yml: resolves main's moving head by branch name"
dsr="$wf/deploy-site-release.yml"
rel_if="$(awk '/^    if: /,/^    runs-on:/' <<<"$(job_text "$dsr" dispatch)")"
for want in "github.event.workflow_run.event == 'workflow_dispatch'" \
  "github.event.workflow_run.head_repository.full_name == github.repository" \
  "github.event.workflow_run.conclusion == 'success'" \
  "github.event.workflow_run.conclusion == 'failure'"; do
  grep -qF "$want" <<<"$rel_if" || bad "deploy-site-release.yml dispatch if: lacks $want"
done
grep -qF 'gh workflow run deploy-site.yml' "$dsr" || bad "deploy-site-release.yml: does not dispatch deploy-site.yml"
! grep -v '^ *#' "$dsr" | grep -qE 'secrets\.|checkout|download|head_sha' || bad "deploy-site-release.yml: reads secrets, code, artifacts or a run head"
grep -q '^# ci-minutes: allow concurrency [a-z]' "$dsr" || bad "deploy-site-release.yml: no ci-minutes allow comment with a reason"
dd_if="$(awk '/^    if: /,/^    runs-on:/' <<<"$decide_job")"
grep -qF "github.ref == 'refs/heads/main'" <<<"$dd_if" || bad "deploy-site.yml: a manual run is not limited to main"
grep -qF 'sha: ${{ steps.release.outputs.sha }}' <<<"$decide_job" || bad "deploy-site.yml: decide has no sha output"
grep -qF 'released: ${{ steps.release.outputs.released }}' <<<"$decide_job" || bad "deploy-site.yml: decide has no released output"
grep -qF 'ref: ${{ needs.decide.outputs.sha }}' <<<"$build_job" || bad "deploy-site.yml: build does not check out decide's sha"
grep -qF 'candidates: ${{ steps.release.outputs.candidates }}' <<<"$decide_job" || bad "deploy-site.yml: decide has no candidates output"
pub_step="$(step_run "$ds" build 'Published release')"
[ -n "$pub_step" ] || bad "deploy-site.yml: build has no 'Published release' step"
grep -qF 'CANDIDATES: ${{ needs.decide.outputs.candidates }}' <<<"$build_job" || bad "deploy-site.yml: build does not pass decide's candidates"
grep -qF 'node apps/site/scripts/published.ts $CANDIDATES' <<<"$pub_step" || bad "deploy-site.yml: 'Published release' does not run published.ts"
grep -qF 'RELEASED_VERSION=$v" >> "$GITHUB_ENV"' <<<"$pub_step" || bad "deploy-site.yml: 'Published release' does not write RELEASED_VERSION to GITHUB_ENV"
grep -qF 'node apps/site/scripts/gen-docs.ts' <<<"$build_job" || bad "deploy-site.yml: build does not regenerate the docs"
! grep -qE 'RELEASED_VERSION: \$\{\{' <<<"$build_job" || bad "deploy-site.yml: build takes RELEASED_VERSION from decide's tag, not from npm"
order="$(grep -nE 'name: (Published release|Generate the docs for it|Check and build)' <<<"$build_job" | cut -d: -f2- | tr -s ' ' | tr '\n' '|')"
[ "$order" = " - name: Published release| - name: Generate the docs for it| - name: Check and build|" ] || bad "deploy-site.yml: build steps are not Published release, gen-docs, Check and build: $order"
! grep -q 'SITE_REF' "$ds" || bad "deploy-site.yml: SITE_REF is back: a deploy must not wait for a tag"
grep -q '^# ci-minutes: allow concurrency [a-z]' "$ds" || bad "deploy-site.yml: no ci-minutes allow comment with a reason"
# build installs and tests with no secrets and no environment; deploy holds the secrets and only
# runs wrangler on a verified artifact: no checkout, no pnpm.
wrangler_job="$(job_text "$ds" wrangler)"
[ -n "$wrangler_job" ] || bad "deploy-site.yml: no wrangler job"
grep -qF 'deploy --prod --ignore-scripts' <<<"$wrangler_job" || bad "deploy-site.yml: wrangler does not stage from the lockfile with --ignore-scripts"
! grep -qE 'secrets\.|^    environment:' <<<"$wrangler_job" || bad "deploy-site.yml: wrangler job sees secrets or an environment"
! grep -qE 'pnpm install|cache:|run ci' <<<"$wrangler_job" || bad "deploy-site.yml: wrangler job installs the workspace, restores a cache or runs a site script"
! grep -qF 'deploy --prod' <<<"$build_job" || bad "deploy-site.yml: build stages wrangler (only the wrangler job may)"
! grep -qE 'secrets\.|^    environment:' <<<"$build_job" || bad "deploy-site.yml: build job sees secrets or an environment"
grep -qF 'needs.wrangler.outputs.digest' <<<"$deploy" || bad "deploy-site.yml: deploy has no digest from wrangler"
grep -qF 'tar -xzf artifact/site/site-dist.tgz dist' <<<"$deploy" || bad "deploy-site.yml: deploy does not extract only dist from the site archive"
grep -qF -- '! -type f ! -type d' <<<"$deploy" || bad "deploy-site.yml: deploy does not reject anything but files and directories in dist"
! grep -qE 'pnpm|checkout|setup-node|npm install' <<<"$deploy" || bad "deploy-site.yml: deploy job installs or checks out code"
grep -qF 'environment: site' <<<"$deploy" || bad "deploy-site.yml: deploy is not in environment site"
grep -qF 'sha256sum -c' <<<"$deploy" || bad "deploy-site.yml: deploy does not verify the artifact's sha256"
grep -qF 'needs.build.outputs.digest' <<<"$deploy" || bad "deploy-site.yml: deploy has no digest from build"
before_deploy="$(awk '/^      - name: Deploy to Cloudflare/ { exit } { print }' <<<"$deploy")"
! grep -qE 'secrets\.' <<<"$(grep -v 'name: Check secrets' <<<"$(awk '/^      - name: Check secrets/ { skip = 1; next } skip && /^      - / { skip = 0 } !skip' <<<"$before_deploy")")" ||
  bad "deploy-site.yml: a deploy step before 'Deploy to Cloudflare Pages' (other than Check secrets) sees secrets"
unpinned="$(grep -hE '^\s*-?\s*uses:' "$ds" "$dsr" | grep -vE '@[0-9a-f]{40}( |$)' || true)"
[ -z "$unpinned" ] || bad "deploy-site.yml: actions not pinned by SHA: $unpinned"

rel_step="$(step_run "$ds" decide 'Latest release tag reachable from the commit')"
[ -n "$rel_step" ] || bad "deploy-site.yml: step 'Latest release tag reachable from the commit' not found"
mkdir -p "$tmp/ghbin"
# A fake gh. matching-refs prints $FAKE_REFS. commits/vX prints the tag commit's FIRST PARENT (the
# release commit is a child of a main commit, reachable only from its tag): "f..." for tags in
# $FAKE_UNREACHABLE, else "a...". commits/main prints $FAKE_MAIN. compare/<base>...<head> prints
# "diverged" for an "f..." base, $FAKE_MAIN_STATUS for head == $FAKE_MAIN, else "ahead"; a compare
# against a tag name (what a release commit would never satisfy) fails. FAKE_GH_FAIL fails all.
cat >"$tmp/ghbin/gh" <<'FAKEGH'
#!/usr/bin/env bash
args="$*"
echo "$args" >>"$FAKE_GH_LOG"
[ -z "${FAKE_GH_FAIL:-}" ] || { echo "HTTP 500" >&2; exit 1; }
case "$args" in
  *matching-refs*) printf '%s\n' "$FAKE_REFS" ;;
  */commits/main*) printf '%s\n' "$FAKE_MAIN" ;;
  */commits/v*)
    tag="${args##*/commits/}"
    tag="${tag%% *}"
    for t in ${FAKE_UNREACHABLE:-}; do
      [ "$t" != "$tag" ] || { printf 'f%039d\n' 1; exit 0; }
    done
    printf 'a%039d\n' 1 ;;
  */compare/v*) echo "release commits are not on main: compare against a tag name is wrong" >&2; exit 1 ;;
  */compare/*)
    pair="${args##*/compare/}"
    pair="${pair%% *}"
    base="${pair%%...*}"
    head="${pair##*...}"
    if [ "${base:0:1}" = f ]; then echo diverged
    elif [ "$head" = "${FAKE_MAIN:-}" ] && [ -n "${FAKE_MAIN_STATUS:-}" ]; then echo "$FAKE_MAIN_STATUS"
    else echo ahead; fi ;;
  *) echo "unexpected gh call: $args" >&2; exit 1 ;;
esac
FAKEGH
chmod +x "$tmp/ghbin/gh"
SHA_OK=0123456789abcdef0123456789abcdef01234567
# release_case <name> <tag refs> <want: a version, "none" or "error"> [sha] [want sha]
# WANT_CANDIDATES (optional, set per call): the exact `candidates=` line, else only checked to exist.
release_case() {
  local d="$tmp/rel-$1" out="$tmp/rel-$1.out" want="$3" sha="${4-$SHA_OK}" wantsha="${5-${4-$SHA_OK}}"
  mkdir -p "$d"
  : >"$out"
  if (cd "$d" && PATH="$tmp/ghbin:$PATH" FAKE_GH_LOG="$d.log" FAKE_REFS="$2" FAKE_UNREACHABLE="${FAKE_UNREACHABLE:-}" FAKE_GH_FAIL="${FAKE_GH_FAIL:-}" \
    FAKE_MAIN="${FAKE_MAIN:-$sha}" FAKE_MAIN_STATUS="${FAKE_MAIN_STATUS:-}" GITHUB_EVENT_NAME="${EVENT:-push}" \
    GITHUB_OUTPUT="$out" GITHUB_REPOSITORY=o/r GH_TOKEN=x SITE_SHA="$sha" RETRY_SLEEP=0 bash -e -c "$rel_step") >"$d.stdout" 2>&1; then
    [ "$want" != error ] || { bad "release $1: passed, want a failure"; return; }
    [ "$want" != none ] || want=""
    grep -qx "released=$want" "$out" || bad "release $1: $(grep '^released=' "$out" || echo 'no released='), want '$want': $(cat "$d.stdout")"
    if [ "${WANT_CANDIDATES+set}" = set ]; then
      grep -qx "candidates=$WANT_CANDIDATES" "$out" || bad "release $1: $(grep '^candidates=' "$out" || echo 'no candidates='), want '$WANT_CANDIDATES'"
    else
      grep -q '^candidates=' "$out" || bad "release $1: no candidates= output"
    fi
    grep -qx "sha=$wantsha" "$out" || bad "release $1: sha output is not $wantsha: $(cat "$out")"
  else
    [ "$want" = error ] || bad "release $1 failed: $(cat "$d.stdout")"
  fi
}
WANT_CANDIDATES="" release_case none "" none
WANT_CANDIDATES="0.22.0" release_case one "refs/tags/v0.22.0" 0.22.0
WANT_CANDIDATES="0.21.10 0.21.9 0.9.0" release_case highest $'refs/tags/v0.21.9\nrefs/tags/v0.21.10\nrefs/tags/v0.9.0' 0.21.10
release_case major $'refs/tags/v0.22.0\nrefs/tags/v1.0.0' 1.0.0
release_case prerelease-ignored $'refs/tags/v0.22.0-rc.1\nrefs/tags/v0.22.0' 0.22.0
release_case only-prerelease "refs/tags/v0.22.0-rc.1" none
release_case other-suffix $'refs/tags/v0.22.0\nrefs/tags/v0.30.0x' 0.22.0
WANT_CANDIDATES="0.22.0" FAKE_UNREACHABLE="v0.23.0" release_case unreachable-skipped $'refs/tags/v0.22.0\nrefs/tags/v0.23.0' 0.22.0
WANT_CANDIDATES="" FAKE_UNREACHABLE="v0.23.0" release_case all-unreachable "refs/tags/v0.23.0" none
FAKE_GH_FAIL=1 release_case gh-fails "refs/tags/v0.22.0" error
WANT_CANDIDATES="0.22.0 0.21.0" release_case lower-listed $'refs/tags/v0.21.0\nrefs/tags/v0.22.0' 0.22.0
WANT_CANDIDATES="0.22.0 0.21.0" FAKE_UNREACHABLE="v0.23.0" release_case unreachable-not-listed $'refs/tags/v0.21.0\nrefs/tags/v0.22.0\nrefs/tags/v0.23.0' 0.22.0
release_case bad-sha "" error notasha
[ "$fail" = 1 ] || ok "deploy-site: latest reachable release tag, main's sha, guards; build (no secrets) and deploy split"

# --- 4. release.yml plan: the first parent's version --------------------------------------
rel="$wf/release.yml"
plan="$(step_run "$rel" plan 'Did the workspace version change?')"
[ -n "$plan" ] || bad "release.yml: plan step not found"
grep -qF 'persist-credentials: false' <<<"$(job_text "$rel" plan)" || bad "release.yml: plan checkout persists credentials"
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
for j in release; do
  key="$(step_run "$rel" "$j" 'Load the tag deploy key')"
  [ -n "$key" ] || { bad "release.yml $j: no 'Load the tag deploy key' step"; continue; }
  if out="$(RELEASE_TAG_KEY='' RUNNER_TEMP="$tmp" GITHUB_ENV="$tmp/env" bash -e -c "$key" 2>&1)"; then
    bad "release.yml $j: the key step passes without RELEASE_TAG_KEY"
  else
    grep -q '^::error::secret RELEASE_TAG_KEY of the environment release-tags is missing' <<<"$out" ||
      bad "release.yml $j: no clear ::error without RELEASE_TAG_KEY: $out"
  fi
  grep -qF 'github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl' <<<"$key" ||
    bad "release.yml $j: github.com's ed25519 host key is not pinned"
  grep -qF 'SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU' <<<"$key" || bad "release.yml $j: no fingerprint check"
  grep -qF 'StrictHostKeyChecking=yes' <<<"$key" || bad "release.yml $j: host key checking is not strict"
  # Code lines only: the step's own comment warns against these options by name.
  code="$(grep -vE '^[[:space:]]*#' <<<"$key" || true)"
  ! grep -q 'StrictHostKeyChecking=no\|accept-new\|ssh-keyscan' <<<"$code" || bad "release.yml $j: host key not pinned"
  ! grep -qE '>[^&]*(id_|\.key|private)' <<<"$key" || bad "release.yml $j: the key is written to a file"
  jt="$(job_text "$rel" "$j")"
  grep -qF 'RELEASE_TAG_KEY: ${{ secrets.RELEASE_TAG_KEY }}' <<<"$jt" || bad "release.yml $j: no RELEASE_TAG_KEY env"
  stop="$(awk '/^      - name: Stop the ssh-agent$/ { s = 1; next } s && /^      - / { exit } s' <<<"$jt")"
  grep -qx '        if: always()' <<<"$stop" || bad "release.yml $j: no always() step stops the ssh-agent"
done
# The key is loaded twice in job release (tag, then the major tag), stopped after each, and both
# loads are the same script.
[ "$(grep -c '^      - name: Load the tag deploy key$' "$rel")" = 2 ] || bad "release.yml: the deploy key is not loaded exactly twice"
[ "$(grep -c '^      - name: Stop the ssh-agent$' "$rel")" = 2 ] || bad "release.yml: the ssh-agent is not stopped twice"
key_bodies="$(awk '/^      - name: Load the tag deploy key$/ { n++; on = 1; next } on && /^      - / { on = 0 } on && $0 !~ /^        if: / { print n ": " $0 }' "$rel")"
[ "$(grep '^1: ' <<<"$key_bodies" | sed 's/^1: //')" = "$(grep '^2: ' <<<"$key_bodies" | sed 's/^2: //')" ] ||
  bad "release.yml: the two key-loading steps differ"
# Privilege split: the registries run in `publish` (environment release, the owner's one tap,
# id-token, no write permission, no deploy key); the tag key sits in `release` (environment
# release-tags, contents: write, no id-token, no registry step).
pj="$(job_text "$rel" publish)"
rj="$(job_text "$rel" release)"
grep -qx '    environment: release' <<<"$pj" || bad "release.yml publish: not in environment release"
grep -qx '    environment: release-tags' <<<"$rj" || bad "release.yml release: not in environment release-tags"
grep -qx '      id-token: write' <<<"$pj" || bad "release.yml publish: no id-token: write"
! grep -q 'id-token' <<<"$rj" || bad "release.yml release: holds id-token"
grep -qx '      contents: read' <<<"$pj" || bad "release.yml publish: contents is not read"
! grep -qE 'contents: write|RELEASE_TAG_KEY|ssh-agent' <<<"$pj" || bad "release.yml publish: holds a write permission or the tag key"
! grep -qE 'hoist .*release publish|cargo |npm (publish|install)|rust-toolchain' <<<"$(grep -vE '^[[:space:]]*#' <<<"$rj")" || bad "release.yml release: runs registry or dependency code next to the tag key"
[ "$(grep -c '^    environment:' "$rel")" = 2 ] || bad "release.yml: not exactly two jobs use an environment"
[ "$(grep -c '^      id-token: write' "$rel")" = 1 ] || bad "release.yml: id-token: write is not on exactly one job"
# Every toolchain step of publish runs on re-runs too (no `released` condition on them).
for tool in 'actions/setup-node' 'npm install -g npm@' 'dtolnay/rust-toolchain'; do
  blk="$(grep -B1 -A3 -F "$tool" <<<"$pj" || true)"
  [ -n "$blk" ] || { bad "release.yml publish: no $tool step"; continue; }
  ! grep -q "released" <<<"$blk" || bad "release.yml publish: $tool is skipped on a re-run"
done
# One set of assets: job `draft` (no environment, contents: write, no dependency code) makes or
# reuses the draft release and exports its checked assets; `publish` and `release` use only
# those, never this run's build artifacts, and check every file against SHA256SUMS (no
# --ignore-missing: all listed, none missing).
dj="$(job_text "$rel" draft)"
! grep -q '^    environment:' <<<"$dj" || bad "release.yml draft: uses an environment"
grep -qx '      contents: write' <<<"$dj" || bad "release.yml draft: contents is not write"
! grep -qE 'id-token|RELEASE_TAG_KEY|ssh-agent|cargo |npm (publish|install|ci)|pnpm|rust-toolchain|hoist release' <<<"$(grep -vE '^[[:space:]]*#' <<<"$dj")" || bad "release.yml draft: runs dependency code or holds a publish credential"
grep -qF 'manifest_sha_' <<<"$dj" || bad "release.yml draft: build legs' manifests are not checked"
grep -qF 'sha256sum -c --strict SHA256SUMS' <<<"$dj" || bad "release.yml draft: the draft's assets are not checked against SHA256SUMS"
grep -qF 'sums_sha' <<<"$dj" || bad "release.yml draft: exports no digest of SHA256SUMS"
# Reusing an earlier run's draft checks who made it, for which commit, and that it is complete;
# an incomplete one is only deleted while no registry has the version.
dstep="$(step_run "$rel" draft "The draft release (created, or an earlier run's reused)")"
for want in 'target_commitish' 'target" = "$SHA"' 'github-actions[bot]' 'diff expected-all.txt draft-names.txt' 'on_registry' 'gh release delete'; do
  grep -qF -- "$want" <<<"$dstep" || bad "release.yml draft: the draft reuse does not check $want"
done
! grep -qE "build.result == 'success' &&" <<<"$(grep -A4 '^    if: >-' <<<"$dj" | head -5)" || bad "release.yml draft: a retry with a draft needs the build to succeed"
! grep -qE '\[ -n "\$SUMS_SHA" \] &&' "$rel" || bad "release.yml: an empty SUMS_SHA would pass the check"
! grep -E 'sha256sum -c' "$rel" | grep -vq -- '--strict' || bad "release.yml: a sha256sum -c without --strict"
for j in publish release; do
  t="$(job_text "$rel" "$j")"
  grep -qF 'name: draft-assets' <<<"$t" || bad "release.yml $j: does not use the draft's assets"
  ! grep -qE 'pattern: release-\*|name: action-bundle' <<<"$t" || bad "release.yml $j: uses this run's build artifacts"
  ! grep -qF -- '--ignore-missing' <<<"$t" || bad "release.yml $j: SHA256SUMS is checked with --ignore-missing"
  grep -qF 'sha256sum -c --strict SHA256SUMS' <<<"$t" || bad "release.yml $j: files are not all checked against SHA256SUMS"
  grep -qF 'needs.draft.outputs.sums_sha' <<<"$t" || bad "release.yml $j: SHA256SUMS is not tied to the draft job's digest"
done
grep -qF 'needs: [plan, draft]' <<<"$pj" || bad "release.yml publish: does not need draft"
# The tag job turns the draft into the release; it never creates or replaces one.
! grep -qE 'gh release create|DELETE' <<<"$(grep -vE '^[[:space:]]*#' <<<"$rj")" || bad "release.yml release: creates or deletes a release"
# Job release still runs on a re-run (to move the major tag): its release steps are gated.
grep -qF "needs.publish.result == 'success'" <<<"$rj" || bad "release.yml release: does not wait for publish"
n="$(grep -vE '^[[:space:]]*#' "$rel" | grep -cE 'git/refs|extraheader' || true)"
[ "$n" = 0 ] || bad "release.yml: a tag is still written through the API or an HTTPS token"
[ "$fail" = 1 ] || ok "release.yml: tag deploy key fails closed, host key pinned, agent stopped"

# --- 6. release.yml crates and npm: `hoist release publish --only`, a new package is skipped ----
# The publish logic (dependency order, already-published skips, platform packages first, a
# package not on its registry yet pending, a registry error failing) lives in hoist and is tested
# there (hoist/tests/cli_release_publish.rs, `only_*`); scripts/release-result.test.sh covers the
# exit-code handling. Here: the jobs call hoist the way a consumer does, and major still waits.
crates="$(step_run "$rel" publish 'Publish the crates, dependencies first')"
[ -n "$crates" ] || bad "release.yml: crates publish step not found"
grep -qF 'hoist release publish --only crates --json' <<<"$crates" || bad "crates: does not run hoist release publish --only crates"
grep -qF 'bash scripts/release-result.sh result-crates.json "$code" crates' <<<"$crates" || bad "crates: the exit code is not handled by release-result.sh"
npm_step="$(step_run "$rel" publish 'Publish to npm (trusted publishing, provenance), platform packages before their launcher')"
[ -n "$npm_step" ] || bad "release.yml: npm publish step not found"
for want in 'hoist "${args[@]}"' '--only npm' '--provenance' '--npm-tag "$DIST_TAG"' '--assets assets' 'bash scripts/release-result.sh result-npm.json "$code" npm'; do
  grep -qF -- "$want" <<<"$npm_step" || bad "npm: publish step lacks $want"
done
t="$(job_text "$rel" publish)"
code_t="$(grep -vE '^[[:space:]]*#' <<<"$t" || true)"
! grep -qE 'cargo publish|npm publish|trusted_publishing' <<<"$code_t" || bad "release.yml: publishes by hand instead of through hoist"
# The major tag moves only when neither registry step skipped a package.
grep -qF "needs.publish.outputs.crates_skipped == '' && needs.publish.outputs.npm_skipped == ''" <<<"$rj" ||
  bad "release.yml: moves the major tag while a crate or package is skipped"
grep -qE '^  (crates|npm|major):' "$rel" && bad "release.yml: separate crates, npm or major job (each would need its own tap)"
[ "$fail" = 1 ] || ok "release.yml: crates and npm run hoist release publish --only, major waits for skipped packages"

# ci-repo packages every publishable crate (verify on) when Rust is affected.
cr="$(job_text "$wf/ci.yml" ci-repo)"
grep -qF 'run: cargo package --workspace --exclude keel-splice --locked' <<<"$cr" || bad "ci.yml ci-repo: no 'cargo package --workspace' step"
grep -qF "needs.detect.outputs.rust == 'true'" <<<"$(grep -B2 'cargo package --workspace' <<<"$cr")" || bad "ci.yml ci-repo: cargo package is not gated on Rust being affected"
[ "$fail" = 1 ] || ok "ci.yml: ci-repo runs cargo package --workspace --locked when Rust is affected"

exit "$fail"
