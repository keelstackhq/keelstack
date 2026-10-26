---
layout: ../../layouts/Docs.astro
---

Agents write the code; the owner decides what is risky and approves the risky changes. bosun
checks every pull request against risk rules, collects a review verdict from a fresh reviewer, and
turns `risk-check` green only when the rules are met. This page follows bosun 0.2.2
(2026-10-26).

## Risk rules

The rules live in `.bosun/risk.toml` (a seed file: written by `bosun init`, then yours, so later
channel updates do not change it; add rules for the paths
of your project that need an owner, such as auth, payments or migrations). A pull request that
matches any rule is high risk.

| Rule                | Why                                                 | Matches (consumer repo)                                                                                                                                                                                         |
| ------------------- | --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R-gov               | Governance files decide who approves what.          | `.bosun/**`, `governance/**`, `.github/CODEOWNERS`, `.claude/**`, `.splice/**`, `hoist.toml`, `REVIEW.md`, `docs/agents/**`, and every `AGENTS.md` and `CLAUDE.md` (`**/`, since 0.2.1; root files only before) |
| R-ci                | CI decides what is verified.                        | `.github/workflows/**`; rust stacks add `deny.toml` and `rust-toolchain.toml`, ts-pnpm stacks `.npmrc`                                                                                                          |
| R-secrets           | A secret in the repo is a leak.                     | added lines matching `(?i)(api[_-]?key\|secret\|token)\s*=`                                                                                                                                                     |
| R-deps              | A new dependency adds code that nobody reviewed.    | a new dependency in a manifest                                                                                                                                                                                  |
| R-prod-write        | A write to production cannot be undone by a revert. | a task with the capability `prod-write`                                                                                                                                                                         |
| R-unseen (built in) | bosun could not see the whole diff.                 | GitHub lists fewer files than changed, or a changed file without a patch                                                                                                                                        |

bosun's own repo also has R-gate (the gate code itself) and R-ci-review (below). `risk.toml` is a
seed, so a repo set up before a rule change adds new paths by hand.

- **Rules come from the base branch.** `risk-check` evaluates the rules of the pull request's
  base, never its head, so a pull request cannot change the rules that judge it.
- **Rules only escalate.** Nothing lowers the risk.
- **The bosun pin is R-gov.** Moving `.bosun/version` to a new release needs the owner.
- **Content rules** read only added lines. The evidence names file and line, not the line itself,
  so a secret is not copied into the pull request body.
- **Capabilities** come from the linked task's metadata and from `bosun:capability/<c>` labels.
- **Keep risky tasks small:** the target is under 100 changed lines.

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

- Approve needs confidence of at least 0.85 and no blocker finding.
- `escalate_risk: true` is never green; it sends the task to needs-human.
- A request for changes moves the task to fixing.
- A later review of the same pull request covers only the new commits (a delta review).
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
withdraws it.

### Approvals from Claude cloud sessions do not count

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
`--list` again. Without `--sign-key` the posting modes refuse in a cloud session (exit 5, since
0.2.1); `--list` still works there.

### Signed approvals from chat

Since bosun 0.2.1 the owner can approve from a Claude chat by telling the orchestrator. The orchestrator session holds an ed25519 private key in its own container. On the owner's
chat message it posts a comment whose first line is `/bosun approve <head sha>` and a later line
`bosun-sig ed25519 <key id> <signature>`. The signature is the proof, so author, app attribution
and edits do not matter. It counts only when:

- the key id is listed in `.bosun/approvers.toml` on the default branch (never the pull request's
  copy, so a pull request that adds its own key does not count);
- the signature verifies for this repo, this pull request and its current head;
- `governance/owners.toml` lists owners and no owner's newest review on the head requests changes;
- the pull request does not change `.bosun/approvers.toml`, `governance/owners.toml` or a
  `CODEOWNERS` file, and bosun could see its whole diff. Key and owner changes always need the
  owner's own GitHub approval, so a stolen key cannot enroll keys or owners.

```toml
# .bosun/approvers.toml (R-gov)
[[key]]
id = "orchestrator-2026-10"
ed25519 = "<base64 of the raw 32-byte public key>"
```

Make a key with `openssl genpkey -algorithm ed25519 -out key.pem`; its public key is
`openssl pkey -in key.pem -pubout -outform DER | tail -c 32 | base64`. Enroll or rotate a key with
a pull request to `.bosun/approvers.toml` that the owner approves on GitHub. The signer posts with
`bosun approve <pr>@<head> --sign-key key.pem` (`--print` shows the comment without posting). A
repo's `risk` workflow runs the bosun release pinned in `.bosun/version`, so signed approvals
count in CI only once that pin is a release with this feature.

### Delegated decisions

Since bosun 0.2.1 a repo can let one Claude session decide some high-risk pull requests itself. A
key in `.bosun/approvers.toml` gets `role = "delegate"`; the delegate signs with
`bosun approve <pr>@<head> --sign-key <pem> --as delegate`. `risk-check` counts it only when all
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
any change to `.bosun/**`, `governance/**`, a `CODEOWNERS` file, `.github/workflows/**` or
`.github/actions/**`. In bosun's own repo all of its code and manifests are critical too. Those
always go to the owner.

The rules `bosun init` seeds have no `delegate` class, so the mode is off until you opt in rules.
bosun's own repo delegates R-ci, R-gate and R-deps (after the floor, that leaves `deny.toml`, the
toolchain files, `.config/**` and `profiles/**` to a delegate); this opt-in is merged on bosun's
`main` and not in a release yet. To turn delegation off at once, delete the delegate key from
`.bosun/approvers.toml` in a pull request the owner approves. Releases stay human through the
`release` environment.

The `delegate` value needs a pinned release that knows it: a `risk.toml` on the default branch
that uses a value the pinned bosun cannot parse fails every pull request (see
[Troubleshooting](/docs/troubleshooting/)).

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
