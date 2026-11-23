//! #51: every restore loop of the base's config checks `realpath -m` on the target directory, so
//! a PR that makes a parent directory a symlink gets no base copy written outside the checkout.
use std::fs;

const CHECK: &str = "realpath -m -- \"$(dirname -- \"$p\")\"";

fn read(rel: &str) -> String {
    fs::read_to_string(format!("{}/{rel}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The text of the step named "the base's config for the gates" up to its last `done`.
fn restore_step(text: &str) -> &str {
    let from = text.find("the base's config for the gates").unwrap();
    let rest = &text[from..];
    let last = rest.find("git show").unwrap();
    &rest[..last + rest[last..].find("done\n").unwrap()]
}

#[test]
fn every_restore_loop_checks_the_target_directory() {
    for (what, rel) in [
        ("ci.yml", "../.github/workflows/ci.yml"),
        (
            "channel",
            "channel/pieces/core/files/.github/workflows/bosun.yml.jinja",
        ),
        (
            "circleci",
            "channel/pieces/circleci/files/.circleci/src/jobs/bosun-ok.yml.jinja",
        ),
    ] {
        let text = read(rel);
        let step = restore_step(&text);
        assert!(step.contains(CHECK), "{what}");
        assert!(
            step.find(CHECK).unwrap() < step.find("mkdir -p").unwrap(),
            "{what}: the check comes before the write"
        );
    }
    // Both channel jobs (bosun-trusted, the required check) carry it.
    let channel = read("channel/pieces/core/files/.github/workflows/bosun.yml.jinja");
    let n = channel
        .matches("its directory links outside the checkout\"; exit 1 ;;")
        .count();
    assert!(n >= 2, "{n}");
}
