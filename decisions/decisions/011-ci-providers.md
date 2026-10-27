# D-011: GitHub Actions and CircleCI as CI providers

- Date: 2026-10-27
- Scope: all
- Status: accepted
- Supersedes: —

## Context

The owner asked for CircleCI as a second CI provider with every CI job working on both. Governance
(risk-check, review-verdict, main-open) and releases depend on GitHub-only events and approvals.
Design: hull docs/superpowers/specs/2026-10-27-secrets-and-ci-providers-design.md §6-§8.

## Decision

- A repo picks its provider when rendered from hull: no piece = GitHub Actions; `circleci` = both
  (GitHub's ci-ok required, CircleCI advisory); `circleci` + `circleci-only` = CircleCI. The bosun
  channel uses the same pieces as the main template.
- Every CI job (ci-ok, bosun gates, deploy, scheduled desktop/Storybook/nightly) is
  generated for both providers from one step list (hull CI_PLAN) and parity-tested.
- Required check names never change. In CircleCI mode a GitHub `status` workflow mirrors CircleCI's
  terminal `ci/circleci: ci-ok` / `bosun-ok` into `ci-ok` / `bosun-ok`.
- These stay on GitHub Actions in every mode: risk.yml, verdict.yml, main-open.yml, the main-health
  report, splice.yml, release.yml, PR previews (preview.yml, preview-deploy.yml) and the mirror.
- D-007 applies to CircleCI too: local CI mirror before push, one push per PR, setup + one job on
  the PR path, no macOS on the PR path, scheduled pipelines at most weekly.

## Consequences

- hull gains pieces `circleci`, `circleci-only` (also in template-rust); bosun gains matching channel
  pieces and runs gates on CircleCI; hoard runs outside Actions.
- CircleCI production approval is weaker than GitHub environment reviewers; repos that need owner
  approval for prod keep deploy on GitHub.
