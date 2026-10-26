---
layout: ../../layouts/Docs.astro
---

This page takes a GitHub repo of yours, new or existing, to the full setup: a template kept
current by splice, the org decisions channel, bosun running the work, repo settings from hoist,
cached CI with hoard, and berth workspaces. Do the steps in order; each one names who does it.

Versions are the released ones as of 2026-10-26: splice 0.2.0, bosun 0.2.0, hoist 0.2.0,
berth 0.1.0, hoard `@v0`.

## Before you start

- **git** on `PATH` (splice fetches templates through the git CLI).
- **Node 24 and pnpm 12** for hull's `template` (its post step runs `pnpm install`). Running
  splice with `pnpm dlx` needs Node 20 or later. hull's `template-rust` needs cargo, and Node for
  its agents-index post step.
- **A GitHub repo you administer**, with the
  [Claude GitHub App](https://github.com/apps/claude) installed on it.
- **A public repo, or a paid GitHub plan.** A private repo on a free plan has no rulesets: the
  required checks are only advisory and auto-merge cannot work, so the owner merges every pull
  request by hand.

## 1. Render the template (new repo)

Skip to step 2 if your repo already has code and you do not want hull.

```bash
pnpm dlx @keelstackhq/splice@0.2.0 new gh:keelstackhq/hull//template my-repo \
  --data scope=my-scope --piece preact-app:web --piece hono-worker:api
cd my-repo
git add -A && git commit -m "chore: initial commit from hull"
```

- `gh:keelstackhq/hull//template` means repository `keelstackhq/hull`, directory `template`.
  Without `--ref`, splice takes the latest stable tag.
- `base` is always rendered. `--piece ID:NAME` adds a piece and names its instance. The pieces
  and their variables are listed in
  [hull's pieces reference](https://github.com/keelstackhq/hull/blob/main/docs/pieces.md);
  `scope` is the npm scope (default `acme`).
- splice runs `git init` when needed and the template's post commands (`pnpm install`), but it
  never commits. Commit `.splice/lock.toml` with everything else and never edit it by hand.

For a Rust-only repo use `template-rust` (a Cargo workspace, with no Node toolchain to build). No release tag contains it
yet, so render it from a branch:

```bash
pnpm dlx @keelstackhq/splice@0.2.0 new gh:keelstackhq/hull//template-rust my-repo \
  --ref main --piece rust-cli:cli
```

Such a repo records version `0.0.0+g<sha>`. splice 0.2.0 cannot move it forward: `splice update`
looks for the latest stable tag, which has no `template-rust`, and `splice check` (and so the
weekly update bot) offers v0.2.2 and opens a broken pull request. A fix (`splice update --ref`)
is on splice's `main`, not released yet; until hull v0.3.0 is tagged, close those bot pull
requests.

## 2. Add the channels

Run these in the repo root, on a clean tree:

```bash
pnpm dlx @keelstackhq/splice@0.2.0 channel add gh:keelstackhq/hull//rigging --piece claude
pnpm dlx @keelstackhq/splice@0.2.0 channel add gh:keelstackhq/decisions//channel \
  --track main --piece import-project
node scripts/agents-index.mjs     # hull repos: refresh the agents index
git add -A && git commit -m "chore: add rigging and decisions channels"
```

- **rigging** adds the subagents (`.claude/agents/`) to a hull repo. A repo not rendered from
  hull also adds `--piece core` (harness-neutral working rules as a section in `AGENTS.md`); a
  hull repo never does, since its `AGENTS.md` already carries them.
- **decisions** adds `docs/agents/org-decisions.md` and a section that imports it. Use
  `--piece import-project` when the repo has `docs/agents/project.md` (hull repos do), else
  `--piece import-claude`; `--piece import-agents` adds a pointer to `AGENTS.md`.
- The decisions channel brings keelstackhq's own decisions (D-001 to D-006), such as "agents
  never merge, tag or force-push". The channel is an ordinary splice template, so splice can
  apply one from your own repo the same way; that setup is not documented in the sources yet.
- `node scripts/agents-index.mjs` exists only in hull repos.

Never edit inside a `<!-- splice:NAME:begin … -->` block or an owned file; change the source
instead. `splice explain <path>` tells you who owns a file.

## 3. Set up bosun

Who: you or an agent session, with splice and bosun on `PATH` and a `GITHUB_TOKEN` that can push
and open pull requests.

```bash
npm install -g @keelstackhq/splice@0.2.0 @keelstackhq/bosun@0.2.0
bosun init --stack rust --owner <your-github-login>
```

- `--stack` is `rust` or `ts-pnpm`, once per stack; a mixed repo gets one entry per stack
  directory. `--plan FILE` copies a plan into the repo.
- `bosun init` runs `splice channel add gh:keelstackhq/bosun//channel` on branch `bosun/init`,
  commits, pushes and opens one setup pull request whose body lists the owner steps.
- Next to a hull template it detects the repo's own `ci-ok` job and names bosun's aggregate check
  `bosun-ok` instead, does not take `CLAUDE.md`, keeps the files hull owns, and adds the missing
  required checks to `hoist.toml` (it keeps hull's own `hoist.toml`).
- In a Claude cloud session, which can push only to `claude/…` branches, bosun 0.2.0 cannot push
  `bosun/init`. Run `bosun init … --no-pr`, push the commit to a `claude/` branch and open the
  pull request yourself. On bosun's `main` (not released yet), `--branch` sets the branch and the
  cloud default is `claude/bosun-init`.
- bosun's two Claude Code hooks belong in `.claude/settings.json`. In a hull repo that file
  belongs to hull, so `bosun init` leaves it alone. hull's `main` (the unreleased v0.3.0) ships
  the hooks, guarded so they do nothing until `.bosun/bosun.toml` exists; with hull v0.2.2,
  `bosun init` lists adding them as an owner step.

The setup pull request is high risk (R-gov). Its base has no bosun workflows yet, so
`risk-check`, `review-verdict` and `main-open` cannot run on it: the owner reviews it, comments
`/bosun approve <head sha>` and merges it by hand. This is a one-time bootstrap.

## 4. Apply the repo settings

Who: the owner, on their own machine, with a token that has admin on the repo, in a checkout of
the default branch after the setup pull request merged.

```bash
npm install -g @keelstackhq/hoist@0.2.0
hoist plan --area github          # what would change; writes nothing
hoist apply --area github --yes   # write it, then verify like `hoist check`
gh label create bosun:halt --color B60205
```

`hoist apply` reads `hoist.toml` and the workflows. It turns on auto-merge and
delete-branch-on-merge, and adds a ruleset on the default branch that requires `ci-ok`,
`risk-check`, `review-verdict` and `main-open` (plus `bosun-ok` next to hull's CI).

bosun repos need auto-merge without a merge queue: `risk-check` and `review-verdict` are statuses
on the pull request head, which a merge-queue commit would not carry. The `hoist.toml` bosun seeds
says `merge_queue = false`. In a hull repo, `bosun init` keeps hull's `hoist.toml`, and hoist
expects a merge queue whenever a workflow has a `merge_group` trigger (hull's and bosun's do), so
add this to `hoist.toml` before `hoist apply`:

```toml
[github]
merge_queue = false
```

Without hoist, set the same by hand: a branch ruleset on the default branch with those required
status checks, "Allow auto-merge" and "Automatically delete head branches".

Then, still as the owner:

- Check `governance/owners.toml` (the GitHub logins whose approval counts) and
  `.github/CODEOWNERS`.
- Add the Actions secret `SPLICE_BOT_TOKEN`: a fine-grained token for this repo with Contents,
  Pull requests and Workflows set to read and write. Without it the weekly update pull requests
  run no CI.
- Paste `docs/setup-script.sh` into the setup script of your Claude Code cloud environment. It
  runs as root before each session and installs Node 24, pnpm (ts-pnpm stacks), the Rust
  toolchain with cargo-nextest and cargo-deny (rust stacks), and the pinned splice, hoist, berth
  and bosun.
- Create the three Routines: janitor (every 15 min, Haiku), curator (daily, Sonnet) and notify
  (every 15 min, Haiku). `bosun doctor` prints their prompts.

`bosun doctor` reports the setup checklist as ok or missing (`--remote` also runs
`hoist check`). It cannot see Routines: set `BOSUN_ROUTINES_OK=1` once they exist.

## 5. Cache CI with hoard

hull's `template` already caches Turborepo through hoard, and bosun's gate workflows for
`ts-pnpm` do too. In another Turborepo repo, add one step after the install:

```yaml
- run: pnpm install --frozen-lockfile
- uses: keelstackhq/hoard@v0 # exports TURBO_API, TURBO_TOKEN, TURBO_TEAM
- run: pnpm turbo run build test lint
```

Run the workflow on `push` to `main` as well, so pull requests start from a warm cache. For a
cargo-only repo hoard does nothing; use `Swatinem/rust-cache@v2`.

## 6. Use berth locally

berth gives each agent or task its own worktree, ports, env files and database.

```bash
npm i -g @keelstackhq/berth@0.1.0
berth up fix-login          # worktree ../<repo>.berth/fix-login on branch berth/fix-login
eval "$(berth env fix-login)"   # PORT, <NAME>_PORT, DATABASE_URL, …
berth down fix-login        # after the work merged; or `berth gc` for all merged tasks
```

An optional `berth.toml` at the repo root sets the copied files, port names, the setup command
and the database. For Claude Code, berth's hooks make `claude --worktree` and subagent worktrees
berth worktrees; the settings snippet is in
[berth's integration guide](https://github.com/keelstackhq/berth/blob/main/docs/integration.md).

## 7. Run the first task

Write a plan in `plans/<project>.md` (format:
[plan-format.md](https://github.com/keelstackhq/bosun/blob/main/docs/plan-format.md)). Its
`project:` line must equal `project` in `.bosun/bosun.toml`. An agent session then runs the loop;
everything up to the verdict is what worked on the rust-canary trial:

```bash
bosun plan check plans/my-project.md    # offline validation
bosun join                              # register the session
bosun plan import plans/my-project.md   # epics, tasks, and one ready plan task per item
bosun claim <plan-task>                 # commit the plan on a branch, then:
bosun pr <plan-task>
# after that pull request merged:
bosun plan import plans/my-project.md   # moves the item's tasks to ready
bosun claim <task>                      # test first, commit, then:
bosun pr <task>
bosun review-pack <pr>                  # input for a fresh reviewer subagent
bosun verdict comment --from verdict.json   # from a cloud session; elsewhere `verdict post`
bosun wait --pr <pr>
bosun pr <task> --auto-merge            # after the checks are green; GitHub merges
```

`--auto-merge` needs the ruleset and auto-merge from step 4. On the trial repo (private, free
plan, so no ruleset) every pull request was merged by hand instead.

## 8. Releases

A repo that publishes packages releases through `hoist release` in its release workflow; agents
never tag. The first publish needs the owner once (`hoist release bootstrap`, then a re-run of
the failed publish job within 2 days).
