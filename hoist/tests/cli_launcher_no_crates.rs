//! An npm launcher whose crate opts out of crates.io (`[publish] crates = false`, spec §5,
//! §9.3): the launcher and its platform packages stay in the release model, the crate does not
//! reach crates.io.

mod common;

use common::{fixture_repo, hoist};
use serde_json::Value;

#[test]
fn a_launcher_on_a_crate_with_crates_false_keeps_its_npm_packages() {
    let repo = fixture_repo("release-launcher", "https://github.com/acme/tool.git");
    let d = repo.path();
    std::fs::write(
        d.join("crates/tool/hoist.toml"),
        "[publish]\ncrates = false\ngithub_release = { targets = [\"x86_64-unknown-linux-musl\", \"aarch64-apple-darwin\"] }\n",
    )
    .unwrap();
    let out = hoist()
        .current_dir(d)
        .env("HOIST_GITHUB_API", "http://127.0.0.1:9")
        .args(["--json", "explain"])
        .assert()
        .code(0)
        .get_output()
        .clone();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let items = v["items"].as_array().unwrap();
    let has = |kind: &str, key: &str| items.iter().any(|i| i["kind"] == kind && i["key"] == key);
    for pkg in [
        "@acme/tool",
        "@acme/tool-linux-x64",
        "@acme/tool-darwin-arm64",
    ] {
        assert!(
            has("package", pkg) || has("npm/package", pkg),
            "{pkg}: {v:#}"
        );
    }
    assert!(
        !items.iter().any(
            |i| i["kind"].as_str().is_some_and(|k| k.starts_with("crate")) && i["key"] == "tool"
        ),
        "{v:#}"
    );
}
