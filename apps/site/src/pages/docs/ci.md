---
layout: ../../layouts/Docs.astro
---

CI decides what is verified. In a bosun repo, every gate is a `bosun gate <name>` call, the
workflows run a pinned bosun release instead of the code under review, and a handful of required
checks decide whether GitHub may merge. This page follows bosun 0.2.3 (2026-10-26); where an older
release behaved differently, it says so.

## Required checks

| Check            | Posted by                                                         | Fails when                                                                                 |
| ---------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `ci-ok`          | the job of `.github/workflows/bosun.yml` named after the check    | the pinned bosun does not install, the pin check fails, a gate fails, or the PR is a draft |
| `bosun-ok`       | the same job, renamed, in a repo with its own `ci-ok` (`own_ci`)  | as `ci-ok`                                                                                 |
| `risk-check`     | the `risk` workflow (a commit status)                             | the pull request is high risk and has no approval yet; see [Risk and review](/docs/risk/)  |
| `review-verdict` | `bosun verdict post`, or the `verdict` workflow (a commit status) | the verdict requests changes, has confidence below 0.85, or escalates                      |
| `main-open`      | the `main-open` workflow                                          | main is red and the pull request is not a mainfix                                          |

`bosun pr --auto-merge` turns on auto-merge only when all of them are green. It reads the list
from `BOSUN_REQUIRED_CHECKS` (comma-separated, replaces all), else the four defaults plus any extra
entries in `hoist.toml` `[github] required_checks` on the default branch. `hoist apply` makes the
same list required on the default branch.

`risk-check` and `review-verdict` are commit statuses, not check runs, because only GitHub Apps can
create check runs. They sit on the pull request head, which a merge-queue commit does not carry,
so bosun repos use auto-merge without a merge queue (`merge_queue = false` in `hoist.toml`).

## The gate contract

| Gate           | When                                                | rust                                                 | ts-pnpm                                |
| -------------- | --------------------------------------------------- | ---------------------------------------------------- | -------------------------------------- |
| fmt            | pull request                                        | `cargo fmt --check`                                  | `pnpm run format:check`                |
| lint           | pull request                                        | `cargo clippy --all-targets --locked -- -D warnings` | `pnpm run lint`                        |
| types          | pull request                                        | (in lint)                                            | `pnpm run typecheck`                   |
| test           | pull request (affected packages), merge queue (all) | `cargo nextest run --locked -p {packages}`           | `pnpm --filter ...<pkg> run test`      |
| deps           | pull request                                        | `cargo deny check`                                   | `pnpm audit --prod --audit-level high` |
| test-integrity | always                                              | built in                                             | built in                               |

The table is the spec's gate contract. The channel's `bosun.yml` runs five of them, one after
another in one job (below), in the order test-integrity, deps, fmt, lint, test. A profile maps gates to commands; the ts-pnpm profile calls
the workspace's root `package.json` scripts, so the repo chooses its own tools. When the ts-pnpm `test`
gate runs for all packages (`pnpm -r --include-workspace-root run test`) it also runs the root
`test` script, so that script must not itself be a recursive `pnpm -r run test`.

- **Affected packages.** The profile's packages adapter reads the diff: a change inside a package
  affects that package; a change to a root manifest, lockfile or toolchain file affects all;
  docs outside every package affect none.
- **Empty repos.** A stack whose manifest (`Cargo.toml`) does not exist yet skips its gates, so
  the setup pull request of an empty repo is green.
- **Other stacks.** A repo profile `.bosun/profiles/<name>.toml` can use generic `regex`
  adapters. test-integrity still runs, and `bosun status` reports `"integrity": "reduced"`.
- **Mixed repos.** One `[[stack]]` per directory (`path = "web/"`, `profile = "ts-pnpm"`).
- **Time budgets.** A pull request run at most 10 min, a merge-queue run 15 min, one job 8 min. A
  gate over budget opens a lesson.

## test-integrity

