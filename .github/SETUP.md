# Owner setup for the monorepo's GitHub Actions

Names only; values live in GitHub settings. Nothing here is set by a workflow.

## Branch protection / rulesets (main)

- Required checks: `ci-ok`, `risk-check`, `review-verdict`, `main-open`.
  The per-folder jobs `ci-bosun`, `ci-splice`, `ci-hull`, `ci-hoist`, `ci-hoard`, `ci-berth`,
  `ci-keel`, `ci-decisions`, `ci-site`, `ci-repo` and `ci-gates` are aggregated by `ci-ok`; do not require them one by one
  (a skipped job is fine inside `ci-ok`).
- Merge queue: on (ci.yml listens to `merge_group`).
- Merge method for version-bump PRs: merge or squash (release.yml compares the first parent).
- Tag ruleset for `v*`: allow the release workflow (`github-actions[bot]`) to create `vX.Y.Z` and to
  move the major tag `v0`.

## Environments

| Environment | Deployment branches | Reviewers | Used by |
| --- | --- | --- | --- |
| `release` | default branch only | none (D-022) | release.yml (all publish jobs), cargo-deny-npm.yml |
| `bosun-app` | `main` only | none | risk.yml, verdict.yml, main-open.yml, main-health.yml |
| `bosun-signer` | `main` only | none | signer.yml |
| `review` | `main` only | none | review.yml |
| `hoard-r2` | `main` only | none | hoard-self-test.yml (R2 storage legs) |
| `site` | `main` only | none | deploy-site.yml (deploy job; D-024) |

## Secrets

- `BOSUN_APP_KEY` (environment `bosun-app`): private key of the GitHub App `keelstackhq-bosun-gate`.
- `BOSUN_SIGNER_KEY` (environment `bosun-signer`).
- `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_BUCKET` (environment `hoard-r2`):
  the R2 bucket of hoard's self-test.
- `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID` (environment `site`): deploy `site/` to the Cloudflare Pages
  project `keelstack-landing` (token: Pages edit on that account). Move both from the old site repo's
  environment `production`; PRs never read them (the deploy job runs only off a push, a release run or
  a manual run on main).
- `ANTHROPIC_API_KEY` (environment `review`; only when `BOSUN_REVIEW_CI` is `true`).
- Release: none. crates.io and npm use trusted publishing (OIDC).

## Variables

- `BOSUN_APP_CLIENT_ID`: client id of the `keelstackhq-bosun-gate` app.
- `BOSUN_REVIEW_CI`: `true` to run review.yml (set it in the same change that sets bosun's
  `[review] mode = "ci"`); unset = review.yml does not run.

## Apps and registries

- Install the GitHub App `keelstackhq-bosun-gate` on `keelstackhq/keelstack`.
- npm trusted publisher on every published package (the bosun, splice, hoist, berth and keel
  launchers and their platform packages, and the cargo-deny packages): repository
  `keelstackhq/keelstack`, workflow `release.yml` (`cargo-deny-npm.yml` for the cargo-deny
  packages), environment `release`. npm provenance needs a public repository.
- crates.io trusted publisher on `keel-bosun`, `keel-hoist`, `keel-berth`, `keel-cli`: repository
  `keelstackhq/keelstack`, workflow `release.yml`, environment `release`. A crate that is not on
  crates.io yet must be published once by hand (trusted publishing cannot create a crate).
- Retired with the old repos: `BOSUN_RELEASE`, `BERTH_RELEASE`, `HOIST_RELEASE_TOKEN`, the
  `release-pr` environment.

## Main does not build

The gate workflows (risk, verdict, review, signer, main-open, main-health) build bosun from the
checked-out base (pull_request_target) or the default branch head. If that build fails, the gate
fails closed with an `::error::` (no status is posted, so the required check stays missing) and
`.github/scripts/gate-workflows.test.sh` (ci-repo) keeps it that way. There is no pinned-release
fallback yet: no monorepo release exists, and the old `keelstackhq/bosun` releases (<= 0.21.0,
the version in `.bosun/version`) predate `[[folders]]` and would judge the monorepo wrongly. The
fix path: open a `bosun:kind/mainfix` PR that makes bosun build on main (usually `bosun revert
<pr>` of the breaking merge); the owner merges it past the missing gate statuses (ruleset bypass),
then the gates run again from the fixed main. Once a monorepo release (> 0.21.0) exists and
`.bosun/version` names it, the workflows can fall back to that pinned release again.

## Follow-ups (not done here)

- Download URLs: installers (`hoard/scripts/install.sh`, `hoard/circleci/hoard.yml` and its hull
  template copy, `hoard/action/src/binary.ts`, `hoist/release/action.yml`) send versions > 0.21.0 to
  this repository and <= 0.21.0 to the old ones; release.yml refuses to release a version <= 0.21.0.
  keel's default hull sources (`keel/src/tools/splice.rs`) stay on keelstackhq/hull until keel pins a
  monorepo hull release; `repository.url` in each launcher `package.json` is rewritten at publish.
- Ported to the root (manual, plus weekly for hoard; Linux only, D-007): splice `action-selftest.yml`
  as `splice-action-selftest.yml`, hoard `self-test.yml` as `hoard-self-test.yml` (its R2 leg needs
  the `R2_*` repository secrets, else it is skipped with a notice). Deleted, not ported (manual
  tools, in git history): hoard `bench.yml` and `r2-checks.yml`; splice `release.yml` with
  `scripts/release-tag.mjs` and `scripts/release-major.mjs` and their tests (the root release.yml
  tags, releases and moves the major tag). Still inert in its folder: hoard `release.yml` (its
  tests in `hoard/action/test/release-workflow.test.ts` read it). Not ported: the splice update-bot
  `splice.yml` copies (decisions are in-repo).
