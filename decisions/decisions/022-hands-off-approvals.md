# D-022: Hands-off approvals and releases

- Date: 2026-11-20
- Scope: all
- Status: accepted
- Supersedes: D-017

## Context

D-017 promised merges without owner forms, but bosun still sent most gated PRs to the owner:
the critical floor (all bosun `src/**`, workflows, `.bosun/**`), R-gate, R-gov, R-ci, R-weaken
and R-secrets path hits. Every PR in the chat-approval stack and every template PR waited for an
owner comment, and each release waited for an environment tap. On 2026-11-20 the owner said this
takes too much of their time: it must run without any interaction. They chose scope A (everything
except the trust root) and automatic releases.

## Decision

- The orchestrator approves (signed bosun approval, autonomous form) and merges every PR itself,
  including gated, critical-floor, R-gate, R-gov, R-ci, R-weaken, R-deps and R-secrets path
  matches, and `keelstackhq/decisions` PRs, when all hold on the exact head SHA:
  - all required checks are green (CI, `ci-ok`, main-open);
  - two approving review verdicts (confidence >= 0.85, no blocker or major finding) come from two
    different reviewer sessions that the orchestrator dispatched, neither the author nor the
    signer, and no verdict on that head requests changes;
  - a real secret value in the diff is a blocker, never approvable (the reviewers check each
    R-secrets hit).
- Trust root, owner only: `.bosun/approvers.toml` (who may sign, and with which role), the bosun
  rule that sets this quorum (`[approvals]` in `.bosun/bosun.toml` and the trust-root path list in
  bosun's `src/core/risk.rs`), and this decision's row. A PR that changes any of these needs the
  owner's own approval.
- Releases: a merged version-bump PR releases through the release workflow with no environment
  tap. Each repo's `release` environment keeps its default-branch policy and has no required
  reviewer.
- The owner gets one digest a day (`bosun digest`: merged, released, blocked) instead of
  requests. `bosun halt` stops all merges and releases until the owner lifts it.
- Unchanged: secret values and age keys stay with humans (D-010); child sessions never merge,
  tag, release or force-push; nobody force-pushes.

## Consequences

- bosun ships a quorum policy: an autonomous signature plus two counting verdicts passes
  risk-check on every path except the trust root, and the gate code reads that list from the
  base so a PR cannot widen it. Until that release is pinned, the old owner paths still apply.
- Each repo enrolls the orchestrator key and sets the policy in one owner-approved PR (trust
  root), and the owner removes the `release` reviewer once per repo.
- D-017's R-weaken exception, D-018's chat forms, D-019's `release` tap and D-002's owner merge of
  decisions PRs no longer apply; forms are used only for trust-root PRs.
