# Owner setup for the monorepo's GitHub Actions

Names only; values live in GitHub settings. Nothing here is set by a workflow.

## Branch protection / rulesets (main)

- Required checks: `ci-ok`, `risk-check`, `review-verdict`, `main-open`.
  The per-folder jobs `ci-bosun`, `ci-splice`, `ci-hull`, `ci-hoist`, `ci-hoard`, `ci-berth`,
  `ci-keel`, `ci-decisions` and `ci-repo` are aggregated by `ci-ok`; do not require them one by one
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

## Secrets

- `BOSUN_APP_KEY` (environment `bosun-app`): private key of the GitHub App `keelstackhq-bosun-gate`.
- `BOSUN_SIGNER_KEY` (environment `bosun-signer`).
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

## Follow-ups (not done here)

- bosun reads its config only from `<repo root>/.bosun/`; the monorepo has `bosun/.bosun/` only, so
  governance runs on defaults until a root `.bosun/` exists (paths in risk.toml re-prefixed).
- Download URLs that still point at the old repos: `hoard/scripts/install.sh`,
  `hoard/circleci/hoard.yml`, `hoard/action/src/binary.ts` (`DEFAULT_REPO`), the hull template's
  `hoard.yml`, `splice/src/source.rs`, `keel/src/tools/splice.rs`, `repository.url` in each
  launcher `package.json` (release.yml rewrites it at publish time).
- Not ported (inert, kept in their folders): hoard `bench.yml`, `r2-checks.yml`, `self-test.yml`;
  splice `action-selftest.yml`; the splice update-bot `splice.yml` copies (decisions are in-repo).
