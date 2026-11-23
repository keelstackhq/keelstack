#!/usr/bin/env bash
# Repo-specific release setup. release.yml runs this as `bash scripts/release-pre.sh` in the publish
# job, right before `hoist release publish --assets dist`: the checkout is there, the binaries are
# downloaded into dist/ and Node 24 is available. Use it for toolchain setup and anything hoist does
# not do itself (an npm package's own build step belongs in hoist.toml `[publish] npm`). A non-zero
# exit fails the release. Delete the file to opt out.
# It runs with publish rights (environment `release`, id-token: write): owner-only, like release.yml
# (the guard hook blocks agent writes to it).
set -euo pipefail

# Example: build an npm package with pnpm through corepack.
# corepack enable
# pnpm install --frozen-lockfile
# pnpm build
