# Progress

## Status

hoist 0.5.0 is released (2026-11-15): GitHub Release `v0.5.0` with 5 binaries and `SHA256SUMS`,
crate `keel-hoist`, and `@keelstackhq/hoist` plus five platform packages on npm. Release PR for
0.5.1 (init leaves a repo's own release.yml as a human step, clearer step texts, SKILL docs) is open.
Milestones M1
(read-only), M2 (provisioning), M3 (releasing), M4 (npm launcher) and M5 (secrets repos) are done;
container publish and required-check sources shipped after. Plans: `docs/superpowers/plans/`.

`hoist system init|check|explain|stamp`, `system.toml`, `check --live` and auto rotation (D-031,
spec §17) are in review (PR #145, second review's findings fixed); weekly `system-check` in hull.

The hardening round (JSON errors, pinned CI actions, release action, agent skill) is done; see the task log.

Task log: `docs/archive/PROGRESS-done.md`. Quickstart: `docs/QUICKSTART.md`.

## Blocked

Human-only items, with the exact action:

- Cloudflare dogfood: needs `CLOUDFLARE_API_TOKEN` (Pages read) and `CLOUDFLARE_ACCOUNT_ID`. Run
  `hoist plan`, `hoist check` and `hoist auth check` without `--area` on a repo with a Pages
  target and record Pages and custom-domain results in spec §13.
- GitHub dogfood with a token that reads rulesets, secrets, environments and Actions permissions
  (Administration, Secrets, Environments, Actions read), on a repo whose plan supports rulesets,
  outside the sandbox proxy: verifies those API field shapes.
- Dogfood `hoist apply` on a real repo: needs the owner's go-ahead, a GitHub token with
  Administration/Secrets/Environments/Actions write and a Cloudflare token with API Tokens: Edit.
- Release PR checks: create the GitHub App (Contents and Pull requests: write) and store its id (variable `HOIST_RELEASE_APP_ID`) and its
  key (secret `HOIST_RELEASE_APP_KEY`); without them `release prepare` opens the PR
  with `GITHUB_TOKEN` (no checks), prints a warning and the workflow stays green (CLI exit 2).
- `hoist org` on the real accounts (never run yet; fakes only). Owner steps, in order. Credentials go in the cloud environment's credential form (network secrets, so the proxy injects the real value; the sandbox holds `proxy-injected`), never pasted in chat:
  1. Computer: create the tokens: Cloudflare API token (Zone DNS Edit, Pages Read, Workers Scripts Edit; plus `CLOUDFLARE_ACCOUNT_ID`), npm granular token (read), crates.io API token, Fly org token (`fly tokens create org -o <org>`), a GitHub fine-grained read token for the org.
  2. Computer: Google Cloud project, enable the Site Verification API and the Search Console API, create a service account and a JSON key; in Cloudflare create the account's workers.dev subdomain (once).
  3. Phone or computer: add `FLY_API_TOKEN`, `CLOUDFLARE_API_TOKEN`, `NPM_TOKEN`, `CARGO_REGISTRY_TOKEN`, `GITHUB_TOKEN` and `HOIST_SEARCH_CONSOLE_TOKEN` (`openssl rand -hex 32`, the same value as the Worker's `BRIDGE_TOKEN`) to the credential form, allowed hosts `api.machines.dev`, `api.cloudflare.com`, `registry.npmjs.org`, `crates.io`, `api.github.com` and the Worker host.
  4. Fill `examples/org.toml` (private owner repo), run `hoist org check` (read only; fix what it lists), then `hoist org apply --yes --area cloudflare --area fly`.
  5. Computer terminal, secrets off the command line: `read -rs HOIST_BRIDGE_TOKEN; export HOIST_BRIDGE_TOKEN` (same value as `HOIST_SEARCH_CONSOLE_TOKEN`), `export HOIST_BRIDGE_SA_KEY_FILE=key.json`, then `hoist org apply --area search_console` (or just run it and answer the hidden prompts); delete `key.json`; run it again to verify the domains.
  6. Verify on a canary: the Fly Machines secrets and certificates endpoints and the Worker upload are written from the docs and tested on fakes only (spec §16.7); the crates.io trusted-publisher read with `CARGO_REGISTRY_TOKEN` (spec §16.6).
