# D-007: Spend as few GitHub Actions minutes as possible

- Date: 2026-10-26
- Scope: all
- Status: accepted
- Supersedes: —

## Context

On 2026-10-26 the org's GitHub Actions minutes were close to the free-tier limit. The owner asked
for much lower usage and for checks to run locally first. Most minutes went to repeat pushes per
PR, workflows that started on comments, labels and edits, many small jobs (each billed at a rounded
minimum), Windows and macOS runners (billed at a multiple of Linux) and frequent crons.

## Decision

Applies to every keelstackhq repo's `.github/workflows/*` and to the workflows the bosun channel
ships into other repos.

- Before every push, run the repo's local CI mirror (the script or command its `CLAUDE.md` or
  `CONTRIBUTING` names, e.g. `bash scripts/check-index.sh` here) and push only when it passes.
- One push per PR: do review rounds locally and push the result once. Push again only for review
  feedback or CI failures that cannot be reproduced locally.
- Workflows skip events that are not commands (comments, labels, edits) at job level with `if:`,
  so no runner starts for them.
- Fold small jobs into one job; keep the required aggregate check name (`ci-ok`) so branch
  protection still passes.
- Windows and macOS runners only in release workflows; PR and push CI runs on `ubuntu-latest`.
- Scheduled workflows (`on: schedule`) run at most weekly.

## Consequences

- A repo without a local CI mirror adds one (a script that runs what its CI runs).
- Workflow changes that add OS matrix entries, daily crons, or comment/label triggers without a
  job-level `if:` are wrong and get changed.
- Bugs that only show on Windows or macOS may surface at release time instead of on the PR.
