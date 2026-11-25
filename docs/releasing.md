# Releasing

## How to release keelstack

One version for every tool (lockstep, D-023), one tag `vX.Y.Z`, one workflow
(`.github/workflows/release.yml`). Nobody holds a registry token: crates.io and npm trust the
workflow (trusted publishing).

1. `bash scripts/release-pr.sh X.Y.Z` on a clean checkout. It makes the branch `release/vX.Y.Z`,
   folds the changelog and progress fragments of every folder, bumps every version
   (`scripts/bump-version.sh`: `Cargo.toml`, `Cargo.lock`, npm launchers and platform packages,
   pnpm lock pins, changelogs) and commits. It prints the `git push` and `gh pr create` lines.
2. Open the PR, get it through CI and review, merge it. The merge changes the workspace version,
   which starts `release.yml` (`plan` reads the version, then dispatches the real run on `main`).
3. Tap `release` in the run's "Review deployments" box (the owner, D-028). GitHub asks once per
   stage: `release` (tag, GitHub release with every binary), `crates` and `npm` together, `major`.
4. The run builds five targets, publishes the crates and every npm package, then moves the major
   tag (`v0`). `hoist release check` lists, per package, whether it is on its registry at the
   workspace version.
5. Pin the new bosun in `.bosun/version` and the setup script when needed (`bosun/docs/releasing.md`).

A failed run is re-run the same way (`gh workflow run release.yml --ref main -f sha=<merge commit>
-f version=X.Y.Z`): the tag, release, crates and npm skip what exists.

### A new package or crate

Trusted publishing cannot create a package, so a never-published one is skipped with a notice,
the run stays green and the `v0` tag is held. Once per new package, on the owner's computer:

1. `hoist release bootstrap` (`--dry-run` first). It publishes the npm placeholder, registers the
   npm trusted publisher (needs your npm login and 2FA), publishes a new crate from its tag, and
   ends with what is left.
2. Crates only (crates.io has no API): open
   `https://crates.io/crates/<name>/settings/new-trusted-publisher` and enter owner
   `keelstackhq`, repository `keelstack`, workflow `release.yml`, environment `release`.
3. Re-run the workflow (bootstrap prints the exact line); do it within 2 days, npm drops an unused
   trusted publisher after that.

## How a consumer repo releases with hoist

1. `hoist init` writes `.github/workflows/release.yml`; `hoist apply` creates the `release`
   environment (deployment branch = default branch, you as required reviewer: `[release]
   reviewers = ["<login>"]` in `hoist.toml`) and the tag ruleset.
2. New packages: `hoist release bootstrap` once (as above, crates need the manual trusted
   publisher). `hoist release check` shows what is missing.
3. Each release: `hoist release prepare` opens the release PR (versions, changelog); merge it.
   The workflow runs `hoist release publish` in the `release` environment (after your tap): tag,
   GitHub release, npm and crates.io through trusted publishing, no token. A workflow that tags
   by other means (like keelstack's) calls `hoist release publish --only crates,npm`.
4. Re-run the workflow after a failure: every step checks before it writes.

Spec: `hoist/docs/superpowers/specs/2026-10-16-hoist-design.md` §9.
