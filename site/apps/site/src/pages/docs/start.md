---
layout: ../../layouts/Docs.astro
---

Pick your scenario and paste its block. Each block is exact: commands are copied from the hull, splice
and bosun docs, and versions are the current ones (hull v0.10.0, splice 0.4.0, bosun 0.15.0).
Steps for the owner start with "Owner". Everything else an agent does.

Machine-readable: this page is https://keelstack.dev/docs/start.md.

1. [New repo](#a): hull and bosun.
2. [Adopt a repo](#b): hull assess and splice adopt, or bosun alone.
3. [Change mode](#c): gatekeeper, steady, booster, autonomous.
4. [Upgrade](#d): bosun pin, splice update.
5. [Where agents run](#e): CLI, Claude Code on the web, Claude Tag in Slack.
6. [Approvals](#f): what merges alone and what the owner approves.

<a id="a"></a>

## A. New repo from hull and bosun

```bash
pnpm dlx @keelstackhq/splice@0.4.0 new gh:keelstackhq/hull//template my-repo --ref v0.10.0 --piece preact-app:web --piece hono-worker:api
cd my-repo && pnpm install && pnpm run ci
npm i -g @keelstackhq/bosun@0.15.0 && export GITHUB_TOKEN=$(gh auth token)
bosun init --stack ts-pnpm --plan plans/<name>.md --owner <login>   # opens the setup PR
bosun doctor
```

Owner: create and push the GitHub repo before bosun init (Start a project, item 1); approve the setup PR's head and merge it; then run hoist apply.
Full path: [Start here](/docs/start-here/), [Start a project](/docs/start-project/), [Greenfield repo with bosun](/docs/greenfield-bosun/).

<a id="b"></a>

## B. Adopt an existing repo

With hull: assess first, then splice adopt as a dry run.

```bash
curl -fsSL https://keelstack.dev/assess/v0.10.0.mjs -o /tmp/assess.mjs
echo "2fb956cbc06698ea8672db813a9961a91e19a11fd232a08d61b2a8e3bfb1cf91  /tmp/assess.mjs" | sha256sum -c
node /tmp/assess.mjs --assess --json
npx @keelstackhq/splice@0.4.0 adopt gh:keelstackhq/hull//<template> --ref v0.10.0 --piece <id>:<name> ... --dry-run --json
```

Read the change list, then run the same splice command without --dry-run on a new branch with a clean tree. Never pass --force unless the owner names the files it may replace.

With bosun alone (no hull, no splice):

```bash
npm i -g @keelstackhq/bosun@0.15.0 && export GITHUB_TOKEN=$(gh auth token)
bosun adopt --dry-run                    # writes nothing
bosun adopt --owner <login>              # writes bosun's files; commits nothing
bosun doctor
```

Owner: run bosun keygen --enroll first (in the orchestrator's container) and commit .bosun/approvers.toml in the setup PR; run the git commands in data.commands (branch bosun-setup), open the setup PR and merge it yourself.
Full guides: [Adopt an existing repo](/docs/adopt/), [the nine steps](#long-form) below.

<a id="c"></a>

## C. Change the bosun mode

Modes run gatekeeper, steady, booster, autonomous. One PR per move.

```bash
bosun migrate --to <mode> --dry-run   # survey and planned edits; writes nothing
bosun migrate --to <mode> [--direct]  # one governance PR (draft); --direct skips the rungs between
bosun pr --pr <n> --ready             # after the local gates: ready + auto-merge
bosun migrate --finish                # after the merge: join, plan amend (idempotent)
```

Owner: approve the one PR head (R-gov, and R-weaken for a lighter mode). Ladder, knobs and the two-line prompt for Claude: [Move a repo to bosun booster](https://keelstack.dev/migrate/).

<a id="d"></a>

## D. Upgrade

```bash
bosun pin --latest --check            # list only, writes nothing
bosun pin --latest --pr               # verifies npm sha512, opens a draft PR; the owner approves
npm i -g @keelstackhq/bosun@<new> && bosun --version   # your machine, after the merge
git switch -c chore/splice-update && splice update --all
splice check && pnpm install && pnpm run ci
```

Cloud sessions need nothing: they run the repo's own setup script. Merge only when `ci-ok` is green. Both paths in full: [Update](/docs/update/).

<a id="e"></a>

## E. Where agents run

**Claude Code CLI.** On your machine, in the repo, clean default branch, then paste the prompt.

```bash
npm i -g @keelstackhq/bosun@0.15.0
export GITHUB_TOKEN=$(gh auth token)   # in your shell, never in a file
bosun doctor                           # each missing step with its fix
```

**Claude Code on the web.** Once per repo, in the environment's Setup script (`bosun setup-snippet` prints it as `data.short`):

```bash
for d in . ./*/; do
  [ -f "$d/.bosun/version" ] && [ -f "$d/docs/setup-script.sh" ] || continue
  (cd "$d" && timeout -k 10 1800 bash docs/setup-script.sh </dev/null) || echo "bosun: $d/docs/setup-script.sh failed" >&2
done; exit 0
```

Network access: Limited, with Allow package managers ticked. Add only the hosts your repo's docs/setup-script.sh downloads from: a Rust repo needs github.com and release-assets.githubusercontent.com (prebuilt cargo tools); a repo that only uses npm needs none. No secrets in environment variables and no PAT: GitHub comes from the Claude GitHub App. Details: [Cloud environment](/docs/cloud/).

**Claude Tag in Slack.** Once, an R-gov PR: a private channel, your Slack ID in `governance/owners.toml`, then in `.bosun/bosun.toml` this and the `[cloud]` table from the web setup. No orchestrator session is needed.

```toml
[slack]
channel = "C0123456789"
spawners = "owners"
may_spawn = true
```

In the channel the Claude Tag session labels itself and spawns work:

```bash
bosun join --origin slack --slack-channel C0123456789 [--slack-thread <ts>]
bosun notify --channel-info -     # posts due: halt, main-red, asks
bosun spawn --slack-thread <ts> --slack-user <U...> --request -
```

<a id="f"></a>

## F. Approvals

A normal-risk PR (no rule in .bosun/risk.toml matches) merges on green CI and a clean review verdict. No approval.

The owner approves the exact head of a high-risk PR. That is any PR that touches governance paths (R-gov: .bosun/**, governance/**, .claude/**, .splice/**, docs/setup-script.sh, ...), loosens a gate (R-weaken) or adds a secret (R-secrets). R-ci, R-deps, R-prod-write, R-unseen, R-bundle, R-plan and R-revert are owner-class by default too. Exceptions: R-ci-review takes two independent verdicts, a delegate-class rule can be decided by a delegate, and under the autonomous policy allowlisted paths take the fast lane.

- Chat form: Claude asks in the Claude Code chat (repo, PR, head SHA; approve or hold) and posts the signed approval.
- GitHub: comment /bosun approve <40-character head SHA> from the owner account; a new push needs a new comment.

Opt-in CI signer (keyless fast lane) and Slack digest: [Risk and approvals](/docs/risk/#opt-in-ci-signer-and-slack-digest).

<a id="long-form"></a>

## Long form: bosun on an existing repo, nine steps

1. **Install bosun.** `npm i -g @keelstackhq/bosun` (or `cargo install keel-bosun`).
2. **Give it a token.** `export GITHUB_TOKEN=$(gh auth token)`, with the repo and workflow scopes.
3. **Adopt.** `bosun adopt --owner <you>` writes bosun's files into your worktree, makes the
   labels and the registry issue, and commits nothing. Its JSON lists the exact git commands in
   `data.commands` and only the owner steps still left in `data.owner_steps`. Try
   `bosun adopt --dry-run` first to see what would change; existing files are never overwritten.
4. **Approval key.** `bosun keygen --enroll` makes an ed25519 key, writes the private half to
   `~/.bosun/approver.pem` (mode 0600; it refuses a path inside any git worktree) and adds the
   public half to `.bosun/approvers.toml`. Run it only on the machine that signs approvals. If an
   orchestrator session signs for you, that session runs it in its own container instead and opens
   a PR with `.bosun/approvers.toml`, which you approve.
5. **Setup PR.** Run the commands adopt printed (branch `bosun-setup`, `git add`, commit, push),
   open the pull request and merge it yourself: until bosun's workflows are on the default branch,
   its checks cannot run on that PR.
6. **GitHub settings.** Install the [Claude GitHub App](https://github.com/apps/claude) on the
   repo. Set branch protection on the default branch with the link adopt prints, with the
   required checks `ci-ok` (`bosun-ok` if your repo already has a `ci-ok` job; adopt's printed step names the right one), `risk-check`, `review-verdict` and `main-open`. Turn on **Allow
   auto-merge** (Settings, General).
7. **Cloud environment.** Paste the output of `bosun setup-snippet` once as the cloud
   environment's setup script. You never paste it again: it runs each checkout's own
   `docs/setup-script.sh`, which pins the tool versions and moves them by reviewed PRs. Details:
   [Cloud environment](/docs/cloud/).
8. **Doctor.** `bosun doctor` lists each step as ok, warn, skipped or missing, with the exact fix;
   `data.next_step` names the first one missing. It also runs without a token (the GitHub checks
   are then skipped). Do each fix and run it again until only `plan` is left. On a computer
   `plugins` may stay missing too: cloud sessions install them.
9. **First plan.** Write `plans/<project>.md` in bosun's plan format, then `bosun plan check` it.
   Agents take it from there: [Tasks and sessions](/docs/tasks/).

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
