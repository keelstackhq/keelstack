# D-016: PR shape follows the goal, not a line cap

- Date: 2026-11-05
- Scope: all
- Status: accepted
- Supersedes: D-013

## Context

Agents write all the code in keelstack repos. Small capped PRs lowered the risk, but each one
added a review round, a CI run and a handoff. One PR that reaches the whole goal was often faster
and better than many small ones. D-013 capped low-risk PRs at 1,500 lines, and only until
2026-11-30. The owner wants no line cap: the plan decides the shape.

## Decision

- **No line cap for non-gated PRs.** At plan time, bosun picks a shape for each epic
  (`shape: single|split|auto` in the plan):
  - tasks that are independent and can run in parallel → one agent and one PR each;
  - tightly coupled work, or a goal that can only be tested as a whole → one PR of any size.
  - `auto` chooses `single` unless a parallel split clearly wins.
- **Gated paths stay small and alone:** at most 400 changed lines and one task per PR, never
  bundled with other work. Gated paths are:
  - workflows, templates and `docs/template-format.md`;
  - governance and `.bosun/**`;
  - approval or risk code;
  - secrets, auth, environments and release paths;
  - any risk:high task.
- **Big PRs get chunked review.** `bosun review-pack` splits the diff into chunks, and a
  reviewer subagent reads each chunk. The verdict must list every chunk; a verdict with a
  missing chunk is refused.
- **Size is a drift signal only.** A PR more than 2x its planned size gets a "re-plan?" note
  in bosun's `next`. It is never blocked for its size.
- Changed lines exclude tests, snapshots, lockfiles and generated files.

## Consequences

- bosun: plan-format `shape`, `wave plan` single-run entries, review-pack chunks, and risk
  rules forcing gated tasks into their own PR (bosun #403).
- The bootstrap end date (2026-11-30) no longer changes PR sizes.
