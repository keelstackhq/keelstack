use hoist::HoistError;
use hoist::changeset::{Changeset, CsType, glob_match, parse, read_config, read_dir, render};
use std::fs;
use tempfile::TempDir;

fn repo(files: &[(&str, &str)]) -> TempDir {
    let t = tempfile::tempdir().expect("tempdir");
    for (p, c) in files {
        let path = t.path().join(p);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, c).expect("write");
    }
    t
}

fn config_err<T: std::fmt::Debug>(r: Result<T, HoistError>) -> String {
    match r {
        Err(e @ HoistError::Config { .. }) => {
            assert_eq!(e.exit_code(), 3);
            e.to_string()
        }
        other => panic!("expected config error, got {other:?}"),
    }
}

fn rel(name: &str, t: CsType) -> (String, CsType) {
    (name.to_string(), t)
}

#[test]
fn parses_changesets_cli_output_and_crlf() {
    let lf = "---\n\"@scope/a\": minor\n'b': patch\nc: major\n---\n\nline1\nline2\n";
    let crlf = lf.replace('\n', "\r\n");
    for text in [lf.to_string(), crlf] {
        let c = parse("x", &text).expect("parse");
        assert_eq!(c.id, "x");
        assert_eq!(
            c.releases,
            vec![
                rel("@scope/a", CsType::Minor),
                rel("b", CsType::Patch),
                rel("c", CsType::Major)
            ]
        );
        assert_eq!(c.summary, "line1\nline2");
    }
}

#[test]
fn empty_changeset_has_no_releases() {
    for text in ["---\n---\n", "---\n---\n\nonly docs\n", "\n  ---\n\n---\n"] {
        let c = parse("e", text).expect(text);
        assert!(c.releases.is_empty(), "{text:?}");
    }
    assert_eq!(
        parse("e", "---\n---\n\nonly docs\n").unwrap().summary,
        "only docs"
    );
}

#[test]
fn render_matches_changesets_write_bytes() {
    let c = Changeset {
        id: "x".into(),
        releases: vec![rel("@scope/a", CsType::Minor), rel("b", CsType::None)],
        summary: "Add thing.\n\nMore.".into(),
    };
    assert_eq!(
        render(&c),
        "---\n\"@scope/a\": minor\n\"b\": none\n---\n\nAdd thing.\n\nMore.\n"
    );
    let empty = Changeset {
        id: "e".into(),
        releases: vec![],
        summary: String::new(),
    };
    assert_eq!(render(&empty), "---\n---\n");
    let empty_msg = Changeset {
        summary: "docs".into(),
        ..empty
    };
    assert_eq!(render(&empty_msg), "---\n---\n\ndocs\n");
}

#[test]
fn render_then_parse_roundtrips() {
    let c = Changeset {
        id: "r".into(),
        releases: vec![
            rel("keel-bosun", CsType::Patch),
            rel("@k/x", CsType::Major),
            rel("q\"uote", CsType::Minor),
        ],
        summary: "Fix: a \"b\" c.".into(),
    };
    assert_eq!(parse("r", &render(&c)).expect("parse"), c);
    let e = Changeset {
        id: "r".into(),
        releases: vec![],
        summary: "docs".into(),
    };
    assert_eq!(parse("r", &render(&e)).expect("parse"), e);
}

#[test]
fn parse_errors() {
    let cases = [
        ("", "file is empty"),
        ("  \n\n", "file is empty"),
        ("no frontmatter\n", "missing or invalid frontmatter"),
        ("---\na: patch\n", "missing or invalid frontmatter"),
        ("---\na: huge\n---\n", "line 2"),
        ("---\na: huge\n---\n", "huge"),
        ("---\na: patch\nb: minor\na: major\n---\n", "line 4"),
        ("---\na: patch\nb: minor\na: major\n---\n", "duplicate"),
        ("---\na:\n  b: patch\n---\n", "line 2"),
        ("---\n- a\n---\n", "line 2"),
        ("---\n\"\": patch\n---\n", "line 2"),
        ("---\n\"a: patch\n---\n", "line 2"),
    ];
    for (text, want) in cases {
        let e = parse("x", text).expect_err(text);
        assert!(e.contains(want), "{text:?}: {e}");
    }
}

