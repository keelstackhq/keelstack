# Progress

## Status

hoist 0.5.0 is released (2026-11-15): GitHub Release `v0.5.0` with 5 binaries and `SHA256SUMS`,
crate `keel-hoist`, and `@keelstackhq/hoist` plus five platform packages on npm. Release PR for
0.5.1 (init leaves a repo's own release.yml as a human step, clearer step texts, SKILL docs) is open.
Milestones M1
(read-only), M2 (provisioning), M3 (releasing), M4 (npm launcher) and M5 (secrets repos) are done;
container publish and required-check sources shipped after. Plans: `docs/superpowers/plans/`.

Open work (hardening round, this branch): `--json` error object on exit 3, SHA-pinned CI actions
with a pin check, the `release` composite action (`release/`), the agent skill
(`skills/hoist/SKILL.md`), `apply` defaults tested (`ci-ok` required check, `release`
environment reviewers), npm dist-tag cleanup after publish, immutable releases from `apply`,
release PR opened with a GitHub App token. Release-workflow generation by `apply` is in spec
§12.5 (`hoist init`, done).

Task log: `docs/archive/PROGRESS-done.md`. Quickstart: `docs/QUICKSTART.md`.

- 2026-11-21 Changesets CS-1 (ported, opt-in): `src/changeset/` reads and writes `.changeset/<id>.md` and `config.json` (2.31.1 key set); library only, no CLI or release change yet.
- 2026-11-21 Changesets CS-2 (ported, opt-in): `hoist changeset add|init` (ids from branch or words, names checked against the release model), `status` stub exit 3; CS-1 follow-ups (skip AGENTS/CLAUDE/GEMINI.md, tuple `changelog`). No HTTP, no release change.
- 2026-11-21 Changesets CS-3a (ported, opt-in): the release model keeps every internal dependency edge with its kind (normal, build, dev, peer) and range; `src/changeset/range.rs` checks npm ranges and Cargo requirements (0.x caret rules) and rewrites them keeping the operator. `deps` is unchanged (release plan output the same).
- 2026-11-21 Changesets CS-3 (ported, opt-in): `release plan` plans from `.changeset/*.md` only when `.changeset/config.json` exists (`changeset::plan`: highest type, dependents by range, peer minor -> major, `fixed`/`linked`, `ignore`, launchers follow, pins/skips/awaiting publish as before); without it plan and prepare are unchanged (`release/source` = `commits`). `NextVersion` gains `changesets`, `dep_updates`; spec §9.1, README. `prepare` still commit-based (later task).
- 2026-11-21 Changesets CS-3b (ported, opt-in): npm comparator lists, `||`, `< <= >`, Cargo comma lists; dev ranges of packages that do not release are rewritten (`plan_with_rewrites`).
- 2026-11-21 Changesets CS-4 (ported, opt-in): `release prepare` in changesets mode (only when `.changeset/config.json` exists): plan from changesets, rewrites only planned ranges, changesets changelog layout with dated heading (O1), `## Unreleased` moves, consumed changesets deleted, PR body `## Changesets`; `publish` reads a bare `## <version>` heading; spec §9.2/§9.3, README.
- 2026-11-21 Changesets CS-5 (ported, opt-in): `hoist changeset status [--since REV]`: files changed since the merge base, matched by `changedFilePatterns` per package (launcher and platform dirs count for their crate), need a changeset the diff adds naming the package or an empty one; `hoist/release` exempt (same-repo PR only); changesets read from HEAD via git; no HTTP. Spec §3, README.
- 2026-11-23 Review fixes (#47, #48, #58 nit) and required-check sources (#31, #37, spec §6.1 as built): `required_checks` entries `{ context, app | integration_id }` pin the ruleset's `integration_id` (slug via `GET /apps/{slug}`, exit 3 if unresolvable); unknown-key errors name the right table; `secrets check` on a non-repo or `--remote` without `secrets.toml` is exit 3; shell-quoted `gh variable set` steps; non-Unix `--emit-secrets` refused; `import` does not write pins yet.
- 2026-11-23 `hoist org` part 2 (spec §16.3-§16.5): npm (scope, package, trusted publisher with `NPM_TOKEN`), crates (owners, trusted publisher with `CARGO_REGISTRY_TOKEN`, raw `Authorization`) and Search Console (bridge reads, apply: token, Cloudflare TXT, insert, property, sitemaps) sections; tests on fakes only.

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
  5. Computer terminal (hidden prompt, once): `HOIST_BRIDGE_TOKEN=<same token> HOIST_BRIDGE_SA_KEY_FILE=key.json hoist org apply --area search_console` deploys the bridge and sets its two secrets; delete `key.json`; run it again to verify the domains and sitemaps.
  6. Verify on a canary: the Fly Machines secrets and certificates endpoints and the Worker upload are written from the docs and tested on fakes only (spec §16.7); the crates.io trusted-publisher read with `CARGO_REGISTRY_TOKEN` (spec §16.6).
- 2026-11-25 · release painless · hoist: `release publish --only`, `release check`, bootstrap checklist (spec §9.3.1, §9.6.1); `release` environment gets the owner as required reviewer in hoist.toml (D-028); root release.yml calls hoist; scripts/release-pr.sh; docs/releasing.md · this PR
- 2026-11-26 · control plane (D-029) · hoist org `[fly]` (spec §16.7) and bridge Worker deploy (§16.8), `examples/org.toml`; tests on fakes only · this PR
