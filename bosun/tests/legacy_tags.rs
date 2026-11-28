//! Every old bosun release tag maps to a commit in this repository that carries that version.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bosun has a parent")
        .to_path_buf()
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn parse(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut it = l.split_whitespace();
            let (v, c) = (it.next().expect("version"), it.next().expect("commit"));
            assert!(it.next().is_none(), "extra field in line: {l}");
            assert!(
                c.len() >= 8 && c.chars().all(|ch| ch.is_ascii_hexdigit()),
                "bad commit in line: {l}"
            );
            (v.to_string(), c.to_string())
        })
        .collect()
}

fn cargo_version(toml: &str) -> Option<String> {
    toml.lines()
        .find_map(|l| l.strip_prefix("version = \""))
        .and_then(|r| r.strip_suffix('"'))
        .map(str::to_string)
}

#[test]
fn legacy_tags_exist_and_carry_their_version() {
    let text =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("legacy-tags.txt"))
            .expect("bosun/legacy-tags.txt exists");
    let entries = parse(&text);
    assert!(
        entries.len() >= 25,
        "expected 0.1.0..0.21.0, got {}",
        entries.len()
    );
    assert!(
        entries
            .iter()
            .any(|(v, c)| v == "0.21.0" && c.starts_with("f8941aa1"))
    );

    if git(&["rev-parse", "--is-shallow-repository"]).as_deref() != Some("false") {
        eprintln!("skipping: shallow or missing git history");
        return;
    }
    for (version, commit) in entries {
        let kind = git(&["cat-file", "-t", &commit]);
        assert_eq!(
            kind.as_deref(),
            Some("commit"),
            "{version}: {commit} is not in history"
        );
        let toml = git(&["show", &format!("{commit}:bosun/Cargo.toml")])
            .unwrap_or_else(|| panic!("{version}: no bosun/Cargo.toml at {commit}"));
        assert_eq!(
            cargo_version(&toml).as_deref(),
            Some(version.as_str()),
            "{version}: {commit} carries a different bosun version"
        );
    }
}
