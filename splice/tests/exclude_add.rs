//! `splice channel exclude NAME PATH...`: add paths to a channel's stored `exclude` (idempotent).

mod common;

use common::*;
use splice::lock::ChannelLock;
use std::path::Path;
use tempfile::TempDir;

const OWNED: &str = ".claude/agents/reviewer.md";
const LOCK: &str = ".splice/channels/rig.toml";

fn setup() -> (TempDir, TempDir, TempDir) {
    let tpl = template_repo(&["v1"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let ch = channel_repo(&["v1"]);
    let src = format!("{}//channel", ch.path().display());
    chan_commit(
        repo.path(),
        &[
            "channel", "add", &src, "--as", "rig", "--ref", "v1.0.0", "--piece", "claude",
        ],
    );
    (tpl, repo, ch)
}

fn rig(repo: &Path) -> ChannelLock {
    ChannelLock::read(repo, "rig").unwrap().expect("lock")
}

#[test]
fn excluding_a_deleted_channel_file_settles_check_and_keeps_the_disk() {
    let (_t, repo, _c) = setup();
    let r = repo.path();
    assert!(rig(r).lock.files.contains_key(OWNED));
    std::fs::remove_file(r.join(OWNED)).unwrap();
    chan(r, &["check", "--offline"]).code(1);

    let out = chan(r, &["channel", "exclude", "rig", OWNED]).success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["ok"], true);

    let lock = rig(r);
    assert_eq!(lock.meta.exclude, vec![OWNED.to_string()]);
    assert!(!lock.lock.files.contains_key(OWNED));
    assert!(!r.join(OWNED).exists(), "the disk is not touched");
    chan(r, &["check", "--offline"]).success();
}

#[test]
fn excluding_an_edited_file_keeps_the_local_text() {
    let (_t, repo, _c) = setup();
    let r = repo.path();
    std::fs::write(r.join(OWNED), "mine\n").unwrap();
    chan(r, &["check", "--offline"]).code(1);
    chan(r, &["channel", "exclude", "rig", OWNED]).success();
    assert_eq!(std::fs::read_to_string(r.join(OWNED)).unwrap(), "mine\n");
    chan(r, &["check", "--offline"]).success();
}

#[test]
fn it_is_idempotent_and_keeps_existing_filters() {
    let (_t, repo, _c) = setup();
    let r = repo.path();
    chan(r, &["channel", "exclude", "rig", OWNED, "a/b.md"]).success();
    let first = std::fs::read(r.join(LOCK)).unwrap();
    chan(r, &["channel", "exclude", "rig", OWNED]).success();
    assert_eq!(std::fs::read(r.join(LOCK)).unwrap(), first);
    assert_eq!(
        rig(r).meta.exclude,
        vec![OWNED.to_string(), "a/b.md".to_string()]
    );
}

#[test]
fn an_unknown_channel_is_a_usage_error() {
    let (_t, repo, _c) = setup();
    chan(repo.path(), &["channel", "exclude", "nope", "x"]).code(2);
}
