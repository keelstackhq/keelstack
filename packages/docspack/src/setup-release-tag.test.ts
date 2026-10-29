// scripts/setup-release-tag.sh (the owner's one-time docspack release setup) against a fake `gh`
// that logs every call and answers the API reads with canned JSON.
import { execFileSync, spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vite-plus/test";

const script = new URL("../../../scripts/setup-release-tag.sh", import.meta.url).pathname;
const KEY = "-----BEGIN RSA PRIVATE KEY-----\nsecret-material\n-----END RSA PRIVATE KEY-----\n";

const FAKE_GH = `#!/usr/bin/env bash
log="$GH_LOG"
if [ "$1" = secret ] || [ "$1" = variable ]; then
  printf '%s | stdin=%s\\n' "$*" "$(cat | wc -c | tr -d ' ')" >> "$log"; exit 0
fi
body=""
case "$*" in *--input*) body="$(cat)";; esac
printf '%s%s\\n' "$*" "\${body:+ | body=$body}" >> "$log"
case "$*" in
  *"-X "*|*"--method"*) echo '{}'; exit 0;;
esac
for a in "$@"; do case "$a" in -*) ;; api) ;; *) path="$a"; break;; esac; done
case "$path" in
  repos/keelstackhq/keelstack/rulesets) echo '[{"id":8,"name":"release-tags","source_type":"Organization"},{"id":7,"name":"release-tags","source_type":"Repository"}]';;
  repos/keelstackhq/keelstack/rulesets/7)
    if [ -n "\${FAKE_NO_BYPASS:-}" ]; then echo '{"id":7,"name":"release-tags"}'
    else echo '{"id":7,"bypass_actors":[{"actor_id":5,"actor_type":"RepositoryRole","bypass_mode":"always"}]}'; fi;;
  repos/keelstackhq/keelstack/environments/release-tag) echo '{"message":"Not Found"}'; exit 1;;
  repos/keelstackhq/keelstack/environments/release-tag/deployment-branch-policies) echo '{"branch_policies":[]}';;
  repos/keelstackhq/keelstack/environments/release) echo '{"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}';;
  repos/keelstackhq/keelstack/environments/release/deployment-branch-policies) echo '{"branch_policies":[{"name":"*-v*","type":"tag"}]}';;
  orgs/keelstackhq/installations*) echo '{"installations":[{"id":9,"app_id":123,"repository_selection":"selected"}]}';;
  user/installations/9/repositories*) echo '{"repositories":[{"full_name":"keelstackhq/keelstack"}]}';;
  *) echo '{}';;
esac
`;

function run(args: string[], extraEnv: Record<string, string> = {}) {
  const dir = mkdtempSync(join(tmpdir(), "setup-release-tag-"));
  writeFileSync(join(dir, "gh"), FAKE_GH);
  chmodSync(join(dir, "gh"), 0o755);
  const key = join(dir, "app.pem");
  writeFileSync(key, KEY);
  const log = join(dir, "gh.log");
  writeFileSync(log, "");
  const r = spawnSync("bash", [script, ...args.map((a) => (a === "KEY" ? key : a))], {
    encoding: "utf8",
    env: { ...process.env, PATH: `${dir}:${process.env.PATH}`, GH_LOG: log, ...extraEnv },
  });
  return { ...r, log: readFileSync(log, "utf8") };
}

const ARGS = ["--app-id", "123", "--client-id", "Iv23liClient", "--key", "KEY"];
const WRITE = /-X (PUT|POST|PATCH|DELETE)|^secret set|^variable set/m;

describe("setup-release-tag.sh", () => {
  it("is shellcheck-clean", () => {
    expect(() => execFileSync("shellcheck", [script])).not.toThrow();
  });

  it("exits 2 with usage when an argument is missing", () => {
    const r = run(["--app-id", "123"]);
    expect(r.status).toBe(2);
    expect(r.stderr).toContain("usage:");
  });

  it("writes nothing on --dry-run and says what it would do", () => {
    const r = run([...ARGS, "--dry-run"]);
    expect(r.status).toBe(0);
    expect(r.log).not.toMatch(WRITE);
    expect(r.stdout).toContain("dry run");
    expect(r.stdout).toContain("environment release-tag");
  });

  it("sets the variable, the main-only environment, the key from stdin and the bypass", () => {
    const r = run(ARGS);
    expect(r.status).toBe(0);
    expect(r.log).toMatch(/^variable set HOIST_RELEASE_APP_CLIENT_ID .*--body Iv23liClient/m);
    expect(r.log).toMatch(
      /-X PUT repos\/keelstackhq\/keelstack\/environments\/release-tag .*"custom_branch_policies":true/,
    );
    expect(r.log).toMatch(
      /-X POST .*release-tag\/deployment-branch-policies .*"name":"main","type":"branch"/,
    );
    expect(r.log).toMatch(/^secret set HOIST_RELEASE_APP_KEY .*--env release-tag .*\| stdin=\d+/m);
    expect(r.log).not.toContain("secret-material");
    expect(r.log).toMatch(
      /-X PUT repos\/keelstackhq\/keelstack\/rulesets\/7 .*"actor_id":123,"actor_type":"Integration"/,
    );
    expect(r.log).not.toMatch(
      /environments\/release(\/|\s).*-X|-X \S+ repos\/keelstackhq\/keelstack\/environments\/release\b(?!-)/,
    );
    expect(r.stdout).toContain("Summary");
  });

  it("uses the repository's own release-tags ruleset, not an org-level one", () => {
    const r = run(ARGS);
    expect(r.log).toMatch(/-X PUT repos\/keelstackhq\/keelstack\/rulesets\/7 /);
    expect(r.log).not.toMatch(/rulesets\/8/);
  });

  it("stops without touching the ruleset when it cannot see the bypass list", () => {
    const r = run(ARGS, { FAKE_NO_BYPASS: "1" });
    expect(r.status).not.toBe(0);
    expect(r.stderr).toContain("bypass list");
    expect(r.log).not.toMatch(/-X PUT \S*rulesets/);
  });
});
