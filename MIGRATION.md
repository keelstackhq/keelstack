# Migration to one monorepo

Decision D-023 (`decisions/decisions/023-one-monorepo.md`) moves every keelstackhq repo into this one.
Each repo was imported with its full history into its own folder (`git filter-repo
--to-subdirectory-filter <folder>`, then `git merge --allow-unrelated-histories`). Tags were not
imported.

The final re-sync merges each source repo's commits after the imported SHA below.

| Folder | Source repo | Imported SHA (main) |
| --- | --- | --- |
| bosun | https://github.com/keelstackhq/bosun | b8029e16de95a5883cc451f7e9266c024c7690ae |
| splice | https://github.com/keelstackhq/splice | 1c8b33b751bfdb242daccc7fe26eb7d9e70afb47 |
| hull | https://github.com/keelstackhq/hull | baab2ff26a0943f5be034a78014bc4a5ed40a3fa |
| hoist | https://github.com/keelstackhq/hoist | cb200b5369ac0ef55bdc6a47ebe7f2553be6e82b |
| hoard | https://github.com/keelstackhq/hoard | 3b4b96bca151c241050b06752d407d243cd3593a |
| berth | https://github.com/keelstackhq/berth | 5d1db8191bf8e46937f18a30a2e70a3be7e4a734 |
| keel | https://github.com/keelstackhq/keel | e34f177359f889b2482814f5b611ec90c8fe9339 |
| decisions | https://github.com/keelstackhq/decisions | 5b05c178f0a27d93e06d93089afb330e97bb1243 |
| site | https://github.com/keelstackhq/site | d43156f4161b75218da5287af56f349663cc03f5 |

Re-sync check 2026-11-20: every source repo's main still equals its imported SHA (nothing to bring in).

## Changes made by the move

- One Cargo workspace at the root (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
  `deny.toml`, `.cargo/config.toml`); the per-folder copies were deleted.
- All Rust crates share one lockstep version (`[workspace.package] version = "0.21.0"`, the
  highest tool version at the move). The npm launcher packages follow it. Nothing was released.
- Release profiles moved to the root (cargo ignores member profiles): one profile for every
  binary (panic = "unwind", fat LTO, one codegen unit, stripped; dependencies at opt-level 3,
  bosun, splice and berth code at "s"). This changes the berth and splice binaries, which used
  panic = "abort" and thin LTO; nothing depended on the abort.
- splice no longer needs serde_json `preserve_order`: workspace feature unification would have
  turned it on for every tool and changed their JSON key order. splice keeps key order with its
  own ordered value type (`splice/src/merge/value.rs`).
- Root CI and governance workflows (`.github/workflows/`) replace the per-repo ones. bosun's,
  splice's, keel's and decisions' `.github/` were deleted, and so were most per-repo workflows of
  the others. The files still under `berth/`, `hoard/`, `hoist/` and `hull/` `.github/workflows/`
  do not run here: tests still read them, or they are not ported yet (below).
