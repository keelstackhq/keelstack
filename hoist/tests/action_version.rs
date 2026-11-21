//! The release and secrets actions read hoist's version from the action's Cargo.toml: `[package]`
//! of `hoist/Cargo.toml` (the old per-tool repo), else `[workspace.package]` of the monorepo root
//! (as the root release.yml's `cargo_version`), never the first `version =` line of any table.

use std::path::Path;
use std::process::Command;
use yaml_serde::Value;

/// The version-reading lines of an action's install script (`cargo_version` through its `fi`).
fn version_snippet(action: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(action);
    let a: Value = yaml_serde::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let run = a["runs"]["steps"][0]["run"].as_str().unwrap().to_string();
    let start = run.find("cargo_version() {").expect("cargo_version");
    let end = start + run[start..].find("\nfi\n").expect("fi") + 4;
    run[start..end].to_string()
}

/// Run the snippet with `ACTION_PATH=<root>/hoist/<sub>`; the version it reads.
fn read_version(action: &str, root: &Path) -> String {
    let sub = action.split('/').next().unwrap();
    std::fs::create_dir_all(root.join("hoist").join(sub)).unwrap();
    let script = format!(
        "set -euo pipefail\n{}printf '%s' \"$version\"\n",
        version_snippet(action)
    );
    let out = Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("ACTION_PATH", root.join("hoist").join(sub))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

const ACTIONS: [&str; 2] = ["release/action.yml", "secrets/action.yml"];

#[test]
fn the_monorepo_version_comes_from_workspace_package_not_the_first_version_line() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("hoist")).unwrap();
    std::fs::write(
        d.path().join("hoist/Cargo.toml"),
        "[package]\nname = \"keel-hoist\"\nversion.workspace = true\n\n[dependencies]\nx = { version = \"1\" }\n",
    )
    .unwrap();
    // A decoy `version =` line in a table above [workspace.package].
    std::fs::write(
        d.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"hoist\"]\n\n[workspace.metadata.x]\nversion = \"9.9.9\"\n\n[workspace.package]\nedition = \"2024\"\nversion = \"0.22.0\"\n\n[profile.dev]\nversion = \"8.8.8\"\n",
    )
    .unwrap();
    for a in ACTIONS {
        assert_eq!(read_version(a, d.path()), "0.22.0", "{a}");
    }
}

#[test]
fn the_old_repo_layout_reads_package_version() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("hoist")).unwrap();
    std::fs::write(
        d.path().join("hoist/Cargo.toml"),
        "[workspace]\n\n[package]\nname = \"hoist\"\nversion = \"0.5.1\"\n",
    )
    .unwrap();
    for a in ACTIONS {
        assert_eq!(read_version(a, d.path()), "0.5.1", "{a}");
    }
}

#[test]
fn this_checkout_reads_the_lockstep_version() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for a in ACTIONS {
        assert_eq!(read_version(a, &root), env!("CARGO_PKG_VERSION"), "{a}");
    }
}
