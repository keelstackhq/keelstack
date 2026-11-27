---
name: hoist
description: Provision and verify a repo's GitHub, Cloudflare and release setup with the hoist CLI. Use when asked to set up, check, diff or release a repo's settings, rulesets, environments, secrets, Pages or npm/crates publishing.
---

# hoist

hoist reads what the repo says (workflows, `wrangler.jsonc`, `package.json`, `hoist.toml`) and
makes GitHub and Cloudflare match. Always pass `--json` and parse stdout (one object).

## Happy path (run in this order, `--json`)

1. `hoist plan`: read it; fix exit 3 first (`hoist auth check` for token problems).
2. `hoist apply --yes`: only after the user's go-ahead. Writes GitHub/Cloudflare, then verifies.
3. `hoist init`: writes `.github/workflows/release.yml` (and `ci.yml` with the `ci-ok` gate when
   the repo has none). You commit and push the files. Exit 2 lists what the owner must do.
4. `hoist check`: exit 0 means in sync. Exit 1: run `plan`, then `apply` again.

## Other commands

- Understand the setup: `hoist explain`. Existing repo without `hoist.toml`: `hoist import`.
- Limit any of plan/apply/check with `--area github|cloudflare|npm|crates|release`.
- Org (`org.toml`, example `examples/org.toml`: GitHub org, Cloudflare zones, npm scope, crates owners, Search Console, Fly.io; the closed list of six, D-029): `hoist org plan|check` (GET only; exit 2 lists human steps such as `npm trust github ...`, `cargo owner --add ...`), `hoist org apply --yes` writes Cloudflare, the Search Console bridge Worker and sites, and Fly apps and certificates (GitHub only in Actions with an app token; never a Fly secret value; Worker secrets come from `HOIST_BRIDGE_TOKEN`/`HOIST_BRIDGE_SA_KEY(_FILE)` or a hidden prompt). `FLY_API_TOKEN` (dummy `proxy-injected` under a proxy) is the Fly credential; a missing one makes Fly items `unknown`. Missing registry/bridge tokens make items `unknown`, never exit 3 (except `apply`). Spec section 16.
- System description (`system.toml`, D-031, spec §17; offline, no HTTP, never values): `hoist system init` drafts it (only if absent; `--print` to preview, fill every `"TODO"`), `hoist system check` after adding or changing any secret or provider (exit 1 undeclared/unused/expired/missing metadata/`max_ttl`/long-lived where keyless exists, 2 rotation due, 3 bad file), `hoist system explain --write` regenerates `SYSTEM.md` (`--check` in CI; `--catalog PATH`, env `HOIST_CATALOG` or `catalog.toml` next to system.toml adds each provider's alternatives with their migration notes; pass the same catalog to `--write` and `--check`). `hoist system check --live` also asks each provider (GET only; tokens from env; `unknown` + fix when a token or permission is missing, never a failure): extra kinds `live` (per credential), `live_storage` (declared storage present / age), `undeclared` (a secret at GitHub or Fly no credential declares). Rotate a credential with `hoist secrets rotate <credential-id>`: `auto` (Cloudflare, Fly) creates, tests, seals into every storage, stamps system.toml, then revokes the old last (exit 0; 1 failed, old kept; a partial set adds human step `partial:<id>` naming the new token and storages and records it in `rotation.pending_ref`, fix the storage and rerun: the rerun revokes it); `assisted`/`manual` exit 2 with the human steps — after doing them run `hoist system stamp <id> --created YYYY-MM-DD --expires YYYY-MM-DD [--by WHO] [--ref REF]`. `--json` items have `area` `system`; `kind` is `undeclared|unused|consumer|expiry|rotation|metadata|max_ttl|keyless|credential|live|live_storage|stamp`.
- Secrets: `hoist secrets set|rotate` (`set NAME --from-file key.pem` for multi-line values such as PEMs; the prompt reads one line and refuses a pasted PEM); shared secrets repos: `grant|revoke|sync`. `hoist secrets check`
  in a repo with no `access.toml` or `secrets.toml` exits 0 (one `secrets/config` item, `ok`).
- Releases: `hoist release plan` (preview), `prepare` (release PR), `publish` (CI only; a workflow
  that tags itself runs `publish --only crates,npm [--npm-dir D] [--provenance] [--npm-tag T]`:
  idempotent, exit 2 = a package is not on its registry yet), `check` (read-only: per package
  published?, newest vs workspace, trusted publisher `unknown`), `bootstrap` (first publish and
  trusted publishers of new packages, local, human; ends with the exact `gh workflow run` to
  publish the rest and, for crates, the crates.io `/settings/new-trusted-publisher` step).
- Changesets (no HTTP): `hoist changeset init [--from-commits]` writes `.changeset/config.json` and
  `README.md`; `changeset add <pkg>:<major|minor|patch|none>... -m TEXT [--empty] [--id ID]` writes
  `.changeset/<id>.md`; `changeset status [--since REV]` exits 1 when a changed package has no
  changeset. You commit the files; `release prepare` then consumes them and moves a changelog's
  `## Unreleased` notes under the new version.
- `init` human steps: `ci-ok` (hand-written workflow needs the job), `ci-ok-npm-test` (npm `test`
  script is the placeholder; the gate runs cargo only), `ci-ok-duplicate` (delete hoist's ci.yml),
  `release-workflow` (repo has its own release.yml, left alone: keep it and add `environment: release`
  to its publish job, or run `hoist init --force` to replace it), `release-workflow-build` (npm
  launcher repo: init wrote no file; the release workflow must build the binaries, then run
  `hoist release publish --assets <dir>`), `release-workflow-race:<file>` (another workflow publishes:
  remove it or stop it publishing; `[registry.*]` workflows are exempt).
- `[publish] npm = { build = "<cmd>" }`: `release publish` runs it in the package directory before
  publishing that npm package (not on `--dry-run`, not when the version is already on npm).
- `apply --dry-run` does not exist: use `hoist plan`.
- `plan-limit:rulesets` (exit 2): the GitHub plan gives this private repo no rulesets, so branch
  protection is not enforced; only the owner can fix it (upgrade the plan or make the repo public).
- `release publish --dry-run` without `--assets` plans the release before any build (binaries
  counted, not checked); the real publish needs `--assets DIR`.
- Item kinds you will see besides settings and secrets: `environment` (every workflow `environment:`,
  also `with: {environment: X}` on a reusable-workflow call, plus `[github.environments.<name>]`
  in `hoist.toml`: `branches = [..]` deployment branch policies, `manage = false` to leave one alone;
  an `unknown` one whose key is `${{ ... }}` is an expression hoist cannot resolve: name the real
  environments in `hoist.toml`; an `update` says which branch policy is missing, `apply` adds it and
  never deletes extras) and `variable` / `optional_variable` (a workflow's `vars.X`).

- Monorepo keys (root `hoist.toml`): `[workspace] members` (extra package/target dirs), `[github] apps`
  (human step `app:<slug>`, url `https://github.com/apps/<slug>/installations/new`), `[github.tag_ruleset]`
  (`patterns`; `bypass = ["github-actions"]` is required with `[github] tags_bypass = "actions"`, the default, and unset with `"deploy_key"`, where `hoist apply` adds the write deploy key `hoist: release tags` and seals its private key as secret `RELEASE_TAG_KEY` of the release environment: item `github/tag_deploy_key`; human step `deploy-keys-policy` (exit 2) when the organization disallows deploy keys; item `github/tag_ruleset` key `hoist: tags`), `[github] admin_bypass = true` (opt-in: the admin role may merge a PR past `hoist: default branch`, never push), `[release]`
  `reviewers = []` (no reviewer, no `release-reviewer` step) and `workflow = "release.yml"`,
  `[registry.npm."<name>"]` / `[registry.crates."<name>"]` `workflow` (packages outside the release model:
  `npm-trust:`/`crates-trust:`/`first-publish:` steps), target `[deploy] environment` (deploy secrets live in
  that environment), `[publish] crates = false`. Crate `first-publish:<name>` points at `hoist release bootstrap`, which also covers every `[registry.npm]` entry (placeholder + trusted publisher with the entry's workflow). npm drops a trusted publisher that no publish used within 48 h: bootstrap's trust items and last line print the deadline; publish before it.

## Exit codes

0 in sync. 1 drift, failed write or failed verification. 2 a human step is pending. 3 usage, config,
auth or API error: read `error.message` and `error.fix` in the JSON, fix, run again. Do not retry 3.

## JSON

`{version, command, ok, exit_code, items[], human_steps[], error?}`. An item has `area`, `kind`,
`key`, `action` (`create|update|ok|drift|pending|unknown`), `reason`, `source`. `unknown` means
hoist could not read it (often a plan or permission limit), not that it is fine.

## Needs a human

Anything in `human_steps` (create a token, install an app, set a repo secret, set a repo variable
(`variable:<NAME>`: the title is the exact `gh variable set NAME --repo o/r --body '<value>'`; ask the
user for the value, or put it in `hoist.toml` `[github.variables]` and `apply`), approve an npm
publisher). Report each step's `title` and `url`; do not try to do them. Never print, log or pass
token values as arguments; tokens come from env or `gh auth token`.

## Rules

- Run `plan` before `apply`; never `apply` without the user's go-ahead.
- `hoist.toml` unknown keys are an error. Fix the key; do not delete the file.
- Releases, tags, npm and crates publishing are human-approved; never run `publish` locally.
