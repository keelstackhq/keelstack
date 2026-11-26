import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { actionPin, checkVersion } from "./check-version.mjs";

const cargo = (v) => `[package]\nname = "keel-splice"\nversion = "${v}"\n`;
const platforms = JSON.parse(readFileSync(new URL("../npm/splice/platforms.json", import.meta.url), "utf8"));
const launcher = (version = "0.2.0", dep = "0.2.0") => ({
  version,
  optionalDependencies: Object.fromEntries(platforms.map((p) => [p.npm, dep])),
});

test("tag differing from Cargo.toml throws", () => {
  assert.throws(
    () => checkVersion({ tag: "v0.3.0", cargoToml: cargo("0.2.0") }),
    /tag v0\.3\.0 does not match Cargo\.toml 0\.2\.0/,
  );
});

test("non-semver tag is not a release tag", () => {
  assert.throws(() => checkVersion({ tag: "v1", cargoToml: cargo("0.2.0") }), /not a release tag/);
});

test("matching inputs return the version", () => {
  assert.equal(checkVersion({ tag: "v0.2.0", cargoToml: cargo("0.2.0") }), "0.2.0");
  assert.equal(checkVersion({ tag: "v0.2.0-rc.1", cargoToml: cargo("0.2.0-rc.1") }), "0.2.0-rc.1");
});

test("null tag returns the Cargo version", () => {
  assert.equal(checkVersion({ tag: null, cargoToml: cargo("0.2.0") }), "0.2.0");
});

test("launcher version mismatch names npm/splice/package.json", () => {
  assert.throws(
    () => checkVersion({ tag: "v0.2.0", cargoToml: cargo("0.2.0"), launcherPkg: launcher("0.1.0") }),
    /npm\/splice\/package\.json version 0\.1\.0/,
  );
});

test("optional dependency pin mismatch names the package", () => {
  assert.throws(
    () => checkVersion({ tag: null, cargoToml: cargo("0.2.0"), launcherPkg: launcher("0.2.0", "0.1.0") }),
    /@keelstackhq\/splice-linux-x64.*0\.1\.0/,
  );
});

test("missing optional dependency for a platform throws", () => {
  const pkg = launcher();
  delete pkg.optionalDependencies["@keelstackhq/splice-win32-x64"];
  assert.throws(
    () => checkVersion({ tag: null, cargoToml: cargo("0.2.0"), launcherPkg: pkg }),
    /missing optionalDependencies.*@keelstackhq\/splice-win32-x64/,
  );
});

test("consistent launcher passes", () => {
  assert.equal(checkVersion({ tag: "v0.2.0", cargoToml: cargo("0.2.0"), launcherPkg: launcher() }), "0.2.0");
});

// The monorepo's one release workflow (D-023) builds splice; it starts on the merge of a
// version-bump PR or a manual run, never on a tag push, so moving the Action's major tag (v0, v1)
// never starts a release, and the tag it creates is always v<exact X.Y.Z>.
test("the root release.yml never runs on a tag push and tags only an exact X.Y.Z", () => {
  const yml = readFileSync(new URL("../../.github/workflows/release.yml", import.meta.url), "utf8");
  const on = yml.slice(yml.indexOf("\non:\n"), yml.indexOf("\npermissions:"));
  assert.doesNotMatch(on, /\n  push:|tags:/);
  assert.match(yml, /echo "tag=v\$version"/);
  const re = new RegExp(yml.match(/re='([^']+)'/)[1]);
  assert.ok(re.test("0.2.0"));
  assert.ok(!re.test("1"));
  assert.ok(!re.test("1.0"));
  assert.ok(!re.test("v0.2.0"));
});

test("the root release.yml build matrix covers exactly the platforms.json targets", () => {
  const yml = readFileSync(new URL("../../.github/workflows/release.yml", import.meta.url), "utf8");
  const targets = [...yml.matchAll(/^\s+- target: ([a-z0-9_-]+)\s*$/gm)].map((m) => m[1]).sort();
  assert.deepEqual(targets, platforms.map((p) => p.target).sort());
});

test("the real npm/splice/package.json matches the workspace Cargo.toml", () => {
  const cargoToml = readFileSync(new URL("../../Cargo.toml", import.meta.url), "utf8");
  const launcherPkg = JSON.parse(readFileSync(new URL("../npm/splice/package.json", import.meta.url), "utf8"));
  const actionYml = readFileSync(new URL("../action/action.yml", import.meta.url), "utf8");
  // action/action.yml pins the last splice published on npm (0.6.0 until the first monorepo
  // release), never an unreleased version: it may lag the workspace version, never lead it.
  // scripts/bump-version.sh moves it to X.Y.Z in the version-bump PR whose merge releases X.Y.Z
  // (scripts/bump-version.test.sh checks that), and with a tag the two must be equal.
  assert.equal(checkVersion({ tag: null, cargoToml, launcherPkg, actionYml }), launcherPkg.version);
  if (actionPin(actionYml) !== launcherPkg.version) {
    assert.throws(
      () => checkVersion({ tag: `v${launcherPkg.version}`, cargoToml, actionYml }),
      /action\/action\.yml pins splice/,
      "a release tag needs the action default bumped to the workspace version",
    );
  }
});

