//! `keel owner`: runs `bosun owner setup` with inherited stdio and returns its exit code.
use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use std::os::unix::fs::PermissionsExt;

fn stub_bosun(dir: &std::path::Path, code: i32) {
    let p = dir.join("bosun");
    std::fs::write(&p, format!("#!/bin/sh\necho \"bosun $*\"\nexit {code}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn keel(path: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("keel").unwrap();
    c.env("PATH", path);
    c
}

#[test]
fn owner_runs_bosun_owner_setup_and_passes_dry_run_and_extra_args() {
    let d = tempfile::tempdir().unwrap();
    stub_bosun(d.path(), 0);
    keel(d.path())
        .args(["owner", "--dry-run", "--", "--manual"])
        .assert()
        .code(0)
        .stdout(predicates::str::contains(
            "bosun owner setup --dry-run --manual",
        ));
    keel(d.path())
        .arg("owner")
        .assert()
        .code(0)
        .stdout(predicates::str::contains("bosun owner setup\n"));
}

#[test]
fn owner_returns_the_exit_code_of_bosun() {
    let d = tempfile::tempdir().unwrap();
    stub_bosun(d.path(), 5);
    keel(d.path()).arg("owner").assert().code(5);
}

#[test]
fn owner_without_bosun_is_one_line_with_the_fix() {
    let d = tempfile::tempdir().unwrap();
    keel(d.path()).arg("owner").assert().code(3).stderr(
        predicates::str::contains("bosun is not installed").and(predicates::str::contains(
            "npm install -g @keelstackhq/bosun",
        )),
    );
}
