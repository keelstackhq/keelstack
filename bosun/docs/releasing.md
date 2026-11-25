# Releasing bosun

bosun releases with every other tool of the monorepo: one lockstep version, one tag `vX.Y.Z`, one
workflow (`.github/workflows/release.yml` in the repository root). The whole flow, for keelstack
and for a consumer repo, is in the root `docs/releasing.md`. Agents never tag or publish.

What a bosun release publishes: five static binaries and `SHA256SUMS` on the GitHub Release
(`bosun-<target>[.exe]`), the npm launcher `@keelstackhq/bosun` with its platform packages
`@keelstackhq/bosun-{linux-x64,linux-arm64,darwin-x64,darwin-arm64,win32-x64}`, and the crate
`keel-bosun`.

Fragments: bosun's `changelog.d/` and `docs/progress.d/` fragments are folded by
`bash scripts/release-pr.sh X.Y.Z` (repository root) before it bumps the version; it runs
`bash scripts/fold-fragments.sh` with bosun's folder as the root. Outside a release, run the same
script yourself from `bosun/`.

`tests/release_config.rs` keeps bosun's versions, names and pins in step with the manifests.

## After a release: pin it (R-gov PR)

Done for v0.1.0 (the first pin also switched `ci.yml` and the setup script to npm, see below)
and for v0.2.0 (#205). For a later release, the PR sets `.bosun/version` and `BOSUN_VERSION` in
`docs/setup-script.sh` to the new version (and its three `BOSUN_*_SHA512` to the `npm view
@keelstackhq/<package>@<version> dist.integrity` of `bosun`, `bosun-linux-x64` and `bosun-linux-arm64`) and updates the Status in `docs/PROGRESS.md`. No new paste
is needed: the pasted bootstrap runs each checkout's `docs/setup-script.sh`. The gates run the pinned release, whose config parser rejects unknown keys: a key that
release adds to `.bosun/bosun.toml` (such as `[approvals] bootstrap_until`, D-012) is set in this
same pin PR, never before, and only then does the release need an entry in the `KNOWN` tables of
`tests/pinned_config.rs`: an unlisted release is checked against the newest older entry. A release
that removes or renames a key breaks `release_tables_only_grow`: change that test and `known_for`
with it so no lookup falls back across that release.

For v0.1.0, an agent opened one PR, and the owner approved it on its head SHA (R-gov, R-ci). It:

- sets `.bosun/version` to `0.1.0`;
- changes job `bosun-bin` in `ci.yml` to install the pinned release
  (`npm install -g @keelstackhq/bosun@$(cat .bosun/version)`) instead of building the base
  commit (spec §11);
- changes `docs/setup-script.sh` to install `@keelstackhq/bosun@0.1.0` with the other tools,
  instead of `cargo install --git … --branch main`;
- updates the Status in `docs/PROGRESS.md`.

## cargo-deny on npm (not part of the bosun release)

`.github/workflows/cargo-deny-npm.yml` republishes upstream cargo-deny's prebuilt Linux binaries
(the `cargo-deny-<version>-{x86_64,aarch64}-unknown-linux-musl.tar.gz` assets of the
EmbarkStudios/cargo-deny GitHub releases), so setup scripts install cargo-deny in seconds instead
of building it (145 s). Three packages, all `MIT OR Apache-2.0` like upstream:

- `@keelstackhq/cargo-deny-linux-x64` and `@keelstackhq/cargo-deny-linux-arm64`: upstream's
  binary, unchanged, in `bin/cargo-deny`, with upstream's `LICENSE-*` files;
- `@keelstackhq/cargo-deny`: the launcher (`npm/cargo-deny/bin/cargo-deny.js`, bin `cargo-deny`).

The npm version is the upstream version. The owner starts each run by hand, on `main`
(Actions → cargo-deny-npm → Run workflow, input `version`), and approves the `release`
deployment. The run checks each tarball against upstream's `.sha256`, runs `cargo-deny
--version`, sets the version in the three `package.json` files, and publishes the platform
packages, then the launcher, through npm trusted publishing with provenance. A re-run skips what
npm already has. `pnpm-workspace.yaml` lists the bosun packages one by one and leaves
`npm/cargo-deny*` out, so hoist never releases these packages with bosun.

One-time setup (owner, computer): npm adds a trusted publisher only to a package that exists, so
publish a `0.0.0` placeholder of each new package with your own npm login, then add the trusted
publisher (repo `keelstackhq/bosun`, workflow `cargo-deny-npm.yml`, environment `release`).
Never `npm publish` inside `npm/cargo-deny*`: its `package.json` carries a real version without
the binary, npm versions are final, and the workflow then fails for that version.

```bash
npm install -g npm@12.2.0 && npm login
for p in cargo-deny-linux-x64 cargo-deny-linux-arm64 cargo-deny; do
  d=$(mktemp -d)
  printf '{"name":"@keelstackhq/%s","version":"0.0.0","license":"MIT OR Apache-2.0","description":"placeholder"}\n' "$p" > "$d/package.json"
  (cd "$d" && npm publish --access public)
done
for p in cargo-deny-linux-x64 cargo-deny-linux-arm64 cargo-deny; do
  npm trust github "@keelstackhq/$p" --repo keelstackhq/bosun --file cargo-deny-npm.yml \
    --env release --allow-publish
done
```

(Or on npmjs.com: each package → Settings → Trusted Publisher → GitHub Actions, the same values.)

Then run the workflow once with version 0.20.2 or newer. Check the result with
`npm view @keelstackhq/cargo-deny version`.
