# D-027: Owner delegation of approvals

- Date: 2026-11-24
- Scope: all
- Status: accepted
- Amends: D-022 (the trust-root clause, only where a repo opts in)

## Context

Under D-022 the orchestrator signs and merges every PR after green required checks and two clean
verdicts from different reviewer sessions on the exact head, except the trust root
(`.bosun/approvers.toml`, bosun `[approvals]` and its trust-root path list, the D-022 row), which
only the owner approves. On 2026-11-24 the owner decided in chat: "from now on go with full
approval from my side. don't ask anymore. if it's currently not supported, implement a mode where
I allow you (or any agent) to approve any PR".

## Decision

- bosun gets an opt-in owner delegation: `[approvals] owner_delegation = "all"` in
  `.bosun/bosun.toml`, valid only with `policy = "quorum"`. With it, a quorum signature (an
  autonomous-form signature of a key the owner enrolled in `.bosun/approvers.toml`, or the CI
  signer's key) approves every PR, the trust root and R-weaken included.
- The floor stays: green required checks, two clean counting verdicts (approve, confidence
  >= 0.85, no blocker/major) from two different reviewer sessions on the exact head, a linked task
  leased by the author, no halt, a diff bosun can see whole (R-unseen), and no strong secret value
  in the diff. The approver identity is recorded (risk-check `approved_by`, the key id; the CI
  signer also names the requesting session).
- The setting is read from the PR's base only, so a PR cannot turn it on for itself; turning it on
  is a trust-root edit the owner approves. `bosun approve` refuses to sign when the default branch
  has turned it off since the base, and a plain (non-quorum) key signature never covers the trust
  root. risk-check reads only the base: removing the setting on main stops new signatures, while
  open PRs whose base still has it are judged with it.
- Outside bosun and unchanged: environment taps (release, deploy), GitHub settings (rulesets,
  apps) and secrets stay with the owner; they are not PR approvals.
- keelstackhq/keelstack turns it on in the PR that adds it (the owner approves that PR).

## Consequences

- Without the setting nothing changes (D-022 as before).
- With it, the owner's only remaining PR lever is `bosun halt` and removing the setting (a PR the
  quorum may now approve too); a compromised enrolled key plus two colluding reviewer sessions can
  change the trust root. The owner accepted this. CI workflows are covered as well: verdict
  records count only when bound to the default branch's verdict workflow run, so a workflow a PR
  adds cannot post counting verdicts.
