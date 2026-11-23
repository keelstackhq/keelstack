//! The enumerated intended allows: the only Bash commands the live guard allows although the
//! legacy judge (main's guard) denies them. Each is a literal shape (one command, every word
//! plain: no quotes, `$`, backquotes, redirections, separators, newlines or escapes; globs only
//! where a shape says so) with an example in [`EXAMPLES`], tested both ways. The new guard
//! still judges every one of them first (it reads the scripts they run). Adding one needs
//! evidence that it is intended (a PR or review round) and an example.

/// One enumerated allow: its name and whether `words` (the literal command) has its shape.
struct Allow {
    name: &'static str,
    shape: fn(&[&str]) -> bool,
}

const ALLOWS: &[Allow] = &[
    Allow {
        name: "bump-version",
        shape: bump_version,
    },
    Allow {
        name: "parse-only",
        shape: parse_only,
    },
    Allow {
        name: "pathspec-glob",
        shape: pathspec_glob,
    },
];

/// One example command per enumerated allow: legacy denies it, the live guard allows it.
pub const EXAMPLES: &[(&str, &str)] = &[
    (
        "bump-version",
        "BUMP_DATE=2026-11-23 bash scripts/bump-version.sh 0.22.0",
    ),
    ("parse-only", "bash -n scripts/bump-version.sh"),
    ("pathspec-glob", "git add src/*.rs"),
];

/// A plain word: letters, digits and `-_./:=@,+%`.
fn plain(w: &str) -> bool {
    !w.is_empty()
        && w.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@,+%".contains(c))
}

/// A version `X.Y.Z` (digits only).
fn version(w: &str) -> bool {
    let parts: Vec<&str> = w.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// `[BUMP_DATE=YYYY-MM-DD] bash scripts/bump-version.sh X.Y.Z` (the lockstep bump, AGENTS.md).
fn bump_version(w: &[&str]) -> bool {
    let w = match w.first().and_then(|f| f.strip_prefix("BUMP_DATE=")) {
        Some(d) if d.len() == 10 && d.chars().all(|c| c.is_ascii_digit() || c == '-') => &w[1..],
        Some(_) => return false,
        None => w,
    };
    matches!(w, ["bash", "scripts/bump-version.sh", v] if version(v))
}

/// `bash -n <file>...` or `sh -n <file>...`: the shell only parses the files (#60, round 2).
fn parse_only(w: &[&str]) -> bool {
    matches!(w, [sh, "-n", files @ ..] if matches!(*sh, "bash" | "sh") && !files.is_empty()
        && files.iter().all(|f| plain(f) && !f.starts_with('-')))
}

/// A read-only git command (or `git add`) whose glob words are pathspecs: each glob word has a
/// `/` before its first glob character, or follows a literal `--`, so it cannot expand to an
/// option (#60 g1, #64 round 4). Globs are `*` and `?` only.
fn pathspec_glob(w: &[&str]) -> bool {
    let ["git", sub, rest @ ..] = w else {
        return false;
    };
    if !matches!(
        *sub,
        "add" | "diff" | "log" | "ls-files" | "show" | "status" | "blame"
    ) {
        return false;
    }
    let mut after_dashes = false;
    let mut globbed = false;
    for word in rest {
        if *word == "--" {
            after_dashes = true;
            continue;
        }
        let glob_at = word.find(['*', '?']);
        let ok = word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@,+%*?".contains(c));
        if !ok {
            return false;
        }
        match glob_at {
            None => {
                if word.starts_with('-') && after_dashes {
                    return false;
                }
            }
            Some(i) => {
                // The last path part keeps a literal character (`*.rs`, not `*` or `src/*`),
                // so it cannot expand to a program name such as `git`.
                let last = word.rsplit('/').next().unwrap_or(word);
                if word.starts_with('-')
                    || !(after_dashes || word[..i].contains('/'))
                    || !last.chars().any(|c| c.is_ascii_alphanumeric())
                {
                    return false;
                }
                globbed = true;
            }
        }
    }
    globbed
}

/// The name of the enumerated allow `command` matches exactly, if any.
pub fn allow(command: &str) -> Option<&'static str> {
    let command = command.trim_end_matches('\n');
    if command.contains(['\n', '\r', '\\']) {
        return None;
    }
    let words: Vec<&str> = command
        .split([' ', '\t'])
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return None;
    }
    // Globs only for the pathspec shape; every other shape is all plain words.
    let found = ALLOWS.iter().find(|a| (a.shape)(&words)).map(|a| a.name)?;
    if found != "pathspec-glob" && !words.iter().all(|w| plain(w)) {
        return None;
    }
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::allow;

    #[test]
    fn only_the_literal_shapes_match() {
        for (cmd, want) in [
            ("bash scripts/bump-version.sh 1.2.3", Some("bump-version")),
            ("bash scripts/bump-version.sh 1.2.3; git push -f", None),
            ("bash scripts/bump-version.sh $V", None),
            ("BUMP_DATE=$(x) bash scripts/bump-version.sh 1.2.3", None),
            ("GIT_DIR=x bash scripts/bump-version.sh 1.2.3", None),
            ("bash scripts/bump-version.sh 1.2.3 extra", None),
            // The legacy judge allows it already: no enumerated allow needed.
            ("bash decisions/scripts/check-index.sh", None),
            ("bash -n a.sh b.sh", Some("parse-only")),
            ("bash -n -c x", None),
            ("bash -n a.sh && bash a.sh", None),
            ("bash -n a*.sh", None),
            ("git diff -- *.md", Some("pathspec-glob")),
            ("git log src/*.rs", Some("pathspec-glob")),
            ("git ls-files docs/**/*.md", Some("pathspec-glob")),
            ("git add *.rs", None),
            ("git log --oneline -- src/*", None),
            ("git add ./*", None),
            ("git diff -- *", None),
            ("git add -- -*", None),
            ("git push origin src/*", None),
            ("git diff -- *.md | wc -l", None),
            ("git diff -- '*.md'", None),
            ("git log src/[a]*.rs", None),
            ("git diff src/*.rs\ngit push -f", None),
        ] {
            assert_eq!(allow(cmd), want, "{cmd}");
        }
    }
}
