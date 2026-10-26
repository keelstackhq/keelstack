# D-008: Who merges, tags and force-pushes

- Date: 2026-10-26
- Scope: all
- Status: accepted
- Supersedes: D-006

## Context

D-006 kept every agent away from merges and tags, so each PR waited for the owner to merge it.
keelstackhq/splice `.github/workflows/release.yml` creates the version tag itself when a
version-bump PR merges (since 2026-10-26). On 2026-10-26 the owner approved replacing D-006 with
an orchestrator merge exception and workflow-created tags.

## Decision

Everything in D-006 stays, except (a) and (b):

- Child sessions (every Claude session that is not the orchestrator) never merge a PR (`gh pr merge`
  without `--auto`, the GitHub merge API, or a local `git merge` pushed to a default branch), never
  push, move or delete a tag, and never force-push. They open the PR, get the required checks green,
  and may run `gh pr merge --auto` where the repo allows auto-merge.
- (a) The orchestrator session (the one Claude session that coordinates child sessions for the
  owner) may merge PRs on the owner's standing authorization, once all required checks are green on
  the head commit and any owner approval the repo requires is in. It never tags or force-pushes.
- Required checks: `ci-ok` in every repo; bosun also requires `risk-check`, `review-verdict` and
  `main-open`. No agent edits branch protection or required checks to get a merge through.
- (b) Release workflows may create the version tag themselves when a version-bump PR merges
  (keelstackhq/splice `.github/workflows/release.yml` does this). The owner still approves the
  `release` environment. Otherwise releases come from the repo's release workflow (`hoist release`
  or `.github/workflows/release.yml`), started the way the repo documents.
- Tag-only repos without such a workflow (hull, splice `v1`) are tagged by the owner. An agent that
  thinks a tag is due says so in the PR or to the owner and stops there.
- keelstackhq/decisions stays owner-merged without auto-merge (D-002).

## Consequences

- Orchestrators do not relay merge, tag or force-push commands to child sessions; the orchestrator
  merges itself when (a) allows it.
- Fixing a pushed mistake is a new commit on the branch or a new PR, not a force-push or rewrite.
- A repo that wants workflow-created tags adds that step to its release workflow, gated by the
  `release` environment.
