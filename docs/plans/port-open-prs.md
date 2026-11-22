# Port the old repos' open PRs, then archive the old repos

Status: plan, 2026-11-21, made from keelstack main 2f0bef7. Each verdict compares the PR head
diff (merge-base..head, or parent..head for a stacked PR) with `<folder>/` in main. No PR
applies to main cleanly in either direction, so none of them is already in main.
Method: `git fetch` of `refs/pull/*/head` (anonymous git; the REST API of the old repos was
not reachable from the session). CI and review state come from the PR pages. The PR pages
show no CI status unless the text says so.

Scope: 34 open PRs: bosun 13, splice 2, hull 1, hoist 11, hoard 2, berth 3, decisions 1,
site 1 (#69), keel 0. Open issues: 76 in all (bosun 50, hull 7, hoist 6, splice 4, hoard 4,
site 3, berth 1, keel 1, decisions 0).

Verdicts: PORT 9, PARTIAL 2, NEEDS-OWNER 13 (Q1 decides 4 of them, Q2 decides 9), OBSOLETE 10.

## 1. All PRs

| PR | Change | State | Verdict and reason |
| --- | --- | --- | --- |
| bosun#705 | `[approvals] policy="quorum"`: orchestrator signature plus 2 verdicts from different sessions, trust-root list, digest (+1558/-60) | bot verdict request-changes 0.92 (2 blocker, 6 major; issue bosun#706) | PORT (WP6). It is the D-022 code, and main has no quorum policy. Rework: the trust root re-roots under `[[folders]]`; the `.bosun/version` pin and tag lookup go to keelstackhq/keelstack (`unreleased`, `vX.Y.Z`); fix the bot findings |
| bosun#702 | bootstrap fixes: gate-neutral bootstrap PR, tokenless init/doctor/plan/prereqs/pr, one pins source, channel changes (+4106/-204) | last verdict approve 0.90; ci-ok was red earlier (pins test) | PORT (WP7, in 2 parts). Onboarding of other repos is still needed. Remap `.bosun/pins.toml` and `risk.toml` to the root `.bosun/`; channel source `gh:keelstackhq/keelstack//bosun/channel` |
| bosun#672 | T13b: `bosun merge` (no CI) runs gates last in a sealed worktree with a fresh HOME/CARGO_HOME (+825/-94) | no verdict | PORT (WP3). D-019 no-CI merge stays under D-022 |
| bosun#678 | T13c: gate helper git/cargo calls run in the same isolated env (+34/-20), on #672 | no verdict | PORT (WP3) |
| bosun#676 | settings-lock: deny agent edits of `.claude/settings*.json`, `~/.claude/**`, `disableAllHooks` (+613/-8), on #642 | no verdict | PORT (WP4) on main, without #642. It protects the root `.claude/settings.json` |
| bosun#643 | T7: `setup-snippet --install-hooks`, session-start finds un-adopted repos, `.bosun` dir lock (+989/-13) | no verdict | PORT (WP5). The adopted check must accept the root `.bosun/` and `version = unreleased` |
| bosun#679 | T7b: adopted check ignores replace refs, cache keyed by origin URL (+72/-6), on #643 | no verdict | PORT (WP5) |
| bosun#642 | T6: PreToolUse chat-lock against forged chat records (+658/-4) | no verdict; stale against old main | NEEDS-OWNER (Q1). Needed only while a chat yes still counts as an approval |
| bosun#680 | T6b: child-flag tamper follow-up (+68), on #642 | no verdict | NEEDS-OWNER (Q1), together with #642 |
| bosun#673 | T14: opt-in `chat_signed` approvals with an enrolled owner key (own delta about 1.5k lines) | no verdict | NEEDS-OWNER (Q1). Likely OBSOLETE: D-022 removes owner chat approvals except for the trust root |
| bosun#644 | T12: `intake verify --chat` counts the intake chat record (+1076/-30), on #673 | no verdict | NEEDS-OWNER (Q1). Likely OBSOLETE |
| bosun#641 | T11: docs for chat approval, chat_signed and no-CI merge (+57/-12), on #644 | no verdict | PARTIAL (WP8). Port only the no-CI `bosun merge` text. The chat text describes forms that D-022 retired |
| bosun#472 | bot budget reports for the old repo (`reports/budget/2026-11.*`) | bot PR | OBSOLETE. Generated data. The root `budget-report.yml` writes `reports/budget/` now |
| splice#54 | piece discovery: suggestions for an unknown piece, `splice pieces [--json]` (18 files, about +677) | bot verdict approve 0.85; main-open was red then | PORT (WP2). One README hunk conflicts (URL moved to keelstack). Drop the "rule 32 ... (D-003)" sentence: D-023 ended D-003. Re-review under D-022 |
| splice#19, hull#82, hoard#19, berth#10, site#69 | splice update bot: splice 0.2.0 to 0.6.0, re-renders `docs/agents/org-decisions.md` (table ends at D-020) | bot PRs | OBSOLETE. The folders have no `.splice/` and the bot was not ported (MIGRATION.md). site#69 would remove D-021 to D-024 |
| hoist#62 | `[github] status_checks`: verify required checks posted as commit statuses (+218/-2) | ci-ok red: macOS runner cancelled | PORT (WP1). Independent; conflicts in config.rs, desired.rs, tests/change.rs, README |
| hoist#63 | changesets design and plan docs (+476) | none | NEEDS-OWNER (Q2) |
| hoist#64 | CS-1: read and write the changesets format, `src/changeset/` (+761) | risk bot: high (R-gate src/lib.rs, old rules) | NEEDS-OWNER (Q2), then PORT (WP9) |
| hoist#65 | CS-2: `hoist changeset add` and `init` (+772), on #64 | none | NEEDS-OWNER (Q2), then PORT |
| hoist#66 | CS-3a: dependency kinds and ranges (+330/-33), on #64 | none | NEEDS-OWNER (Q2), then PORT; release/mod.rs conflicts with main's no_crates |
| hoist#67 | CS-3: plan versions from changesets (+981/-12), on #66 | none | NEEDS-OWNER (Q2), then PORT |
| hoist#68 | `changeset init --from-commits` (+526), on #65 | none | NEEDS-OWNER (Q2), then PORT (low priority) |
| hoist#69 | CS-5: `hoist changeset status`, on #67 | none | NEEDS-OWNER (Q2), then PARTIAL: take only status.rs and its cli/lib lines. Its delta repeats #65 |
| hoist#70 | CS-3b: multi-comparator ranges, dev-only range rewrites (+258/-20), on #67 | none | NEEDS-OWNER (Q2), then PORT |
| hoist#71 | CS-4: `release prepare` consumes changesets and writes the changelog, on #70 | none | NEEDS-OWNER (Q2), then PORT with extra review. `release prepare` is the path consumers use (hoist/release/action.yml, hull template release.yml) |
| hoist#85, berth#22 | release PRs keel-hoist@0.6.0, keel-berth@0.2.2 | release bot | OBSOLETE. Lockstep 0.21.0; versions change only through `scripts/bump-version.sh` (D-023) |
| hoard#45 | pin bosun 0.21.0 in hoard's own `.bosun/` | none | OBSOLETE. One root `.bosun/` (`version = unreleased`) |
| berth#24 | adopt bosun: per-repo `.bosun/`, 5 workflows, governance | none | OBSOLETE. The root `.bosun/` and root CI cover berth |
| decisions#19 | align D-011, D-014, D-017 with D-018/D-019 (base `claude/chat-approvals`) | none | PARTIAL (WP8). Port only the D-011 sentence: `provider = "none"` repos merge only through `bosun merge` (D-019). D-017 is superseded and D-022 overrides the D-014/D-018 edits |

