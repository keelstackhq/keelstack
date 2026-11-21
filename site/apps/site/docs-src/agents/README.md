# keelstack: agent guide

You are an AI agent in the TARGET repo (set up, adopt, upgrade, change mode), not in keelstack. Follow this file in full. Never invent commands; at `TODO(copy)` ask.

## Use

Versions: hull {{ks.hull}}, splice {{ks.splice}}, bosun {{ks.bosun}}, hoist {{ks.hoist}}, hoard {{ks.hoard}}, berth {{ks.berth}}. JSON: https://keelstack.dev/agents/versions.json. {{ks.verified}}

bosun prints exactly one JSON object on stdout: `{version, command, ok, exit_code, data, error, next}`; `error` is `{kind, message, fix}`. Do what `error.fix` says; make the calls in `next`; never work around a refusal. Exit codes: 0 ok, 1 error, 2 usage, 3 conflict, 4 halted, 5 needs-human, 6 limit. On exit 5 stop and tell the owner.

## Decide

First match wins.

1. Empty repo or no commits: path A.
2. `.bosun/version` exists and the owner asks for a mode change: path C (before D). Mode change but no `.bosun/version`: set bosun up first (B).
3. `.splice/` or `.bosun/version`: path D (upgrade), except `.splice/` without `.bosun/version` when the owner wants bosun: its `bosun init` step (B).
4. Otherwise run the assess block of path B. Read `template` in its JSON (schema `hull.assess/1`: fields in the hull adopt guide, Sources). `stack` is null whenever `template` is null.
   - `template` is `template` or `template-rust`: hull adopt (B), then `bosun init` with the printed `stack`.
   - `template` is null: channels only (B). With bosun: `bosun init --stack rust|ts-pnpm` after the channel (B); `bosun adopt` (also `--stack other`) only for other stacks or no splice.
   - `confidence` low: ask the owner each `unsure` question; never guess.

## Human-only steps

For A and B, tell the owner the whole list at once (D-009); one message may answer all. C and D have their own owner steps (approve the PR head). You run everything else, `gh repo create` included. After each step run its check; go on only when it passes.

{{ks.humanSteps.table}}

{{ks.humanSteps.note}}

## Steps

Install bosun first (every path assumes it):

{{ks.block.installBlock}}

Needs git, Node, `gh auth login`; pnpm unless C or B bosun alone. Use a clean branch. Blocks are exact. Fill `<placeholders>` from the owner or the JSON. Each path ends in Verify.

A. New repo. The owner's brief gives the name, `<owner>`, the pieces (`<id>:<name>`, e.g. `preact-app`) and the plan: after `splice new` write `plans/<name>.md` from it and `bosun plan check` it; `bosun init --plan` copies it. Rust: `//template-rust`, `--stack rust`, piece `rust-cli` (libraries need none); skip `pnpm install && pnpm run ci`; run its `.claude/skills/verify/SKILL.md` steps. For signed approvals pause before `bosun init` for step 2.

{{ks.block.newRepoBlock}}

Owner step 4, after the merge:

{{ks.block.hoistBlock}}

B. Adopt, with hull. Take `<template>` and each `<id>:<name>` from the assess JSON (`next`, `pieces`). The owner confirms them and the change list.

{{ks.block.adoptHullBlock}}

Then run it for real and check. Never `--force` unless the owner names the files to replace. Their `ci.yml` is an owned clash (refused): the owner takes the template's or keeps theirs with its job renamed `ci-ok`; a kept owned workflow stays refused, so adopt later or channels only.

{{ks.block.adoptHullRunBlock}}

Open the PR; the owner merges. Then `bosun init --stack <stack> --plan plans/<name>.md --owner <login>` (`stack` from assess; plan as in A), then owner step 4 with `--adopt`:

{{ks.block.hoistAdoptBlock}}

B. Adopt, channels only (no hull template). Bosun for rust or ts-pnpm: then `bosun init --stack <stack> --plan plans/<name>.md --owner <login>` (plan from the owner's brief):

{{ks.block.channelsBlock}}

B. Adopt, with bosun alone (other stacks, or no splice). Write `plans/<name>.md` from the owner's brief:

{{ks.block.adoptBosunBlock}}

{{ks.humanSteps.alone}}

C. Change mode. Needs bosun {{ks.min.bosunMigrate}} or later in the repo. No `bosun init` or new plan: `--finish` amends open epics' plans (`--plan <file>` adds one). One rung per PR unless the owner says `--direct`; the owner approves the PR head.

{{ks.block.migrateBlock}}

D. Upgrade. bosun first, then hull, each its own PR. With `.splice/channels/bosun.toml` `bosun pin` refuses: `splice update --all` also moves bosun; skip the pin and npm lines and install the version in `.bosun/version`.

{{ks.block.upgradeBlock}}

Approvals (owner-class PRs): the owner approves the exact head SHA.

{{ks.block.approveBlock}}

## Verify

A red `pnpm run ci` is yours to fix. `splice check` also exits 1 on an available update (not your failure).

{{ks.block.verifyBlock}}

## Rollback

Nothing merges until the owner merges. `--dry-run` writes nothing.

{{ks.block.rollbackBlock}}

## Sources

- This file: https://keelstack.dev/agents.md
- One scenario each: https://keelstack.dev/docs/start/a.md to f.md
- hull adopt guide at the tag: https://raw.githubusercontent.com/keelstackhq/hull/{{ks.hull}}/docs/adopt.md
