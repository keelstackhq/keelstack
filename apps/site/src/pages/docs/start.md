---
layout: ../../layouts/Docs.astro
---

The shortest path from a repo you already have to agents working on it with bosun. You do nine
steps once; after the first plan, agents do the rest. It follows bosun's README ("Start in 5
minutes") and `docs/adopt-existing-repo.md`, the long version.

Status: bosun 0.10.0 is the release on npm. Steps 3 to 6 and 8 as written need bosun `main` after
0.10.0: adopt's printed commands and settings link, `bosun keygen` and doctor's exact fixes are not
released yet.
<!-- TODO(copy): drop this status note when the next bosun release ships them. -->

Machine-readable: this page is https://keelstack.dev/docs/start.md.

## Before you start

On a computer, in a clean checkout of your repo's default branch. You need Node, the `gh` CLI
signed in, and admin on the repo.

## The nine steps (owner)

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
