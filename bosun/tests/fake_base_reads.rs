//! Guard for the fake GitHub's base -> main read fallback (`tests/common/fake.rs`).
//!
//! A pull's base sha (`base0000`) reads a path nobody wrote at that sha from the default branch
//! (`refs/heads/main`). That is the only fallback: it must stay explicit (`absent_at` turns it
//! off per path) and it must never apply to another ref, or a test of a head-side or base-side
//! read would pass for the wrong reason.
mod common;

use common::fake::FakeGithub;

const BASE: &str = "base0000";
const MAIN: &str = "refs/heads/main";
const HEAD: &str = "headfeed";

/// The status of `GET contents/<path>?ref=<git_ref>` and the decoded text (if 200).
fn read(fake: &FakeGithub, path: &str, git_ref: &str) -> (u16, Option<String>) {
    let url = format!(
        "{}/repos/acme/widget/contents/{path}?ref={git_ref}",
        fake.url()
    );
    let mut resp = ureq::get(&url)
        .header("Authorization", "Bearer t")
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .unwrap();
    let code = resp.status().as_u16();
    let text = (code == 200).then(|| {
        let v: serde_json::Value =
            serde_json::from_str(&resp.body_mut().read_to_string().unwrap()).unwrap();
        let raw: String = v["content"]
            .as_str()
            .unwrap()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        String::from_utf8(bosun::github::b64::decode(&raw).unwrap()).unwrap()
    });
    (code, text)
}

fn fake() -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "");
    fake
}

#[test]
fn a_base_read_falls_back_to_main_only_for_the_base_sha() {
    let fake = fake();
    fake.content("a.toml", MAIN, "main text");
    // The documented fallback: the pull's base sha reads main's file.
    assert_eq!(read(&fake, "a.toml", BASE), (200, Some("main text".into())));
    // Nothing else falls back: not the head, not an unknown sha.
    assert_eq!(read(&fake, "a.toml", HEAD).0, 404);
    assert_eq!(read(&fake, "a.toml", "sha-of-nothing").0, 404);
}

#[test]
fn a_file_written_at_the_base_wins_over_main_and_absent_at_stops_the_fallback() {
    let fake = fake();
    fake.content("a.toml", MAIN, "main text");
    fake.content("a.toml", BASE, "base text");
    assert_eq!(read(&fake, "a.toml", BASE), (200, Some("base text".into())));
    assert_eq!(read(&fake, "a.toml", MAIN), (200, Some("main text".into())));
    // A file main has and the base lacks: `absent_at` is how a test says so.
    fake.content("b.toml", MAIN, "main only");
    assert_eq!(read(&fake, "b.toml", BASE).0, 200);
    fake.absent_at("b.toml", BASE);
    assert_eq!(read(&fake, "b.toml", BASE).0, 404);
    assert_eq!(read(&fake, "b.toml", MAIN).0, 200);
}

#[test]
fn without_a_pull_no_ref_is_a_base() {
    let fake = FakeGithub::start();
    fake.content("a.toml", MAIN, "main text");
    assert_eq!(read(&fake, "a.toml", BASE).0, 404);
}
