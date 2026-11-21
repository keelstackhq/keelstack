# Owner setup for the monorepo

keel and hoist do the one-time repository setup from the root `keel.toml` and `hoist.toml`.
Nothing here is clicked by hand except the steps keel prints as its checklist (D-009).

## Prerequisites (computer)

1. `gh auth login` as a user with **admin** on `keelstackhq/keelstack` (`gh auth status`).
2. A Cloudflare API token with Pages Write and Account API Tokens Write, and the account id, in the
   environment: `export CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ACCOUNT_ID=...` (never on a command
   line you keep in history; `read -rs CLOUDFLARE_API_TOKEN; export CLOUDFLARE_API_TOKEN` works).
   Without them keel skips Cloudflare and lists it in the checklist.
3. keel and hoist from this checkout: `cargo build -p keel-cli -p keel-hoist --locked &&
   export PATH=$PWD/target/debug:$PATH`.

## One command

```sh
keel plan        # read-only (GET only): every item below, what would change, the checklist
keel up          # applies everything hoist can write, then prints the checklist; exit 5 while it is not empty
```

Re-run `keel up` after each checklist step: it re-checks and resumes. A step hoist cannot read
back (a trusted publisher) is marked done with `keel up --confirm <id>`.

## The checklist keel prints

Computer steps first, then phone steps. Typical first run:

