---
layout: ../../layouts/Docs.astro
---

CI decides what is verified. In a bosun repo, every gate is a `bosun gate <name>` call, the
workflows run a pinned bosun release instead of the code under review, and a handful of required
checks decide whether GitHub may merge. This page follows bosun 0.2.0; behaviour that is only on
bosun's `main` is marked.

## Required checks

| Check            | Posted by                                                         | Fails when                                                                                |
| ---------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `ci-ok`          | the aggregate job of `.github/workflows/bosun.yml`                | a `gate-*` job, `bosun-bin` (installs the pinned bosun) or `pin` did not succeed          |
| `bosun-ok`       | the same job, renamed, in a repo with its own `ci-ok` (`own_ci`)  | as `ci-ok`                                                                                |
| `risk-check`     | the `risk` workflow (a commit status)                             | the pull request is high risk and has no approval yet; see [Risk and review](/docs/risk/) |
| `review-verdict` | `bosun verdict post`, or the `verdict` workflow (a commit status) | the verdict requests changes, has confidence below 0.85, or escalates                     |
| `main-open`      | the `main-open` workflow                                          | main is red and the pull request is not a mainfix                                         |

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

The table is the spec's gate contract. The channel's `bosun.yml` runs five of them as jobs
`gate-fmt`, `gate-lint`, `gate-test`, `gate-deps` and `gate-test-integrity` (8 min each). A profile maps gates to commands; the ts-pnpm profile calls
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

## Workflows run the pinned release

The channel's workflows install the bosun release pinned in `.bosun/version` from npm. They never
build bosun from the change under review:

- `bosun.yml` reads the pin of the pull request's base commit. The setup pull request, whose base
  has no pin yet, uses its own and says so in a warning.
- `risk.yml` (on `pull_request_target` and `issue_comment`) and `verdict.yml` (on
  `issue_comment`) run the workflow file from the base branch and never check out pull request
  code. One limit: `risk.yml` also runs on reviews, and a review event runs the pull request's
  copy of the file; a pull request that changes workflows is R-ci, so the owner sees it.
- The pin moves with an R-gov pull request. The splice update bot brings a new channel version and
  the matching pin together.

## With your own CI (`own_ci`)

Next to a hull template, which has its own `ci.yml` with a `ci-ok` job, `bosun init` sets
`own_ci`: bosun's aggregate job is named `bosun-ok`, a fifth required check, and main-health
closes main-red only when the latest push run on `main` of both workflows is green.

In bosun 0.2.0 both CIs run all their checks, so a pull request runs fmt, lint and tests twice
(the rust-canary trial measured 26 s for hull's job and 69 to 83 s for bosun's 8 jobs). On bosun's
`main`, not released yet, a pull request with `own_ci` runs only the `test-integrity` gate and
relies on the repo's `ci-ok` for the rest; set the repo variable `BOSUN_ALL_GATES=1` when that CI
lacks one of them, such as cargo-deny. Pushes, the merge queue and the nightly run still run all
five.

## Docs-only fast path

On bosun's `main`, not released yet: a pull request whose files are all `*.md` under `docs/` or
`plans/`, or at the root except `README.md` (which can hold doctests), skips the gate jobs. A
`changes` job decides; the aggregate accepts skipped jobs only then. The fast path never applies
with the ts-pnpm stack, where prettier checks Markdown. In bosun 0.2.0 a one-file Markdown change
runs all gates.

hull's `template` CI exits early when nothing relevant changed, docs-only pull requests
included. hull's `template-rust` (unreleased v0.3.0) does the same, with a budget of 15 s.

## Caching

- **Turborepo and Vite+:** `keelstackhq/hoard@v0` after the install step. It starts a local
  remote-cache server and exports `TURBO_API`, `TURBO_TOKEN` and `TURBO_TEAM`. Storage is the
  GitHub Actions cache by default (10 GB per repo, branch-scoped) or your own Cloudflare R2
  bucket. The cache never fails a build: an error is a miss. Run CI on `push` to `main` so pull
  requests read a warm cache.
- **Rust:** `Swatinem/rust-cache@v2`. hoard caches Turborepo and Vite+ tasks, not cargo builds;
  in a cargo-only repo its step only logs a notice.

hull's `template` CI already uses hoard; bosun's ts-pnpm gate jobs do too, and its rust gate jobs
use rust-cache.