#[test]
fn read_dir_errors_name_the_file() {
    let t = repo(&[(".changeset/bad-one.md", "---\na: huge\n---\n")]);
    let e = config_err(read_dir(t.path()));
    assert!(e.contains(".changeset/bad-one.md"), "{e}");
    assert!(e.contains("huge"), "{e}");
    let t = repo(&[(".changeset/empty.md", "")]);
    let e = config_err(read_dir(t.path()));
    assert!(
        e.contains(".changeset/empty.md") && e.contains("file is empty"),
        "{e}"
    );
}

#[test]
fn read_dir_refuses_whitespace_only_ids() {
    for name in [" .md", "  .md", "\t.md"] {
        let t = repo(&[(&format!(".changeset/{name}"), "---\na: patch\n---\n")]);
        let e = config_err(read_dir(t.path()));
        assert!(e.contains("id") && e.contains("blank"), "{name:?}: {e}");
    }
}

#[test]
fn read_dir_skips_readme_and_dotfiles() {
    let t = repo(&[
        (".changeset/README.md", "# not a changeset"),
        (".changeset/readme.MD", "nope"),
        (".changeset/AGENTS.md", "nope"),
        (".changeset/claude.md", "nope"),
        (".changeset/GEMINI.md", "nope"),
        (".changeset/.hidden.md", "nope"),
        (".changeset/config.json", "{}"),
        (".changeset/notes.txt", "nope"),
        (".changeset/sub/inner.md", "nope"),
        (".changeset/zeta.md", "---\nz: patch\n---\n\nz\n"),
        (".changeset/alpha.md", "---\n---\n"),
    ]);
    let ids: Vec<String> = read_dir(t.path())
        .expect("read")
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(ids, vec!["alpha", "zeta"]);
    let none = repo(&[("x", "")]);
    assert!(read_dir(none.path()).expect("no dir").is_empty());
}

#[cfg(unix)]
#[test]
fn read_dir_refuses_symlinks() {
    use std::os::unix::fs::symlink;
    let t = repo(&[
        ("outside.md", "---\na: patch\n---\n\nx\n"),
        ("outdir/changes.json", "{}"),
    ]);
    fs::create_dir_all(t.path().join(".changeset")).expect("mkdir");
    symlink(
        t.path().join("outside.md"),
        t.path().join(".changeset/link.md"),
    )
    .expect("symlink");
    let e = config_err(read_dir(t.path()));
    assert!(e.contains(".changeset/link.md"), "{e}");
    assert!(
        e.contains("is a symlink; changesets must be regular files"),
        "{e}"
    );
    fs::remove_file(t.path().join(".changeset/link.md")).expect("rm");
    symlink(t.path().join("outdir"), t.path().join(".changeset/dirlink")).expect("symlink");
    assert!(
        read_dir(t.path()).is_ok(),
        "non-.md symlinks are ignored like other non-changesets"
    );
}

#[cfg(unix)]
#[test]
fn read_dir_refuses_a_fifo_named_like_a_changeset() {
    let t = repo(&[("x", "")]);
    fs::create_dir_all(t.path().join(".changeset")).expect("mkdir");
    let fifo = t.path().join(".changeset/pipe.md");
    let ok = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "mkfifo");
    // Opening a FIFO for reading would block forever; it must be refused or skipped.
    let (tx, rx) = std::sync::mpsc::channel();
    let root = t.path().to_path_buf();
    std::thread::spawn(move || {
        let r = read_dir(&root);
        let _ = tx.send(r.map(|v| v.len()).map_err(|e| e.to_string()));
    });
    let r = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("read_dir blocked on a FIFO");
    let e = r.expect_err("a FIFO changeset is refused");
    assert!(e.contains(".changeset/pipe.md"), "{e}");
    assert!(e.contains("not a regular file"), "{e}");
}

#[cfg(unix)]
#[test]
fn a_symlinked_changeset_dir_or_config_is_refused() {
    use std::os::unix::fs::symlink;
    let t = repo(&[
        ("elsewhere/one.md", "---\na: patch\n---\n\nx\n"),
        ("elsewhere/config.json", INIT),
    ]);
    symlink(t.path().join("elsewhere"), t.path().join(".changeset")).expect("symlink");
    let e = config_err(read_dir(t.path()));
    assert!(e.contains(".changeset"), "{e}");
    assert!(e.contains("is a symlink"), "{e}");
    let e = config_err(read_config(t.path()));
    assert!(e.contains("is a symlink"), "{e}");
    fs::remove_file(t.path().join(".changeset")).expect("rm");
    fs::create_dir_all(t.path().join(".changeset")).expect("mkdir");
    symlink(
        t.path().join("elsewhere/config.json"),
        t.path().join(".changeset/config.json"),
    )
    .expect("symlink");
    let e = config_err(read_config(t.path()));
    assert!(e.contains(".changeset/config.json"), "{e}");
    assert!(e.contains("is a symlink"), "{e}");
}