| Where | Step id | What |
| --- | --- | --- |
| computer | `cloudflare-token` | only when the prerequisite 2 is missing |
| computer | `variable:BOSUN_APP_CLIENT_ID` | `gh variable set BOSUN_APP_CLIENT_ID --repo keelstackhq/keelstack --body '<client id of keelstackhq-bosun-gate>'` |
| computer | `secret:<NAME>` | `hoist secrets set <NAME>`: value from env `<NAME>` or a hidden prompt, sealed into its environment (never argv, files or logs). `BOSUN_APP_KEY` (bosun-app), `BOSUN_SIGNER_KEY` (bosun-signer), `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_BUCKET` (hoard-r2); `ANTHROPIC_API_KEY` (review) only with `BOSUN_REVIEW_CI = true` |
| computer | `first-publish:<crate>` | `hoist release bootstrap` publishes every crate not on crates.io yet (trusted publishing cannot create a crate) |
| phone | `app:keelstackhq-bosun-gate` | install `keelstackhq-bosun-gate` on the repo: https://github.com/apps/keelstackhq-bosun-gate/installations/new |
| phone | `crates-trust:<crate>` | trusted publisher on `keel-bosun`, `keel-hoist`, `keel-berth`, `keel-cli`: repository `keelstackhq/keelstack`, workflow `release.yml`, environment `release` (https://crates.io/crates/<crate>/settings) |
| phone | `npm-trust:<package>` | trusted publisher on every npm package: the bosun, splice, hoist, berth and keel launchers and their platform packages (workflow `release.yml`), the cargo-deny packages (workflow `cargo-deny-npm.yml`); environment `release` (https://www.npmjs.com/package/<package>/access). npm provenance needs a public repository |

`BOSUN_REVIEW_CI` stays unset (review.yml off) until bosun's `[review] mode = "ci"`; then
`gh variable set BOSUN_REVIEW_CI --repo keelstackhq/keelstack --body true` in the same change.

## Appendix: what `keel up` does

- `preflight`: `gh` logged in with admin on the repo, `origin` is the repo, `hoist` runs.
- `settings`: `hoist check`, then `hoist apply --yes` (all areas; without the Cloudflare token
  every area but `cloudflare`). From `hoist.toml` and inference hoist writes:
  - ruleset `hoist: default branch` on `main`: required checks `ci-ok`, `risk-check`,
    `review-verdict`, `main-open` (`ci-ok` aggregates the per-folder `ci-*` jobs; do not require
    them one by one), merge queue off (`merge_queue = false`: bosun posts `risk-check`,
    `review-verdict` and `main-open` on the PR head, never on a merge-group commit, so a queue
    would hold every PR), plus the rules `deletion` (main cannot be deleted) and
    `non_fast_forward` (no force-push to main), and one bypass actor: the repository admin role
    in `pull_request` mode (an admin may merge a pull request past the missing gate statuses, for
    the mainfix path below, but never pushes to main directly);
  - ruleset `hoist: tags`: creation, update and deletion of `v*` tags (`vX.Y.Z` and the major
    tag `v0`) are blocked for everyone but the bypass actor, the GitHub Actions integration as a
    whole. That is wider than release.yml: any workflow job whose token has `contents: write`
    can create, move or delete `v0`, which every `keelstackhq/keelstack/<tool>@v0` consumer runs.
    Accepted risk (no separate release App): today only release.yml (jobs `release` and `major`)
    and budget-report.yml (job `report`, for its reports branch) ask for `contents: write`; the
    repository default token is read-only; every workflow file is R-ci in `.bosun/risk.toml`
    (`.github/workflows/**`, owner only and on bosun's critical floor) and owned by the owner in
    `.github/CODEOWNERS`, so no workflow gains or uses `contents: write` without the owner's
    approval. `hoist apply` updates of `hoist: tags` keep owner-added includes, rules and bypass
    actors (hoist reads, merges, writes);
  - environments `release`, `bosun-app`, `bosun-signer`, `review`, `hoard-r2`, `site`: default
    branch only, no reviewers (D-022);
  - repository settings (auto-merge, delete branch on merge) and Actions permissions (read);
  - Cloudflare Pages project `keelstack-landing` (`site/hoist.toml`; deploy-site.yml builds and
    deploys `site/apps/site/dist`), a minted deploy token sealed with the
    account id as `CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` into environment `site`.
    deploy-site deploys only a released version: a version-bump push skips the deploy (job
    `decide` finds no tag `vX.Y.Z` yet) and the release run's completion deploys it; a manual
    run deploys only from main. Pull requests build and test the site in ci.yml (`ci-site`);
    deploy-site has no `pull_request` trigger. Only its `Check secrets` and `Deploy to
    Cloudflare Pages` steps receive `CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID`;
  - reads only: app installation, environment secrets and variables (present or not), crates.io and
    npm packages (first publish needed or not).
- Release: no secrets; crates.io and npm use trusted publishing (OIDC). Environment `release`
  has no required reviewer (D-022), so anyone with write access can dispatch release.yml or
  cargo-deny-npm.yml (an npm publish of the cargo-deny packages) from main; the environment's
  default-branch-only rule keeps both on main. Retired with the old
  repos: `BOSUN_RELEASE`, `BERTH_RELEASE`, `HOIST_RELEASE_TOKEN`, the `release-pr` environment.

## Main does not build

The gate workflows (risk, verdict, review, signer, main-open, main-health) build bosun from the
checked-out base (pull_request_target) or the default branch head. If that build fails, the gate
fails closed with an `::error::` (no status is posted, so the required check stays missing) and
`.github/scripts/gate-workflows.test.sh` (ci-repo) keeps it that way. There is no pinned-release
fallback yet: no monorepo release exists (`.bosun/version` is `unreleased`), and the old
`keelstackhq/bosun` releases (<= 0.21.0) predate `[[folders]]` and would judge the monorepo wrongly. The
fix path: open a `bosun:kind/mainfix` PR that makes bosun build on main (usually `bosun revert
<pr>` of the breaking merge); the owner, as repository admin, merges that pull request past the
missing gate statuses (the default-branch ruleset's admin bypass in `pull_request` mode; a direct
push or force-push to main stays blocked),
then the gates run again from the fixed main. Once a monorepo release (> 0.21.0) exists and
`.bosun/version` names it, the workflows can fall back to that pinned release again.

## Follow-ups (not done here)

- Narrower tag bypass without a new App: a deploy key as the only bypass actor of `hoist: tags`,
  its private key kept in environment `release`, release.yml pushing tags over SSH. Needs a hoist
  model change (a deploy-key bypass actor) and an owner-created key.

- Download URLs: installers (`hoard/scripts/install.sh`, `hoard/circleci/hoard.yml` and its hull
  template copy, `hoard/action/src/binary.ts`, `hoist/release/action.yml`) send versions > 0.21.0 to
  this repository and <= 0.21.0 to the old ones; release.yml refuses to release a version <= 0.21.0.
  keel's default hull sources (`keel/src/tools/splice.rs`) stay on keelstackhq/hull until keel pins a
  monorepo hull release; `repository.url` in each launcher `package.json` is rewritten at publish.
- Ported to the root (manual, plus weekly for hoard; Linux only, D-007): splice `action-selftest.yml`
  as `splice-action-selftest.yml`, hoard `self-test.yml` as `hoard-self-test.yml` (its R2 leg needs
  the `R2_*` secrets of environment `hoard-r2`, else it is skipped with a notice). Deleted, not ported (manual
  tools, in git history): hoard `bench.yml` and `r2-checks.yml`; splice `release.yml` with
  `scripts/release-tag.mjs` and `scripts/release-major.mjs` and their tests (the root release.yml
  tags, releases and moves the major tag). Still inert in its folder: hoard `release.yml` (its
  tests in `hoard/action/test/release-workflow.test.ts` read it). Not ported: the splice update-bot
  `splice.yml` copies (decisions are in-repo).
