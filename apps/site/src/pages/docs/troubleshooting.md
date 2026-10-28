---
layout: ../../layouts/Docs.astro
---

Known pitfalls, each with what you see, why, and the fix. Most were found on the rust-canary trial
(2026-10-25), a repo set up from hull's `template-rust` and bosun 0.2.0 the way these docs
describe.

## Repo and GitHub settings

### Required checks are only advisory; auto-merge never fires

**Why.** A private repo on GitHub's free plan has no rulesets. `hoist plan --area github` reports
"cannot read rulesets: Upgrade to GitHub Pro or make this repository public". Since bosun
0.2.1, `bosun doctor` reads the enforced checks and says "cannot verify" (0.2.0 still said
"required checks: ok").
Without a ruleset, `ci-ok`, `bosun-ok`, `risk-check`, `review-verdict` and `main-open` decide
nothing, so the owner merges every pull request by hand.

**Fix.** Make the repo public or use a paid plan, then run `hoist apply --area github --yes` on
your machine with an admin token.

### The setup pull request never gets `risk-check` or `review-verdict`

**Why.** Its base has no bosun workflows yet, so nothing can post those checks.

**Fix.** Expected once. The owner reviews it, comments `/bosun approve <head sha>`, merges it by
hand, then applies the required checks.

### `bosun init` cannot push from a Claude cloud session

**Why.** Cloud sessions push only to `claude/…` branches; bosun 0.2.0 always used `bosun/init`.

**Fix.** Use bosun 0.2.1 or later: in a cloud session it defaults to `claude/bosun-init`, and
`--branch` sets another branch. With 0.2.0, run `bosun init … --no-pr`, push the commit to a
`claude/` branch and open the pull request yourself.

## Approvals and reviews

### `/bosun approve <sha>` was posted but `risk-check` stays red

Check, in order:

- **Posted from a Claude cloud session or the Claude GitHub tool?** It never counts. The cloud
  agent proxy rewrites every GitHub write (app attribution and a footer), and risk-check counts
  only comments with `performed_via_github_app: null`. Post it in the GitHub app, with `gh` and
  your own token, or with `bosun approve` on your own machine.
- **Old head?** The comment binds to one SHA; a new push needs a new comment. The full
  40-character SHA is required.
- **Edited?** An edited comment does not count; post a new one.
- **Not an owner?** The login must be in `governance/owners.toml` on the default branch.
- **Changes requested?** Your newer review requesting changes on the head withdraws the comment.

### Every weekly splice update pull request needs the owner

**Why.** It always changes `.splice/**` (often `.claude/**` too), which is R-gov. This is by
design.

**Fix.** Approve its head SHA with `/bosun approve <sha>` each week.

### A review verdict was posted but `review-verdict` did not change

**Why.** For each reviewer session, its newest verdict record on the current head decides, so a
later request for changes withdraws an earlier approval; a verdict for an older head does not
count. Since bosun 0.2.1 a newer verdict comment replaces only one from the same session, so two
reviewers posting close together both count. **Practice:** how verdicts of several sessions
combine is open as bosun issue #238 (per the orchestrator).

**Fix.** After a push, post a new verdict for the new head.

### A cloud session gets 403 posting the review verdict

**Why.** The Claude cloud proxy refuses commit-status writes.

**Fix.** Use `bosun verdict comment --from -` instead of `verdict post`; the `verdict` workflow
posts the status.

### `bosun pr --auto-merge` gets "GraphQL is not available from Claude Code sessions"

Handled: bosun 0.2.0 falls back to the cloud route for auto-merge and reports `auto_merge_route`.
Agents must not call these routes themselves; the merge-lock hook refuses it.

## Bots and tokens

### The bot's pull requests run no CI

**Why.** Pushes and pull requests made with `GITHUB_TOKEN` do not trigger `pull_request`
workflows, and `GITHUB_TOKEN` cannot push changes to workflow files.

**Fix.** Give the splice update bot `SPLICE_BOT_TOKEN` (a fine-grained token with Contents, Pull
requests and Workflows read and write) in the main-only environment `splice-bot`, and the hoist
release workflow `HOIST_RELEASE_TOKEN` (Contents and Pull requests write; in the environment
`release-pr` on bosun's `main` and hull v0.4.0's `template-rust`). hull v0.4.0 (not tagged yet)
also takes GitHub Apps instead (`SPLICE_APP_CLIENT_ID` and `SPLICE_APP_KEY`; for template-rust
releases `HOIST_RELEASE_APP_CLIENT_ID` and `HOIST_RELEASE_APP_KEY`), or `SPLICE_BOT=off` to run
`splice update` in a session instead of the bot.

### A splice post step fails for want of a token

