---
layout: ../../layouts/Docs.astro
---

keelstack is a family of small tools for repos where agents write the code. A template gives the
repo its stack, CI and agent rules; a renderer keeps them current; an orchestration helper runs
the work through GitHub issues, pull requests and CI; and a person sets the repo up, owns the risk
decisions and approves high-risk pull requests. Every tool is public under
[keelstackhq](https://github.com/keelstackhq); these docs show how to use them in a repo of
your own.

Versions below are the released ones as of 2026-10-29. To set up a repo of your own, follow
[Start here](/docs/start-here/) (the ordered path) or [Getting started](/docs/getting-started/)
(the details). [Greenfield repo with bosun](/docs/greenfield-bosun/) walks a new bosun repo end to end, and
[Start a project](/docs/start-project/) lists every owner step
before agents can work on a new hull repo, with how hull's `ready` check (since hull v0.5.0) verifies each one. How to run it day to day with an orchestrator session and
child sessions is in the [Playbook](/docs/playbook/).

> **For AI agents:** point your agent at https://keelstack.dev/llms-full.txt (every docs page in
> one Markdown file), or at https://keelstack.dev/llms.txt for the index. Each page is also served
> as Markdown next to its HTML page, for example https://keelstack.dev/docs/playbook.md.
> To answer offline from inside a repo, install the docs as a docspack package
> ([Getting started, step 8](/docs/getting-started/#8-local-docs-for-agents-docspack)).

## The tools

| Tool                                                  | What it does                                                                                                                            | Released           | Get it                                                            |
| ----------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ------------------ | ----------------------------------------------------------------- |
| [hull](https://github.com/keelstackhq/hull)           | An opinionated, AI-native monorepo template. You pick pieces; hull owns structure, stack, tool versions and config.                     | v0.5.0             | Rendered by splice from `gh:keelstackhq/hull//template`           |
| [splice](https://github.com/keelstackhq/splice)       | Renders a template into a repo and keeps it up to date. A Copier alternative in one static binary.                                      | 0.3.1              | `pnpm dlx @keelstackhq/splice@0.3.1 <command>`                    |
| [bosun](https://github.com/keelstackhq/bosun)         | Orchestration for agent-built repos: claims and leases, spawn limits, waves, gates, risk checks, review verdicts, lessons and cost.     | 0.3.0              | `npm i -g @keelstackhq/bosun` or `cargo install keel-bosun`       |
| [hoist](https://github.com/keelstackhq/hoist)         | Provisions and verifies a repo's GitHub and Cloudflare setup from what the repo already says, and runs releases.                        | 0.3.0              | `npm install -g @keelstackhq/hoist` or `cargo install keel-hoist` |
| [hoard](https://github.com/keelstackhq/hoard)         | A Turborepo (and Vite+) remote cache for GitHub Actions (and CircleCI) in one line of YAML. No account, no server.                      | v0.5.0 (pin `@v0`) | `uses: keelstackhq/hoard@v0`                                      |
| [berth](https://github.com/keelstackhq/berth)         | Gives each parallel agent an isolated workspace: a git worktree, copied env files, a block of ports, a setup hook and its own database. | 0.1.0              | `npm i -g @keelstackhq/berth` or `cargo install keel-berth`       |
| [decisions](https://github.com/keelstackhq/decisions) | Settled cross-repo decisions, delivered to each repo as a splice channel.                                                               | tracks `main`      | `splice channel add gh:keelstackhq/decisions//channel …`          |

splice, bosun, hoist and berth are static Rust binaries. Every command prints one JSON object
with `--json` (bosun always does) and has fixed exit codes, so an agent can act on the result
without parsing prose.

## How they fit together

### A template, rendered and kept current

splice renders a template into your repo and writes `.splice/lock.toml`: the source, the version,
the values you chose and a hash per owned or shared file. Each file has one of three classes:

| Class    | On `splice update`                                                             |
| -------- | ------------------------------------------------------------------------------ |
| `owned`  | Replaced. A local edit blocks the update (`--force` overrides).                |
| `shared` | 3-way merged: JSON, YAML and TOML by key, text by line. Conflicts get markers. |
| `seed`   | Written once, then yours.                                                      |

hull has two templates: `template` (pnpm workspace, Turborepo, TypeScript 7, Vite+, one CI job
with a `ci-ok` check, a Claude Code layer) and `template-rust` (a Cargo workspace with a pinned
toolchain, cargo-deny and cargo-nextest, and no Node). `template-rust` is released since hull v0.3.0.

### Channels: more templates next to the main one

A channel is a second template applied next to the main one (or alone), with its own lock in
`.splice/channels/<name>.toml`. Three channels matter here:

- **rigging** (`gh:keelstackhq/hull//rigging`, piece `claude`): the subagents. Since hull v0.2.0
  they are no longer part of `base`.
- **decisions** (`gh:keelstackhq/decisions//channel`): a snapshot of the org decisions in
  `docs/agents/org-decisions.md`, imported by your agent rules (D-004).
- **bosun** (`gh:keelstackhq/bosun//channel`): bosun's governance files, the gate, risk, verdict
  and main-health workflows, and the Claude Code hooks. `bosun init` adds it for you. In a hull
  repo, hull owns `.claude/settings.json`, so the hooks come from hull instead (since v0.3.0,
  guarded so they do nothing in a repo without bosun).

The splice update bot (`keelstackhq/splice/action@v1`) runs `splice update --all` on a schedule
and opens one pull request with the changes to the template and every channel.

### bosun runs the work on GitHub

bosun owns the process logic of an agent-built repo. A Claude Code session calls
`bosun <command>` and acts on the one JSON object it prints. GitHub issues, labels, comments and
pull requests are the only shared state; there is no other database.

1. A plan file (`plans/<project>.md`) becomes issues: one epic per item, one issue per task.
2. A session claims a task (a lease with the paths it will touch), works in its own workspace
   and opens a pull request with `bosun pr`.
3. CI runs the gates. A fresh reviewer posts a verdict. `risk-check` compares the diff with the
   risk rules; a high-risk pull request waits for the owner's approval.
4. When `ci-ok`, `risk-check`, `review-verdict` and `main-open` are green (plus `bosun-ok` in a
   repo with its own CI, such as a hull repo),
   `bosun pr --auto-merge` turns on auto-merge, and GitHub merges.

bosun uses the other tools for the parts it does not own:

- **berth**: wave agents get their workspaces from `berth up <task>`; `berth gc` cleans up after
  merge.
- **hoist**: `hoist apply` sets branch protection, auto-merge, the required checks and
  environments; `bosun doctor --remote` runs `hoist check`; releases run `hoist release` in a
  workflow.
- **hoard**: generated gate workflows for pnpm stacks cache through `keelstackhq/hoard@v0`. Rust
  gate jobs use `Swatinem/rust-cache`, since hoard caches Turborepo and Vite+ tasks, not cargo
  builds.

### Who merges and who tags

Child sessions (the agents that write code) never merge pull requests, push or move tags, or
force-push. They get the required checks green and may turn on auto-merge. An orchestrator session
may merge once the required checks are green and any owner approval is in; without one, GitHub's
auto-merge or the owner merges. Release workflows may tag on a version-bump merge, and the owner still
approves the `release` environment (splice and, since v0.3.0, hull tag this way); a repo with no
such workflow (splice's `v1` Action tag) is tagged by the owner. This is org decision D-008, which replaced D-006 on 2026-10-26. Where bosun's hooks are installed
they refuse merge, tag and force-push commands; branch protection stays the real guard.

## Who does what

| Person (the owner)                                                                           | Agents                                                                 |
| -------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| Creates the repo, installs the Claude GitHub App, applies repo settings                      | Write all code through bosun tasks                                     |
| Fills `governance/owners.toml`                                                               | Claim, test, open pull requests, review with a fresh reviewer subagent |
| Approves high-risk pull requests on their head SHA, on GitHub or by chat (a signed approval) | Ask the owner with `bosun ask` when a decision is needed               |
| Approves the `release` environment for each release                                          | Child sessions never merge, tag or force-push                          |
| Can stop all work with a `bosun:halt` issue                                                  | Stop when bosun reports halted (exit 4)                                |