## 2. Port plan: one port PR, built from work packages

The port lands as one PR, `claude/port-impl`, opened from main. D-021 sets no line cap, D-007
allows one push per PR, and the closing comments in section 4 link this PR. Each work
package (WP) has its own session and sub-branch. The orchestrator merges the sub-branches into
the port branch in the order below, then runs the full checks once.

Port method: `git apply --3way --directory=<folder>` of the PR's own delta (bare clones and
diffs: `git fetch https://github.com/keelstackhq/<repo> refs/pull/<n>/head`). Next, fix the
conflicts and rewrite the old-repo references (`gh:keelstackhq/<tool>`, per-repo `.bosun/`,
per-tool versions, D-003). Commit the result as one commit per source PR, with the trailer
`Ported-from: keelstackhq/<repo>#<n>`. Do not copy `changelog.d/` or `progress.d/` entries
unchanged. Rewrite them for the monorepo changelog.

Common checks (AGENTS.md): `cargo fmt --all --check`. The folder checks are listed with each WP.
keel calls bosun, splice and hoist, so a WP in one of these also runs
`cargo test -p keel-cli --locked`. A WP in splice also runs the hull checks.

| WP | Source PRs (order) | Folder, main files | Size | Risk (.bosun/risk.toml) | Checks |
| --- | --- | --- | --- | --- | --- |
| WP1 | hoist#62 | hoist: config.rs, desired.rs, repo/workflows.rs, verify.rs, tests/{change,cli_check}.rs, README, spec | S/M | normal (no rule hit) | `cargo clippy -p keel-hoist --all-targets --locked -- -D warnings`, `cargo test -p keel-hoist --locked`, keel-cli tests |
| WP2 | splice#54 | splice: src/ops/pieces.rs (new), cli.rs, lib.rs, manifest.rs, output.rs, ops/{add,update,mod}.rs, tests/cli_pieces.rs, README, docs/reference.md, skills/splice/SKILL.md | M | normal; the contract file is not touched | `cargo test -p keel-splice --locked`, clippy, `pnpm --dir hull test`, keel-cli tests |
| WP3 | bosun#672 then #678 | bosun: cli/{merge,merge_gates,pr_no_ci,gate,stamps,test_integrity,mod}.rs, core/risk.rs, risk_default.toml, gates/{mod,integrity,packages}.rs, tests/cli_merge.rs, tests/common/fake.rs; bosun's `.bosun/risk.toml` hunk moves to the root `.bosun/risk.toml` | M | R-gate (delegate); R-gov for the root risk.toml hunk | `cargo clippy -p keel-bosun --all-targets --locked -- -D warnings`, `cargo test -p keel-bosun --locked`, keel-cli tests |
| WP4 | bosun#676 (on main) | bosun: core/guard.rs, core/guard/{interp,settings,writes}.rs, hooks/pre_tool_use.rs, tests/cli_hook.rs, docs/spec/08-risk.md | M | R-gate | same as WP3 |
| WP5 | bosun#643 then #679 | bosun: cli/{setup_snippet,doctor,mod}.rs, setup_bootstrap.sh, hooks/{session_start,pre_tool_use,mod}.rs, core/guard.rs, guard/writes.rs, schema/data.rs, lib.rs, about 15 test files, spec 11 | M | R-gate | same as WP3 |
| WP6 | bosun#705 plus the fixes from issue bosun#706 | bosun: core/{risk,approve,intake,mode}.rs, cli/{approve,risk_check,digest,adopt,doctor,mod}.rs, config.rs, github/mod.rs, schema/data.rs, channel/splice.toml, channel/.../bosun.toml.jinja, tests/cli_quorum.rs and 6 more, spec 08/14/18 | L | R-gate, R-ci-review (channel/**). Trust root: the owner sets `[approvals] policy = "quorum"` in the root `.bosun/` (Q3) | same as WP3, plus `cargo test -p keel-bosun --test channel_render --locked` (needs splice) |
| WP7a | bosun#702, gates part | bosun: gates/{mod,integrity}.rs, risk_default.toml, the base_requires and template-test parts | M | R-gate | same as WP3 |
| WP7b | bosun#702, rest | bosun: cli/{init,plan,doctor,prereqs,pr_single}.rs, core/prereqs.rs, channel/pieces/{core,circleci}/**, profiles/rust.toml, docs/setup-script.sh, spec 03/04/07/11/12/14 | L | R-gate, R-ci-review (channel/**), R-gov (bosun's `docs/setup-script.sh` and `.bosun/*` hunks: map them to the root or drop them) | same as WP6, plus `pnpm --dir site run ci` (the site reads bosun's setup script and channel facts) |
| WP8 | bosun#641 (no-CI merge text only), decisions#19 (D-011 sentence only) | bosun: README.md, AGENTS.md, docs/adopt-existing-repo.md; decisions: DECISIONS.md D-011 row, decisions/011-ci-providers.md | S | R-gov (decisions/**, **/agents.md) | `bash decisions/scripts/check-index.sh`, `pnpm --dir site run ci` (the site shows decisions) |
| WP9 (if Q2 = yes) | hoist#63, #64, #65, #66, #67, #70, #71, #69 (status only), #68 | hoist: src/changeset/** (new), cli.rs, lib.rs, git.rs, release/{mod,planner,changelog,edit,gitlog}.rs, ops/release/{plan,prepare/files,bootstrap}.rs, launcher, about 8 tests, docs/superpowers/{specs,plans} | L | normal, but give it 2 reviews: `release prepare` is the consumers' release path | WP1 checks, plus `pnpm --dir hull test` (the template's release.yml calls hoist) |
| WP10 (if Q1 = keep) | bosun#642, #680, then #673, #644 | bosun: core/guard.rs, hooks/pre_tool_use.rs, tests/cli_hook.rs, cli/approve_chat.rs, core/approve.rs, cli/{risk_check,intake_file}.rs, config.rs | L | R-gate | same as WP3 |

