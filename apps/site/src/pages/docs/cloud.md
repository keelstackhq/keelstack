---
layout: ../../layouts/Docs.astro
---

Agents in a keelstack repo run as Claude Code sessions on claude.ai/code. Each session runs in a
cloud environment: a container with a setup script, a network policy and saved settings. Give the
repo an environment of its own and start every session in it. The product's reference for these
settings is [Cloud environments](https://code.claude.com/docs/en/cloud-environments).

## 1. Create a dedicated environment

Create a new environment for the repo instead of using Default. You change its settings from the
cloud environment menu in a session's title bar, then **Edit**.

Every child session must run in it. An orchestrator starts children with the claude-code-remote
`create_session` tool; without an `environment_id` the child inherits the orchestrator's
environment, so start the orchestrator itself in the dedicated environment. To pass the id
explicitly, call `list_environments` and use the `env_…` id of your environment as
`create_session`'s `environment_id`. bosun returns the `create_session` call in `next`; the
orchestrator adds the id when it makes the call.

## 2. Paste the setup script

The setup script runs as root before each session and installs the pinned tools. This is bosun's
own script as of bosun 0.2.3. Since bosun 0.3.0 its `docs/setup-script.sh` installs splice 0.3.0, hoist and berth only from npm tarballs
whose sha512 matches the pinned integrity, and takes the Rust version from the repo's
`rust-toolchain.toml`; paste that version (bosun's `docs/setup-script.sh` at `v0.3.0`) instead. A change to `docs/setup-script.sh` needs
the owner (R-gov).

```bash
#!/usr/bin/env bash
# Setup script for the Claude Code cloud environment of bosun and of repos that use it
# (spec D23). Paste it into the environment's setup script; it runs as root before a session.
set -euo pipefail

# Pinned tool versions. Move a pin with a PR that edits this file (R-ci reviews it).
NODE_MAJOR=24
PNPM_VERSION=12.15.0
RUST_VERSION=1.97
SPLICE_VERSION=0.3.1
HOIST_VERSION=0.2.0
BERTH_VERSION=0.1.0
# The bosun release this repo pins in .bosun/version.
BOSUN_VERSION=0.2.3

# Node 24 from NodeSource, unless it is already there.
if ! node --version 2>/dev/null | grep -q "^v${NODE_MAJOR}\."; then
  curl -fsSL "https://deb.nodesource.com/setup_${NODE_MAJOR}.x" | bash -
  apt-get install -y nodejs
fi

# pnpm 12 ships a native binary; its install.js puts it in place.
npm install -g "pnpm@${PNPM_VERSION}"
(cd "$(npm root -g)/pnpm" && node install.js) || true

# Rust and the cargo tools the gates run.
rustup toolchain install "${RUST_VERSION}" --profile minimal --component rustfmt,clippy
rustup default "${RUST_VERSION}"
cargo install --locked cargo-nextest cargo-deny

# The keelstackhq tools, pinned.
npm install -g "@keelstackhq/splice@${SPLICE_VERSION}" "@keelstackhq/hoist@${HOIST_VERSION}" \
  "@keelstackhq/berth@${BERTH_VERSION}" "@keelstackhq/bosun@${BOSUN_VERSION}"

bosun --version
```

A repo set up with `bosun init` has its own `docs/setup-script.sh`, written by the bosun channel
for its stacks (pnpm only for ts-pnpm, Rust only for rust, the toolchain from the repo's
`rust-toolchain.toml`); paste that one. **Paste it again after every pin change**: the script
installs the bosun release pinned in `.bosun/version`, so a bumped pin with an old script runs
the wrong bosun. The setup script cannot read the environment's variables or secrets.

## 3. Network access

Choose **Limited** with **Allow package managers** ticked, and add these under **Allowed
domains**, or choose **Full**:

```text
github.com
api.github.com
codeload.github.com
raw.githubusercontent.com
objects.githubusercontent.com
release-assets.githubusercontent.com
registry.npmjs.org
crates.io
index.crates.io
static.crates.io
static.rust-lang.org
sh.rustup.rs
deb.nodesource.com
archive.ubuntu.com
security.ubuntu.com
keelstack.dev
```

**Practice:** this is the list keelstackhq uses for the setup script (NodeSource, npm, rustup,
crates.io), GitHub release downloads and these docs. A denied host shows up as a failed command;
add the host and start a new session.

## 4. Secrets and keys

- **The owner's token stays off the cloud environment.** The owner's fine-grained token
  (`BOSUN_OWNER_TOKEN`, for `bosun approve` without a signing key) belongs on the owner's own
  machine only; an approval posted from a cloud session never counts anyway. **Practice:** put no
  personal access token in a cloud environment at all; sessions reach GitHub through the Claude
  GitHub App.
- **The Claude GitHub App** must be installed on the repo (bosun's setup checklist, step 2).
- **The orchestrator's signing key** for chat approvals: the orchestrator makes it in its own
  container (`openssl genpkey -algorithm ed25519 -out key.pem`) and never shares the private key;
  **Practice:** keep it in the orchestrator's scratchpad only. Enroll the public key with a pull
  request to `.bosun/approvers.toml` that the owner approves on GitHub
  ([steps](/docs/risk/#signed-approvals-from-chat)).
- Service tokens a session needs (for example a read-only API token) go in the environment's
  network secrets or variables, never in the chat.

## 5. What the owner does from a phone

**Practice:** most owner steps fit on a phone:

- **Approve a pull request:** say "approve 222" in the orchestrator's chat; it posts the signed
  approval.
- **Release:** open the release workflow run in the GitHub app and choose **Approve and deploy**
  for the `release` environment.
- **Stop everything:** open an issue with the label `bosun:halt` in the GitHub app.

Key, owner and CODEOWNERS changes still need the owner's own approval on GitHub (a review or a
`/bosun approve <sha>` comment typed in the GitHub app).
