# D-010: Secrets live in sops-encrypted secrets repos

- Date: 2026-10-27
- Scope: all
- Status: accepted
- Supersedes: —

## Context

The owner asked for secrets stored in one or more repos with sops, usable from predefined GitHub
Actions and from CircleCI. Repo secrets in GitHub had no review, no history and could be read by any
same-repo PR's workflow, including agent PRs (hull's preview.yml). Design:
hull docs/superpowers/specs/2026-10-27-secrets-and-ci-providers-design.md.

## Decision

- Secrets live sops+age-encrypted in private `keelstackhq/secrets-*` repos rendered from
  `gh:keelstackhq/hull//template-secrets`. `access.toml` is the only access file; `.sops.yaml` is
  generated from it by `hoist secrets sync` and CI-checked.
- One age key pair per (consumer repo, environment) plus at least one owner key per file. hoist
  generates CI keys and stores the private key straight into the GitHub environment secret or the
  CircleCI context `SOPS_AGE_KEY`; nobody sees it.
- Consumers declare what they read in a root `secrets.toml`. CI reads secrets repos with the
  `keelstackhq-secrets-read` GitHub App (contents: read, ciphertext only).
- CI decrypts only with `hoist secrets exec` (the `keelstackhq/hoist/secrets` action, the
  `secrets-run` reusable workflow, or the CircleCI `secrets-exec` command), in jobs whose workflow
  and executed code come from main, a release tag or a `workflow_run` deploying an artifact; never
  on `pull_request`, `pull_request_target`, `issue_comment` or forks. Every GitHub environment and
  CircleCI context holding `SOPS_AGE_KEY` is restricted to `main` (or release tags), and consumers
  pin the action by commit SHA.
- Agents never hold an age private key or see plaintext; decrypting commands are owner- or CI-only.
  Changes to secrets.toml, .sops.yaml, access.toml, *.sops.yaml, workflows and .circleci/** are
  owner-approved (bosun R-secrets).

## Consequences

- hoist gains `secrets check|exec|sync|grant|revoke|rotate-key|edit|repo init`; hull gains the
  `secrets` piece, two-phase previews and `template-secrets`; bosun extends R-secrets and its hooks.
- Repo-level `CLOUDFLARE_API_TOKEN` secrets are retired once a repo has the `secrets` piece.
- Revoking a consumer always means rotating the values it could read.
