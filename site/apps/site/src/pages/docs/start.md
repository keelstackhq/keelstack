---
layout: ../../layouts/Docs.astro
---

Pick your scenario and paste its block. Each block is exact: commands are copied from the hull, splice
and bosun docs, and versions are the current ones (hull v0.10.0, splice 0.4.0, bosun 0.15.0).
Steps for the owner start with "Owner". Everything else an agent does.

Machine-readable: this page is https://keelstack.dev/docs/start.md.

## Setup

Once on your machine (every block below assumes it):

```bash
npm i -g @keelstackhq/bosun@0.15.0
export GITHUB_TOKEN=$(gh auth token)   # in your shell, never in a file
```

Needs git, Node, and `gh auth login` done once (token scopes: repo, workflow). Run in a clean checkout of the default branch. `bosun` exit 5 means stop and tell the owner; never work around it.

1. [New repo](#a): hull and bosun.
2. [Adopt a repo](#b): hull assess and splice adopt, or bosun alone.
3. [Change mode](#c): gatekeeper, steady, booster, autonomous.
4. [Upgrade](#d): bosun pin, splice update.
5. [Where agents run](#e): CLI, Claude Code on the web, Claude Tag in Slack.
6. [Approvals](#f): what merges alone and what the owner approves.

<a id="a"></a>

## A. New repo from hull and bosun

Needs [Setup](#setup).

After `splice new`, write `plans/<name>.md` in the new repo from the owner's brief (`bosun plan check` it): `bosun init` needs it. Use `//template-rust` and `--stack rust` for a Rust repo; piece ids and `my-repo` are your choices.

```bash
pnpm dlx @keelstackhq/splice@0.4.0 new gh:keelstackhq/hull//template my-repo --ref v0.10.0 --piece preact-app:web --piece hono-worker:api
cd my-repo && pnpm install && pnpm run ci
git add -A && git commit -m "chore: render from hull"   # splice new only runs git init
gh repo create <owner>/my-repo --private --source . --push   # owner step, before bosun init
bosun init --stack ts-pnpm --plan plans/<name>.md --owner <login>   # opens the setup PR
bosun doctor
```

Owner: create and push the GitHub repo before bosun init (Start a project, item 1); approve the setup PR's head and merge it; then, on your machine in a checkout of the default branch:

```bash
npm install -g @keelstackhq/hoist@0.4.1
hoist plan --area github          # what would change; writes nothing
hoist apply --area github --yes   # write it, then verify like `hoist check`
```

Agents in the cloud: [E](#e). Full path: [Start here](/docs/start-here/), [Start a project](/docs/start-project/), [Greenfield repo with bosun](/docs/greenfield-bosun/).

<a id="b"></a>

## B. Adopt an existing repo

With hull: from the repo root on a new branch with a clean tree, assess first, then splice adopt as a dry run. Take `<template>` (`template` or `template-rust`) and each `<id>:<name>` from the assess JSON (`next`, `pieces`); with no template it is channels-only: use the channel step of the guide.

```bash
curl -fsSL https://keelstack.dev/assess/v0.10.0.mjs -o /tmp/assess.mjs
echo "2fb956cbc06698ea8672db813a9961a91e19a11fd232a08d61b2a8e3bfb1cf91  /tmp/assess.mjs" | sha256sum -c
node /tmp/assess.mjs --assess --json
npx @keelstackhq/splice@0.4.0 adopt gh:keelstackhq/hull//<template> --ref v0.10.0 --piece <id>:<name> ... --dry-run --json
```

Read the change list, then run the same splice command without --dry-run. Never pass --force unless the owner names the files it may replace. Open the PR; the owner merges it. After the merge, for a Rust or ts-pnpm stack run bosun init --stack rust|ts-pnpm --plan plans/<name>.md --owner <login> (plan as in A; bosun adopt below is for repos without hull) and the hoist steps of A (hoist apply sets the ci-ok ruleset).

With bosun alone (no hull, no splice), after [Setup](#setup):

```bash
bosun adopt --dry-run                    # writes nothing
bosun adopt --owner <login>              # writes bosun's files; commits nothing
bosun doctor
```

Owner: before the setup PR, the session that will sign approvals runs bosun keygen --enroll (in its own container or machine: the private key never leaves it) and you commit .bosun/approvers.toml in the setup PR; run the git commands in data.commands (branch bosun-setup), open the setup PR and merge it yourself.
Then, in the setup PR's repo settings: install the [Claude GitHub App](https://github.com/apps/claude), protect the default branch with the required checks `ci-ok` (`bosun-ok` if a `ci-ok` job exists; adopt prints which), `risk-check`, `review-verdict` and `main-open`, and turn on Allow auto-merge. Run `bosun doctor` until only `plan` is left, then write `plans/<project>.md` and run `bosun plan check`. Cloud setup script: [E](#e). adopt never overwrites existing files; merge the setup PR yourself, since bosun's checks cannot run on it before its workflows are on the default branch; `plugins` may stay missing on a computer in doctor.
Full guide: [Adopt an existing repo](/docs/adopt/).

<a id="c"></a>

## C. Change the bosun mode

Needs [Setup](#setup).

Modes run gatekeeper, steady, booster, autonomous. One PR per move.

```bash
bosun migrate --to <mode> --dry-run   # survey and planned edits; writes nothing
bosun migrate --to <mode> [--direct]  # one governance PR (draft); --direct skips the rungs between
bosun pr --pr <n> --ready             # n = data.pr from migrate's JSON; after the local gates: ready + auto-merge
bosun migrate --finish                # after the merge: join, plan amend (idempotent)
```

Needs a repo that already runs bosun 0.15.0 or later. Use `--direct` only when the owner says so; otherwise one rung per PR, `--finish` after each merge. Owner: approve the one PR head as in [F](#f) (R-gov, and R-weaken for a less strict mode). Ladder, knobs and the two-line prompt for Claude: [Move a repo to bosun booster](https://keelstack.dev/migrate/).

<a id="d"></a>

## D. Upgrade

Needs [Setup](#setup).

```bash
bosun pin --latest --check            # lists tools behind, downloads and writes nothing; run on the default branch
bosun pin --latest --pr               # verifies npm sha512, opens a draft PR; the owner approves
# after it merges, on your machine (<new> = the version pin --check printed):
npm i -g @keelstackhq/bosun@<new> && bosun --version
# then hull, on its own branch:
git switch <default-branch> && git pull && git switch -c chore/splice-update
pnpm dlx @keelstackhq/splice@0.4.0 update --all
pnpm dlx @keelstackhq/splice@0.4.0 check && pnpm install && pnpm run ci
git add -A && git commit -m "chore: splice update to hull vX.Y.Z" && git push -u origin HEAD
```

Do bosun first, then hull; each is its own PR and each is R-gov (.bosun/**, .claude/**, .splice/**), so the owner approves each head as in F. A repo with .splice/channels/bosun.toml cannot run bosun pin (it refuses): use splice update --channel bosun. Cloud sessions need nothing: they run the repo's own setup script. Merge only when ci-ok is green. Both paths in full: [Update](/docs/update/).

<a id="e"></a>

## E. Where agents run

Assumes a repo set up by [A](#a) or [B](#b), with the setup PR merged: the setup script below runs only repos that have `.bosun/version` and `docs/setup-script.sh`, and silently skips the rest.

**Claude Code CLI.** After [Setup](#setup), in the repo on a clean default branch, then paste the prompt.

```bash
bosun doctor   # each missing step with its fix
```

**Claude Code on the web.** Once per repo: install the [Claude GitHub App](https://github.com/apps/claude) on it, create an environment for it (session title bar, Edit), and paste this verbatim as its Setup script (no need to run anything first; `bosun setup-snippet` prints the same as `data.short`):

```bash
for d in . ./*/; do
  [ -f "$d/.bosun/version" ] && [ -f "$d/docs/setup-script.sh" ] || continue
  (cd "$d" && timeout -k 10 1800 bash docs/setup-script.sh </dev/null) || echo "bosun: $d/docs/setup-script.sh failed" >&2
done; exit 0
```

Network access: Limited, with Allow package managers ticked. Add only the hosts your repo's docs/setup-script.sh downloads from: a Rust repo needs github.com and release-assets.githubusercontent.com (prebuilt cargo tools); a repo that only uses npm needs none. No secrets in environment variables and no PAT: GitHub comes from the Claude GitHub App. A failed repo script only warns in the session's setup log: if a tool is missing, read the log and add the host. Children inherit the caller's environment; an R-gov PR can set `[cloud] environment_id = "env_..."` in `.bosun/bosun.toml` for spawns by relay or wave (the id is on the environment's settings). Owner: also create the Routines `bosun doctor` lists; doctor cannot see them. Details: [Cloud environment](/docs/cloud/).

**Claude Tag in Slack.** Needs Claude Tag installed in your Slack workspace and invited to a private channel with no guests that is not shared. Claude Tag runs each thread on the environment chosen for that channel in Claude Tag settings: use the one from the web setup above. Once, an R-gov PR (a CLI or web session opens it; the owner approves it as in [F](#f)): your Slack member ID in `governance/owners.toml` (the `slack` field of your owner entry, see the spec), and in `.bosun/bosun.toml` this `[slack]` table, plus the `[cloud]` table from the web setup if relay or wave spawns should use that environment. `channel` is the channel ID (channel details, at the bottom); `spawners = "owners"` lets only owners with a Slack ID start work; `may_spawn = true` allows `bosun spawn` from a thread, under the same checks and budget as any spawn. The owner also creates the hourly notify Routine. Fields, stdin and the notify and ack flow: [bosun's Slack spec](https://github.com/keelstackhq/bosun/blob/834c4c83762a5b42fff05d7964092a43b1729ac3/docs/spec/10-slack.md).

```toml
[slack]
channel = "C0123456789"
spawners = "owners"
may_spawn = true
```

In the channel the Claude Tag session labels itself and spawns work. `<ts>` and `<U...>` are the thread's timestamp and the poster's Slack user ID; `--channel-info -` (on `bosun notify`) reads the channel's JSON from stdin; `--request -` (on `bosun spawn`) reads the owner's message. `bosun notify` lists due posts in `next` as `slack_post`; the session posts them and then runs `bosun notify --ack <key>`. Slack never approves: approve on GitHub as in [F](#f).

```bash
bosun join --origin slack --slack-channel C0123456789 [--slack-thread <ts>]
bosun notify --channel-info -     # posts due: halt, main-red, asks
bosun spawn --slack-thread <ts> --slack-user <U...> --request -
```

<a id="f"></a>

## F. Approvals

A normal-risk PR (no rule in .bosun/risk.toml matches) merges on green CI and a clean review verdict. No approval.

The owner approves the exact head of a high-risk PR. That is any PR that touches governance paths (R-gov: .bosun/**, governance/**, .claude/**, .splice/**, docs/setup-script.sh, ...), loosens a gate (R-weaken) or adds a secret (R-secrets). R-ci, R-deps, R-prod-write, R-unseen, R-bundle, R-plan and R-revert are owner-class by default too. Exceptions: R-ci-review takes two independent verdicts, a delegate-class rule can be decided by a delegate, and under the autonomous policy allowlisted paths take the fast lane.

- Chat form: the owner tells the signing session (repo, PR, head SHA; approve or hold) and it posts the signed approval below.
- GitHub: a plain /bosun approve <40-character head SHA> comment counts only when posted with a plain user token (typed in the GitHub web or mobile app, or from the owner's own machine); comments posted by the Claude app or a cloud session never count. A new push needs a new comment. Agent PRs are opened under the owner's login and GitHub forbids self-review, so the owner uses a plain comment, a signed approval, or merges.

Signed form (works from a cloud session): a bosun release with signed approvals (0.2.1 or later) in .bosun/version, the signer's key enrolled in .bosun/approvers.toml on the default branch (bosun keygen --enroll; the enrolling PR is R-gov, so the owner approves it with a GitHub review or a plain comment, since no signed approval counts on that PR), and the PEM held by the signing session.

```bash
bosun approve --list                                          # ready PRs and heads; writes nothing
bosun approve <pr>@<40-character head SHA> --sign-key <pem> --print   # shows the signed comment
bosun approve <pr>@<40-character head SHA> --sign-key <pem>           # posts it
```

A review that requests changes (or was dismissed) withdraws comment, signed and delegate approvals; a later approving review ends that state. After it is settled, comment `/bosun recheck` or re-run the `risk` workflow. Slack never approves.

Opt-in CI signer (keyless fast lane) and Slack digest: [Risk and approvals](/docs/risk/#opt-in-ci-signer-and-slack-digest).

## For agents

Agents call `bosun` commands and act on the one JSON object each prints: `ok`, `exit_code`,
`data`, `error.fix` and `next`. Do what `error.fix` says, make the calls in `next`, and never work
around a refusal. Start with `bosun status`, then `bosun claim <issue>`.

| Exit code | Meaning     |
| --------- | ----------- |
| 0         | ok          |
| 1         | error       |
| 2         | usage       |
| 3         | conflict    |
| 4         | halted      |
| 5         | needs-human |
| 6         | limit       |

The full loop for one task is bosun's `AGENTS.md`; the ordered path for a new repo is
[Start here](/docs/start-here/).

## Undo

`bosun uninstall --dry-run`, then `bosun uninstall`: it deletes the files adopt wrote whose bytes
are unchanged and keeps the ones you edited; `--labels` also removes the labels it created and
closes the registry issue. Commit the deletions in a gated PR and remove the required checks
`risk-check`, `review-verdict` and `main-open` from the branch rules.
