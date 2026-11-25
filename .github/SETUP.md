# Owner setup for the monorepo

keel and hoist do the one-time repository setup from the root `keel.toml` and `hoist.toml`.
Nothing here is clicked by hand except the steps keel prints as its checklist (D-009).

## Prerequisites (computer)

1. `gh auth login` as a user with **admin** on `keelstackhq/keelstack` (`gh auth status`).
2. A Cloudflare API token with Pages Write and Account API Tokens Write, and the account id, in the
   environment: `export CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ACCOUNT_ID=...` (never on a command
   line you keep in history; `read -rs CLOUDFLARE_API_TOKEN; export CLOUDFLARE_API_TOKEN` works).
   Without them keel skips Cloudflare and lists it in the checklist.
3. keel and hoist from this checkout: `cargo build -p keelstack-cli -p keel-hoist --locked &&
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
| computer | `secret:<NAME>` | `hoist secrets set <NAME>`: value from env `<NAME>` or a hidden prompt (PEMs such as `BOSUN_APP_KEY`: `--from-file key.pem`), sealed into its environment (never argv, files or logs). `BOSUN_APP_KEY` (bosun-app), `BOSUN_SIGNER_KEY` (bosun-signer), `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_BUCKET` (hoard-r2); `ANTHROPIC_API_KEY` (review) only with `BOSUN_REVIEW_CI = true` |
| computer | `deploy-keys-policy` | allow deploy keys for the organization (https://github.com/organizations/keelstackhq/settings), then re-run `keel up` |
| computer | `first-publish:<crate>` | `hoist release bootstrap` publishes every crate not on crates.io yet (trusted publishing cannot create a crate) |
| phone | `app:keelstackhq-bosun-gate` | install `keelstackhq-bosun-gate` on the repo: https://github.com/apps/keelstackhq-bosun-gate/installations/new |
| phone | `crates-trust:<crate>` | trusted publisher on `keel-bosun`, `keel-hoist`, `keel-berth`, `keelstack-cli`: repository `keelstackhq/keelstack`, workflow `release.yml`, environment `release` (https://crates.io/crates/<crate>/settings) |
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
    `non_fast_forward` (no force-push to main), and, because `hoist.toml` sets
    `[github] admin_bypass = true` (opt-in, default false), one bypass actor: the repository
    admin role in `pull_request` mode. That is only the owner's mainfix path below: an admin may
    merge a pull request past the missing gate statuses, never push to main directly. The merge
    identity matters here: bosun (the orchestrator, D-022) merges with the workflow's
    `GITHUB_TOKEN`, never an admin token, so the admin bypass never applies to bosun's merges and
    the required checks stay enforced for them;
  - ruleset `hoist: tags` (`[github] tags_bypass = "deploy_key"`): creation, update and deletion
    of `v*` tags (`vX.Y.Z` and the major tag `v0`, which every `keelstackhq/keelstack/<tool>@v0`
    consumer runs) are blocked for everyone but one bypass actor, the write deploy key
    `hoist: release tags`. No workflow token can create, move or delete a `v*` tag, whatever its
    `contents:` scope, and neither can a push-triggered workflow on an unmerged branch. `hoist
    apply` creates the key pair, adds the public key as deploy key `hoist: release tags` (write
    access), seals the private key as secret `RELEASE_TAG_KEY` into environment `release` and
    makes that deploy key the only bypass actor; no step for the owner. release.yml's `release`
    and `major` jobs (environment `release`, main only) load `RELEASE_TAG_KEY` (ed25519, OpenSSH
    format, no passphrase) into ssh-agent (never on disk), pin github.com's ed25519 host key
    (`StrictHostKeyChecking=yes`) and push `refs/tags/vX.Y.Z` and `+refs/tags/v0` over SSH; they
    fail closed when the secret is missing. GitHub's DeployKey bypass actor takes no id, so it
    covers every write deploy key of the repository: `hoist check` reports any other write
    deploy key as drift (remove it by hand). Re-running `hoist apply` keeps the key and the
    secret; to rotate, delete deploy key `hoist: release tags` and secret `RELEASE_TAG_KEY`, then
    re-run `keel up` (or `hoist apply`). Unwanted live bypass actors on either ruleset are
    reported as drift, never removed. Deleting a wrong tag needs an admin to first edit or
    disable the tags ruleset (its only bypass actor is the deploy key), or to use the deploy key. The repository stays public (npm provenance). `hoist apply` updates of
    `hoist: tags` keep owner-added includes and rules (hoist reads, merges, writes);
  - environments `bosun-app`, `bosun-signer`, `review`, `hoard-r2`, `site`: default branch only,
    no reviewers (D-022); environment `release`: default branch only and the owner as required
    reviewer (`[release] reviewers` in hoist.toml, D-028);
  - repository settings (auto-merge, delete branch on merge) and Actions permissions (read);
  - Cloudflare Pages project `keelstack-landing` (`site/hoist.toml`; deploy-site.yml builds and
    deploys `site/apps/site/dist`), a minted deploy token sealed with the
    account id as `CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` into environment `site`.
    deploy-site deploys main's site on every push to main touching `site/` or a dependency, and
    after a release run (deploy-site-release.yml dispatches it), always building main's head. Version facts (version, install
    commands, `@vX` sources) come from the latest release tag `vX.Y.Z` reachable from the built
    commit (job `decide`), never from an untagged Cargo version; before the first release > 0.21.0
    the page pins the old repos' last releases. Job `build` runs install, tests and build with no
    secrets; job `deploy` (environment `site`) verifies the artifact's sha256 and runs the wrangler in it
    (installed from the lockfile by build), with no checkout and no install. A manual run deploys only from main. Pull requests build and
    test the site in ci.yml (`ci-site`); deploy-site has no `pull_request` trigger. Only its
    `Check secrets` and `Deploy to Cloudflare Pages` steps receive `CLOUDFLARE_API_TOKEN` /
    `CLOUDFLARE_ACCOUNT_ID`;
  - reads only: app installation, environment secrets and variables (present or not), crates.io and
    npm packages (first publish needed or not).
- Release: no publish secrets (crates.io and npm use trusted publishing, OIDC); the one
  release secret is `RELEASE_TAG_KEY`, created and sealed by `hoist apply` (above). Environment `release`
  waits for the owner's tap (D-028): a version-bump merge or a dispatch of release.yml (or
  cargo-deny-npm.yml, an npm publish of the cargo-deny packages) publishes nothing before it;
  the environment's default-branch-only rule keeps both on main. How to release:
  `docs/releasing.md`. Retired with the old
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

- Download URLs: installers (`hoard/scripts/install.sh`, `hoard/circleci/hoard.yml` and its hull
  template copy, `hoard/action/src/binary.ts`, `hoist/release/action.yml`) send versions > 0.21.0 to
  this repository and <= 0.21.0 to the old ones; release.yml refuses to release a version <= 0.21.0.
  keel's default hull sources (`keel/src/tools/splice.rs`) stay on keelstackhq/hull until keel pins a
  monorepo hull release; `repository.url` in each launcher `package.json` is rewritten at publish.
- Ported to the root (manual, plus weekly for hoard; Linux only, D-007): splice `action-selftest.yml`
  as `splice-action-selftest.yml`, hoard `self-test.yml` as `hoard-self-test.yml` (its R2 leg needs
  the `R2_*` secrets of environment `hoard-r2`, else it is skipped with a notice). Deleted, not ported (manual
  tools, in git history): hoard `bench.yml` and `r2-checks.yml`; splice `release.yml` with
  `scripts/release-tag.mjs` and `scripts/release-major.mjs` (the root release.yml tags, releases
  and moves the major tag; `splice/scripts/release-tag.test.mjs` and `release-major.test.mjs` stay
  and test the root release.yml). Still inert in its folder: hoard `release.yml` (its
  tests in `hoard/action/test/release-workflow.test.ts` read it). Not ported: the splice update-bot
  `splice.yml` copies (decisions are in-repo).
