//! End to end: the hull template-rust `release.yml` `detect` job (its `gate` and `detect` steps,
//! run by bash as GitHub Actions would) against a one-crate repo with binaries, with this hoist.

mod common;

use common::{FakeApi, fixture_repo, git};
use std::path::{Path, PathBuf};
use std::process::Command;

const WORKFLOW: &str = "../hull/template-rust/pieces/base/files/.github/workflows/release.yml";

/// The `run:` script of step `id` in job `detect`.
fn step(id: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(WORKFLOW);
    let text = std::fs::read_to_string(&path).unwrap();
    let wf: yaml_serde::Value = yaml_serde::from_str(&text).unwrap();
    let steps = wf["jobs"]["detect"]["steps"].as_sequence().unwrap();
    let s = steps
        .iter()
        .find(|s| s["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("no step {id}"));
    s["run"].as_str().unwrap().to_string()
}

/// A one-crate repo (`solo`, a binary with two `github_release` targets) on `main`, untagged.
fn repo() -> tempfile::TempDir {
    let repo = fixture_repo("release-single", "https://github.com/acme/x.git");
    let d = repo.path();
    std::fs::write(
        d.join("hoist.toml"),
        "[publish]\ncrates = {}\ngithub_release = { targets = [\"x86_64-unknown-linux-musl\", \"x86_64-pc-windows-msvc\"] }\n",
    )
    .unwrap();
    git(d, &["checkout", "-q", "-b", "main"]);
    git(d, &["add", "-A"]);
    git(d, &["commit", "-q", "-m", "chore: init"]);
    repo
}

/// Run one step's script with bash (`-e`, as Actions does), `hoist` on PATH; returns the exit
/// code and the lines it wrote to `$GITHUB_OUTPUT`.
fn run(dir: &Path, api: &FakeApi, script: &str, release: &str) -> (i32, String) {
    let bin = tempfile::tempdir().unwrap();
    let hoist = PathBuf::from(env!("CARGO_BIN_EXE_hoist"));
    std::os::unix::fs::symlink(&hoist, bin.path().join("hoist")).unwrap();
    let out = tempfile::NamedTempFile::new().unwrap();
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let status = Command::new("bash")
        .args([
            "--noprofile",
            "--norc",
            "-e",
            "-o",
            "pipefail",
            "-c",
            script,
        ])
        .current_dir(dir)
        .env_clear()
        .env("PATH", path)
        .env("HOME", dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GITHUB_OUTPUT", out.path())
        .env("HOIST_RELEASE", release)
        .env("GH_TOKEN", "test-token")
        .env("HOIST_GITHUB_API", api.url())
        .env("HOIST_NPM_REGISTRY", api.url())
        .env("HOIST_CRATES_API", api.url())
        .env("HOIST_CLOUDFLARE_API", "http://127.0.0.1:9")
        .status()
        .unwrap();
    (
        status.code().unwrap_or(-1),
        std::fs::read_to_string(out.path()).unwrap(),
    )
}

#[test]
fn detect_finds_a_pending_binary_crate_before_any_build() {
    let repo = repo();
    let api = FakeApi::start(&[
        ("GET /repos/acme/x/releases", 200, "[]", &[]),
        ("GET /api/v1/crates/solo", 200, r#"{"crate":{}}"#, &[]),
    ]);
    let (code, gate) = run(repo.path(), &api, &step("gate"), "enabled");
    assert_eq!((code, gate.as_str()), (0, "enabled=true\n"));
    let (code, out) = run(repo.path(), &api, &step("detect"), "enabled");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "pending=true\n");
    api.assert_only_get();
}

#[test]
fn detect_with_everything_released_is_not_pending() {
    let repo = repo();
    git(repo.path(), &["tag", "v0.1.0"]);
    git(
        repo.path(),
        &["commit", "-q", "--allow-empty", "-m", "docs: later"],
    );
    let api = FakeApi::start(&[]);
    let (code, out) = run(repo.path(), &api, &step("detect"), "enabled");
    assert_eq!((code, out.as_str()), (0, "pending=false\n"));
}

#[test]
fn the_gate_is_the_first_step_and_detect_waits_for_it() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(WORKFLOW);
    let wf: yaml_serde::Value =
        yaml_serde::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let steps = wf["jobs"]["detect"]["steps"].as_sequence().unwrap();
    assert_eq!(steps[0]["id"].as_str(), Some("gate"));
    for s in &steps[1..] {
        assert_eq!(
            s["if"].as_str(),
            Some("steps.gate.outputs.enabled == 'true'"),
            "{s:?}"
        );
    }
}