#[test]
fn legacy_changes_json_dir_is_an_error() {
    let t = repo(&[(".changeset/old-one/changes.json", "{}")]);
    let e = config_err(read_dir(t.path()));
    assert!(e.contains(".changeset/old-one"), "{e}");
    assert!(e.contains("changes.json"), "{e}");
}

const INIT: &str = r#"{
  "$schema": "https://unpkg.com/@changesets/config@3.1.4/schema.json",
  "changelog": "@changesets/cli/changelog",
  "commit": false,
  "fixed": [],
  "linked": [],
  "access": "restricted",
  "baseBranch": "main",
  "updateInternalDependencies": "patch",
  "ignore": []
}
"#;

fn cfg(json: &str) -> TempDir {
    repo(&[(".changeset/config.json", json)])
}

#[test]
fn init_defaults_parse() {
    let t = cfg(INIT);
    let c = read_config(t.path()).expect("read").expect("some");
    assert_eq!(c.base_branch, "main");
    assert!(c.changelog);
    assert!(c.fixed.is_empty() && c.linked.is_empty() && c.ignore.is_empty());
    assert_eq!(c.update_internal_dependencies, CsType::Patch);
    assert_eq!(c.changed_file_patterns, vec!["**".to_string()]);
    assert!(!c.only_peer_out_of_range);
    assert!(!c.dependents_always);
    assert_eq!(c.access.as_deref(), Some("restricted"));
}

#[test]
fn no_config_is_commits_mode() {
    let t = repo(&[(".changeset/a.md", "---\n---\n")]);
    assert!(read_config(t.path()).expect("read").is_none());
}

#[test]
fn all_known_keys_parse() {
    let t = cfg(r#"{
      "$schema": "x", "changelog": false, "commit": false, "access": "public",
      "privatePackages": {"version": true, "tag": false}, "prettier": false,
      "snapshot": {"useCalculatedVersion": true}, "bumpVersionsWithWorkspaceProtocolOnly": true,
      "baseBranch": "main", "fixed": [["a", "b"]], "linked": [["@k/*"]], "ignore": ["c"],
      "updateInternalDependencies": "minor", "changedFilePatterns": ["src/**"],
      "___experimentalUnsafeOptions_WILL_CHANGE_IN_PATCH": {
        "onlyUpdatePeerDependentsWhenOutOfRange": true, "updateInternalDependents": "always"
      }
    }"#);
    let c = read_config(t.path()).expect("read").expect("some");
    assert!(!c.changelog);
    assert_eq!(c.fixed, vec![vec!["a".to_string(), "b".to_string()]]);
    assert_eq!(c.linked, vec![vec!["@k/*".to_string()]]);
    assert_eq!(c.ignore, vec!["c".to_string()]);
    assert_eq!(c.update_internal_dependencies, CsType::Minor);
    assert_eq!(c.changed_file_patterns, vec!["src/**".to_string()]);
    assert!(c.only_peer_out_of_range && c.dependents_always);
    assert_eq!(c.access.as_deref(), Some("public"));
}

