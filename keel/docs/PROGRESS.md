# Progress

## Status

M1 in progress on branch `claude/keel-cli`: the Rust CLI (`up`, `plan`, `doctor`, the 9 steps and
their tool adapters) and the npm launcher with five platform packages (0.1.0). Spec:
`docs/superpowers/specs/2026-11-20-keel-design.md`. Plan: `docs/superpowers/plans/2026-11-20-keel-m1.md`.
keel is released with the other tools: one lockstep version, `.github/workflows/release.yml` in the monorepo root (docs/releasing.md).

Deploy, DNS, backup and verify (`[deploy.fly]`, `[backup]`; steps deploy, dns, backup, verify) and
issue #2 land on `claude/keel-deploy-backup-dns`. The fly-setup.sh, hoist `--area`/`--secrets-to` flags (relayed) and
bosun `pr --single` flags are assumed behind adapters until upstream relays the exact ones.

Issue #45: a step timeout kills the whole process group (fly-setup.sh and flyctl stop too); dns says so when hoist
has no dns_record item (not "remove old records"); verify names the newer run when a push cancelled the deploy.

`keel migrate` (spec section 11) moves a consumer repo to a release in one command: tools, splice, bosun pins, hoist,
setup snippet, verification, one PR. Docs: `docs/migrate.md` at the repo root.

## Blocked

Nothing for releases: `keelstack-cli` (crate), `@keelstackhq/keel` and its five platform packages
are published (0.22.1) and release with every lockstep version through the monorepo's
`release.yml` (`docs/releasing.md`).
