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
own, [`docs/setup-script.sh`](https://github.com/keelstackhq/bosun/blob/0d5b909c55e4441e8f556840efc18d24465db76a/docs/setup-script.sh)
at commit `0d5b909` (bosun 0.4.0 pinned), copied verbatim. It installs splice, hoist, berth and
bosun only from npm tarballs whose sha512 matches the pinned `dist.integrity` (a mismatch skips
that tool with a warning), and takes the Rust version from the checkout's `rust-toolchain.toml`.
Since bosun 0.4.0 it installs cargo-nextest and cargo-deny with rustup's stable default when no
`rust-toolchain.toml` exists yet (as before the checkout), and makes Node 24 the `node` that
resolves first when the image has an older Node earlier in `PATH`; a failed step warns and never
fails the script. Paste it again after each bosun release.
A change to `docs/setup-script.sh` needs the owner (R-gov).

```bash
#!/usr/bin/env bash
# Setup script for the Claude Code cloud environment of bosun and of repos that use it
# (spec D23). Paste it into the environment's setup script; it runs as root before a session.
set -euo pipefail

# Pinned tool versions. Move a pin with a PR that edits this file (R-ci reviews it).
NODE_MAJOR=24
PNPM_VERSION=12.15.0
SPLICE_VERSION=0.3.1
# npm's dist.integrity (sha512) of each tool's launcher and linux binary packages, from
# `npm view @keelstackhq/<package>@<version> dist.integrity`; move them with the version.
SPLICE_SHA512=FyJMvUQ4SxjaLtECPp1YmRpMMzYwRaGsdCJ/den/PRZBkkecMyWRLq3v7UECIpJfWuIGWlDmWz48l6X9cNsxeA==
SPLICE_LINUX_X64_SHA512=OtpywXIof4d/eP7dhvt8q3hh5k65qCpE7YtTh9EmA9RqtPtFxksBaxWT3FjWHkqJ7RQm7JCKme2xIBVnBpAhHA==
SPLICE_LINUX_ARM64_SHA512=gJ4y/6sOgGooEkf1flpUUIt9u77T4oe7CBKzCEWXSeGwHyRH7jE3/v/lHFKtaGRF5OhW7sq+wbN3z7uCBQIkxQ==
HOIST_VERSION=0.3.0
HOIST_SHA512=0OO9wf3zQgSmZzGLUkLvjrdhQsUq1zGtyJ4CDi/EprxDmOFxvyxJfPpEDIez0xvwN9UNqhjwDw9x77GrFXeMVQ==
HOIST_LINUX_X64_SHA512=dfsuZovno5ZMtepOLDhYiLQLu9K0hTdAgVVsO4VhwKw6fWnH4PGE3Ks/sqnShT+ajSzN25pETEVV9euIZvxcag==
HOIST_LINUX_ARM64_SHA512=dFY+6R78093CTKpipt3I7c037oz2SDZ8cUgJjH00OXkbrdzTWgHqh7wytOs9/JF56aHW38K1ATS0L0jc21AmVg==
BERTH_VERSION=0.1.0
BERTH_SHA512=4V71F1YqcduupUm/yqc3HaK5/VMH6bD1PZa6xskA+84UiSs1UZRnkX7W+zoLPNizoT2CM1bVGmEtk2cjjYg3Eg==
BERTH_LINUX_X64_SHA512=3eaIWInNsevz8vPlkSN+QsPnf82ULEZJz2X4KFOsP3aQR9EanDBDxmm2+J+jEvUBHKxS3xfQMT7HqCO35TUysw==
BERTH_LINUX_ARM64_SHA512=sG6PA8qfOmThoXxL/3P+DSTnr8OL+5MieaTWBv9QfjzcC/ocITUHqi5xptPh2u+71ZY8SqpEcgE6dPmvmi5YXw==
# The bosun release this repo pins in .bosun/version.
BOSUN_VERSION=0.4.0
BOSUN_SHA512=PgbHQLJg9P14b8zsl1nY3dF8RvY1VT/2YB3TtJr8XfECFhtxRk5ouJitH3LwUClAAMVbVG16laD+3lIb0sD71w==
BOSUN_LINUX_X64_SHA512=heHB2hOzfMu1jlelVPOiG6dTzrN/ep3jIEC2JcxhS/vX/C0FefwGrVUzxGzoEb27g1Ay817XqRsRGp8fnVHpGg==
BOSUN_LINUX_ARM64_SHA512=NiPlqlENitXHnrOZ01Nv5F/xGLiv5CfFWC+lkfAY7hpmwSfm33NaNAav3i1Xj6E30l+jQAwfGasw/wrnyg7GDw==

# Rust is not pinned here: the version is the `channel` of the checkout's rust-toolchain.toml.
# Without one the script warns, skips the pinned toolchain and still installs the cargo tools. `bash docs/setup-script.sh --rust-version` prints the version
# and installs nothing.
RUST_VERSION=""
if [ -f rust-toolchain.toml ]; then
  RUST_VERSION="$(sed -nE "s/^[[:space:]]*channel[[:space:]]*=[[:space:]]*[\"']([^\"']*)[\"'].*/\1/p" rust-toolchain.toml | head -n1)"
fi
# The channel reaches rustup as root: only a toolchain name passes, anything else is skipped like a
# missing file (printf %q shows the rejected value safely).
if [ -n "$RUST_VERSION" ] && ! [[ "$RUST_VERSION" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]; then
  printf 'setup-script: invalid channel %q in rust-toolchain.toml; skipping the Rust toolchain install (the cargo tools still install with the default toolchain)\n' "$RUST_VERSION" >&2
  RUST_VERSION=""
  if [ "${1:-}" = --rust-version ]; then exit 1; fi
elif [ -z "$RUST_VERSION" ]; then
  # Never fail here: a failing setup script blocks every session of the environment.
  echo "setup-script: no rust-toolchain.toml with a channel in $PWD; skipping the Rust toolchain install (the cargo tools still install with the default toolchain), rustup installs the checkout's pinned toolchain on first cargo use" >&2
  if [ "${1:-}" = --rust-version ]; then exit 1; fi
fi
if [ "${1:-}" = --rust-version ]; then echo "$RUST_VERSION"; exit 0; fi

# Node 24 from NodeSource, unless it is already there.
if ! node --version 2>/dev/null | grep -q "^v${NODE_MAJOR}\."; then
  curl -fsSL "https://deb.nodesource.com/setup_${NODE_MAJOR}.x" | bash -
  apt-get install -y nodejs
fi

# The image may put an older Node (/opt/node22/bin) before NodeSource's /usr/bin in PATH. Make Node 24 the
# `node` that resolves first, whatever the PATH order: link it into a directory that later shells put
# first (/etc/profile.d), and replace the older `node` that resolves today. Never fails: it warns.
NODE_BIN_DIR="${SETUP_NODE_BIN_DIR:-/usr/bin}"
NODE_LINK_DIR="${SETUP_NODE_LINK_DIR:-/usr/local/lib/bosun-node}"
NODE_PROFILE="${SETUP_NODE_PROFILE:-/etc/profile.d/bosun-node.sh}"
node_major_ok() { "$1" --version 2>/dev/null | grep -q "^v${NODE_MAJOR}\."; }
if ! node_major_ok "$(command -v node || echo node)"; then
  if node_major_ok "$NODE_BIN_DIR/node"; then
    first="$(command -v node || true)"
    {
      mkdir -p "$NODE_LINK_DIR"
      for b in node npm npx; do [ -e "$NODE_BIN_DIR/$b" ] && ln -sf "$NODE_BIN_DIR/$b" "$NODE_LINK_DIR/$b"; done
      printf 'export PATH=%q:"$PATH"\n' "$NODE_LINK_DIR" >"$NODE_PROFILE"
    } 2>/dev/null || echo "setup-script: could not link Node ${NODE_MAJOR} into $NODE_LINK_DIR or write $NODE_PROFILE" >&2
    if [ -n "$first" ] && [ "$first" != "$NODE_BIN_DIR/node" ]; then
      ln -sf "$NODE_BIN_DIR/node" "$first" 2>/dev/null ||
        echo "setup-script: could not replace $first with $NODE_BIN_DIR/node" >&2
    fi
  fi
  if ! node_major_ok "$(command -v node || echo node)"; then
    echo "setup-script: node resolves to $(command -v node || echo none) ($(node --version 2>/dev/null || echo unknown)), not Node ${NODE_MAJOR} at $NODE_BIN_DIR/node" >&2
  fi
fi

# pnpm 12 ships a native binary; its install.js puts it in place.
npm install -g "pnpm@${PNPM_VERSION}"
(cd "$(npm root -g)/pnpm" && node install.js) || true

# Rust (the version read above) and the cargo tools the gates run.
# Without a toolchain file (the checkout does not exist yet when this runs) rustup's stable default
# serves the tools, and rustup installs the checkout's pinned toolchain on first cargo use there.
# A failed step warns and the script goes on: a failing setup script blocks every session.
if [ -n "$RUST_VERSION" ]; then
  { rustup toolchain install "${RUST_VERSION}" --profile minimal --component rustfmt,clippy &&
    rustup default "${RUST_VERSION}"; } ||
    echo "setup-script: installing the Rust toolchain ${RUST_VERSION} failed; continuing" >&2
elif command -v rustup >/dev/null 2>&1 && ! rustup show active-toolchain >/dev/null 2>&1; then
  { rustup toolchain install stable --profile minimal --component rustfmt,clippy &&
    rustup default stable; } ||
    echo "setup-script: installing the stable Rust toolchain failed; continuing" >&2
fi
if command -v cargo >/dev/null 2>&1; then
  cargo install --locked cargo-nextest cargo-deny ||
    echo "setup-script: cargo install of cargo-nextest and cargo-deny failed; continuing without them" >&2
else
  echo "setup-script: no cargo found; skipping cargo-nextest and cargo-deny" >&2
fi

# The keelstackhq tools, pinned, install only from npm tarballs that match the pinned sha512:
# each launcher and this machine's binary package, side by side. A mismatch or a failed install
# skips that tool with a warning (a failing setup script blocks every session).
tool_pack() {
  f="$(cd "$2" && npm pack --silent "$1")"
  got="$(node -e 'process.stdout.write(require("crypto").createHash("sha512").update(require("fs").readFileSync(process.argv[1])).digest("base64"))' "$2/$f")"
  if [ "$got" != "$3" ]; then echo "setup-script: $1 does not match its pinned sha512; skipping it" >&2; return 1; fi
  echo "$2/$f"
}
# install_tool <name> <version> <launcher sha512> <linux-x64 sha512> <linux-arm64 sha512>
install_tool() {
  case "$(uname -m)" in aarch64 | arm64) a=arm64 s="$5" ;; *) a=x64 s="$4" ;; esac
  t="$(mktemp -d)"
  if l="$(tool_pack "@keelstackhq/$1@$2" "$t" "$3")" && b="$(tool_pack "@keelstackhq/$1-linux-$a@$2" "$t" "$s")"; then
    # npm still reads the optional packages' metadata: a registry error must not stop the script.
    npm install -g --ignore-scripts --omit=optional "$l" "$b" ||
      echo "setup-script: $1 install failed; skipping $1" >&2
  fi
}
install_tool splice "$SPLICE_VERSION" "$SPLICE_SHA512" "$SPLICE_LINUX_X64_SHA512" "$SPLICE_LINUX_ARM64_SHA512"
install_tool hoist "$HOIST_VERSION" "$HOIST_SHA512" "$HOIST_LINUX_X64_SHA512" "$HOIST_LINUX_ARM64_SHA512"
install_tool berth "$BERTH_VERSION" "$BERTH_SHA512" "$BERTH_LINUX_X64_SHA512" "$BERTH_LINUX_ARM64_SHA512"
install_tool bosun "$BOSUN_VERSION" "$BOSUN_SHA512" "$BOSUN_LINUX_X64_SHA512" "$BOSUN_LINUX_ARM64_SHA512"

bosun --version || echo "setup-script: bosun is not installed" >&2
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