- One root `.bosun/` (bosun's config, `[[folders]]` for every folder) and root
  `governance/owners.toml`, `REVIEW.md`, `docs/lessons.toml` and `.claude/settings.json` (bosun's
  hooks and deny rules). Every per-folder `.bosun/` was deleted; the old risk rules of each folder
  apply re-rooted under it in the root `risk.toml`. `bosun/governance/` stays: bosun's tests read it.
- Tags were not imported. `hull/legacy-tags.txt` maps hull's old release tags to their commits
  here (hull's update tests use it).

## Follow-ups (not done by the move)

- Ported, manual runs only (hoard's also weekly; Linux only; not run yet): splice's action
  self-test (`.github/workflows/splice-action-selftest.yml`) and hoard's self-test
  (`.github/workflows/hoard-self-test.yml`; its R2 legs need environment `hoard-r2`). Deleted with
  their scripts, because the root `release.yml` does their job: splice's `release.yml` and its
  `release-tag`/`release-major` scripts (their tests, `splice/scripts/release-tag.test.mjs` and
  `release-major.test.mjs`, stay and test the root `release.yml`); hoard's `bench.yml` and
  `r2-checks.yml`.
- Not ported: the splice update bot (hull's and bosun's `splice.yml`; its hull test is gone).
  main-health's weekly job covers only the Rust workspace.
- Tests that still read an old per-folder release workflow: hoard
  `action/test/release-workflow.test.ts` (keeps `hoard/.github/workflows/release.yml`) and berth
  `tests/release_config.rs` (keeps `berth/.github/`). Port them to the root `release.yml`, then
  delete the old files.
- `.bosun/version` is `unreleased`: bosun runs from the workspace. `[[folders]]` is known from
  bosun 0.22.0; pin a release only once one > 0.21.0 exists. Until then a main that does not build
  blocks its own fix (the gates fail closed; see `.github/SETUP.md`, "Main does not build").
- `hoist init` without `--sha` fails until the first release > 0.21.0 exists. A keel pin of a
  monorepo hull tag needs `repo = "keelstackhq/keelstack"` in `[refs.hull]`.
- hull's `affected-combos.mjs` sees only hull's paths, so a root workflow change selects no combos.
- bosun's channel templates and `bosun/CLAUDE.md` still describe a standalone bosun repo.
- Templates rendered into other repos (`hull/template/**`, `bosun/channel/**`) keep pins to the
  old repos' last releases until the first monorepo release (> 0.21.0) exists.

## Site (D-024)

`site/` was imported from keelstackhq/site (`git filter-repo --to-subdirectory-filter site`, merge with
`--allow-unrelated-histories`; branch `claude/monorepo-site`). Its own `.github/`, `.bosun/`, `.claude/`,
`.splice/`, `governance/`, `renovate.json`, turbo and `scripts/setup-release-tag.sh` were deleted: the
root ones replace them. Not ported: docspack's npm release workflow (`packages/docspack` still
releases by hand until it is) and the two docspack tests that read it. The docs pages keep their pins
to the old repos' last releases (`site/apps/site/src/data/versions.ts`) until they move onto the
generated facts. The Cloudflare secrets move from the old repo's environment `production` to the
environment `site` here (`.github/SETUP.md`).

## Trust files in the monorepo (D-022)

A PR that changes a trust file (`TRUST_FILES`, and `TRUST_CODE_FILES` under each of bosun's code
roots, in `bosun/src/core/approve.rs`) never passes on a signed approval (owner, delegate or the
orchestrator's key) and never carries an approval over a merge of main (carry-over rule 4 uses
the same code-root-aware list). Only the owner's own GitHub review or comment counts. The list
also holds the code and files that apply it, because a signed PR that weakened one of them would
have the next PR judged by the weakened copy:

- `bosun/src/cli/risk_check.rs` (runs the check and reads the code roots),
  `bosun/src/core/folders.rs` (re-roots the rules and the floor under a folder) and
  `bosun/src/core/glob.rs` (matches every rule and trust path), next to `bosun/src/core/risk.rs`
  and `bosun/src/core/approve.rs`;
- the code that sets and applies the verdict quorum: `bosun/src/core/verdict.rs`
  (`APPROVE_MIN`), `bosun/src/cli/verdict.rs`, `bosun/src/core/carry.rs` and
  `bosun/src/cli/carry_over.rs` (the approval carry-over);
- `.bosun/risk.toml` (the rules risk-check reads from the base) and
  `.github/workflows/risk.yml`, `verdict.yml`, `signer.yml`, `main-open.yml` and `review.yml`
  (they build and run risk-check, the verdict, the signer, the main-open required check and
  the review).

The claim is narrowed to this: the trust list covers the files that hold or apply the trust and
quorum rules. Dependency manifests (`Cargo.toml`, `Cargo.lock`, which pick the ed25519 crate)
are owner-approved under R-deps, not trust files, so a dependency bump needs no owner signature.

`.bosun/bosun.toml` is a trust file as a whole (for its `[approvals]` table), so every edit of it,
also `[[folders]]` (which names the code roots) or `[slack]`, is owner-signed only. D-022's own
file is the trust root and is not changed by this.

## Quorum policy at the root (D-022)

The root `.bosun/bosun.toml` sets `[approvals] policy = "quorum"` (owner answer Q3 in
`docs/plans/port-open-prs.md`; this PR is trust root, so the owner approves it). Under it the
orchestrator's signature plus two clean verdicts from different reviewer sessions (and different
relayers) pass every PR except the trust root, which bosun re-roots under each code root
(`bosun/` here): the trust files above, `.bosun/approvers.toml`, `.bosun/bosun.toml`,
`.bosun/risk.toml`, the risk, verdict, signer, main-open, review and release workflows,
`.claude/settings*.json`, and under `bosun/` every file of `src/` and `profiles/`, its
`Cargo.toml` and launchers (`bosun/src/core/risk.rs` `QUORUM_CODE_FILES`, spec §8). Every
`.github/workflows/**` and `.github/actions/**` change is trust root in every `[checks]` mode. `.bosun/version` stays `unreleased`: moving it to the first
monorepo release is the owner's (the pin rule fails closed from an unreleased base).
