//! The monorepo one-time setup keys (spec §5): `[workspace] members`, `[github] apps`,
//! `[github.tag_ruleset]`, `[release] reviewers = []` and `workflow`, `[registry.*]` and
//! `[deploy] environment`. Every test runs against a fixture repo and the fake API.

mod common;

use common::{FakeApi, RouteSpec, fixture_repo, git_repo, hoist};
use serde_json::Value;
use std::path::Path;

const SITE: &str = "https://github.com/acme/x.git";
const REPO: (&str, &str) = ("GET /repos/acme/x", "github/repo_admin.json");
const PERMS: (&str, &str) = (
    "GET /repos/acme/x/actions/permissions/workflow",
    "github/actions_permissions.json",
);

fn write(dir: &Path, file: &str, text: &str) {
    let path = dir.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn run(api: Option<&FakeApi>, dir: &Path, args: &[&str], code: i32) -> Value {
    let mut cmd = hoist();
    cmd.current_dir(dir)
        .env("CLOUDFLARE_ACCOUNT_ID", "acct-test")
        .env("GITHUB_TOKEN", "gh-test-token");
    if let Some(api) = api {
        cmd.env("HOIST_GITHUB_API", api.url())
            .env("HOIST_CLOUDFLARE_API", api.url())
            .env("HOIST_NPM_REGISTRY", api.url())
            .env("HOIST_CRATES_API", api.url());
    }
    let out = cmd
        .arg("--json")
        .args(args)
        .assert()
        .code(code)
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("{e}: {stdout}\n{}", String::from_utf8_lossy(&out.stderr)))
}

fn start(routes: &[RouteSpec]) -> FakeApi {
    FakeApi::start(routes)
}

fn find<'a>(report: &'a Value, kind: &str, key: &str) -> Option<&'a Value> {
    report["items"]
        .as_array()?
        .iter()
        .find(|i| i["kind"] == kind && i["key"] == key)
}

fn item<'a>(report: &'a Value, kind: &str, key: &str) -> &'a Value {
    find(report, kind, key).unwrap_or_else(|| panic!("no {kind} {key} in {report}"))
}

fn step<'a>(report: &'a Value, id: &str) -> Option<&'a Value> {
    report["human_steps"]
        .as_array()?
        .iter()
        .find(|s| s["id"] == id)
}

fn writes(api: &FakeApi) -> Vec<String> {
    api.requests()
        .iter()
        .filter(|r| r.method != "GET")
        .map(|r| format!("{} {}", r.method, r.path))
        .collect()
}

// H1 -----------------------------------------------------------------------------------------

