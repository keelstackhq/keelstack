//! The rolling follow-ups issue (D-028, spec section 19): the non-blocking findings of review
//! verdicts, one unchecked item each, in the body of one open issue labelled `bosun:followups`.
//! Pure text logic; `cli::followups` does the GitHub I/O.

use crate::core::verdict::{Finding, Verdict};

/// Label of the one rolling issue.
pub const LABEL: &str = "bosun:followups";
/// Title of the rolling issue.
pub const TITLE: &str = "Follow-ups (non-blocking findings)";
/// Opening text of a new rolling issue.
pub const HEADER: &str = "Non-blocking review findings (D-028), one item per finding. The next PR in the area picks an item up and ticks it; bosun only appends. Edit by ticking boxes, never by deleting items (a removed item is added again by the next `bosun followups add` of its verdict).\n\n";

const OPEN: &str = "- [ ] ";
const DONE: &str = "- [x] ";
const MAX_CLAIM: usize = 200;

/// The rank of a severity: blocker 3, major 2, minor 1, nit 0.
fn rank(severity: &str) -> u8 {
    match severity {
        "blocker" => 3,
        "major" => 2,
        "minor" => 1,
        _ => 0,
    }
}

/// The findings of `v` below the blocking level: with `block_on_blocker` everything but a
/// blocker, else minor and nit only.
pub fn non_blocking(v: &Verdict, block_on_blocker: bool) -> Vec<&Finding> {
    let level = if block_on_blocker { 3 } else { 2 };
    v.findings
        .iter()
        .filter(|f| rank(&f.severity) < level)
        .collect()
}

/// One line of text from untrusted finding text: no newline, no marker, no backtick fence.
fn one_line(text: &str, max: usize) -> String {
    let flat: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("<!--", "<!-\u{200b}-")
        .replace("-->", "-\u{200b}->");
    flat.chars().take(max).collect()
}

/// The text of an item (what dedupe compares): `#PR file:line [severity] claim`.
pub fn item_text(pr: u64, f: &Finding) -> String {
    let at = match (f.file.trim(), f.line) {
        ("", _) => "-".to_string(),
        (file, Some(line)) => format!("{}:{line}", one_line(file, 120)),
        (file, None) => one_line(file, 120),
    };
    format!(
        "#{pr} {at} [{}] {}",
        f.severity,
        one_line(&f.claim, MAX_CLAIM)
    )
}

/// The item texts already in `body`, ticked or not.
pub fn existing(body: &str) -> Vec<&str> {
    body.lines()
        .filter_map(|l| l.strip_prefix(OPEN).or_else(|| l.strip_prefix(DONE)))
        .collect()
}

/// The unchecked items of `body`, in order.
pub fn open_items(body: &str) -> Vec<&str> {
    body.lines().filter_map(|l| l.strip_prefix(OPEN)).collect()
}

/// The PR an item text starts with (`#12 ...`).
pub fn item_pr(text: &str) -> Option<u64> {
    text.strip_prefix('#')?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// `body` with the items of `texts` not yet in it appended, and the texts that were new.
pub fn append(body: &str, texts: &[String]) -> (String, Vec<String>) {
    let have = existing(body);
    let mut fresh: Vec<String> = Vec::new();
    for t in texts {
        if !have.contains(&t.as_str()) && !fresh.contains(t) {
            fresh.push(t.clone());
        }
    }
    let mut out = body.trim_end().to_string();
    for t in &fresh {
        out.push_str(&format!("\n{OPEN}{t}"));
    }
    out.push('\n');
    (out, fresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(sev: &str, claim: &str) -> Finding {
        Finding {
            severity: sev.into(),
            file: "a.rs".into(),
            line: Some(3),
            claim: claim.into(),
            evidence: "e".into(),
        }
    }

    fn v(findings: Vec<Finding>) -> Verdict {
        Verdict {
            pr: 5,
            head_sha: "h".into(),
            verdict: "approve".into(),
            confidence: 0.9,
            findings,
            lessons_hit: vec![],
            escalate_risk: false,
            chunks_reviewed: vec![],
        }
    }

    #[test]
    fn the_blocking_level_follows_block_on() {
        let x = v(vec![f("blocker", "a"), f("major", "b"), f("minor", "c"), f("nit", "d")]);
        let sev = |b| {
            non_blocking(&x, b)
                .iter()
                .map(|f| f.severity.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(sev(false), ["minor", "nit"]);
        assert_eq!(sev(true), ["major", "minor", "nit"]);
    }

    #[test]
    fn items_are_one_line_and_cannot_carry_a_marker() {
        let t = item_text(5, &f("minor", "line one\nline <!-- bosun:x --> two"));
        assert!(!t.contains('\n') && !t.contains("<!--") && !t.contains("-->"), "{t}");
        assert!(t.starts_with("#5 a.rs:3 [minor] line one line"), "{t}");
        assert_eq!(item_pr(&t), Some(5));
    }

    #[test]
    fn append_dedupes_ticked_and_repeated_items() {
        let body = "head\n\n- [x] #5 a.rs:3 [minor] done\n- [ ] #5 a.rs:3 [nit] open\n";
        let texts: Vec<String> = [
            "#5 a.rs:3 [minor] done",
            "#5 a.rs:3 [nit] open",
            "#6 b.rs:1 [nit] new",
            "#6 b.rs:1 [nit] new",
        ]
        .map(String::from)
        .into();
        let (out, fresh) = append(body, &texts);
        assert_eq!(fresh, ["#6 b.rs:1 [nit] new"]);
        assert_eq!(open_items(&out), ["#5 a.rs:3 [nit] open", "#6 b.rs:1 [nit] new"]);
        assert_eq!(append(&out, &texts).1.len(), 0);
    }
}
