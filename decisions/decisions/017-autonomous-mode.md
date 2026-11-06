# D-017: Autonomous mode

- Date: 2026-11-05
- Scope: all
- Status: accepted
- Supersedes: —

## Context

Asking the owner for every gated PR, head change and decisions merge took too long and too many
taps. On 2026-11-05 the owner said: build everything autonomously; no owner approval is needed.

## Decision

- The orchestrator approves and merges every PR itself, gated ones included (workflows, `.bosun/**`,
  governance, approval code, decisions), with no owner form. The bosun approval is signed with the
  orchestrator key, and the comment says "Signed in autonomous mode (D-017), not an owner answer".
- Conditions for every merge stay: CI green on the exact head, and an approving verdict on that
  head from an independent reviewer session the orchestrator dispatched.
- The orchestrator may merge `keelstackhq/decisions` PRs that follow `CLAUDE.md` there.
- Exception (owner, 2026-11-06): a PR that weakens a gate (bosun risk rule `R-weaken`: lower
  limits or thresholds, looser deny or lint rules, removed or softened CI steps, fewer tests)
  always needs the owner's own approval. An orchestrator signature never counts on it. Until
  bosun ships `R-weaken`, the independent reviewer flags such PRs and the orchestrator asks the owner.
- Still out of reach: steps GitHub reserves for a human (environment approvals such as the release
  tap), secrets, and actions refused by permission checks; the orchestrator reports those once and
  continues with other work. Agents still never force-push, and child sessions never merge.
- The owner ends this mode by saying so; D-012 then applies again.

## Consequences

- D-012 owner forms and bootstrap-mode limits no longer gate merges while this mode is on.
- The owner sees results, not requests.
