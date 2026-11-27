//! `keel migrate --drift seed|take|keep`: settle the owned files the owner edited or deleted
//! before the update, so `splice check` ends clean and no local line is lost (spec §5 migrate).
//!
//! The splice calls live in `migrate.rs`; this file holds the decisions and the text work.
use serde::Serialize;

/// What `--drift` does with a drifted path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Seed,
    Take,
    Keep,
}

impl Mode {
    /// An empty string is the default (`seed`); clap already rejects anything else.
    pub fn parse(s: &str) -> Mode {
        match s {
            "take" => Mode::Take,
            "keep" => Mode::Keep,
            _ => Mode::Seed,
        }
    }
}

/// One settled path, listed in the JSON `drift` array and the PR body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Moved {
    pub path: String,
    /// `moved` (local lines went to the seed file, the owned file is the template's),
    /// `restored` (a deleted owned file came back), `excluded` (channel file left to the owner),
    /// `taken` (template side forced).
    pub action: String,
    pub moved_lines: usize,
    /// The seed file that received the lines (only for `moved`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<String>,
}

/// The seed file the template names for an owned file: `<dir>/CLAUDE.md` (`CLAUDE.md` at the root).
pub fn seed_path(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/CLAUDE.md"),
        None => "CLAUDE.md".into(),
    }
}

/// The lines of `local` that the template's `restored` text does not have (compared trimmed, blank
/// lines ignored) and that `seed` does not hold yet: what must move so none is lost.
pub fn lines_to_move(local: &str, restored: &str, seed: &str) -> Vec<String> {
    let known = |text: &str, l: &str| text.lines().any(|t| t.trim() == l.trim());
    let mut out: Vec<String> = Vec::new();
    for l in local.lines() {
        if l.trim().is_empty() || known(restored, l) || known(seed, l) {
            continue;
        }
        if !out.iter().any(|o| o.trim() == l.trim()) {
            out.push(l.to_string());
        }
    }
    out
}

/// The seed text with the moved lines appended under a heading naming their origin.
pub fn append_to_seed(seed: &str, from: &str, lines: &[String]) -> String {
    let mut s = seed.to_string();
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    if !s.is_empty() {
        s.push('\n');
    }
    s.push_str(&format!("## Local notes moved from {from}\n\n"));
    for l in lines {
        s.push_str(l);
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_file_is_next_to_the_owned_one() {
        assert_eq!(seed_path("AGENTS.md"), "CLAUDE.md");
        assert_eq!(seed_path("a/b/AGENTS.md"), "a/b/CLAUDE.md");
    }

    #[test]
    fn only_lines_the_template_and_the_seed_lack_move() {
        let local = "# t\nrule\n\nmine\n  mine  \nalready\n";
        let got = lines_to_move(local, "# t\nrule\n", "already\n");
        assert_eq!(got, vec!["mine".to_string()]);
    }

    #[test]
    fn append_keeps_the_seed_and_names_the_origin() {
        let t = append_to_seed("# seed", "x/AGENTS.md", &["a".into(), "b".into()]);
        assert_eq!(
            t,
            "# seed\n\n## Local notes moved from x/AGENTS.md\n\na\nb\n"
        );
        assert!(append_to_seed("", "A", &["a".into()]).starts_with("## Local"));
    }

    #[test]
    fn the_mode_defaults_to_seed() {
        assert_eq!(Mode::parse(""), Mode::Seed);
        assert_eq!(Mode::parse("take"), Mode::Take);
        assert_eq!(Mode::parse("keep"), Mode::Keep);
    }
}
