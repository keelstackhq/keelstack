# Quickstart

```bash
npm install -g @keelstackhq/hoist        # or: cargo install keel-hoist
export GITHUB_TOKEN=...                  # or `gh auth login`; never pass tokens as arguments
cd your-repo                             # needs a GitHub `origin`
```

1. `hoist explain`: the desired model and where each item came from. No network writes.
2. `hoist plan`: desired vs live GitHub (and Cloudflare, if the repo has a target).
3. `hoist auth check`: which permissions your tokens have vs need.
4. `hoist apply`: shows the plan, asks to confirm (`--yes` in CI), writes, then verifies.
5. `hoist check`: plan + verify, read-only. Run it in CI or after every change.

Exit codes: 0 in sync, 1 drift or failure, 2 a human step is pending, 3 usage, config, auth or
API error. With `--json`, stdout is one object, also on exit 3 (`ok: false`, `error`).

Settings live in the repo files hoist already reads; add a `hoist.toml` only to override.
Releasing: `hoist init` writes `release.yml`
(an existing own `release.yml` is kept: exit 2, step `release-workflow`; add `environment: release` to its
publish job, or run `hoist init --force` to replace it); `hoist release plan` previews (README "The release action").
A new package (never published) needs one local `hoist release bootstrap` (your npm login and 2FA;
it ends with the exact steps left: the crates.io trusted publisher page and the `gh workflow run`
that publishes the rest); `hoist release check` reads, per package, whether it is on its registry.
Every later release: merge the release PR, the workflow publishes through trusted publishing, no
token. A workflow that tags by other means calls `hoist release publish --only crates,npm`.
Agents: read `skills/hoist/SKILL.md` first. Full docs: README.md, docs/SECRETS.md.
