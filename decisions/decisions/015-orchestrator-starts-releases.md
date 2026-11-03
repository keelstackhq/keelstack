# D-015: The orchestrator starts releases; the owner taps `release`

- Date: 2026-11-02
- Scope: all
- Status: accepted
- Supersedes: D-008

## Context

Under D-008 (b) and D-012 the owner started every release: ran the release workflow or pushed the
release tag, then approved the `release` environment. Each release cost the owner two steps, and
the first one is mechanical. On 2026-11-02 the owner chose (chat form) to let the orchestrator start
releases, while keeping one human tap on the `release` environment before anything publishes.

## Decision

Everything in D-008 stays, except release starts:

- Child sessions (every Claude session that is not the orchestrator) never merge a PR (`gh pr merge`
  without `--auto`, the GitHub merge API, or a local `git merge` pushed to a default branch), never
  push, move or delete a tag, never start a release workflow, and never force-push. They open the
  PR, get the required checks green, and may run `gh pr merge --auto` where the repo allows it.
- The orchestrator session (the one Claude session that coordinates child sessions for the owner)
  may merge PRs on the owner's standing authorization, once all required checks are green on the
  head commit and any owner approval the repo requires is in. It never force-pushes.
- **New:** the orchestrator may start a release once the version-bump or release PR is merged, CI
  is green on that commit, and an independent reviewer session approved the release PR's head. It
  starts the release only through the repo's release workflow: `workflow_dispatch` on
  `.github/workflows/release.yml` (or the repo's documented release workflow), or by merging the
  version-bump PR when the workflow runs on that merge. It never pushes, moves or deletes a tag
  itself; the workflow creates the version tag inside the `release` environment.
- The owner still approves every run of the `release` environment (and `prod`, `preview`,
  `splice-bot`). No agent removes or weakens an environment's required reviewer.
- Every release workflow offers `workflow_dispatch` and creates its own tag after the `release`
  approval. A repo whose release still needs a hand-pushed tag (for example a floating `v1` or `v0`
  tag) keeps that tag with the owner until its workflow creates it.
- Required checks: `ci-ok` in every repo; bosun also requires `risk-check`, `review-verdict` and
  `main-open`. No agent edits branch protection or required checks to get a merge through.
- keelstackhq/decisions stays owner-merged without auto-merge (D-002).
- In D-012, "starting release workflows" is no longer owner-only; tags created by hand,
  environment approvals and the rest of that list still are.

## Consequences

- The owner's part of a release is one tap: approve the `release` deployment the orchestrator
  started. The orchestrator says which run to approve.
- Release workflows in hull, splice, bosun, hoist, hoard, berth and keelstack (docspack) gain
  `workflow_dispatch` where missing and create the tag themselves, each in its own gated PR.
- Orchestrators log each release they start (repo, version, commit, reviewer session, run URL).
- Fixing a pushed mistake is still a new commit or a new PR, never a force-push or a rewrite.
