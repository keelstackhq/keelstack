# D-021: gated PRs have no line cap

- Date: 2026-11-19
- Scope: all
- Status: accepted
- Supersedes: D-016

## Context

D-016 removed the line cap for normal PRs but kept one for gated paths: at most 400 changed
lines, one task, never bundled with other work. That still split coupled work into many small
gated PRs, each with its own review round, CI run and approval. The owner decided: no PR size
limit at all. A PR is as big as its goal needs, gated paths included.

## Decision

- **No line cap for any PR**, gated paths included. Gated paths are workflows, templates and
  `docs/template-format.md`, governance and `.bosun/**`, approval or risk code, secrets, auth,
  environments and release paths, and any risk:high task. A gated PR may hold more than one
  task and may be bundled with other work when the plan says so.
- **The plan picks the shape** for each epic (`shape: single|split|auto` in the bosun plan):
  - independent tasks that can run in parallel → one agent and one PR each;
  - tightly coupled work, or a goal that can only be tested as a whole → one PR of any size;
  - `auto` chooses `single` unless a parallel split clearly wins.
- **Big PRs get chunked review.** `bosun review-pack` splits the diff into chunks, and a
  reviewer subagent reads each chunk. The verdict must list every chunk; a verdict with a
  missing chunk is refused.
- **Gated paths still need the owner's approval** (D-014, D-017, D-018 rules unchanged). Only
  their size and bundling limits are gone.
- **Size is a drift signal only.** A PR more than 2x its planned size gets a "re-plan?" note
  in bosun's `next`. It is never blocked for its size.
- Changed lines exclude tests, snapshots, lockfiles and generated files.

## Consequences

- bosun: risk-check, plan-format/plan lint, review-pack, `docs/plan-format.md`, docs/spec,
  AGENTS.md/CLAUDE.md and channel templates drop the 400-line, one-task, alone rule for gated
  paths and cite D-021; chunked review stays.
- Other repos that copy the cap (hull template/rigging, splice) drop it in their own PRs.
- D-016 is superseded; D-013 stays superseded.
