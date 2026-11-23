//! What a reviewer needs besides the diff (spec §7): the use cases of the task's epic. The
//! lessons come from `core::lessons`.

use crate::core::verdict::{Finding, Verdict};

/// The `- UC-…` lines of an epic body.
pub fn use_cases(body: &str) -> Vec<String> {
    body.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("- UC-"))
        .map(str::to_string)
        .collect()
}

/// Group files (name, patch bytes) in the given order into chunks of at most `max_bytes` each,
/// for one reviewer subagent per chunk. A file over `max_bytes` gets a chunk of its own.
pub fn chunks(files: &[(String, usize)], max_bytes: usize) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut used = 0usize;
    for (name, size) in files {
        match out.last_mut() {
            Some(c) if used + size <= max_bytes => {
                c.push(name.clone());
                used += size;
            }
            _ => {
                out.push(vec![name.clone()]);
                used = *size;
            }
        }
    }
    out
}

/// Merge the verdicts of the review chunks of one PR head into one (CI review of a big PR).
/// `results` holds, per chunk number (1-based), its verdict or why it was not reviewed; `total`
/// is the number of chunks of the pack. Fail closed: the merge approves only when all `total`
/// chunks were reviewed and every one approves. Confidence is the minimum (0 when a chunk is
/// missing), findings are the union tagged `[chunk N]`, `chunks_reviewed` lists the chunks that
/// really produced a verdict.
pub fn merge_chunk_verdicts(
    pr: u64,
    head_sha: &str,
    total: u32,
    results: &[(u32, Result<Verdict, String>)],
) -> Verdict {
    let mut merged = Verdict {
        pr,
        head_sha: head_sha.to_string(),
        verdict: "approve".into(),
        confidence: 1.0,
        findings: vec![],
        lessons_hit: vec![],
        escalate_risk: false,
        chunks_reviewed: vec![],
    };
    for k in 1..=total {
        match results.iter().find(|(n, _)| *n == k).map(|(_, r)| r) {
            Some(Ok(v)) if v.pr == pr && v.head_sha == head_sha => {
                merged.chunks_reviewed.push(k);
                let serious = v
                    .findings
                    .iter()
                    .any(|f| f.severity == "blocker" || f.severity == "major");
                if v.verdict != "approve" || serious {
                    merged.verdict = "request-changes".into();
                }
                merged.confidence = merged.confidence.min(v.confidence);
                merged.escalate_risk |= v.escalate_risk;
                for l in &v.lessons_hit {
                    if !merged.lessons_hit.contains(l) {
                        merged.lessons_hit.push(l.clone());
                    }
                }
                merged.findings.extend(v.findings.iter().map(|f| Finding {
                    claim: format!("[chunk {k}] {}", f.claim),
                    ..f.clone()
                }));
            }
            other => {
                let why = match other {
                    Some(Err(e)) => e.as_str(),
                    Some(Ok(_)) => "verdict is for a different pr or head",
                    _ => "no review result",
                };
                merged.verdict = "request-changes".into();
                merged.confidence = 0.0;
                merged.findings.push(Finding {
                    severity: "major".into(),
                    file: String::new(),
                    line: None,
                    claim: format!("[chunk {k}] chunk {k} of {total} was not reviewed"),
                    evidence: why.to_string(),
                });
            }
        }
    }
    if total == 0 {
        merged.verdict = "request-changes".into();
        merged.confidence = 0.0;
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_case_lines() {
        let body = "Epic\n- UC-M1-1: a\n  - UC-M1-2: b\n* not\n";
        assert_eq!(use_cases(body), ["- UC-M1-1: a", "- UC-M1-2: b"]);
    }

    fn ver(v: &str, conf: f64, sev: &[&str]) -> Verdict {
        Verdict {
            pr: 1,
            head_sha: "h".into(),
            verdict: v.into(),
            confidence: conf,
            findings: sev
                .iter()
                .map(|s| Finding {
                    severity: s.to_string(),
                    file: "a.rs".into(),
                    line: Some(1),
                    claim: "c".into(),
                    evidence: "e".into(),
                })
                .collect(),
            lessons_hit: vec![],
            escalate_risk: false,
            chunks_reviewed: vec![],
        }
    }

    #[test]
    fn merge_approves_only_when_every_chunk_approves_and_takes_min_confidence() {
        let ok = [
            (1, Ok(ver("approve", 0.95, &["minor"]))),
            (2, Ok(ver("approve", 0.88, &["nit"]))),
        ];
        let m = merge_chunk_verdicts(1, "h", 2, &ok);
        assert_eq!((m.verdict.as_str(), m.confidence), ("approve", 0.88));
        assert_eq!(m.chunks_reviewed, [1, 2]);
        assert_eq!(m.findings.len(), 2);
        assert!(m.findings[1].claim.starts_with("[chunk 2] "));
        let one_no = [
            (1, Ok(ver("approve", 0.95, &[]))),
            (2, Ok(ver("request-changes", 0.9, &["blocker"]))),
        ];
        assert_eq!(
            merge_chunk_verdicts(1, "h", 2, &one_no).verdict,
            "request-changes"
        );
    }

    #[test]
    fn merge_requests_changes_when_an_approving_chunk_has_a_blocker_or_major() {
        for sev in ["blocker", "major"] {
            let r = [
                (1, Ok(ver("approve", 0.95, &[]))),
                (2, Ok(ver("approve", 0.95, &[sev]))),
            ];
            assert_eq!(
                merge_chunk_verdicts(1, "h", 2, &r).verdict,
                "request-changes",
                "{sev}"
            );
        }
    }

    #[test]
    fn merge_fails_closed_on_a_failed_or_missing_chunk() {
        let r = [
            (1, Ok(ver("approve", 0.95, &[]))),
            (2, Err("API down".to_string())),
        ];
        let m = merge_chunk_verdicts(1, "h", 3, &r);
        assert_eq!((m.verdict.as_str(), m.confidence), ("request-changes", 0.0));
        assert_eq!(m.chunks_reviewed, [1]);
        let claims: Vec<&str> = m.findings.iter().map(|f| f.claim.as_str()).collect();
        assert_eq!(claims.len(), 2, "{claims:?}");
        assert!(m.findings.iter().all(|f| f.severity == "major"));
        assert!(merge_chunk_verdicts(1, "h", 0, &[]).verdict == "request-changes");
    }

    #[test]
    fn merge_rejects_a_chunk_verdict_for_another_pr_or_head() {
        let mut other_head = ver("approve", 0.95, &[]);
        other_head.head_sha = "old".into();
        let mut other_pr = ver("approve", 0.95, &[]);
        other_pr.pr = 2;
        for bad in [other_head, other_pr] {
            let r = [(1, Ok(ver("approve", 0.95, &[]))), (2, Ok(bad))];
            let m = merge_chunk_verdicts(1, "h", 2, &r);
            assert_eq!((m.verdict.as_str(), m.confidence), ("request-changes", 0.0));
            assert_eq!(m.chunks_reviewed, [1]);
        }
    }

    fn f(v: &[(&str, usize)]) -> Vec<(String, usize)> {
        v.iter().map(|(n, s)| (n.to_string(), *s)).collect()
    }

    #[test]
    fn chunks_fill_greedily_in_order() {
        assert!(chunks(&[], 10).is_empty());
        let c = chunks(&f(&[("a", 4), ("b", 5), ("c", 2), ("d", 9)]), 10);
        assert_eq!(c, [vec!["a", "b"], vec!["c"], vec!["d"]]);
        let c = chunks(&f(&[("a", 3), ("b", 3), ("c", 4)]), 10);
        assert_eq!(c, [vec!["a", "b", "c"]]);
    }

    #[test]
    fn a_big_file_is_alone_and_no_chunk_is_empty() {
        let c = chunks(&f(&[("s", 1), ("big", 50), ("t", 1), ("z", 0)]), 10);
        assert_eq!(c, [vec!["s"], vec!["big"], vec!["t", "z"]]);
        assert!(chunks(&f(&[("big", 50)]), 10).iter().all(|c| !c.is_empty()));
    }
}
