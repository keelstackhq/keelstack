# Progress

## Status

M1 in progress on branch `claude/keel-cli`: the Rust CLI (`up`, `plan`, `doctor`, the 9 steps and
their tool adapters) and the npm launcher with five platform packages (0.1.0). Spec:
`docs/superpowers/specs/2026-11-20-keel-design.md`. Plan: `docs/superpowers/plans/2026-11-20-keel-m1.md`.
Nothing is released or published yet.

Deploy, DNS, backup and verify (`[deploy.fly]`, `[backup]`; steps deploy, dns, backup, verify) and
issue #2 land on `claude/keel-deploy-backup-dns`. The fly-setup.sh, hoist `--area`/`--secrets-to` flags (relayed) and
bosun `pr --single` flags are assumed behind adapters until upstream relays the exact ones.

## Blocked

Human-only items, with the exact action:

- npm scope: make sure the `@keelstackhq` scope is reserved, then publish `@keelstackhq/keel` and
  `@keelstackhq/keel-{linux-x64,linux-arm64,darwin-x64,darwin-arm64,win32-x64}` (first publish
  is done by hoist, see "first release").
- `release` environment: in repo keelstackhq/keel, Settings, Environments, create `release` (for the release workflow, still to be written) and add
  required reviewers (or run `hoist apply` once the repo has a `hoist.toml`).
- crates.io: publish the crate `keel-cli` (the binary is `keel`); needs a crates.io owner token
  (`CARGO_REGISTRY_TOKEN`) stored as a secret on the `release` environment.
- First release: run `hoist release bootstrap` from a checkout with the owner's npm and crates.io
  tokens (bootstrap does the first publish locally); later releases will go through a release workflow that is still to be written (only `.github/workflows/ci.yml` exists).
