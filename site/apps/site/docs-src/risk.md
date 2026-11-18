---
layout: ../../layouts/Docs.astro
---

Agents write the code; the owner decides what is risky and approves the risky changes. bosun
checks every pull request against risk rules, collects a review verdict from a fresh reviewer, and
turns `risk-check` green only when the rules are met. This page was last checked line by line against bosun 0.19.0; its links point at bosun {{ks.bare.bosun}}.
Long details live in bosun's
[risk spec](https://github.com/keelstackhq/bosun/blob/{{ks.ref.bosunSpec}}/docs/spec/08-risk.md).

## Cheat sheet: who approves what

| Case                          | What happens                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Normal risk (no rule matches) | A normal-risk PR (no rule in .bosun/risk.toml matches) merges on green CI and a clean review verdict. No approval.                                                                                                                                                                                                                                                                                                                                                                                                                              |
| High risk (a rule matches)    | The owner approves the exact head of a high-risk PR. That is any PR that touches governance paths (R-gov: `.bosun/**`, `governance/**`, `.claude/**`, `.splice/**`, `docs/setup-script.sh`, ...), loosens a gate (R-weaken) or adds a secret (R-secrets). R-ci, R-deps, R-prod-write, R-unseen, R-bundle, R-plan and R-revert are owner-class by default too. Exceptions: R-ci-review takes two independent verdicts, a delegate-class rule can be decided by a delegate, and under the autonomous policy allowlisted paths take the fast lane. |

How the owner approves:

- Chat form: the owner tells the signing session (repo, PR, head SHA; approve or hold) and it posts the signed approval ([how](#signed-approvals-from-chat)).
- GitHub: a plain `/bosun approve <40-character head SHA>` comment counts only when posted with a plain user token (typed in the GitHub web or mobile app, or from the owner's own machine); comments posted by the Claude app or a cloud session never count. A new push needs a new comment. Agent PRs are opened under the owner's login and GitHub forbids self-review, so the owner uses a plain comment, a signed approval, or merges.

## Risk rules

The rules live in `.bosun/risk.toml` (a seed file: written by `bosun init`, then yours, so later
channel updates do not change it; add rules for the paths
of your project that need an owner, such as auth, payments or migrations). A pull request that
matches any rule is high risk.

| Rule                | Why                                                 | Matches (consumer repo)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ------------------- | --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R-gov               | Governance files decide who approves what.          | `.bosun/**`, `governance/**`, `.github/CODEOWNERS`, `CODEOWNERS`, `docs/CODEOWNERS` (and a `CODEOWNERS` file in any directory, any case), `.claude/**`, `.claude-plugin/**`, `.splice/**`, every `hoist.toml`, `docs/setup-script.sh`, `REVIEW.md`, `docs/agents/**`, `**/plans/**`, and every `AGENTS.md`, `CLAUDE.md` and `CLAUDE.local.md` (any case)                                                                                                                                                          |
| R-ci                | CI decides what is verified.                        | `.github/workflows/**`, `.github/actions/**`, every `action.yml` and `action.yaml`, `.circleci/**`, `.gitlab-ci.yml`, `.gitlab/ci/**`, `.buildkite/**`, and the config of Jenkins, Azure Pipelines, Bitbucket Pipelines, Travis and Drone, `.github/dependabot.yml`, every file under `scripts/`, `Makefile`, `justfile`, `Dockerfile` and `.husky/**`; the rust stack adds every `deny.toml`, `.deny.toml`, `deny.exceptions.toml`, `.deny.exceptions.toml` and `rust-toolchain.toml`, and ts-pnpm adds `.npmrc` |
| R-secrets           | A secret in the repo is a leak.                     | sops files (`secrets.toml`, `access.toml`, `*.sops.*`), and added lines where a name (`api_key`, `secret`, `token`, `password`, `private_key` and close variants) is followed by `=` or `:` and a value, an unquoted yaml literal under a secret key, a Kubernetes env `value:` literal after a secret `name:`, a php or ruby `'password' => 'x'` array entry, `user:pw@` in a URL, an age secret key or `SOPS_AGE_KEY`                                                                                           |
| R-deps              | A new dependency adds code that nobody reviewed.    | a new dependency, a new root pin or catalog entry, or a source change in a manifest; a lockfile line (`source =` or `resolved`) that fetches from a host other than crates.io, npm or yarn; any change to `.cargo/config.toml`, `.npmrc`, `.yarnrc`, `.yarnrc.yml`, `.pnpmfile.cjs`, `bunfig.toml`, `.yarn/plugins/**` or `.yarn/releases/**` in any directory                                                                                                                                                    |
| R-prod-write        | A write to production cannot be undone by a revert. | a task with the capability `prod-write`                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| R-unseen (built in) | bosun could not see the whole diff.                 | GitHub lists fewer files than changed, or a file (other than a removed one or a pure rename) without a patch                                                                                                                                                                                                                                                                                                                                                                                                      |
| R-weaken (built in) | A loosened gate weakens every later check.          | a base-to-head compare finds a raised cap, a lowered lint or deny level, a lower coverage floor, removed CI steps, fewer tests or more skips, a growing `*_ALLOWED` list, or a move to a less strict mode; the findings are the evidence                                                                                                                                                                                                                                                                          |
| R-plan (built in)   | The PR is never rated below its task.               | a linked task is `risk: high` in the plan                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| R-bundle (built in) | Gated work needs its own PR.                        | one PR closes more than one issue and changes a gated path                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| R-revert (built in) | A revert must undo exactly what it names.           | a `bosun revert` PR that is not the exact inverse of the PR it names, or whose original needed the owner                                                                                                                                                                                                                                                                                                                                                                                                          |

Each rule's exact paths and the built-in judgments are in the
[risk spec](https://github.com/keelstackhq/bosun/blob/{{ks.ref.bosunSpec}}/docs/spec/08-risk.md).
`risk.toml` is a seed, so a repo set up before a rule change adds new paths by hand. bosun's own
repo also has R-gate (the gate code itself) and R-ci-review (below).

- **Only the owner approves R-weaken.** No orchestrator, bootstrap, policy, autonomous or delegate
  signature counts on it, nor on a PR with the `weaken` capability. Agents may propose
  loosening a gate, never apply it. A less strict mode in `[mode]` is itself an R-weaken change
  ([modes](https://github.com/keelstackhq/bosun/blob/{{ks.ref.bosunSpec}}/docs/spec/18-modes.md)).
- **R-deps** counts new root pins, new catalog entries and source changes (git, `package =`
  renames, `[patch]`/`[replace]`, npm and pnpm overrides, `npm:` aliases), and the registry and
  install-hook files above: they redirect where every package comes from. It does not count
  `x.workspace = true` edges, in-repo `path` dependencies or pnpm `catalog:`/`workspace:`
  references.
- **R-secrets** does not flag code bindings without a literal (`let x = v`) or files under a
  `tests`, `fixtures` or similar directory (never env or config files; a file name such as
  `*.spec.*` exempts nothing). No comment marker exempts a line: `bosun:allow-secret` is gone.
  Strong formats (age, PEM and PGP private keys, GitHub, `sk-`, Stripe live, AWS and
  Slack tokens) are never exempt and match on any added line.
  The config forms (unquoted yaml literals, Kubernetes env `value:` literals) apply to `.yml`,
  `.yaml`, `.properties` and env files only, not lockfiles or `pnpm-workspace.yaml`; a php or ruby
  array entry matches in any file. A value with a space, a reference (`$X`, `${X}`), a boolean, a
  duration, a version or a path is no literal. Known over-denial: a one-word label under a secret key
  in a locale file (`password: Password`).
- **The root `hoist.toml` is on the critical floor** (below): it lists the ruleset's required checks,
  so only the owner approves it, never a delegate.
- **Approvals and verdicts carry over a merge of main.** A high-risk approval, and the
  `review-verdict` status, carry to a head that is at most 5 clean merges of the default branch
  into the approved head, when no owner withdrew. A merge whose only conflict is `Cargo.lock` or
  `pnpm-lock.yaml` carries too, when every whole entry of the regenerated lockfile is an entry of the
  approved head's copy or main's. Only the approval carries; rules, owners and keys are
  recomputed on the new head.
- **Rules come from the base branch.** `risk-check` evaluates the rules of the pull request's
  base, never its head, so a pull request cannot change the rules that judge it.
- **Rules only escalate.** Nothing lowers the risk.
- **The bosun pin is R-gov.** Moving `.bosun/version` to a new release needs the owner.
- **Content rules** read only added lines. The evidence names file and line, not the line itself,
  so a secret is not copied into the pull request body.
- **Capabilities** come from the linked task's metadata and from `bosun:capability/<c>` labels.
- **Keep risky tasks small:** the target is under 100 changed lines.
- **`risk-check` posts `pending` while it judges**, so auto-merge waits; a run that cannot finish
  posts `error`. A stuck `pending` or `error` only blocks: comment `/bosun recheck` (owners,
  members and collaborators only) or re-run the `risk` workflow. A cloud session, which cannot
  post statuses, can read the judgment with `bosun risk-check <pr> --dry-run`.

bosun writes a Risk note into the pull request body: which rules matched, why, and what the
approver must check.

## Approval classes

Each rule has `approval = "owner"` (the default), `"two-verdicts"` or `"delegate"` (since 0.2.1).
A high-risk pull request needs the owner when any matching rule needs the owner, else a delegate
when any match is `delegate`, else two verdicts.

**Two verdicts** pass when, on the current head, verdict records of at least two distinct sessions
approve with confidence of at least 0.9. Sessions that hold or held a lease on a linked task do
not count, since they wrote the change. A pull request that links no task can only pass with the
owner. Every rule in the file `bosun init` seeds is an owner rule. bosun's own repo uses the class
for `channel/**` (R-ci-review), the CI it ships to other repos. The session in a verdict comment
is self-declared, so this class is weaker than an owner approval.

## Review verdicts

Every pull request gets a review from a fresh reviewer. `bosun review-pack <pr>` builds its input:
the diff, the task, its UCs, the matching lessons and `REVIEW.md`. The reviewer returns one JSON
verdict:

```json
{
  "pr": 42,
  "head_sha": "abc123",
  "verdict": "approve",
  "confidence": 0.9,
  "findings": [
    {
      "severity": "major",
      "file": "src/core/lease.rs",
      "line": 88,
      "claim": "...",
      "evidence": "..."
    }
  ],
  "lessons_hit": ["L-012"],
  "escalate_risk": false
}
```

- Approve needs confidence of at least 0.85 and no blocker finding; the status text shows the
  threshold (`approve 0.80 (approves at 0.85)`).
- A pack with more than one chunk of the diff needs every chunk listed in `chunks_reviewed`.
- `escalate_risk: true` is never green; it sends the task to needs-human.
- A request for changes moves the task to fixing.
- A later review of the same pull request covers only the new commits (a delta review); after a
  merge of main it lists only the pull request's own files.
- Each verdict carries the reviewer's model. The optional `[approvals] review_models` in
  `.bosun/bosun.toml` limits which models count for two-verdicts, delegate and autonomous
  approval; it does not limit the plain `review-verdict` status.
- Where to post it:
  - `bosun verdict post --from -` writes the `review-verdict` status directly.
  - A Claude cloud session cannot write commit statuses (its proxy answers 403), so it runs
    `bosun verdict comment --from -`. That posts a pull request comment; the `verdict` workflow
    checks it (unedited, author has write access, current head) and posts the status. On the
    rust-canary trial this took about 20 s.
- A verdict is a reviewer's opinion, not an owner's approval.

Review mode (b), `bosun review-ci` in a workflow with an Anthropic API key, exists in bosun but
is off unless `review.mode = "ci"`. The channel ships no workflow for it; bosun's own repo keeps
the subagent review.

## Owner approval

A pull request with an owner rule needs the approval of a login listed in
`governance/owners.toml`, on its current head SHA. Either counts:

- **A comment** whose whole body, trimmed, is `/bosun approve <head sha>` with the full
  40-character SHA. This path exists because GitHub does not let you approve your own pull
  request, and agent pull requests are opened under the owner's login.
- **A GitHub review** that approves the head SHA, when the owner did not open the pull request.

The `risk` workflow re-runs on the comment, so `risk-check` turns green without a push. A new push
needs a new comment. An edited comment does not count, and a newer review that requests changes
withdraws it (and every signed or delegated approval with it). No review event runs the `risk`
workflow, so a withdrawal counts on the next run: comment `/bosun recheck` or re-run the workflow.
A dismissed request for changes still withdraws; a fresh plain-token comment from that owner, or a
signed `v2` approval that names the dismissed review, clears it (details in the
[risk spec](https://github.com/keelstackhq/bosun/blob/{{ks.ref.bosunSpec}}/docs/spec/08-risk.md)).

### Unsigned approvals from Claude cloud sessions do not count

This applies to plain `/bosun approve` comments. A signed approval (below) counts from anywhere.

Only a comment written with the owner's plain user token counts: GitHub marks it with
`performed_via_github_app: null`. Agent sessions act as the owner's login through the Claude app,
and the cloud agent proxy rewrites every GitHub write (app attribution and a footer). So a
`/bosun approve` comment posted from a Claude cloud session, or through the Claude GitHub tool,
never counts, whatever the token.

Approve in one of these ways:

1. Type the comment in the GitHub web or mobile app.
2. Post it with `gh` and your own token on your machine.
3. Run `bosun approve` on your own machine (a local shell or a local Claude Code session, also
   through Remote Control), with `BOSUN_OWNER_TOKEN` set to a fine-grained token of yours:

```bash
bosun approve --list          # ready PRs: high risk, review-verdict and ci-ok green
bosun approve 199@<sha> 200@<sha>   # run exactly the `data.approve_command` that --list printed
```

`bosun approve` posts nothing if any named pull request moved to another head (exit 3); run
`--list` again. Since 0.4.0 `bosun approve --list` also works without `BOSUN_OWNER_TOKEN`: it reads
unauthenticated, writes nothing, and lists what it could not read in `data.skipped`. An owner's
answer relayed in chat moves a needs-human task to ready with `bosun answer --chat`, counted only
with a verified signature of an owner key in `.bosun/approvers.toml` (`--chat --sign-key` makes
the signed form). Without `--sign-key` the posting modes refuse in a cloud session (exit 5, since
0.2.1); `--list` still works there.

### Signed approvals from chat

Since bosun 0.2.1 the owner can approve from a Claude chat: they tell the orchestrator session,
and the orchestrator posts a signed approve comment. The signature is the proof, so the comment's
author, its app attribution and later edits do not matter, and it works from a Claude cloud
session.

**The comment.** Line 1, trimmed, is the approve line; a later line, trimmed, is the signature
(single spaces). Other text, such as a footer, is ignored:

```text
/bosun approve <head sha>
bosun-sig ed25519 <key id> <base64 of the 64-byte signature>
```

The signed message is exactly the UTF-8 bytes `bosun-approve v1 <owner>/<repo>#<pr> <head sha>`,
with no trailing newline and owner and repo in lower case. So a signature for another head,
another pull request or another repo never verifies. `risk-check` counts it, as
`approved by key <id>`, only when:

- the key id is listed in `.bosun/approvers.toml` read from the default branch (never the pull
  request's copy, so a pull request that adds its own key does not count);
- the signature verifies for this repo, this pull request and its current head, and line 1 names
  that head;
- `governance/owners.toml` lists owners and no owner's newest review on the head requests changes;
- the pull request does not change `.bosun/approvers.toml`, `governance/owners.toml` or a
  `CODEOWNERS` file, and bosun could see its whole diff (no R-unseen). Key and owner changes always
  need the owner's own GitHub approval, so a stolen key cannot enroll keys or owners.

A missing or unreadable `.bosun/approvers.toml` turns the feature off; a malformed file, an
unknown key id or a bad signature counts as no approval and never errors the check.

**Enroll a key** (once per key; rotate the same way):

1. Where the signer runs (the orchestrator's own container; the private key is never shared),
   make the key and its block: `bosun keygen --id <key id> --out <pem outside any git worktree>
   --enroll`. The PEM is mode 0600 and never overwritten; `--enroll` appends the block below to
   `.bosun/approvers.toml`. (By hand: `openssl genpkey -algorithm ed25519`, then the public key
   with `openssl pkey -in key.pem -pubout -outform DER | tail -c 32 | base64`.)
2. Open a pull request with that change (R-gov). The owner approves it on GitHub, with a review
   or a plain-token `/bosun approve <sha>` comment, and it merges.

   ```toml
   [[key]]
   id = "orchestrator-2026-10"
   ed25519 = "<base64 of the raw 32-byte public key>"
   role = "owner"   # the default; "delegate" for delegated decisions (below)
   ```

3. Sign and post: `bosun approve <pr>@<head> --sign-key <pem>`. The `<head>` is the commit the
   owner approved; if the pull request moved on, nothing is signed (exit 3). `--print` shows the
   comment without posting. `bosun approve --list` lists the ready pull requests with their heads.

A removed key stops counting once its removal merges. The `risk` workflow runs the bosun release
pinned in `.bosun/version`, so signed approvals count in CI only with a pin of 0.2.1 or later.

### Delegated decisions

Since bosun 0.2.1 a repo can let one Claude session decide some high-risk pull requests itself. A
key in `.bosun/approvers.toml` gets `role = "delegate"`; the delegate signs the message
`bosun-delegate v1 <owner>/<repo>#<pr> <head sha>` (same comment shape) with
`bosun approve <pr>@<head> --sign-key <pem> --as delegate`. Each role verifies only its own
message, so a delegate signature never counts as an owner's. `risk-check` counts it only when all
hold:

- every rule matched on the head is `approval = "delegate"` (any owner rule needs the owner, and a
  delegate never stands in for two verdicts);
- the pull request is not critical (below), bosun is not halted, and it targets the default
  branch;
- it links open bosun tasks, each with a lease record;
- at least one review verdict on the head, from a session without a lease on a linked task,
  approves with confidence of at least 0.9 and no major findings;
- the owner checks of signed approvals above.

**The critical floor** is hard-coded, whatever a rule says: R-secrets, R-prod-write, R-unseen, and
any change to `.bosun/**`, `governance/**`, `CODEOWNERS` (root, `.github/` or `docs/`), the root `hoist.toml`,
`.github/workflows/**`, `.github/actions/**`, any `action.yml`, `.circleci/**` or another CI
provider's config. In bosun's own repo all of its code and manifests are critical too. Those
always go to the owner.

The rules `bosun init` seeds have no `delegate` class, so the mode is off until you opt in rules.
bosun's own repo delegates R-ci, R-gate and R-deps (after the floor, that leaves `deny.toml`, the
toolchain files, `.config/**` and `profiles/**` to a delegate); its release does this since 0.2.3. To turn delegation off at once, delete the delegate key from
`.bosun/approvers.toml` in a pull request the owner approves. Releases stay human through the
`release` environment.

The `delegate` value needs a pinned release that knows it: a `risk.toml` on the default branch
that uses a value the pinned bosun cannot parse fails every pull request (see
[Troubleshooting](/docs/troubleshooting/)).

### Approval policy and bootstrap mode

`[approvals] policy` in `.bosun/bosun.toml` says when the orchestrator's own signature counts
(`owner`, the default, `owner-gated`, `orchestrator` or `autonomous`; `bosun adopt --policy` writes
it). A workflow or CI file change, R-weaken, R-secrets and R-prod-write always need the owner.
Bootstrap mode (`bootstrap_until`, capped at 2026-11-30 in bosun's code) lets the orchestrator
sign gated approvals until that day, after green checks and an independent clean verdict; see
[Operating model](/docs/operating-model/). Under `autonomous` it may approve only paths under
`src/`, `tests/`, `docs/`, `changelog.d/` or `.changeset/*.md` (or the globs of `[approvals] autonomous_allow` in `.bosun/bosun.toml`, which only the owner's PR may set; the deny list still wins), with a clean verdict from a
session that holds no lease on the task. A one-off task over 300 changed lines (R-oneoff-size) always needs the owner. The boosters that use it are in
[modes](https://github.com/keelstackhq/bosun/blob/{{ks.ref.bosunSpec}}/docs/spec/18-modes.md).
Set a value only after `.bosun/version` pins a release that reads it.

## Asking the owner

`bosun ask <issue> "<question>"` mentions the owners from `governance/owners.toml` on the issue and
sets the task to needs-human. There is no timeout and no automatic approval.

## The kill switch

Stop all work at once:

```bash
gh label create bosun:halt --repo <owner>/<repo> --color B60205   # once
gh issue create --repo <owner>/<repo> --label bosun:halt --title "halt" --body "<why>"
```

While an issue with that label, opened by an owner, is open, every command that starts or changes
work exits 4. Close the issue to resume. An issue opened by anyone else is ignored. Agents must
never close it, but bosun cannot enforce that yet; against a misbehaving agent also set the repo
variable `BOSUN_HALT=1`, which agents cannot change.

## Opt-in CI signer and Slack digest

Both are off by default.

**CI signer** (keyless fast lane for cloud and Claude Tag sessions, which hold no secrets). Four steps, once per repo:

1. Pin a release that reads the key, then set `signer = true` under [approvals] and [checks] mode = "checks" (with the gate App) in .bosun/bosun.toml, in an owner-approved PR (R-weaken). It needs the autonomous policy.
2. `bosun keygen --role signer --id signer-ci --out <pem outside the repo> --enroll`
3. Merge that .bosun/approvers.toml PR (R-gov: the owner approves).
4. Create the GitHub environment bosun-signer (deployment branches: the default branch only) with the secret BOSUN_SIGNER_KEY set to the PEM, then delete the PEM.

The signer key verifies only the autonomous form, so a leaked key can sign at most what the fast lane already lets the orchestrator approve. Verdict session tags are self-declared, so independence rests on honest agents.

**Slack digest.** The digest (`bosun digest`; one a day from steady up) lists the approvals waiting. `bosun notify` posts it only when `notify` in the `[slack]` table names it:

```toml
notify = ["owner-question", "main-red", "budget-halt", "halt", "thread-result", "digest"]
```

Slack never approves: the owner approves by chat form or on GitHub; Slack only carries the links.
