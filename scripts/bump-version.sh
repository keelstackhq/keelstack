#!/usr/bin/env bash
# Lockstep version bump (D-023): every tool crate and npm launcher moves to one X.Y.Z.
# Usage: bash scripts/bump-version.sh X.Y.Z   (from anywhere; edits files, never commits or tags).
# The tag vX.Y.Z is created by the release workflow after the bump merges, never by hand.
set -euo pipefail
v="${1:-}"
[[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "usage: $0 X.Y.Z" >&2; exit 2; }
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
# Root workspace version: the only `version =` line under [workspace.package].
sed -i.bak -E '/^\[workspace\.package\]/,/^\[/ s/^version = "[^"]*"/version = "'"$v"'"/' Cargo.toml
rm -f Cargo.toml.bak
grep -q "^version = \"$v\"" Cargo.toml || { echo "bump-version: no [workspace.package] version in Cargo.toml" >&2; exit 3; }
# npm launchers and platform packages of each tool (cargo-deny's repackaging keeps its own version).
node - "$v" <<'JS'
const fs = require("fs"), path = require("path");
const v = process.argv[2];
const tools = ["bosun", "splice", "hoist", "berth", "keel"];
let n = 0;
for (const t of tools) {
  const dir = path.join(t, "npm");
  if (!fs.existsSync(dir)) continue;
  for (const d of fs.readdirSync(dir)) {
    if (d !== t && !d.startsWith(t + "-")) continue;
    const f = path.join(dir, d, "package.json");
    if (!fs.existsSync(f)) continue;
    let s = fs.readFileSync(f, "utf8");
    s = s.replace(/^(  "version": )"[^"]*"/m, `$1"${v}"`);
    s = s.replace(new RegExp(`("@keelstackhq/${t}-[a-z0-9-]+": )"[^"]*"`, "g"), `$1"${v}"`);
    fs.writeFileSync(f, s);
    n++;
  }
}
console.log(`bump-version: ${n} package.json files at ${v}`);
JS
# splice's GitHub Action pins the splice npm version it installs (splice/scripts/check-version.mjs).
sed -i.bak -E '/^  version:/,/^  [a-z]/ s/^(    default: )[0-9]+\.[0-9]+\.[0-9]+$/\1'"$v"'/' splice/action/action.yml
rm -f splice/action/action.yml.bak
cargo update --workspace --offline >/dev/null
echo "bump-version: Cargo.toml and Cargo.lock at $v"