The gate that keeps tests honest. It refuses a pull request that deletes a test, adds a skip
marker (or a new `.only`), or lowers the assertion count, unless the task's plan allows it. A
snapshot change needs the line `snapshot-change: <reason>` in the pull request body. Files are
compared with the merge base. Agents never skip, delete or weaken a test to get green.

## One job, and what runs in it

Since bosun 0.2.2 the channel's `bosun.yml` runs in one job (since 0.2.3, rust stacks only) named after the required
check (`ci-ok`, or `bosun-ok`); before, it took 8 jobs (9 in 0.2.1). Each job is billed as at least
one minute on a private repo, so one job costs about 3 to 4 minutes per push instead of 10 to 12.
On the rust-canary trial (a hull `template-rust` repo with bosun) a pull request went from about
13 jobs to 6 with this change. In order:

1. **Docs-only check, draft check.** A draft pull request fails the check in seconds and runs no
   gate (`ready_for_review` runs them; a skipped required check would count as passed).
2. **The pinned release.** The job installs the bosun release pinned in `.bosun/version` of the
   pull request's base commit, from npm, and checks the pin the pull request sets. It never builds
   bosun from the change under review. The setup pull request, whose base has no pin yet, uses its
   own and says so in a warning.
3. **The base's config.** Before any toolchain runs, the job puts the base commit's `.bosun/`,
   every `.cargo` directory, every `rust-toolchain(.toml)`, every `.npmrc` and `.pnpmfile.*` over
   the checkout
   and deletes those the base lacks. A pull request's edits to them apply only after merge; the run
   prints a notice naming them. The toolchain comes from the base's `rust-toolchain.toml`
   (`RUSTUP_TOOLCHAIN`), and cargo's `RUSTC` and wrappers are overridden.
4. **The gates, trusted ones first.** test-integrity and deps run before the gates that run pull
   request code, and the job stops at the first failure, so pull request code never runs after a
   failure it could hide.

- **Edits.** A pull request body or title edit changes no file. If the head already passed, the
  edit run keeps the file gates and runs only test-integrity, which reads the body.
- **ts-pnpm and mixed stacks: two jobs** (since 0.2.3). `bosun-trusted` runs test-integrity and
  deps with no pull request code: no install, no cache, and `pnpm audit` reads the pull request's
  lockfile with the base's pnpm config. Only after it passed does the required check's job install
  the dependencies and run fmt, lint and test. (0.2.2 ran everything in one job with a hardened
  install.)
- **Other workflows.** `risk.yml` (on `pull_request_target` and `issue_comment`) and
  `verdict.yml` (on `issue_comment`) run the workflow file from the base branch and never check
  out pull request code. In 0.2.3 `risk.yml` also runs on reviews, and a review event runs the
  pull request's own copy of the file with `statuses: write`, so a same-repo author could forge
  `risk-check`. On bosun's `main` (after 0.2.3, not released yet) `risk.yml` no longer runs on
  reviews: an owner's review, or its withdrawal, counts on the next run, and a `/bosun recheck`
  pull request comment from an owner, member or collaborator starts a run without a push
  (otherwise re-run the risk workflow). main-health runs the gates on every push to `main` and weekly (nightly before 0.2.2).
- **The pin** moves with an R-gov pull request. The splice update bot brings a new channel version
  and the matching pin together.

bosun's own `ci.yml` follows the same rules with a job `ci` that does the work and a job `ci-ok`
that aggregates it. Saving CI minutes is org decision D-007 (see
[Releases and updates](/docs/releases/)).

## With your own CI (`own_ci`)

Next to a hull template, which has its own `ci.yml` with a `ci-ok` job, `bosun init` sets
`own_ci`: bosun's aggregate job is named `bosun-ok`, a fifth required check, and main-health
closes main-red only when the latest push run on `main` of both workflows is green.

