# Plan: one big PR for everything open

project: keelstack

The owner wants every open item in ONE PR. Integration branch `claude/big-pr` = main + PR #56
(`claude/release-refs`) + PR #57 (`claude/guard-feedback`) + `claude/release-0.22.0` (crate
`keelstack-cli`, bin `keel`, folded fragments, no version bump). PR #60
(`claude/bosun-guard-feedback`) is merged by the integrator (item IN).

Workers: one session per item below, branch `claude/big-<ws>` from `claude/big-pr`; edit only
your item's `touch:` globs (they are disjoint between items); run the checks of every folder
you touch (AGENTS.md); push your branch; open no PR. The integrator opens the one PR.

## Triage

DONE (implemented; close with the big PR):

- #6 PI port: PR #4 merged (ef70543); chat-approval tests gone.
- #12 PR #96 findings: `hoist/src/fly.rs:29` current_dir, `hoist/src/ops/apply.rs:56` --prune gate, `hoist/tests/cli_r2.rs:704`.
- #13 M7/T4: `bosun/src/gates/adapters.rs:103`, `bosun/src/gates/integrity.rs:232`, mixed stacks `bosun/tests/uc_m7.rs:169`.
- #14 M7/T2: `bosun/src/cli/init.rs:1045` runs splice channel add.
- #15 M7/P, #17 M4/P, #19 M3/P, #21 M1/P: UCs in `bosun/plans/bosun.md` (227, 130, 101, 39).
- #16 M7: children #13 #14 #42 #43 #44 done.
- #18 M4: `bosun/src/cli/claim.rs:104` spend guard, `bosun/src/cli/budget.rs:856` janitor stop.
- #20 M3: `bosun/src/gates/main_open.rs`, `bosun/src/core/bisect.rs:12`, `bosun/src/cli/revert.rs:44`.
- #22 M1: `bosun/src/hooks/pre_tool_use.rs` label lock, `bosun/src/core/stop.rs:7`, `bosun/src/cli/resume.rs:22`.
- #26 B4/T1: `bosun/src/core/risk.rs:1978` team entries, other-org refused at :2060.
- #27 B4/P, #34 B2W/P: `bosun/docs/superpowers/plans/2026-10-28-app-checks-team-owners.md`.
- #32 B2W/T2: `bosun/channel/pieces/core/files/.github/workflows/risk.yml.jinja:63` `{% if bosun_app %}`.
- #33 B2W/T1: App token in `.github/workflows/risk.yml:80`, `verdict.yml:49`; dual mode `.bosun/bosun.toml:44`.
- #36 B2/T6: `.bosun/risk.toml:11` `**/hoist.toml` R-gov, `bosun/src/core/risk.rs:855` floor.
- #42 M7/T5: `bosun/src/cli/doctor.rs:1662` apply_hoist on --remote.
- #43 M7 T3+T4: `bosun/profiles/ts-pnpm.toml`, `bosun/src/gates/adapters.rs:119`, `bosun/tests/uc_m7.rs:192`.
- #44 M7 T1+T2: channel pieces + `bosun/src/cli/init.rs:1045`, `bosun/tests/cli_init.rs`.
- #54 auditConfig: `bosun/channel/pieces/core/files/.github/workflows/bosun.yml.jinja:517`.

OBSOLETE (close with the big PR):

- #10 auto-filed lesson for main-red #9; capture exists (`bosun/src/cli/main_health.rs:69`); the analysis is #11.

Stay open (owner steps, not closable by code):

- #38 owner checklist (H2-H7). #30 B2W/T4 flips to checks mode only after the owner's ruleset
  pins (H5) and #31. #35 B2W epic stays open until #30.

Review-verdict follow-ups: #57 minors (substitution order, brace refspec, numeric workflow id)
are fixed by round 7 (528a1ec, merged here). #58 risk.rs nit and #59 session_rules nit are stale
PR-body text (obsolete). #60 has no reviewer verdict yet: its minors go to the integrator.

