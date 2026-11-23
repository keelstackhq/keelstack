//! Differential test of the no-regression rule: every probe from the review rounds (PR #57,
//! #60, #64 and #66 comments) and every command literal of the guard tests is judged by the
//! legacy judge (main's guard) and by the live guard; the live guard must deny whatever the
//! legacy judge denies, unless the command is an enumerated intended allow.

use serde_json::json;

use super::{Script, intended, legacy, script_rules, tool_rules};

const PROBES: &str = include_str!("differential_probes.json");

/// Script files the probes run: a shell script that force-pushes, and binaries (a copy of git,
/// a program with a protected literal) read as text by the legacy judge.
fn read(p: &str) -> Option<Script> {
    let p = p.trim_start_matches("./");
    let evil = "#!/bin/sh\ngit push --force origin main\n";
    let bin =
        "\u{7f}ELF\u{2}\u{1}\u{1}\0\0 usage: git push --force origin main \0 gh pr merge 1 \0";
    match p {
        "scripts/bump-version.sh" => Some(Script::Text(
            include_str!("../../../../scripts/bump-version.sh").into(),
        )),
        "decisions/scripts/check-index.sh" => Some(Script::Text(
            include_str!("../../../../decisions/scripts/check-index.sh").into(),
        )),
        "evil.sh" | "x.sh" | "s.sh" | "a.sh" | "f.sh" | "t.sh" => Some(Script::Text(evil.into())),
        "g" | "gl" | "h" | "evil" | "bin" | "xl" => Some(Script::Binary(Some(bin.into()))),
        _ => None,
    }
}

fn legacy_read(p: &str) -> Option<legacy::Script> {
    read(p).map(|s| match s {
        Script::Text(t) | Script::Binary(Some(t)) => legacy::Script::Text(t),
        Script::Binary(None) => legacy::Script::Unreadable("larger than 1 MiB"),
        Script::Unreadable(w) => legacy::Script::Unreadable(w),
    })
}

#[test]
fn head_denies_whatever_main_denies_outside_the_enumerated_allows() {
    let probes: Vec<String> = serde_json::from_str(PROBES).expect("probes");
    assert!(probes.len() > 1000, "probe set lost");
    let mut leaks = Vec::new();
    let mut caught = 0;
    for branch in ["feat", "main"] {
        let at = |_: &[String]| Some(branch.to_string());
        for p in &probes {
            let input = json!({ "command": p });
            let old = legacy::tool_rules("Bash", &input, &at, "main", "session_x")
                .or_else(|| legacy::script_rules(p, &legacy_read, &at, "main"));
            if old.is_none() {
                continue;
            }
            let raw = super::tool_rules_new("Bash", &input, &at, "main", "session_x")
                .or_else(|| super::script_rules_new(p, &read, &at, "main"));
            let head = tool_rules("Bash", &input, &at, "main", "session_x")
                .or_else(|| script_rules(p, &read, &at, "main"));
            if raw.is_none() {
                caught += 1;
            }
            if head.is_none() && intended::allow(p).is_none() {
                leaks.push(p.clone());
            }
        }
    }
    assert!(leaks.is_empty(), "head allows what main denies: {leaks:#?}");
    eprintln!("differential: {caught} probe(s) the new rules alone would allow, held by legacy");
}

#[test]
fn every_enumerated_allow_is_one_legacy_denies() {
    // An allow that legacy does not deny is not needed: keep the list minimal.
    let at = |_: &[String]| Some("feat".to_string());
    for (name, cmd) in intended::EXAMPLES {
        assert_eq!(intended::allow(cmd), Some(*name), "{cmd}");
        let input = json!({ "command": cmd });
        let old = legacy::tool_rules("Bash", &input, &at, "main", "session_x")
            .or_else(|| legacy::script_rules(cmd, &legacy_read, &at, "main"));
        assert!(
            old.is_some(),
            "legacy allows {cmd}; drop the allow `{name}`"
        );
        let head = tool_rules("Bash", &input, &at, "main", "session_x")
            .or_else(|| script_rules(cmd, &read, &at, "main"));
        assert!(head.is_none(), "{cmd}: {head:?}");
    }
}

/// Opus hunt on #66: the allow shapes themselves. An edited bump script keeps main's verdict;
/// `+nc` after `-n` runs code; a glob after a `--` that is an option's value.
#[test]
fn the_allow_shapes_do_not_carry_main_denies() {
    let at = |_: &[String]| Some("feat".to_string());
    let body = include_str!("../../../../scripts/bump-version.sh");
    let evil = format!("{body}\nbash +c -- '(/usr/bin/g?t push -f origin HEAD:main)'\n");
    let edited = |p: &str| match p.trim_start_matches("./") {
        "scripts/bump-version.sh" => Some(Script::Text(evil.clone())),
        _ => read(p),
    };
    let cmd = "bash scripts/bump-version.sh 1.2.3";
    assert!(
        script_rules(cmd, &edited, &at, "main").is_some(),
        "edited bump script"
    );
    assert!(
        script_rules(cmd, &read, &at, "main").is_none(),
        "pinned bump script"
    );
    for cmd in [
        "bash -n +nc ./evil.sh",
        "sh -n +cn evil.sh",
        "git blame --since -- *.md",
    ] {
        let input = json!({ "command": cmd });
        let head = tool_rules("Bash", &input, &at, "main", "session_x")
            .or_else(|| script_rules(cmd, &read, &at, "main"));
        assert!(head.is_some(), "allowed: {cmd}");
    }
}