Since bosun 0.2.1 a pull request with `own_ci` runs only the `test-integrity` gate and relies on
the repo's `ci-ok` for the rest; set the repo variable `BOSUN_ALL_GATES=1` when that CI lacks one
of them, such as cargo-deny. Pushes, the merge queue and the weekly run still run all five. (In
0.2.0 both CIs ran everything: the rust-canary trial measured 26 s for hull's job and 69 to 83 s
for bosun's 8 jobs.)

## CircleCI

hull v0.4.0 and bosun's `main` (after
0.2.3, not released yet) can run CI on CircleCI, for both `template` and `template-rust`:

- **Both** (`circleci` piece): a CircleCI job `ci-ok` next to the unchanged GitHub `ci.yml`.
  bosun's `circleci` channel piece adds a CircleCI job `bosun-ok` (or `ci-ok` without `own_ci`)
  that runs `bosun.yml`'s gates with the pinned, sha256-checked bosun.
- **CircleCI only** (`circleci-only` piece): no `ci.yml` (and no `desktop.yml` or `deploy.yml`);
  `circleci-status.yml` mirrors CircleCI's `ci-ok` and `bosun-ok` as GitHub statuses, so the
  ruleset still requires them. bosun's `circleci-only` piece has no GitHub `bosun.yml`, and
  main-health reads the CircleCI status of `main`'s head.
- On CircleCI, `bosun gate` and `test-integrity` take their base from the merge base with the
  default branch on a pull request, and run everything on a push to the default branch.
- hoard runs there too (since hoard 0.4.0); see [Caching](#caching).
- Desktop builds (`circleci-only` with `tauri-app`) need a weekly scheduled trigger on `main` in
  CircleCI's Project Settings → Triggers.

An existing hull repo switches with `splice add circleci`, then `splice add circleci-only` (which
removes `ci.yml`).

## Repo-specific CI (`scripts/ci-extra.sh`)

hull v0.4.0 seeds `scripts/ci-extra.sh`, a no-op that the repo owns, and runs it as the last CI step
`ci-extra` on GitHub and CircleCI. It is skipped on docs-only and unaffected pull requests. Put
checks that only this repo needs there instead of editing the owned workflow.

## Pinned actions

In hull v0.4.0 every action is pinned by full commit SHA, with its version in a comment.
[hull's `docs/actions.md`](https://github.com/keelstackhq/hull/blob/main/docs/actions.md) lists
them all, for an org allowlist of actions. Renovate (`helpers:pinGitHubActionDigests`) keeps the
SHAs current in generated repos. splice's update Action is pinned the same way.

## Docs-only fast path

Since bosun 0.2.1: a pull request whose files are all `*.md` under `docs/` or `plans/`, or at the
root except `README.md` (which can hold doctests), skips the gates; it costs about one minute.
The fast path never applies with the ts-pnpm stack, where prettier checks Markdown.

hull's `template` CI exits early when nothing relevant changed, docs-only pull requests
included. hull's `template-rust` (since v0.3.0) does the same, with a budget of 15 s.

## Caching

- **Turborepo and Vite+:** `keelstackhq/hoard@v0` after the install step. It starts a local
  remote-cache server and exports `TURBO_API`, `TURBO_TOKEN` and `TURBO_TEAM`. Storage is the
  GitHub Actions cache by default (10 GB per repo, branch-scoped) or your own Cloudflare R2
  bucket. The cache never fails a build: an error is a miss. Run CI on `push` to `main` so pull
  requests read a warm cache.
- **CircleCI:** hoard's `circleci/hoard.yml` (since hoard 0.4.0) starts the server with
  `hoard serve --detach` and stops it with `hoard stop`. CircleCI caches are not branch-scoped:
  with hoard 0.5.0 (not released yet), pass `restore: false` on the default branch.
- **Rust:** `Swatinem/rust-cache@v2`. hoard caches Turborepo and Vite+ tasks, not cargo builds;
  in a cargo-only repo its step only logs a notice.

hull's `template` CI already uses hoard; bosun's ts-pnpm gate jobs do too, and its rust gate jobs
use rust-cache.
