//! Plan files carry `allow:` and touch lists, so every `plans/` directory is R-gov (owner-gated),
//! in the built-in rules, this repo's rules and the channel's template.

use bosun::core::risk::Rules;
use std::path::Path;

fn read(path: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

/// The committed copy of `path` (HEAD), else the working-tree file: CI puts the base's `.bosun/`
/// over the checkout before the gates (as in tests/governance.rs). `path` is relative to the
/// monorepo root, whose `.bosun/` bosun runs on.
fn committed(path: &str) -> String {
    std::process::Command::new("git")
        .args(["show", &format!("HEAD:{path}")])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| read(&format!("../{path}")))
}

fn gov_matches(rules: &Rules, path: &str) -> bool {
    rules
        .rules
        .iter()
        .find(|r| r.id == "R-gov")
        .expect("R-gov")
        .paths
        .iter()
        .any(|g| g.matches(path))
}

#[test]
fn any_plans_directory_is_r_gov() {
    let built = Rules::defaults();
    let repo = Rules::parse(&committed(".bosun/risk.toml")).unwrap();
    for rules in [&built, &repo] {
        for p in [
            "plans/bosun.md",
            "docs/superpowers/plans/2026-10-22-bosun-seed.md",
        ] {
            assert!(gov_matches(rules, p), "{p}");
        }
        assert!(!gov_matches(rules, "src/plan.rs"));
    }
    assert!(read("channel/pieces/core/files/.bosun/risk.toml.jinja").contains(r#""**/plans/**""#));
}

/// `system.toml` (D-031) lists credentials and who owns and rotates them, so it is R-gov in the
/// built-in rules, this repo's rules and the channel template. The generated `SYSTEM.md` is not.
#[test]
fn system_toml_is_r_gov_but_system_md_is_not() {
    let built = Rules::defaults();
    let repo = Rules::parse(&committed(".bosun/risk.toml")).unwrap();
    for rules in [&built, &repo] {
        for p in ["system.toml", "hoist/system.toml", "crates/x/system.toml"] {
            assert!(gov_matches(rules, p), "{p}");
        }
        assert!(!gov_matches(rules, "SYSTEM.md"));
        assert!(!gov_matches(rules, "src/system.rs"));
    }
    assert!(
        read("channel/pieces/core/files/.bosun/risk.toml.jinja").contains(r#""**/system.toml""#)
    );
}
