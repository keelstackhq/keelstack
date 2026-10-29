# D-012: Owner approvals by chat form; bootstrap mode until 2026-11-30

- Date: 2026-10-29
- Scope: all
- Status: accepted
- Supersedes: —

## Context

Agents told the owner that approvals can only be given as GitHub comments. The owner works
mobile-first (D-009) and approves in the Claude Code chat, where the orchestrator asks with a form
and then posts the signed bosun approval. While the org is still being bootstrapped, waiting for
the owner on every gated PR slows all repos down, so the owner wants agents to approve those PRs for
a limited time.

## Decision

- **Default channel.** An agent that needs an owner approval asks for it in the Claude Code chat as
  a form (`AskUserQuestion`). The form names the repo, the PR number and the head SHA, and it
  offers approve and hold. Child sessions send the request to the orchestrator, which asks the
  form. On an approve answer, the orchestrator posts the signed bosun approval for that exact head
  (`bosun approve <PR>@<sha> --sign-key … --print`), with the line
  `Signed on the owner's approval in the orchestrator chat`. An approval comment that the owner
  writes on GitHub still counts. Agents never ask the owner to write one, and docs and skills
  describe the form as the way to approve.
- **Bootstrap mode** is on from 2026-10-29 until 2026-11-30 23:59 UTC. The owner can switch it
  off sooner by telling the orchestrator, or by a PR that marks this decision superseded. While it
  is on, the orchestrator may give PR gate approvals itself: template, workflow, contract, R-gate
  and R-gov changes, and any other PR whose repo requires owner approval (D-008 a). Three
  conditions apply:
  - all required checks are green on the head;
  - an independent reviewer session that the orchestrator dispatched approved that exact head;
  - the signed approval says `Signed in bootstrap mode (D-012, until 2026-11-30)` and does not
    claim an owner answer.
- **Not covered by bootstrap mode** (these still need the owner):
  - approvals of the `release`, `prod`, `preview` and `splice-bot` environments;
  - tags and starting release workflows (D-008 b);
  - merges in keelstackhq/decisions (D-002);
  - changes to branch protection, rulesets or required checks;
  - secrets and age keys (D-010).
- After 2026-11-30, gate approvals go back to the owner form without any further change. Where
  bosun checks approvals, it stops accepting bootstrap-mode approvals after the date.

## Consequences

- bosun: its docs and agent skill describe the chat form as the approval channel. It gains a
  bootstrap setting with an expiry that its approval check enforces.
- Orchestrators log each bootstrap-mode approval (repo, PR, SHA, reviewer session) in their state
  file, so the owner can audit them.
- Release taps stay human. Removing them would need environment settings changes, which this
  decision does not make.