fn npm_pkg(dir: &Path, rel: &str, name: &str) {
    write(
        dir,
        &format!("{rel}/package.json"),
        &format!(r#"{{"name":"{name}","version":"1.0.0","private":false}}"#),
    );
}

#[test]
fn workspace_members_are_read_like_pnpm_packages_and_deduped() {
    let repo = git_repo();
    let d = repo.path();
    npm_pkg(d, "tools/a", "@acme/a");
    npm_pkg(d, "libs/b", "@acme/b");
    npm_pkg(d, "libs/c", "@acme/c");
    write(
        d,
        "hoist.toml",
        "[workspace]\nmembers = [\"tools/a\", \"libs/*\", \"tools/a\"]\n",
    );
    let report = run(None, d, &["explain"], 0);
    for name in ["@acme/a", "@acme/b", "@acme/c"] {
        item(&report, "package", name);
    }
    let count = report["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["area"] == "npm" && i["key"] == "@acme/a")
        .count();
    assert_eq!(count, 1);
}

#[test]
fn workspace_members_are_added_beside_pnpm_workspace_dirs() {
    let repo = fixture_repo("publish", SITE);
    let d = repo.path();
    npm_pkg(d, "tools/a", "@acme/a");
    write(d, "hoist.toml", "[workspace]\nmembers = [\"tools/a\"]\n");
    let report = run(None, d, &["explain"], 0);
    item(&report, "package", "@acme/a");
    item(&report, "package", "@acme/lib");
}

#[test]
fn workspace_members_make_pages_targets() {
    let repo = git_repo();
    let d = repo.path();
    write(d, "hoist.toml", "[workspace]\nmembers = [\"site\"]\n");
    write(
        d,
        "site/hoist.toml",
        "[deploy]\nprovider = \"cloudflare-pages\"\nproject = \"acme-landing\"\n",
    );
    let report = run(None, d, &["explain"], 0);
    item(&report, "pages_project", "acme-landing");
}

#[test]
fn workspace_in_a_target_hoist_toml_is_an_unknown_key() {
    let repo = git_repo();
    let d = repo.path();
    write(d, "hoist.toml", "[workspace]\nmembers = [\"site\"]\n");
    write(
        d,
        "site/hoist.toml",
        "[deploy]\nprovider = \"cloudflare-pages\"\n[workspace]\nmembers = []\n",
    );
    let out = hoist()
        .current_dir(d)
        .env("CLOUDFLARE_ACCOUNT_ID", "a")
        .args(["explain"])
        .assert()
        .code(3);
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("workspace"), "{err}");
}

#[test]
fn launcher_platform_packages_resolve_next_to_a_member_launcher() {
    let repo = git_repo();
    let d = repo.path();
    write(
        d,
        "hoist.toml",
        "[workspace]\nmembers = [\"tool/npm/tool\"]\n",
    );
    write(
        d,
        "Cargo.toml",
        "[workspace]\nmembers = [\"tool\"]\n[workspace.package]\nversion = \"1.0.0\"\n",
    );
    write(
        d,
        "tool/Cargo.toml",
        "[package]\nname = \"acme-tool\"\nversion.workspace = true\n",
    );
    write(
        d,
        "tool/hoist.toml",
        "[publish]\ncrates = {}\ngithub_release = { targets = [\"x86_64-unknown-linux-musl\"], bin = \"tool\" }\n",
    );
    write(
        d,
        "tool/npm/tool/package.json",
        r#"{"name":"@acme/tool","version":"1.0.0","private":false,"bin":{"tool":"bin/tool.js"}}"#,
    );
    write(
        d,
        "tool/npm/tool/hoist.toml",
        "[publish]\nnpm = { launcher = true }\n",
    );
    write(
        d,
        "tool/npm/tool-linux-x64/package.json",
        r#"{"name":"@acme/tool-linux-x64","version":"1.0.0"}"#,
    );
    let report = run(None, d, &["plan", "--area", "npm"], 0);
    for name in ["@acme/tool", "@acme/tool-linux-x64"] {
        item(&report, "trusted_publisher", name);
        assert!(step(&report, &format!("npm-trust:{name}")).is_some());
    }
}

// H2 -----------------------------------------------------------------------------------------

#[test]
fn a_configured_app_that_is_not_installed_is_a_pending_step() {
    let repo = git_repo();
    write(
        repo.path(),
        "hoist.toml",
        "[github]\napps = [\"acme-gate\"]\n",
    );
    let api = start(&[
        (REPO.0, 200, REPO.1, &[]),
        ("GET /repos/acme/x/rulesets", 200, "[]", &[]),
        PERMS_ROUTE,
        ("GET /repos/acme/x/environments", 200, ENVS_NONE, &[]),
        (
            "GET /user/installations",
            200,
            r#"{"installations":[]}"#,
            &[],
        ),
    ]);
    let report = run(Some(&api), repo.path(), &["plan", "--area", "github"], 0);
    let app = item(&report, "app", "acme-gate");
    assert_eq!(app["action"], "pending");
    assert_eq!(app["source"], "hoist.toml:github.apps");
    let s = step(&report, "app:acme-gate").expect("app step");
    assert_eq!(
        s["url"],
        "https://github.com/apps/acme-gate/installations/new"
    );
    api.assert_only_get();
}

const PERMS_ROUTE: RouteSpec<'static> = (PERMS.0, 200, PERMS.1, &[]);
const ENVS_NONE: &str = r#"{"total_count":0,"environments":[]}"#;

#[test]
fn an_installed_configured_app_is_ok() {
    let repo = git_repo();
    write(
        repo.path(),
        "hoist.toml",
        "[github]\napps = [\"acme-gate\"]\n",
    );
    let api = start(&[
        (REPO.0, 200, REPO.1, &[]),
        ("GET /repos/acme/x/rulesets", 200, "[]", &[]),
        PERMS_ROUTE,
        ("GET /repos/acme/x/environments", 200, ENVS_NONE, &[]),
        (
            "GET /user/installations",
            200,
            r#"{"installations":[{"id":5,"app_slug":"acme-gate"}]}"#,
            &[],
        ),
        (
            "GET /user/installations/5/repositories",
            200,
            r#"{"repositories":[{"full_name":"acme/x"}]}"#,
            &[],
        ),
    ]);
    let report = run(Some(&api), repo.path(), &["plan", "--area", "github"], 0);
    assert_eq!(item(&report, "app", "acme-gate")["action"], "ok");
}

// H3 -----------------------------------------------------------------------------------------

const ENV_UNREVIEWED: &str = r#"{"total_count":1,"environments":[{"name":"release","protection_rules":[{"type":"wait_timer"}]}]}"#;

fn release_routes<'a>(envs: &'a str) -> Vec<RouteSpec<'a>> {
    vec![
        (REPO.0, 200, REPO.1, &[]),
        ("GET /repos/acme/x/environments", 200, envs, &[]),
        ("GET /@acme%2Flib", 200, r#"{"name":"@acme/lib"}"#, &[]),
        ("GET /api/v1/crates/tool", 200, r#"{"crate":{}}"#, &[]),
        ("GET /user", 200, r#"{"id":42,"login":"owner"}"#, &[]),
        ("PUT /repos/acme/x/environments/release", 200, "{}", &[]),
        (
            "POST /repos/acme/x/environments/release/deployment-branch-policies",
            200,
            "{}",
            &[],
        ),
    ]
}

#[test]
fn an_unreviewed_release_environment_is_an_update_by_default() {
    let repo = fixture_repo("publish", SITE);
    let api = start(&release_routes(ENV_UNREVIEWED));
    let report = run(Some(&api), repo.path(), &["plan", "--area", "release"], 0);
    assert_eq!(item(&report, "environment", "release")["action"], "update");
}

#[test]
fn empty_reviewers_means_no_required_reviewer() {
    let repo = fixture_repo("publish", SITE);
    write(repo.path(), "hoist.toml", "[release]\nreviewers = []\n");
    let api = start(&release_routes(ENV_UNREVIEWED));
    let report = run(Some(&api), repo.path(), &["plan", "--area", "release"], 0);
    assert_eq!(item(&report, "environment", "release")["action"], "ok");
    assert!(step(&report, "release-reviewer").is_none());
}

#[test]
fn apply_creates_the_release_environment_without_reviewers() {
    let repo = fixture_repo("publish", SITE);
    write(repo.path(), "hoist.toml", "[release]\nreviewers = []\n");
    let mut routes = release_routes(ENVS_NONE);
    routes.push((
        "GET /repos/acme/x/environments",
        200,
        r#"{"total_count":1,"environments":[{"name":"release"}]}"#,
        &[],
    ));
    let api = start(&routes);
    let report = run(
        Some(&api),
        repo.path(),
        &["apply", "--yes", "--area", "release"],
        0,
    );
    assert!(step(&report, "release-reviewer").is_none(), "{report}");
    let put = api
        .requests()
        .into_iter()
        .find(|r| r.method == "PUT")
        .expect("environment PUT");
    let body: Value = serde_json::from_str(put.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["reviewers"], serde_json::json!([]), "{body}");
    assert!(!api.requests().iter().any(|r| r.path == "/user"));
}

// H4 -----------------------------------------------------------------------------------------

const OTHER_WORKFLOW: &str = "name: aaa\non: [push]\njobs:\n  deny:\n    runs-on: ubuntu-latest\n    environment: release\n    steps:\n      - run: echo hi\n";

#[test]
fn the_release_workflow_is_the_configured_file() {
    let repo = fixture_repo("publish", SITE);
    let d = repo.path();
    write(d, ".github/workflows/aaa.yml", OTHER_WORKFLOW);
    let api = start(&release_routes(ENV_UNREVIEWED));
    let report = run(Some(&api), d, &["plan", "--area", "npm"], 0);
    let s = step(&report, "npm-trust:@acme/lib").expect("trust step");
    assert!(
        s["title"].as_str().unwrap().contains("--file aaa.yml"),
        "{s}"
    );

    write(d, "hoist.toml", "[release]\nworkflow = \"release.yml\"\n");
    let report = run(Some(&api), d, &["plan", "--area", "npm"], 0);
    let s = step(&report, "npm-trust:@acme/lib").expect("trust step");
    assert!(
        s["title"].as_str().unwrap().contains("--file release.yml"),
        "{s}"
    );
    let report = run(Some(&api), d, &["plan", "--area", "release"], 0);
    let w = item(&report, "workflow", "release");
    assert_eq!(w["action"], "ok");
    assert_eq!(w["source"], ".github/workflows/release.yml");
}

#[test]
fn a_configured_workflow_without_the_environment_is_pending() {
    let repo = fixture_repo("publish", SITE);
    let d = repo.path();
    write(d, ".github/workflows/aaa.yml", OTHER_WORKFLOW);
    write(d, "hoist.toml", "[release]\nworkflow = \"publish.yml\"\n");
    let api = start(&release_routes(ENV_UNREVIEWED));
    let report = run(Some(&api), d, &["plan", "--area", "release"], 0);
    let w = item(&report, "workflow", "release");
    assert_eq!(w["action"], "pending");
    assert!(w["reason"].as_str().unwrap().contains("publish.yml"), "{w}");
    let report = run(Some(&api), d, &["plan", "--area", "crates"], 0);
    let s = step(&report, "crates-trust:tool").expect("trust step");
    assert!(
        s["title"]
            .as_str()
            .unwrap()
            .contains("workflow publish.yml")
    );
}

// H5 -----------------------------------------------------------------------------------------

const TAGS_TOML: &str =
    "[github.tag_ruleset]\npatterns = [\"v*\"]\nbypass = [\"github-actions\"]\n";

fn tag_ruleset_json(bypass: &str, rules: &str) -> String {
    format!(
        r#"{{"id":12,"name":"hoist: tags","target":"tag","enforcement":"active","conditions":{{"ref_name":{{"include":["refs/tags/v*"],"exclude":[]}}}},"rules":{rules},"bypass_actors":{bypass}}}"#
    )
}

const GOOD_RULES: &str = r#"[{"type":"creation"},{"type":"update"},{"type":"deletion"}]"#;
const GOOD_BYPASS: &str =
    r#"[{"actor_id":15368,"actor_type":"Integration","bypass_mode":"always"}]"#;

fn tag_routes<'a>(list: &'a str, detail: &'a str) -> Vec<RouteSpec<'a>> {
    vec![
        (REPO.0, 200, REPO.1, &[]),
        ("GET /repos/acme/x/rulesets", 200, list, &[]),
        ("GET /repos/acme/x/rulesets/12", 200, detail, &[]),
        PERMS_ROUTE,
        ("GET /repos/acme/x/environments", 200, ENVS_NONE, &[]),
        ("POST /repos/acme/x/rulesets", 201, "{}", &[]),
        ("PUT /repos/acme/x/rulesets/12", 200, "{}", &[]),
    ]
}

const LIST_TAGS: &str = r#"[{"id":12,"name":"hoist: tags"}]"#;

#[test]
fn a_missing_tag_ruleset_is_created_with_the_actions_app_as_bypass() {
    let repo = git_repo();
    write(repo.path(), "hoist.toml", TAGS_TOML);
    let api = start(&tag_routes("[]", "{}"));
    let plan = run(Some(&api), repo.path(), &["plan", "--area", "github"], 0);
    assert_eq!(
        item(&plan, "tag_ruleset", "hoist: tags")["action"],
        "create"
    );
    api.assert_only_get();

    run(
        Some(&api),
        repo.path(),
        &["apply", "--yes", "--area", "github"],
        1,
    );
    let post = api
        .requests()
        .into_iter()
        .find(|r| r.method == "POST")
        .expect("ruleset POST");
    let body: Value = serde_json::from_str(post.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["name"], "hoist: tags");
    assert_eq!(body["target"], "tag");
    assert_eq!(body["enforcement"], "active");
    assert_eq!(
        body["conditions"]["ref_name"]["include"],
        serde_json::json!(["refs/tags/v*"])
    );
    let types: Vec<&str> = body["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["creation", "update", "deletion"]);
    assert_eq!(
        body["bypass_actors"],
        serde_json::json!([{"actor_id":15368,"actor_type":"Integration","bypass_mode":"always"}])
    );
}

#[test]
fn a_matching_tag_ruleset_is_ok_and_a_drifted_one_is_updated_in_place() {
    let repo = git_repo();
    write(repo.path(), "hoist.toml", TAGS_TOML);
    let good = tag_ruleset_json(GOOD_BYPASS, GOOD_RULES);
    let api = start(&tag_routes(LIST_TAGS, &good));
    let plan = run(Some(&api), repo.path(), &["plan", "--area", "github"], 0);
    assert_eq!(item(&plan, "tag_ruleset", "hoist: tags")["action"], "ok");

    let drifted = tag_ruleset_json("[]", GOOD_RULES);
    let api = start(&tag_routes(LIST_TAGS, &drifted));
    let plan = run(Some(&api), repo.path(), &["plan", "--area", "github"], 0);
    let t = item(&plan, "tag_ruleset", "hoist: tags");
    assert_eq!(t["action"], "update");
    assert!(t["reason"].as_str().unwrap().contains("bypass"), "{t}");
    run(
        Some(&api),
        repo.path(),
        &["apply", "--yes", "--area", "github"],
        1,
    );
    assert_eq!(writes(&api), ["PUT /repos/acme/x/rulesets/12"]);
    assert_eq!(
        put_body(&api),
        serde_json::json!({
            "name": "hoist: tags",
            "target": "tag",
            "enforcement": "active",
            "conditions": {"ref_name": {"include": ["refs/tags/v*"], "exclude": []}},
            "rules": [{"type": "creation"}, {"type": "update"}, {"type": "deletion"}],
            "bypass_actors": [
                {"actor_id": 15368, "actor_type": "Integration", "bypass_mode": "always"}
            ]
        })
    );
}

fn put_body(api: &FakeApi) -> Value {
    let put = api
        .requests()
        .into_iter()
        .find(|r| r.method == "PUT")
        .expect("ruleset PUT");
    serde_json::from_str(put.body.as_deref().unwrap()).unwrap()
}

fn tag_ruleset_with(include: &str, exclude: &str, bypass: &str, rules: &str) -> String {
    format!(
        r#"{{"id":12,"name":"hoist: tags","target":"tag","enforcement":"active","conditions":{{"ref_name":{{"include":{include},"exclude":{exclude}}}}},"rules":{rules},"bypass_actors":{bypass}}}"#
    )
}

#[test]
fn an_owner_added_tag_include_is_ok_and_kept_by_an_update() {
    let repo = git_repo();
    write(repo.path(), "hoist.toml", TAGS_TOML);
    let include = r#"["refs/tags/v*","refs/tags/release-*"]"#;
    let good = tag_ruleset_with(include, "[]", GOOD_BYPASS, GOOD_RULES);
    let api = start(&tag_routes(LIST_TAGS, &good));
    let plan = run(Some(&api), repo.path(), &["plan", "--area", "github"], 0);
    let t = item(&plan, "tag_ruleset", "hoist: tags");
    assert_eq!(t["action"], "ok", "{t}");
    assert!(
        t["reason"]
            .as_str()
            .unwrap()
            .contains("`refs/tags/release-*`"),
        "{t}"
    );

    // Drifted (no bypass actor): the update keeps the owner's include.
    let drifted = tag_ruleset_with(include, "[]", "[]", GOOD_RULES);
    let api = start(&tag_routes(LIST_TAGS, &drifted));
    run(
        Some(&api),
        repo.path(),
        &["apply", "--yes", "--area", "github"],
        1,
    );
    let body = put_body(&api);
    assert_eq!(
        body["conditions"]["ref_name"]["include"],
        serde_json::json!(["refs/tags/v*", "refs/tags/release-*"])
    );
}

#[test]
fn an_owner_added_tag_rule_and_bypass_actor_are_ok_and_kept_by_an_update() {
    let repo = git_repo();
    write(repo.path(), "hoist.toml", TAGS_TOML);
    let rules = r#"[{"type":"creation"},{"type":"update"},{"type":"deletion"},{"type":"required_signatures"}]"#;
    let bypass = r#"[{"actor_id":15368,"actor_type":"Integration","bypass_mode":"always"},{"actor_id":7,"actor_type":"Team","bypass_mode":"always"}]"#;
    let good = tag_ruleset_with(r#"["refs/tags/v*"]"#, "[]", bypass, rules);
    let api = start(&tag_routes(LIST_TAGS, &good));
    let plan = run(Some(&api), repo.path(), &["plan", "--area", "github"], 0);
    let t = item(&plan, "tag_ruleset", "hoist: tags");
    assert_eq!(t["action"], "ok", "{t}");
    let reason = t["reason"].as_str().unwrap();
    assert!(reason.contains("`required_signatures`"), "{t}");
    assert!(reason.contains("`Team:7:always`"), "{t}");

    // Drifted (no `deletion` rule, enforcement off): rule and actor survive the update.
    let rules = r#"[{"type":"creation"},{"type":"update"},{"type":"required_signatures"}]"#;
    let drifted = tag_ruleset_with(r#"["refs/tags/v*"]"#, "[]", bypass, rules)
        .replace(r#""enforcement":"active""#, r#""enforcement":"evaluate""#);
    let api = start(&tag_routes(LIST_TAGS, &drifted));
    let plan = run(Some(&api), repo.path(), &["plan", "--area", "github"], 0);
    let t = item(&plan, "tag_ruleset", "hoist: tags");
    assert_eq!(t["action"], "update", "{t}");
    run(
        Some(&api),
        repo.path(),
        &["apply", "--yes", "--area", "github"],
        1,
    );
    let body = put_body(&api);
    assert_eq!(body["enforcement"], "active");
    assert_eq!(
        body["rules"],
        serde_json::json!([
            {"type": "creation"}, {"type": "update"}, {"type": "required_signatures"},
            {"type": "deletion"}
        ])
    );
    assert_eq!(
        body["bypass_actors"],
        serde_json::from_str::<Value>(bypass).unwrap()
    );
}

#[test]
fn an_adopt_update_never_clears_tag_excludes() {
    let repo = git_repo();
    write(repo.path(), "hoist.toml", TAGS_TOML);
    let live = tag_ruleset_with(
        r#"["refs/tags/v*"]"#,
        r#"["refs/tags/v0"]"#,
        "[]",
        GOOD_RULES,
    );
    let api = start(&tag_routes(LIST_TAGS, &live));
    // Without --adopt: an update clears the exclude (it unprotects release tags).
    let plan = run(Some(&api), repo.path(), &["plan", "--area", "github"], 0);
    assert_eq!(
        item(&plan, "tag_ruleset", "hoist: tags")["action"],
        "update"
    );
    // With --adopt: drift naming what an update would drop, nothing written.
    let plan = run(
        Some(&api),
        repo.path(),
        &["plan", "--adopt", "--area", "github"],
        0,
    );
    let t = item(&plan, "tag_ruleset", "hoist: tags");
    assert_eq!(t["action"], "drift", "{t}");
    assert!(
        t["reason"]
            .as_str()
            .unwrap()
            .contains("excludes `refs/tags/v0`"),
        "{t}"
    );
    run(
        Some(&api),
        repo.path(),
        &["apply", "--adopt", "--yes", "--area", "github"],
        1,
    );
    assert!(writes(&api).is_empty(), "{:?}", writes(&api));
}

#[test]
fn other_rulesets_are_never_touched_by_the_tag_ruleset() {
    let repo = git_repo();
    write(repo.path(), "hoist.toml", TAGS_TOML);
    let list = r#"[{"id":99,"name":"someone else"}]"#;
    let mut routes = tag_routes(list, "{}");
    routes.push((
        "GET /repos/acme/x/rulesets/99",
        200,
        r#"{"id":99,"name":"someone else","target":"branch","enforcement":"active"}"#,
        &[],
    ));
    let api = start(&routes);
    run(
        Some(&api),
        repo.path(),
        &["apply", "--yes", "--area", "github"],
        1,
    );
    assert_eq!(writes(&api), ["POST /repos/acme/x/rulesets"]);
}

#[test]
fn a_tag_ruleset_on_github_free_is_drift_not_an_error() {
    let repo = git_repo();
    write(repo.path(), "hoist.toml", TAGS_TOML);
    let api = start(&[
        (REPO.0, 200, REPO.1, &[]),
        (
            "GET /repos/acme/x/rulesets",
            403,
            r#"{"message":"Upgrade to GitHub Pro or make this repository public to enable this feature."}"#,
            &[],
        ),
        PERMS_ROUTE,
        ("GET /repos/acme/x/environments", 200, ENVS_NONE, &[]),
    ]);
    let plan = run(Some(&api), repo.path(), &["plan", "--area", "github"], 0);
    assert_eq!(item(&plan, "tag_ruleset", "hoist: tags")["action"], "drift");
}

#[test]
fn another_bypass_value_exits_3() {
    let repo = git_repo();
    write(
        repo.path(),
        "hoist.toml",
        "[github.tag_ruleset]\npatterns = [\"v*\"]\nbypass = [\"everyone\"]\n",
    );
    let out = hoist()
        .current_dir(repo.path())
        .arg("explain")
        .assert()
        .code(3);
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        err.contains("bypass") && err.contains("github-actions"),
        "{err}"
    );
}

// H6 -----------------------------------------------------------------------------------------

#[test]
fn registry_packages_get_trusted_publisher_and_first_publish_steps() {
    let repo = fixture_repo("publish", SITE);
    write(
        repo.path(),
        "hoist.toml",
        "[registry.npm.\"@acme/extra\"]\nworkflow = \"extra.yml\"\n[registry.crates.\"acme-extra\"]\nworkflow = \"extra.yml\"\n",
    );
    // `@acme/extra` and `acme-extra` answer 404: never published.
    let api = start(&release_routes(ENV_UNREVIEWED));
    let report = run(
        Some(&api),
        repo.path(),
        &["plan", "--area", "npm", "--area", "crates"],
        0,
    );
    let npm = step(&report, "npm-trust:@acme/extra").expect("npm trust");
    assert!(npm["title"].as_str().unwrap().contains("--file extra.yml"));
    assert!(npm["title"].as_str().unwrap().contains("--env release"));
    let krate = step(&report, "crates-trust:acme-extra").expect("crates trust");
    assert!(
        krate["title"]
            .as_str()
            .unwrap()
            .contains("workflow extra.yml")
    );
    assert_eq!(
        item(&report, "first_publish", "@acme/extra")["action"],
        "pending"
    );
    assert_eq!(
        item(&report, "first_publish", "acme-extra")["action"],
        "pending"
    );
    assert!(step(&report, "first-publish:@acme/extra").is_some());
    let fp = step(&report, "first-publish:acme-extra").expect("crate first publish");
    assert!(
        fp["title"]
            .as_str()
            .unwrap()
            .contains("hoist release bootstrap")
    );
    // The model packages keep the release workflow.
    let lib = step(&report, "npm-trust:@acme/lib").expect("model package");
    assert!(
        lib["title"]
            .as_str()
            .unwrap()
            .contains("--file release.yml")
    );
}

#[test]
fn a_registry_entry_without_a_workflow_exits_3() {
    let repo = fixture_repo("publish", SITE);
    write(
        repo.path(),
        "hoist.toml",
        "[registry.npm.\"@acme/extra\"]\n",
    );
    hoist()
        .current_dir(repo.path())
        .arg("explain")
        .assert()
        .code(3);
}

// H8 -----------------------------------------------------------------------------------------

#[test]
fn deploy_environment_holds_the_deploy_secrets_without_a_workflow() {
    let repo = git_repo();
    let d = repo.path();
    write(
        d,
        "hoist.toml",
        "[github]\nsecret_scope = \"environment\"\n[workspace]\nmembers = [\"site\"]\n",
    );
    write(
        d,
        "site/hoist.toml",
        "[deploy]\nprovider = \"cloudflare-pages\"\nproject = \"acme-landing\"\nenvironment = \"site\"\n",
    );
    let report = run(None, d, &["explain"], 0);
    item(&report, "environment", "site");
    for name in ["CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID"] {
        let s = item(&report, "secret", name);
        assert_eq!(s["source"], "site/hoist.toml:deploy.environment");
    }

    // The secrets are missing in environment `site`; the environment is created first.
    let api = start(&[
        (REPO.0, 200, REPO.1, &[]),
        ("GET /repos/acme/x/rulesets", 200, "[]", &[]),
        PERMS_ROUTE,
        ("GET /repos/acme/x/environments", 200, ENVS_NONE, &[]),
        (
            "GET /repos/acme/x/actions/secrets",
            200,
            r#"{"total_count":0,"secrets":[]}"#,
            &[],
        ),
        (
            "GET /accounts/acct-test/tokens",
            200,
            r#"{"success":true,"result":[]}"#,
            &[],
        ),
    ]);
    let plan = run(Some(&api), d, &["plan", "--area", "github"], 0);
    let env = item(&plan, "environment", "site");
    assert_eq!(env["action"], "create");
    let token = item(&plan, "secret", "CLOUDFLARE_ACCOUNT_ID");
    assert_eq!(token["action"], "create");
    assert!(
        token["reason"]
            .as_str()
            .unwrap()
            .contains("environment(s) site"),
        "{token}"
    );
    api.assert_only_get();
}

// `[publish] crates = false` -----------------------------------------------------------------

#[test]
fn a_crate_with_crates_false_never_reaches_crates_io() {
    let repo = fixture_repo("publish", SITE);
    let d = repo.path();
    write(
        d,
        "crates/tool/hoist.toml",
        "[publish]\ncrates = false\ngithub_release = { targets = [\"x86_64-unknown-linux-musl\"] }\n",
    );
    let report = run(None, d, &["explain"], 0);
    assert!(find(&report, "crate", "tool").is_none(), "{report}");
    item(&report, "package", "tool");
    let api = start(&release_routes(ENV_UNREVIEWED));
    let plan = run(Some(&api), d, &["plan", "--area", "crates"], 0);
    assert!(find(&plan, "trusted_publisher", "tool").is_none(), "{plan}");
}

// review fixes -------------------------------------------------------------------------------

const ENV_REVIEWED: &str = r#"{"total_count":1,"environments":[{"name":"release","protection_rules":[{"type":"required_reviewers","prevent_self_review":true,"reviewers":[{"type":"User","reviewer":{"id":7,"login":"alice"}}]}]}]}"#;
const ENV_DETAIL: &str = r#"{"name":"release","protection_rules":[{"type":"wait_timer","wait_timer":5},{"type":"required_reviewers","prevent_self_review":true,"reviewers":[{"type":"User","reviewer":{"id":7,"login":"alice"}}]}]}"#;

#[test]
fn empty_reviewers_with_a_live_reviewer_is_an_update_naming_them() {
    let repo = fixture_repo("publish", SITE);
    write(repo.path(), "hoist.toml", "[release]\nreviewers = []\n");
    let api = start(&release_routes(ENV_REVIEWED));
    let report = run(Some(&api), repo.path(), &["plan", "--area", "release"], 0);
    let it = item(&report, "environment", "release");
    assert_eq!(it["action"], "update");
    assert!(it["reason"].as_str().unwrap().contains("alice"), "{it}");
}

#[test]
fn apply_drops_the_live_reviewers_and_keeps_the_other_protections() {
    let repo = fixture_repo("publish", SITE);
    write(repo.path(), "hoist.toml", "[release]\nreviewers = []\n");
    let mut routes = release_routes(ENV_REVIEWED);
    routes.push((
        "GET /repos/acme/x/environments/release",
        200,
        ENV_DETAIL,
        &[],
    ));
    let api = start(&routes);
    run(
        Some(&api),
        repo.path(),
        &["apply", "--yes", "--area", "release"],
        // The static fake still lists the reviewer, so the verify read reports drift.
        1,
    );
    let put = api
        .requests()
        .into_iter()
        .find(|r| r.method == "PUT")
        .expect("environment PUT");
    let body: Value = serde_json::from_str(put.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["reviewers"], serde_json::json!([]), "{body}");
    assert_eq!(body["wait_timer"], 5, "{body}");
    assert_eq!(body["prevent_self_review"], true, "{body}");
}

#[test]
fn a_tag_ruleset_needs_bypass_github_actions() {
    for bypass in ["", "bypass = []\n"] {
        let repo = git_repo();
        write(
            repo.path(),
            "hoist.toml",
            &format!("[github.tag_ruleset]\npatterns = [\"v*\"]\n{bypass}"),
        );
        let out = hoist()
            .current_dir(repo.path())
            .arg("explain")
            .assert()
            .code(3);
        let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        assert!(err.contains("github.tag_ruleset.bypass"), "{err}");
    }
}

#[test]
fn deploy_environment_without_environment_scope_exits_3() {
    let repo = git_repo();
    let d = repo.path();
    write(d, "hoist.toml", "[workspace]\nmembers = [\"site\"]\n");
    write(
        d,
        "site/hoist.toml",
        "[deploy]\nprovider = \"cloudflare-pages\"\nproject = \"acme-landing\"\nenvironment = \"site\"\n",
    );
    let out = hoist().current_dir(d).arg("explain").assert().code(3);
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        err.contains("deploy.environment")
            && err.contains("secret_scope")
            && err.contains("site/hoist.toml"),
        "{err}"
    );
}