#[test]
fn unknown_key_is_exit_3_naming_key() {
    let e = config_err(read_config(
        cfg(r#"{"baseBranch": "main", "bogusKey": 1}"#).path(),
    ));
    assert!(
        e.contains(".changeset/config.json") && e.contains("bogusKey"),
        "{e}"
    );
    let e = config_err(read_config(
        cfg(r#"{"___experimentalUnsafeOptions_WILL_CHANGE_IN_PATCH": {"nope": 1}}"#).path(),
    ));
    assert!(e.contains("nope"), "{e}");
}

#[test]
fn commit_true_refused() {
    for v in ["true", r#""@changesets/cli/commit""#] {
        let e = config_err(read_config(cfg(&format!(r#"{{"commit": {v}}}"#)).path()));
        assert!(e.contains("commit"), "{e}");
    }
}

#[test]
fn custom_changelog_refused() {
    for v in [
        r#""@changesets/changelog-github""#,
        r#"["@changesets/changelog-github", {"repo": "a/b"}]"#,
        "true",
    ] {
        let e = config_err(read_config(cfg(&format!(r#"{{"changelog": {v}}}"#)).path()));
        assert!(e.contains("changelog"), "{e}");
    }
    for v in [
        r#""@changesets/changelog-git""#,
        r#"["@changesets/cli/changelog", {}]"#,
        r#"["@changesets/changelog-git", {"x": 1}]"#,
        r#"["@changesets/cli/changelog", {"repo": "a/b"}]"#,
    ] {
        let ok = read_config(cfg(&format!(r#"{{"changelog": {v}}}"#)).path());
        assert!(ok.expect("ok").expect("some").changelog, "{v}");
    }
}

#[test]
fn changelog_false_ok() {
    let c = read_config(cfg(r#"{"changelog": false}"#).path())
        .expect("ok")
        .expect("some");
    assert!(!c.changelog);
}

#[test]
fn base_branch_defaults_to_master() {
    let c = read_config(cfg("{}").path()).expect("ok").expect("some");
    assert_eq!(c.base_branch, "master");
    assert!(c.changelog);
    assert_eq!(c.update_internal_dependencies, CsType::Patch);
    assert_eq!(c.changed_file_patterns, vec!["**".to_string()]);
}

#[test]
fn bad_values_refused() {
    for json in [
        r#"{"updateInternalDependencies": "major"}"#,
        r#"{"access": "secret"}"#,
        r#"{"___experimentalUnsafeOptions_WILL_CHANGE_IN_PATCH": {"updateInternalDependents": "never"}}"#,
        r#"{"fixed": ["a"]}"#,
        "not json",
    ] {
        config_err(read_config(cfg(json).path()));
    }
}

#[test]
fn glob_patterns() {
    let yes = [
        ("**", "src/a/b.rs"),
        ("**", "x"),
        ("src/**", "src/a/b.rs"),
        ("src/**/*.rs", "src/b.rs"),
        ("src/**/*.rs", "src/a/b.rs"),
        ("@k/*", "@k/bosun"),
        ("keel-?", "keel-x"),
        ("*.md", "README.md"),
        ("Cargo.toml", "Cargo.toml"),
        ("npm/**", "npm/bosun/package.json"),
    ];
    let no = [
        ("@k/*", "@k/a/b"),
        ("*.md", "docs/a.md"),
        ("keel-?", "keel-xy"),
        ("keel-?", "keel-"),
        ("src/**", "srcx/a"),
        ("src/*", "src/a/b"),
        ("Cargo.toml", "Cargo.lock"),
        ("?", "/"),
    ];
    for (p, s) in yes {
        assert!(glob_match(p, s), "{p} should match {s}");
    }
    for (p, s) in no {
        assert!(!glob_match(p, s), "{p} should not match {s}");
    }
}

#[test]
fn cstype_order_and_parse() {
    assert!(CsType::None < CsType::Patch);
    assert!(CsType::Patch < CsType::Minor);
    assert!(CsType::Minor < CsType::Major);
    assert_eq!(CsType::parse("minor"), Some(CsType::Minor));
    assert_eq!(CsType::parse("none"), Some(CsType::None));
    assert_eq!(CsType::parse("Major"), None);
    assert_eq!(CsType::Patch.as_str(), "patch");
}

#[test]
fn read_dir_returns_parsed_entries() {
    let t = repo(&[(".changeset/b.md", "---\n\"x\": minor\n---\n\nhello\n")]);
    let got = read_dir(t.path()).expect("read");
    assert_eq!(
        got,
        vec![Changeset {
            id: "b".into(),
            releases: vec![rel("x", CsType::Minor)],
            summary: "hello".into()
        }]
    );
}

#[test]
fn bom_and_tab_comment_parse() {
    let c = parse(
        "b",
        "\u{feff}---\n\"a\": patch\t# note\nb: minor # x\n---\n\nx\n",
    )
    .expect("parse");
    assert_eq!(
        c.releases,
        vec![rel("a", CsType::Patch), rel("b", CsType::Minor)]
    );
}
