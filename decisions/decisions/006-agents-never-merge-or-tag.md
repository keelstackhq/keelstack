# D-006: Agents never merge, tag or force-push

- Date: 2026-10-22
- Scope: all
- Status: accepted
- Supersedes: —

## Context

On 2026-10-22, the auto-mode safety checks in child sessions refused merges, local merges, tag
pushes and force-pushes that were relayed from another session. Repos whose flow depended on an
agent merging or tagging stalled. The flows that kept no agent in that step worked: owner-held
tags (hull v0.1.0–v0.2.2, splice v1) and workflow releases (splice 0.2.0, hoard 0.1.0–0.2.0,
hoist 0.1.0–0.2.0, berth 0.1.0). The owner settled it the same day (bosun decision D26).

## Decision

- Agents never merge a PR (`gh pr merge` without `--auto`, the GitHub merge API, or a local
  `git merge` pushed to a default branch), never push, move or delete a tag, and never force-push.
- An agent opens the PR, gets the required checks green, and may then run `gh pr merge --auto`
  (or the equivalent "enable auto-merge" call) where the repo allows auto-merge. GitHub merges it
  once the required checks pass; otherwise the owner merges.
- Required checks: `ci-ok` in every repo; bosun also requires `risk-check`, `review-verdict` and
  `main-open`. An agent never edits branch protection or required checks to get a merge through.
- Releases and tags come only from the repo's release workflow (`hoist release` or
  `.github/workflows/release.yml`, whichever the repo uses), started the way the repo documents.
- Tag-only repos (hull, splice `v1`) are tagged by the owner. An agent that thinks a tag is due
  says so in the PR or to the owner and stops there.
- keelstackhq/decisions stays owner-merged without auto-merge (D-002).

## Consequences

- A repo or agent flow that ends in "agent merges" or "agent tags" is wrong; change it to end at
  a green PR with auto-merge enabled, a release workflow run, or a note to the owner.
- Orchestrators do not relay merge, tag or force-push commands to child sessions.
- Fixing a pushed mistake is a new commit on the branch or a new PR, not a force-push or rewrite.