## WG: Guard lets agents open PRs (#64)

area: guard
shape: single

Model: opus. Branch `claude/big-guard`. Closes #64, #46. Also the #55 nit (orphaned `sleep 10`).
The bosun guard over-refusals of #64 (quoted globs, braces, data files, fetch chains) are fixed by
PR #60 (merged by IN); this item does the rest.

### Tasks

- T1: hull guard allows PR creation and names the route
  touch: hull/**
  kind: feature
  risk: high
- T2: bosun one-off refusal names the file and key; documented owner switch "agents open PRs"
  touch: bosun/src/cli/pr_oneoff.rs, bosun/tests/cli_pr_oneoff.rs, bosun/tests/cli_pr_oneoff_guard.rs, bosun/docs/operating-model.md
  risk: high
- T3: guard.mjs and proxy-ca minors from #46; session_start test kills its process group
  touch: hull/**, bosun/src/hooks/session_start.rs, bosun/tests/hook_session_start.rs
  depends: T1

### Done when

- UC-WG-1: in both template copies, `gh api -X POST repos/o/r/pulls -f ...` is allowed and `gh api -X PUT .../pulls/1/merge`, auto-merge, tags and releases stay refused (hull tests).
- UC-WG-2: a `bosun pr --one-off --touch apps/site/**` refusal's fix text names `[approvals] oneoff_allow` and the config file; with the glob in `oneoff_allow` it passes.
- UC-WG-3: `find . -delete` is judged as a write; `proxy-ca.sh` ignores PROXY_CA_UPDATE without PROXY_CA_STORE (tests).

## WB: bosun works with big PRs (epic)

area: bigpr
shape: single

Model: opus. Branch `claude/big-bigpr`. Closes #49, #50, #52 and the new epic issue. The owner
says "one big PR" and bosun supports it end to end. Gaps (file:line) are in the phase-0
analysis: bundles limited to one `single` epic (`bosun/src/cli/pr_bundle.rs:30-58`);
test-integrity and the merge gate take one task's `allow`/`touch` (`test_integrity.rs:100`,
`merge_gates.rs:271`); review-pack `task` is the first linked issue (`core/verdict.rs:148`);
drift is null if one task lacks `size:` (`review_pack.rs:131`); one model call capped at 200 KB
(`review_ci.rs:104`); verdict moves only the first task (`verdict.rs:521`); lease checks read
only the lead (`risk_check.rs:742,1579,1816`); `plan check` has no union report.

### Tasks

- T1: bundle across epics with an owner opt-in; union of touch/allow over all closed tasks in pr, test-integrity, merge gate, lease and delegation checks; every closed task changes state
  touch: bosun/src/cli/pr.rs, bosun/src/cli/pr_bundle.rs, bosun/src/cli/claim.rs, bosun/src/cli/wave.rs, bosun/src/core/wave.rs, bosun/src/cli/test_integrity.rs, bosun/src/cli/merge_gates.rs, bosun/src/cli/risk_check.rs, bosun/src/cli/verdict.rs, bosun/src/cli/review_loop.rs, bosun/src/cli/mod.rs, bosun/src/config.rs, bosun/src/schema/**, bosun/src/github/**, bosun/tests/cli_pr_bundle*.rs, bosun/tests/cli_claim_bundle.rs, bosun/tests/cli_integrity_task_allow.rs, bosun/tests/risk_bundle_gated.rs, bosun/tests/cli_big_pr_*.rs
  risk: high
- T2: review-pack and CI review scale: all closing issues, drift over sized tasks, per-chunk review with merged verdict, `not_shown` files
  touch: bosun/src/cli/review_pack.rs, bosun/src/cli/review_ci.rs, bosun/src/core/review.rs, bosun/src/core/verdict.rs, bosun/tests/cli_review_pack_*.rs, bosun/tests/cli_verdict_chunks.rs, bosun/tests/cli_review_ci*.rs
  depends: T1
  risk: high
- T3: #49 R-plan reads commit refs fail-closed (one compare call); #52 verdicts aggregated per reviewer session; #50 unresolvable wildcard touch is a warning
  touch: bosun/src/core/plan.rs, bosun/src/core/shape.rs, bosun/src/cli/plan.rs, bosun/tests/cli_plan_*.rs, bosun/tests/cli_carry_verdict.rs, bosun/tests/cli_risk_two_verdicts.rs, bosun/tests/cli_verdict_session*.rs
  depends: T1
  risk: high
- T4: docs and spec: plan-format shape, spec 04/05/07/08 (drop "under 100 lines")/12, defaults
  touch: bosun/docs/plan-format.md, bosun/docs/defaults.md, bosun/docs/spec/04-lifecycle.md, bosun/docs/spec/05-spawn.md, bosun/docs/spec/07-gates.md, bosun/docs/spec/08-risk.md, bosun/docs/spec/12-plan.md
  depends: T2, T3

### Done when

- UC-WB-1: `bosun pr` with `--bundle` over tasks of two epics writes one `Closes` per task, and every task moves with the PR.
- UC-WB-2: `test-integrity --pr` and the merge gate accept a PR whose files are covered by the union of its tasks' `touch:`, each `allow:` only on its own touch.
- UC-WB-3: review-pack of a 10k-line PR lists every closing task and chunk; CI review reviews each chunk and `chunks_reviewed` is exact.
- UC-WB-4: R-plan fails closed on an unreadable commit; two sessions' verdicts aggregate per session; a wildcard touch with no match is a plan warning.

## WT: Owner teams, verdict binding, review relay

area: trust
shape: single

Model: opus. Branch `claude/big-trust`. Closes #23, #24, #25, #28, #29, #39, and the #55 minor
(`hoard/action.yaml` missing from TRUST_ROOT, `bosun/src/core/risk.rs:1262`). T1 runs in
parallel; T2 edits WB's files and starts from `claude/big-bigpr` once WB/T1 is pushed.

### Tasks

- T1: teams in kill switch, approve, ask, notify (#24); team example and bus-factor note (#23); TRUST_ROOT `.yaml` twin; review relay workflow (#29)
  touch: bosun/src/core/risk.rs, bosun/src/core/records.rs, bosun/src/app.rs, bosun/src/cli/approve.rs, bosun/src/cli/ask.rs, bosun/src/cli/notify.rs, bosun/src/cli/carry_over.rs, bosun/governance/**, bosun/channel/pieces/core/files/governance/**, bosun/docs/spec/03-layout.md, bosun/docs/spec/16-owner-teams.md, bosun/tests/cli_owner_teams*.rs, bosun/tests/cli_risk_owner_team.rs, bosun/tests/workflow_review_relay.rs, .github/workflows/review-relay.yml, .github/workflows/risk.yml, .github/workflows/verdict.yml
  risk: high
- T2: team expansion with a per-run cache, failing closed (#25); verdict records bound to the verdict run (#39)
  touch: bosun/src/cli/risk_check.rs, bosun/src/cli/verdict.rs, bosun/src/github/**, bosun/tests/common/fake.rs, bosun/docs/spec/08-risk.md
  depends: T1, WB/T1, WB/T4
  risk: high

### Done when

- UC-WT-1: an owner team member can approve, ask, notify and use the kill switch; a team of another org is refused.
- UC-WT-2: team expansion is cached per run and an API error counts no member (fail closed).
- UC-WT-3: a verdict record not written by the verdict workflow run is not trusted.

## WH: hoist and keel follow-ups

area: hoist
shape: single

Model: sonnet. Branch `claude/big-hoist`. Closes #31, #37, #45, #47, #48 and the #58 nit (plan
text 'after build' on a skipped step, `hoist/src/ops/release/publish/registries.rs:363`). The
#45 FLY_API_TOKEN canary item is obsolete (D-023).

### Tasks

- T1: keel process-group kill, DNS text, step timeout test, newer-run pointer (#45)
  touch: keel/**
- T2: hoist DNS/R2/env minors (#47, #48), registries plan text
  touch: hoist/src/lib.rs, hoist/src/dns.rs, hoist/src/diff.rs, hoist/src/github_write.rs, hoist/src/ops/**, hoist/README.md, hoist/docs/**, hoist/tests/**
- T3: `required_checks` table form pins each check to its App (#31, #37)
  touch: hoist/src/config.rs, hoist/src/desired.rs, hoist/src/github.rs, hoist.toml, bosun/tests/governance_owners.rs
  depends: T2
  risk: high

### Done when

- UC-WH-1: keel kills the whole process group on timeout (test).
- UC-WH-2: hoist secrets check fails on a dir without files; `gh variable set` text is quoted; carry-back 403 has a test.
- UC-WH-3: `hoist.toml` `required_checks` accepts `{ name, app }` entries and the ruleset pins the app.

## WC: channel, profiles, review agreement

area: channel
shape: single

Model: sonnet. Branch `claude/big-channel`. Closes #11, #40, #41, #51, #53 and the #59 nit
(ci-minutes fix text for a block-list `on:`).

### Tasks

- T1: #51 channel nits (mixed-stack fmt clause, ci-minutes cost, realpath check); #53 rust profile resolves cargo-deny and cargo-fmt by path; ci-minutes block-list fix text
  touch: bosun/channel/pieces/*/files/.github/**, bosun/channel/pieces/circleci/**, .github/workflows/bosun.yml, bosun/profiles/**, bosun/docs/ci-minutes.md, bosun/src/core/ci_minutes.rs, bosun/src/cli/ci_minutes.rs, bosun/tests/workflows.rs, bosun/tests/channel_render.rs, bosun/tests/*profile*.rs, bosun/tests/cli_ci_minutes*.rs
- T2: mode (a) vs (b) agreement on 20 recorded PRs (#40, closes #41)
  touch: bosun/tests/review_agreement.rs, bosun/tests/fixtures/reviews/**
- T3: #11 root cause of main-red #9 (run 45128620888), the gate that misses it, and a test
  touch: bosun/src/gates/**, bosun/tests/cli_gate_main_red.rs

### Done when

- UC-WC-1: rendered channel workflows check the restore path with `realpath -m`; the rust profile calls cargo-deny and cargo-fmt by resolved path.
- UC-WC-2: the agreement test runs on 20 recorded PRs.
- UC-WC-3: the gate missed in #11 now fails the recorded case.

## IN: Integrate and open the one PR

area: release
shape: single

Model: opus. Runs after every item above is pushed.

### Tasks

- T1: merge `claude/bosun-guard-feedback` (PR #60, plus its verdict minors) and `claude/big-guard`, `claude/big-bigpr`, `claude/big-trust`, `claude/big-hoist`, `claude/big-channel` into `claude/big-pr` (git merge, no rebase); resolve conflicts
  touch: **
  depends: WG/T3, WB/T4, WT/T2, WH/T3, WC/T3
  risk: high
- T2: `BUMP_DATE=<date> bash scripts/bump-version.sh 0.22.0`; full CI mirror (fmt, clippy and test --workspace, hull, site, decisions); open ONE PR with `Closes #N` for every DONE, OBSOLETE and workstream issue
  touch: **
  depends: T1
  kind: bookkeeping
  risk: high

### Done when

- UC-IN-1: one PR from `claude/big-pr` has green `ci-ok` and closes #6 #10 #11 #12 #13 #14 #15 #16 #17 #18 #19 #20 #21 #22 #23 #24 #25 #26 #27 #28 #29 #31 #32 #33 #34 #36 #37 #39 #40 #41 #42 #43 #44 #45 #46 #47 #48 #49 #50 #51 #52 #53 #54 #64 and the big-PR epic; #30 #35 #38 stay open.
