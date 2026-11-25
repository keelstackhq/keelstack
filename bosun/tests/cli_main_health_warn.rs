//! `[main_health] block = false` (D-028): a red main warns instead of blocking. The default
//! (block) is pinned by cli_main_open.rs, cli_leases.rs and cli_spawn.rs.
mod common;

use common::fake::FakeGithub;
use common::run;

const NOW: &str = "2026-10-23T10:00:00Z";
const WARN: &str = "project = \"widget\"\n\n[main_health]\nblock = false\n";

fn red(fake: &FakeGithub) {
    fake.issue(3, "main is red", &["bosun:main-red"], "ci-ok failed");
}

#[test]
fn main_open_passes_with_a_warning_when_block_is_false() {
    let fake = FakeGithub::start();
    red(&fake);
    fake.content(".bosun/bosun.toml", "refs/heads/main", WARN);
    fake.pull(10, "feature", "feat", "No link.");
    let (code, v) = run(
        &fake,
        "session_a",
        NOW,
        &["gate", "main-open", "--pr", "10"],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["pass"], true);
    assert_eq!(v["data"]["main_red"], 3);
    let all = fake.statuses_of("sha10");
    let s = all.last().unwrap();
    assert_eq!(s["state"], "success");
    let d = s["description"].as_str().unwrap();
    assert!(d.contains("main is red (#3) (warning)"), "{d}");
}

#[test]
fn main_open_still_blocks_when_the_config_says_block_true() {
    let fake = FakeGithub::start();
    red(&fake);
    fake.content(
        ".bosun/bosun.toml",
        "refs/heads/main",
        "[main_health]\nblock = true\n",
    );
    fake.pull(10, "feature", "feat", "No link.");
    let (code, v) = run(
        &fake,
        "session_a",
        NOW,
        &["gate", "main-open", "--pr", "10"],
    );
    assert_eq!(code, 4, "{v}");
}

fn claim_with(default_cfg: Option<&str>, tree_cfg: &str) -> i32 {
    let task = r#"Do it.

<!-- bosun:task {"item":"M1","task":"T1","touch":["src/a.rs"],"kind":"feature"} -->"#;
    let fake = FakeGithub::start();
    fake.issue(10, "task", &["bosun:state/ready"], task);
    red(&fake);
    if let Some(c) = default_cfg {
        fake.content(".bosun/bosun.toml", "refs/heads/main", c);
    }
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join(".bosun")).unwrap();
    std::fs::write(d.path().join(".bosun/bosun.toml"), tree_cfg).unwrap();
    let (code, v) = run(
        &fake,
        "session_a",
        NOW,
        &["-C", d.path().to_str().unwrap(), "claim", "10"],
    );
    eprintln!("{v}");
    code
}

#[test]
fn claim_reads_main_health_from_the_default_branch_not_the_working_tree() {
    // The default branch says warn: the claim goes on, whatever the working tree says.
    assert_eq!(claim_with(Some(WARN), "project = \"widget\"\n"), 0);
    assert_eq!(claim_with(Some(WARN), "[main_health]\nblock = true\n"), 0);
    // The working tree says warn, the default branch blocks (or has no config): refused.
    assert_eq!(claim_with(Some("project = \"widget\"\n"), WARN), 4);
    assert_eq!(claim_with(None, WARN), 4);
}
