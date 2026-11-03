# D-014: Zero-touch onboarding and approval policy after bootstrap mode

- Date: 2026-11-03
- Scope: all
- Status: accepted
- Supersedes: —

## Context

The owner wants new and adopted repos to need one chat form at the start and nothing else, except
release taps (D-015). Bootstrap mode (D-012) ends on 2026-11-30, so each repo needs an approval
policy that holds after that date. The design is in keelstackhq/bosun
`docs/superpowers/specs/2026-11-02-zero-touch-onboarding-design.md`; the owner approved it in the
chat form on 2026-11-02, and the security review tightened the provisioning gate.

## Decision

- **Intake.** All upfront answers live in `.bosun/intake.toml` (a seed, checked by
  `bosun intake check`). Agents never ask the owner again what the intake already answers.
- **Approval policy** is the intake answer `approvals`, written to `[approvals] policy`:
  - `owner`: every gated PR needs an owner approval (chat form, D-012).
  - `owner-gated` (recommended): the orchestrator approves low-risk and R-ci PRs after green checks
    and an independent review of that head; the owner approves R-gov, `.bosun/**`, approval code,
    workflows, secrets and releases.
  - `orchestrator`: the bootstrap rules of D-012, without an end date.
  - A repo without the setting is `owner`.
- **In every policy** the owner approves environment deployments (`release`, `prod`, `preview`,
  `splice-bot`, `provision`), decisions merges, branch rules and required checks, and secrets. A
  PR that changes a CI workflow or a composite action (`**/action.yml`, `**/action.yaml`) is
  owner-class. Releases follow D-015: the orchestrator starts them, the owner taps `release`.
- **Provisioning.** Repos are created and configured only by the org provisioner GitHub App, run by
  `provision.yml` in keelstackhq/provision. Its `provision` environment deploys from `main` only.
  The workflow refuses an intake unless `bosun intake verify` accepts its detached signature:
  orchestrator key, bound to repo, mode, nonce, issued time, an expiry of at most one hour, and the
  intake's sha256; a used nonce is refused. The orchestrator signs only on an owner form answer
  that names that repo and mode, and its private key stays in its own session (never in a shared
  environment, setup script, repo or worker). `new` only creates; `adopt` only adds settings and
  never loosens one; protected repos (decisions, provision, bosun, hull, splice, hoist, hoard,
  berth) and repos that already have `.bosun/` are refused.
- **Secrets** are environment secrets in the environments of the jobs that read them (hoist
  `[github] secret_scope = "environment"`), deployable from `main` only, never repo-level. No
  personal access token is used anywhere. npm and crates publish through OIDC trusted publishing.

## Consequences

- bosun enforces the policy in `bosun approve` and risk-check (keelstackhq/bosun#364, #367).
- The owner's one-time steps are the plan prerequisites H1-H7 in keelstackhq/bosun
  `plans/zero-touch-onboarding.md`: org secrets, the Claude and provisioner apps on all
  repositories, the shared `keelstack` cloud environment, the provision repo and its environment.
- After 2026-11-30 an orchestrator approval in a repo with policy `owner` no longer counts.
