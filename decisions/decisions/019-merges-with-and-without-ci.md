# D-019: Merges with and without CI; `bosun merge`

- Date: 2026-11-10
- Scope: all
- Status: accepted
- Supersedes: D-015

## Context

D-015 lets the orchestrator merge once required checks are green; bosun's checks (`risk-check`,
`review-verdict`, `main-open`) run as GitHub Actions statuses. A repo without Actions or CircleCI,
or one whose setup PR is not merged yet, has no checks, so GitHub auto-merge gates nothing. On
2026-11-10 the owner required that bosun also work without the regular CI and chose option (a) of
keelstackhq/bosun `docs/superpowers/specs/2026-11-10-chat-approval-design.md`: an orchestrator-only
`bosun merge <pr>` that re-checks everything locally.

## Decision

Everything in D-015 stays, plus no-CI merges:

- Child sessions (every session that is not the orchestrator) never merge a PR (`gh pr merge`
  without `--auto`, the GitHub merge API, a pushed local `git merge`, or `bosun merge`), never push,
  move or delete a tag, never start a release workflow, and never force-push. They open the PR, get
  the required checks green, and may run `gh pr merge --auto` where the repo has required checks.
- **Repos with CI:** the orchestrator may merge once all required checks are green on the head and
  any owner approval (D-018) is in. Required checks: `ci-ok` everywhere; bosun repos also
  `risk-check`, `review-verdict`, `main-open`. No agent edits branch protection or required checks.
- **Repos without CI** (`.bosun/bosun.toml` `[ci] provider = "none"`, written by `bosun adopt`/
  `init` when the repo has no `.github/workflows/*` and no `.circleci/`): the orchestrator merges
  only with `bosun merge <pr>`. It refuses unless the caller is an orchestrator (D-018) and, for
  the current head: every gate stamp exists, local risk-check with the base branch's rules passes,
  a clean review verdict from another session exists, and a D-018 approval exists when risk-check
  gates the PR. It posts one summary comment (gates, risk, verdict, approval, head), then merges.
  In these repos auto-merge and every other agent merge path stay refused (bosun PreToolUse hook).
  Until a pinned bosun release ships `bosun merge`, no-CI PRs stop at ready and the owner merges.
- **Releases (unchanged from D-015):** the orchestrator starts a release only through the repo's
  release workflow (`workflow_dispatch` or the version-bump merge), after the PR is merged, CI is
  green on that commit and an independent reviewer approved its head; the workflow creates the
  tag. The owner approves every `release`, `prod`, `preview` and `splice-bot` run; hand-pushed
  tags stay with the owner.
- keelstackhq/decisions stays owner-merged without auto-merge (D-002), except as D-017 allows.

## Consequences

- bosun: `[ci] provider` key; `bosun pr <task> --ready` runs the local checks when it is `none`;
  `provider = "none"` also turns off main-health and the ci-minutes gate; `bosun merge` command;
  hook refuses other merge paths there. bosun spec D13 ("agents never merge") changes to match.
- A no-CI repo has no GitHub-side gate: the local checks and the orchestrator guard are the only
  barrier. Repos with CI keep GitHub's required checks as the gate.
- Fixing a pushed mistake is still a new commit or PR, never a force-push.
