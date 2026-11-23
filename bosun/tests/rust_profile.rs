//! The built-in rust profile calls the installed `cargo-deny` and `cargo-fmt` binaries, not
//! `cargo deny` / `cargo fmt`: cargo's subcommand lookup honours `[alias]` entries of a
//! `.cargo/config.toml` (#53). The base-config restore removes a PR's `.cargo` first; this is
//! defence in depth.

const RUST: &str = include_str!("../profiles/rust.toml");

fn cmd_of(gate: &str) -> String {
    let at = RUST.find(&format!("[gates.{gate}]")).expect("gate");
    let rest = &RUST[at..];
    let line = rest.lines().find(|l| l.starts_with("cmd = ")).expect("cmd");
    line.to_string()
}

#[test]
fn fmt_and_deps_call_the_binaries_not_the_cargo_subcommands() {
    assert_eq!(cmd_of("fmt"), r#"cmd = ["cargo-fmt", "--check"]"#);
    assert_eq!(cmd_of("deps"), r#"cmd = ["cargo-deny", "check"]"#);
}

#[test]
fn no_gate_goes_through_a_cargo_alias_lookup_for_fmt_or_deny() {
    for l in RUST.lines().filter(|l| l.starts_with("cmd = ")) {
        assert!(
            !l.contains(r#""cargo", "fmt""#) && !l.contains(r#""cargo", "deny""#),
            "{l}"
        );
    }
}
