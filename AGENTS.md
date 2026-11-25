# Agents in keelstack

One monorepo for the keelstackhq tools (D-023). Agents write all the code; bosun runs the process.

## Layout

| Folder | What | Package / checks |
| --- | --- | --- |
| `bosun/` | orchestration CLI | `keel-bosun` |
| `splice/` | template renderer; owns the contract `splice/docs/template-format.md` | `keel-splice` |
| `hull/` | templates (`hull/template`, `hull/template-rust`), pnpm, not in the Cargo workspace | `pnpm --dir hull` |
| `hoist/` | GitHub/Cloudflare provisioning, release + secrets actions | `keel-hoist` |
| `hoard/` | CI cache server + GitHub Action (`hoard/action`) | `keel-hoard` |
| `berth/` | per-agent workspaces | `keel-berth` |
| `keel/` | bootstrap CLI | `keelstack-cli` (binary `keel`) |
| `site/` | landing page (keelstack.dev), own pnpm workspace; every fact on it is generated from the folders above (D-024) | `pnpm --dir site run ci` |
| `decisions/` | org decisions, read in place: `decisions/DECISIONS.md` | `bash decisions/scripts/check-index.sh` |

Root only: `Cargo.toml` (workspace, version, pins), `Cargo.lock`, `rust-toolchain.toml`,
`deny.toml`, `.cargo/config.toml` (writable `TMPDIR=/tmp` for tests). No per-folder copies.

## Commands (run from the repo root)

```sh
cargo fmt --all --check
cargo clippy -p <crate> --all-targets --locked -- -D warnings   # or --workspace
cargo test -p <crate> --locked                                  # or --workspace
pnpm --dir hull install && pnpm --dir hull test && pnpm --dir hull lint && pnpm --dir hull format:check
```

Run the checks of every folder you touch and of the folders that depend on it (keel calls splice,
bosun and hoist; hull templates are rendered by splice). CI builds and tests only affected
folders and has one required check, `ci-ok` (D-007: run it locally first, push once per PR).

## Releases (lockstep)

- One version for every tool: `[workspace.package] version` in the root `Cargo.toml`, mirrored
  by every npm launcher and platform package. Change it only with
  `bash scripts/release-pr.sh X.Y.Z` (folds fragments, runs `scripts/bump-version.sh`, updates
  `Cargo.lock`, commits on `release/vX.Y.Z`), then a version-bump PR. How to release: `docs/releasing.md`.
- One tag `vX.Y.Z` and one GitHub release with every binary, created by the release workflow
  only. Agents never create tags or releases, never push tags, never publish by hand (D-006).

## Decisions that shape the work

- D-022 as amended by D-028: the orchestrator signs (or the CI signer signs) and merges every PR
  as soon as required checks are green and ONE clean independent verdict (>= 0.9, no blocker)
  is on the exact head. Only blockers (security, data loss, broken main, broken release) block;
  other findings go to the rolling `bosun:followups` issue (`bosun followups add`). A red main
  only warns. The owner approves what the signer refuses (.github/**, scripts, R-ci, secrets,
  environments), batched, and taps `release` once per release. Children never merge, tag,
  release or force-push.
- D-028: one requirement, one issue, one branch, one big PR; in-session implementer subagents by
  default; child sessions only for separate requirements, checked every 20-30 min.
- D-023: one monorepo, one version, one release; D-003 ended (one contract file); old repos are
  archived, their `gh:keelstackhq/<tool>` sources still render. New sources:
  `gh:keelstackhq/keelstack//hull/template`, `gh:keelstackhq/keelstack//decisions/channel`
  (only for repos outside the monorepo), actions `keelstackhq/keelstack/<tool>@vX`.
- All other decisions: `decisions/DECISIONS.md`. Change one with a PR to `decisions/` (D-002).

## Where notes live

- Per folder: `<folder>/CLAUDE.md` (+ `bosun/AGENTS.md`, `keel/AGENTS.md`); read it before editing.
- The loop for one task: `bosun/AGENTS.md` ("The loop for one task"); `.claude/loop.md` is the
  short form. Subagents: `.claude/agents/` (explorer haiku, implementer sonnet, reviewer opus,
  ci-fixer sonnet).
- Import history and old repos: `MIGRATION.md`, `MIGRATION-ARCHIVE.md`.