Dependency order and parallel sessions (overlaps measured on the own-delta file lists):
- Wave A, 4 parallel sessions with no shared files: WP1 (hoist), WP2 (splice), WP3 (bosun
  merge and gates), WP4 (bosun guard). WP3 and WP4 share only `docs/spec/04-lifecycle.md`,
  which WP4 does not change in substance. Keep that hunk in WP3.
- Wave B: WP5, after WP4 (shared files: guard.rs, guard/writes.rs, pre_tool_use.rs,
  cli_hook.rs) and WP3 (shared file: cli/mod.rs). WP8 can run beside it (docs only).
- Wave C: WP6, after Q3 and after WP3 and WP5 (shared files: core/risk.rs, cli/mod.rs,
  doctor.rs, schema/data.rs, tests/common/fake.rs).
- Wave D: WP7a after WP3 (gates/mod.rs, integrity.rs, risk_default.toml), then WP7b after WP6
  (doctor.rs, mod.rs, data.rs, github/mod.rs, channel_render.rs).
- WP9 can start after Q2, beside any bosun wave. Port it serially: every CS PR edits
  changeset/mod.rs and lib.rs. Main 2f0bef7 changed hoist/src/{cli,lib}.rs, so expect small
  conflicts there. WP10 goes after WP5 and WP6, and only if Q1 keeps the chat form.
