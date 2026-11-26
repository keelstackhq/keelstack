//! `bosun pin --to X.Y.Z`: an exact published release for every tool, checked like `--latest`.
mod common;

use common::npm::{Registry, repo, sha};
use common::{bosun, one_object};
use serde_json::{Value, json};
use std::path::Path;

fn pin_to(d: &Path, reg: &Registry, args: &[&str]) -> (i32, Value) {
    let out = bosun()
        .env("BOSUN_NPM_REGISTRY", &reg.base)
        .args(["-C", d.to_str().unwrap(), "pin"])
        .args(args)
        .output()
        .unwrap();
    (out.status.code().unwrap(), one_object(&out.stdout))
}

fn read(d: &Path, rel: &str) -> String {
    std::fs::read_to_string(d.join(rel)).unwrap()
}

const ALL: [(&str, &str); 4] = [
    ("splice", "0.4.0"),
    ("hoist", "0.4.1"),
    ("berth", "0.1.0"),
    ("bosun", "0.9.2"),
];

#[test]
fn an_exact_older_than_latest_release_is_pinned_with_its_own_integrity() {
    let d = repo();
    // npm latest is 0.9.2; the target is 0.9.1, which is published too.
    let reg = Registry::start_with(
        &ALL,
        &[
            ("splice", "0.4.0"),
            ("hoist", "0.4.1"),
            ("berth", "0.1.0"),
            ("bosun", "0.9.1"),
        ],
        &[],
        &[],
        None,
        None,
    );
    let (code, v) = pin_to(d.path(), &reg, &["--to", "0.9.1", "--tool", "bosun"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(
        v["data"]["changed"],
        json!([{"tool": "bosun", "from": "0.9.0", "to": "0.9.1"}])
    );
    assert_eq!(read(d.path(), ".bosun/version"), "0.9.1\n");
    let script = read(d.path(), "docs/setup-script.sh");
    assert!(script.contains("BOSUN_VERSION=0.9.1\n"), "{script}");
    let want = sha(b"bosun@0.9.1");
    assert!(
        script.contains(&format!("BOSUN_SHA512={want}\n")),
        "{script}"
    );
    // The `latest` document was never asked for.
    assert!(
        !reg.log
            .lock()
            .unwrap()
            .iter()
            .any(|u| u.ends_with("/latest"))
    );
}

#[test]
fn an_unpublished_release_is_refused_with_a_fix_and_nothing_is_written() {
    let d = repo();
    let before = read(d.path(), "docs/setup-script.sh");
    let reg = Registry::start(&ALL, None, None);
    let (code, v) = pin_to(d.path(), &reg, &["--to", "0.9.7", "--tool", "bosun"]);
    assert_eq!(code, 1, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("is not published on npm"),
        "{v}"
    );
    assert!(!v["error"]["fix"].as_str().unwrap().is_empty());
    assert_eq!(read(d.path(), "docs/setup-script.sh"), before);
    assert_eq!(read(d.path(), ".bosun/version"), "0.9.0\n");
}

#[test]
fn a_deprecated_release_is_refused() {
    let d = repo();
    let before = read(d.path(), "docs/setup-script.sh");
    let reg = Registry::start_with(&ALL, &[("bosun", "0.9.1")], &["0.9.1"], &[], None, None);
    let (code, v) = pin_to(d.path(), &reg, &["--to", "0.9.1", "--tool", "bosun"]);
    assert_eq!(code, 1, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("deprecated"),
        "{v}"
    );
    assert!(!v["error"]["fix"].as_str().unwrap().is_empty());
    assert_eq!(read(d.path(), "docs/setup-script.sh"), before);
}

#[test]
fn a_deprecated_platform_package_is_refused_too_and_a_missing_one_is_named() {
    let d = repo();
    let before = read(d.path(), "docs/setup-script.sh");
    let reg = Registry::start_with(
        &ALL,
        &[("bosun", "0.9.1")],
        &["bosun-linux-x64@0.9.1"],
        &[],
        None,
        None,
    );
    let (code, v) = pin_to(d.path(), &reg, &["--to", "0.9.1", "--tool", "bosun"]);
    assert_eq!(code, 1, "{v}");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("bosun-linux-x64") && msg.contains("deprecated"),
        "{v}"
    );
    assert_eq!(read(d.path(), "docs/setup-script.sh"), before);
    // The 404 fix names the package of the tool that is missing, not bosun's.
    let (code, v) = pin_to(d.path(), &reg, &["--to", "0.4.7", "--tool", "hoist"]);
    assert_eq!(code, 1, "{v}");
    assert!(
        v["error"]["fix"]
            .as_str()
            .unwrap()
            .contains("@keelstackhq/hoist versions"),
        "{v}"
    );
}

#[test]
fn a_platform_package_that_is_not_published_is_named_in_the_error_and_its_fix() {
    let d = repo();
    let before = read(d.path(), "docs/setup-script.sh");
    let reg = Registry::start_with(
        &ALL,
        &[("bosun", "0.9.1")],
        &[],
        &["bosun-linux-x64@0.9.1"],
        None,
        None,
    );
    let (code, v) = pin_to(d.path(), &reg, &["--to", "0.9.1", "--tool", "bosun"]);
    assert_eq!(code, 1, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("@keelstackhq/bosun-linux-x64@0.9.1 is not published"),
        "{v}"
    );
    assert!(
        v["error"]["fix"]
            .as_str()
            .unwrap()
            .contains("@keelstackhq/bosun-linux-x64 versions"),
        "{v}"
    );
    assert_eq!(read(d.path(), "docs/setup-script.sh"), before);
}

#[test]
fn a_downgrade_with_to_is_still_a_conflict() {
    let d = repo();
    let reg = Registry::start_with(&ALL, &[("bosun", "0.8.0")], &[], &[], None, None);
    let (code, v) = pin_to(d.path(), &reg, &["--to", "0.8.0", "--tool", "bosun"]);
    assert_eq!(code, 3, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("the requested 0.8.0 is older than the pin 0.9.0"),
        "{v}"
    );
    assert_eq!(read(d.path(), ".bosun/version"), "0.9.0\n");
}

#[test]
fn to_needs_an_exact_release_and_excludes_latest() {
    let d = repo();
    for args in [
        vec!["--to", "latest"],
        vec!["--to", "1.2"],
        vec!["--to", "1.2.3-rc1"],
        vec!["--to", "0.9.1", "--latest"],
        vec![],
    ] {
        let out = bosun()
            .args(["-C", d.path().to_str().unwrap(), "pin"])
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}