**Why.** Since 0.3.1, splice runs post and step commands without token- or secret-like
environment variables (`GH_TOKEN`, `NPM_TOKEN`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`, `*_KEY`, …),
so the bot's token never reaches tool logs or package lifecycle scripts.

**Fix.** None in splice: a post step that needs a token is not supported. Run that command in a
workflow step of its own.

### A tag was created but no release workflow ran

**Why.** A tag pushed with `GITHUB_TOKEN` starts no workflow, like any other push with that token.

**Fix.** Create the tag and release it in the same workflow run, as splice's release workflow does
(a `tag` job, then the release jobs), or as `hoist release publish` does (it tags through the API
inside the publish job).

### The first release fails at `publish`

**Why.** Expected: npm and crates.io need the package to exist before a trusted publisher can be
registered (`first-publish:<name>`).

**Fix.** `hoist release bootstrap` on your machine, a crates.io trusted publisher by hand, then
re-run the failed job within 2 days (npm expires an unused trusted publisher).

## Templates

### `splice update` fails in a `template-rust` repo

**Why.** Before hull v0.3.0 no tag contained `template-rust`, so repos were rendered with
`--ref main` and record `0.0.0+g<sha>`. splice 0.2.0's `update` looked for the latest
stable tag (v0.2.2), which has no `template-rust` ("no splice.toml at `template-rust`"), and its
`check` offered that update, so the weekly bot opened a broken pull request.

**Fix.** Use splice 0.2.1 or later: `splice update --ref main` moves the repo forward by git
ancestry, and a plain `update` or `check` only considers stable tags that descend from the locked
commit. The bot's Action uses the latest splice by default. New repos render from a tag (v0.3.0
or later) and need none of this.

### `splice update` says an owned file "was edited locally"

**Fix.** Revert the edit (`git checkout <commit-before-the-edit> -- <file>`, then commit), or run
`splice update --force` to take the template's version. Change owned files in the template
instead.

### `splice channel add gh:keelstackhq/hull//rigging` refuses in a repo from hull v0.1.x

**Why.** The repo still has base's copies of the subagents.

**Fix.** Run `splice update` first (it removes the unedited copies), move or delete any edited copy
it reports, then add the channel.

## bosun

### Every command exits 4

bosun is halted, main is red, or process is red. `bosun status` says which. An owner's open
`bosun:halt` issue (or `BOSUN_HALT=1`) halts everything until the owner closes it. On main-red,
only mainfix work continues; fix main with `bosun revert <pr>` or a smaller forward fix.

### A task stays in `review` after a person merged its pull request

**Why.** In bosun 0.2.0 only a task that `bosun pr --auto-merge` queued moves to merged. Merging by
hand (as on a private repo without rulesets) leaves the state label and the lease behind.

**Fix.** Pin bosun 0.2.1 or later: `bosun pr`, `bosun reap` and
`bosun result --outcome merged --pr N` see a human merge.

### A hook refuses a harmless command that mentions a state label

In bosun 0.2.0 the label-lock hook refuses any shell command naming a `bosun:state/` label that
does not start with `bosun`, even a read-only `grep` or a heredoc. Since 0.2.1, read-only shell
commands pass. The rust-canary trial on 0.2.2 still saw read-only GitHub MCP tools (such as
`list_issues` with a state label filter) and heredoc prose refused; bosun's `main` (after 0.2.3,
not released yet) lets both pass. Until then, keep state labels out of heredocs and use `gh`
for listings by label.

### `bosun doctor` reports the Claude GitHub App as missing

In bosun 0.2.0 it looks for a login starting with `claude`, but the app acts as the owner's login.
Since 0.2.1 it also checks the app attribution of issues, pull requests and comments.

### Every pull request fails `risk-check` after a `risk.toml` change

**Why.** `risk-check` runs the bosun release pinned in `.bosun/version` and reads `risk.toml` from
the base branch. A rule that uses a value the pinned release does not know (bosun 0.2.0 and
`approval = "delegate"`) is a parse error, so every pull request fails, including the one that
would fix it. bosun hit this on its own repo before 0.2.1.

**Fix.** Keep the rules parseable by the pin: move `.bosun/version` to a release that knows the
value first (for `delegate`, 0.2.1), then add the rule. bosun's own repo has a test
(`tests/governance.rs`) that keeps its rules parseable by its pin.

### A child session's push is refused

**Practice:** a child session's permission classifier can block a push when the instruction to
push came from another session, such as the orchestrator. The owner confirms the push in that
child session.

### `bosun pr --auto-merge` fails with "422 Auto merge is not allowed for this repository"

**Why.** The repo setting "Allow auto-merge" is off (found on the rust-canary trial). bosun 0.2.3's
fix text still says to retry; on bosun's `main` (after 0.2.3, not released yet) it names this fix,
and `bosun pr` also says "push the branch" when the branch is not on GitHub yet.

**Fix.** Turn on auto-merge (`hoist apply --area github`, or the repo settings), or let the
orchestrator or the owner merge.

### The edit hook refuses the claiming session's own files

**Why.** With the lease taken under `BOSUN_SESSION=<session>/<task>`, the edit-lock hook sees only
the plain session id and refuses the Edit tool on the leased paths (rust-canary trial, bosun 0.2.2).
The per-task id is meant for wave agents in their own berth workspace.

**Fix.** On bosun's `main` (after 0.2.3, not released yet) a session may edit paths leased under
its own `<session>/<task>` ids. With 0.2.3, the workaround from the trial (findings G7): set
`BOSUN_SESSION=<session>/<task>` only inside a wave agent's `berth up` workspace, and claim
without it in a session that works on its own.

### `.bosun/fold-fragments.sh` gives "permission denied"

**Why.** splice writes files without their mode, so the script lands without its executable bit.

**Fix.** Run it as `bash .bosun/fold-fragments.sh`; since bosun 0.2.3 the channel's docs say so.

## Exit codes

| Tool   | 0       | 1                                   | 2                  | 3                     | 4      | 5           | 6     |
| ------ | ------- | ----------------------------------- | ------------------ | --------------------- | ------ | ----------- | ----- |
| bosun  | ok      | error                               | usage              | conflict              | halted | needs-human | limit |
| splice | ok      | drift, conflict or update available | usage              | template error        |        |             |       |
| hoist  | in sync | drift or failed write               | human step pending | usage, config or auth |        |             |       |
| berth  | ok      | refused or drift                    | usage              | config or environment |        |             |       |
