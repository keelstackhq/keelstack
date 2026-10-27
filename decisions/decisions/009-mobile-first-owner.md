# D-009: Mobile-first owner: human steps up front, then agents run to the end

- Date: 2026-10-27
- Scope: all
- Status: accepted
- Supersedes: —

## Context

The owner mostly follows agent work from a phone. Requests stalled midway on human-only steps
(creating an account, adding a secret, installing an app, approving an environment) that surfaced
one at a time, often needing a computer the owner did not have at hand. On 2026-10-27 the owner
settled that every human step is known and done before agents start the long run.

## Decision

- At the start of every request, the agent (the orchestrator if there is one) lists every
  human-only step of the whole request as one checklist, before doing other work.
- Each step says where it is done (`phone` or `computer`) and how agents verify it is done (a
  command, an API call or a file to check, e.g. `gh secret list`, `gh api ...`). Computer steps come
  first, so the owner can finish them in one sitting and do the rest from a phone.
- Agents verify each step themselves; they do not take "done" on trust where a check exists.
- After the checklist is done, agents run to the end of the request without waiting on a human.
  The only allowed waits are batched chat approvals (the owner answers once, e.g. "approve A B C")
  and taps on release-environment approvals (`release`, D-008).
- An agent that meets a human-only step the checklist did not declare adds it to the checklist as
  a prerequisite, tells the owner once, and works on everything that does not depend on it.
- bosun implements this as `bosun prereqs` (in progress); until it ships, agents keep the checklist
  in their first message and in the PR or plan they open.

## Consequences

- Plans and specs end with an owner-steps section split into computer and phone steps, each with
  its check (e.g. hull docs/superpowers/specs/2026-10-27-secrets-and-ci-providers-design.md §13).
- Asking the owner a question mid-run is a missed checklist item; batch it with other approvals.
- Off the table: one-at-a-time "please do X now" requests in the middle of a run.
