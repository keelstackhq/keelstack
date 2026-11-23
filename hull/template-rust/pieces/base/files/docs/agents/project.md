# Project notes

Notes for AI agents that are specific to this repository. `CLAUDE.md` imports this file; edit it
freely, template updates never touch it.

- What this project is:
- Commands beyond the CI steps in `AGENTS.md`:
- Conventions and decisions:

## Blocked

Release hook: `scripts/release-pre.sh` (seed) runs in the `publish` job before `hoist release publish
--assets dist`, with `dist/` downloaded and Node 24 ready (setup, builds). It runs with publish rights
(environment `release`, `id-token: write`): treat it like `release.yml`, which agents may not edit; the
guard hook blocks agent writes to both and a change to it needs the owner's approval. Install there with
`--ignore-scripts` (any command in it can request an id token).

Human steps before the first release (`release.yml`, hoist):

- Create the `splice-bot` and `release-pr` environments (Settings → Environments) with Deployment
  branches = Selected → `main` BEFORE moving `SPLICE_BOT_TOKEN` / `HOIST_RELEASE_TOKEN` into them
  (a first run auto-creates an unprotected environment); delete any repository-level copies.
- Remove `publish = false` from each crate to release, add its `[publish]` table to
  `crates/<name>/hoist.toml`, and set a license.
- `hoist apply --area release` (creates the `release` environment), then set the repository
  variable `HOIST_RELEASE` to `enabled`.
- After the first tag: `hoist release bootstrap` locally (first crates.io publish) and a crates.io
  trusted publisher (this repo, `release.yml`, environment `release`) for each crate.