test("the version comes from [workspace.package] when there is no [package]", () => {
  const ws = '[workspace]\nmembers = ["splice"]\n\n[workspace.package]\nversion = "1.4.2"\n\n[workspace.dependencies]\nx = "=1.0.0"\n';
  assert.equal(checkVersion({ tag: "v1.4.2", cargoToml: ws, actionYml: action("1.4.2") }), "1.4.2");
});

const action = (d) =>
  `inputs:\n  version:\n    description: "splice npm version"\n    default: ${d}\n  to:\n    default: ""\n`;

test("action default may lag the Cargo version between releases, never lead it", () => {
  assert.equal(checkVersion({ tag: null, cargoToml: cargo("0.21.0"), actionYml: action("0.6.0") }), "0.21.0");
  assert.throws(
    () => checkVersion({ tag: null, cargoToml: cargo("0.2.0"), actionYml: action("0.3.0") }),
    /pins splice 0\.3\.0, expected 0\.2\.0/,
  );
  // A release tag needs the default bumped to exactly the released version.
  assert.throws(
    () => checkVersion({ tag: "v0.2.0", cargoToml: cargo("0.2.0"), actionYml: action("0.1.9") }),
    /pins splice 0\.1\.9, expected 0\.2\.0/,
  );
  assert.equal(checkVersion({ tag: "v0.2.0", cargoToml: cargo("0.2.0"), actionYml: action("0.2.0") }), "0.2.0");
});

test("action default must pin a released version", () => {
  assert.equal(checkVersion({ tag: null, cargoToml: cargo("0.2.0"), actionYml: action("0.2.0") }), "0.2.0");
  assert.throws(
    () => checkVersion({ tag: null, cargoToml: cargo("0.2.0"), actionYml: action("latest") }),
    /action\/action\.yml pins splice latest, expected a released X\.Y\.Z/,
  );
  assert.throws(
    () => checkVersion({ tag: null, cargoToml: cargo("0.2.0"), actionYml: "inputs: {}\n" }),
    /no `version` input default/,
  );
});

const pin = (sha, v) => `- uses: keelstackhq/splice/action@${sha} # v${v}\n`;
const A = "a".repeat(40);
const B = "b".repeat(40);

test("doc Action pins may lag the Cargo version", () => {
  const docs = { "a.md": pin(A, "0.4.0"), "b.md": pin(A, "0.4.0") };
  assert.equal(checkVersion({ tag: null, cargoToml: cargo("0.5.0"), docs }), "0.5.0");
});

test("doc Action pin newer than Cargo.toml throws", () => {
  assert.throws(
    () => checkVersion({ tag: null, cargoToml: cargo("0.5.0"), docs: { "a.md": pin(A, "0.6.0") } }),
    /a\.md pins the Action at v0\.6\.0, newer than Cargo\.toml 0\.5\.0/,
  );
});

test("docs pinning different Action releases throw", () => {
  assert.throws(
    () => checkVersion({ tag: null, cargoToml: cargo("0.5.0"), docs: { "a.md": pin(A, "0.4.0"), "b.md": pin(B, "0.5.0") } }),
    /different Action releases/,
  );
});

test("doc Action pins of the monorepo path (keelstackhq/keelstack/splice/action) count too", () => {
  const mono = (sha, v) => `- uses: keelstackhq/keelstack/splice/action@${sha} # v${v}\n`;
  assert.equal(
    checkVersion({ tag: null, cargoToml: cargo("0.5.0"), docs: { "a.md": mono(A, "0.4.0") } }),
    "0.5.0",
  );
  assert.throws(
    () => checkVersion({ tag: null, cargoToml: cargo("0.5.0"), docs: { "a.md": mono(A, "0.6.0") } }),
    /a\.md pins the Action at v0\.6\.0, newer than Cargo\.toml 0\.5\.0/,
  );
  assert.throws(
    () => checkVersion({ tag: null, cargoToml: cargo("0.5.0"), docs: { "a.md": mono(A, "0.4.0"), "b.md": pin(B, "0.4.0") } }),
    /different Action releases/,
  );
});