- Hot spots: bosun `src/cli/mod.rs` and `src/schema/data.rs` (WP3, WP5, WP6, WP7). Spec docs
  04 and 08 go last in each WP.
- Before the push: the full `ci-ok` mirror for bosun, splice, hoist, keel, hull and site
  (D-007). Then two verdicts on the exact head and the orchestrator merges (D-022).

## 3. Owner questions (one batch)

1. Chat approvals under D-022: retire the D-018 chat yes everywhere? Then bosun#642, #680,
   #673 and #644 are OBSOLETE, and a follow-up removes `approve --chat` counting from
   risk_check. Or keep the chat yes only for trust-root PRs? Then WP10 ports the chat-lock
   guard (#642, #680), and the owner also decides whether `chat_signed` (#673/#644) is wanted.
2. hoist changesets: keep them as an opt-in feature for consumer repos? The monorepo stays
   lockstep (D-023), and with no `.changeset/` the feature does nothing here. If yes: WP9 and
   one line in the decisions record ("changesets are for consumer repos only"). If no: close
   hoist#63 to #71 as OBSOLETE.
3. Trust root: after WP6 lands, may the root `.bosun/` be switched to
   `[approvals] policy = "quorum"`? Is the new trust-root path list (re-rooted under
   `[[folders]]`) approved? Only the owner can change these (D-022).
4. Issue transfer: transfer all 76 open issues? Or close the generated ones (fix-task "review
   findings on PR #N" for old PR numbers, "main is red", "Which gate missed this?", "lesson:
   main went red", "bosun registry") and transfer only the plan and bug issues (about 43:
   bosun M1-M8/B2/B2W/B4 plans, #698, #696, #298, #238, #235, #236, #245; hull#133; hoist#95,
   #97, #98; keel#4)?
5. bosun#472 budget history: copy the old repo's November report into
   `reports/budget/old-bosun/`, or let it go with the archive?

### Owner answers (orchestrator chat, 2026-11-21)

- Q1: yes, retire the chat yes everywhere (D-025). bosun#642, #680, #673 and #644 are OBSOLETE;
  WP10 is dropped.
- Q2: yes, keep changesets as an opt-in feature for consumer repos. WP9 runs.
- Q3: "yes, switch the root .bosun to quorum inside the big PR". The root `.bosun/bosun.toml`
  sets `[approvals] policy = "quorum"` in the port PR.
- Q4: transfer only the plan and bug issues (about 43); close the generated ones.
- Q5: open.

## 4. Archive plan for the old repos

Do this after the port PR merges and after the first monorepo release (> 0.21.0) exists.
Every step is an owner or orchestrator action. Child sessions only prepare the text.

1. Issues: transfer the issues that Q4 keeps to keelstackhq/keelstack
   (`POST /repos/keelstackhq/<tool>/issues/<n>/transfer`, `new_owner=keelstackhq`,
   `new_name=keelstack`). Give each one the label `folder:<tool>` (create labels
   `folder:bosun` ... `folder:site` first) and keep its existing labels where they exist in
   keelstack (area/*, kind/*, state/*). Close the other issues with: "Closed by the move to
   keelstackhq/keelstack (D-023): generated for this repo's CI/PRs; no longer applies."
2. Open PRs: close each one with one comment. Ported: "Ported to keelstackhq/keelstack#<port
   PR> as `<folder>/` (D-023)." Obsolete: the reason from section 1. NEEDS-OWNER: close it
   only after the owner answers, and link the answer.
3. README: replace each repo's README.md with its pointer text from `MIGRATION-ARCHIVE.md`.
   This is the only commit to an old repo, made by the owner or an orchestrator with access.
   Then archive.
4. Archive order: the repos that nothing outside uses first, then the ones consumers still
   pin:
   berth, keel, site (the landing site has deployed from `site/` since #2), decisions
   (`gh:keelstackhq/decisions//channel` still renders from the archived repo), splice, hull,
   hoist, hoard, and bosun last (its PR stacks need the longest time to port).
5. What must stay live (archiving keeps all of it readable, so do not delete, rename or make
   private any old repo, and keep every old release and tag):
   - GitHub Action consumers: `keelstackhq/hoard@v0`/`@v1`, `keelstackhq/splice/action@v1`,
     `keelstackhq/hoist/secrets@v0` and `hoist/release`. Archived repos still serve their
     tags, but `v0`/`v1` can no longer move. Before the archive, the docs and templates point
     to `keelstackhq/keelstack/<tool>@vX` (hoard's docs still name `keelstackhq/hoard@v0`).
   - Release downloads: `github.com/keelstackhq/{hoard,bosun,splice}/releases/download/...`
     (install scripts, pins <= 0.21.0, `site/apps/site/src/data/versions.ts`). These stay
     until the site and templates move to the first monorepo release.
   - splice sources: `gh:keelstackhq/hull//template`, `//template-rust`, `//rigging`,
     `gh:keelstackhq/decisions//channel`, `gh:keelstackhq/bosun//channel` stay renderable
     (D-023).
   - Packages: every crate and npm package already names
     `repository = "https://github.com/keelstackhq/keelstack"` in main. Packages that were
     already published (<= 0.21.0) keep the old URLs. That is acceptable, because GitHub keeps
     archived repos.
   - Old-repo secrets, environments (hoard-r2) and Apps: remove them only after the matching
     root workflow has run once in keelstack.
