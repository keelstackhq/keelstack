//! `bosun followups add|list` (D-028, spec section 19): the non-blocking findings of a review
//! verdict go to one rolling open issue labelled `bosun:followups`, one unchecked item each, so
//! they never block a PR and the next PR in that area picks them up.

use std::io::Read;
use std::path::Path;

use serde_json::json;

use super::{FollowupsAddArgs, FollowupsCmd};
use crate::app::{App, kill_switch};
use crate::cli::risk_check::flow_at;
use crate::core::followups::{
    HEADER, LABEL, MAX_BODY, TITLE, append, existing, item_pr, item_text, non_blocking, open_items,
};
use crate::core::verdict::Verdict;
use crate::github::{Github, Issue, IssuePatch, IssueState, PatchState};
use crate::output::{BosunError, CmdResult, Outcome};

pub fn run(dir: Option<&Path>, cmd: &FollowupsCmd) -> CmdResult {
    match cmd {
        FollowupsCmd::Add(a) => add(dir, a),
        FollowupsCmd::List => list(dir),
    }
}

fn read_input(from: &str) -> Result<String, BosunError> {
    let mut text = String::new();
    let res = if from == "-" {
        std::io::stdin().read_to_string(&mut text).map(|_| ())
    } else {
        std::fs::read_to_string(from).map(|t| text = t)
    };
    res.map_err(|e| {
        BosunError::usage(
            format!("cannot read the verdict from {from}: {e}"),
            "pass the reviewer's verdict JSON on stdin or with --from FILE",
        )
    })?;
    Ok(text)
}

/// The oldest open issue labelled `bosun:followups`.
fn rolling(gh: &dyn Github) -> Result<Option<Issue>, BosunError> {
    Ok(rolling_all(gh)?.into_iter().next())
}

/// Every open `bosun:followups` issue, oldest first.
fn rolling_all(gh: &dyn Github) -> Result<Vec<Issue>, BosunError> {
    let mut all: Vec<Issue> = gh
        .list_issues(&[LABEL], IssueState::Open)?
        .into_iter()
        .filter(|i| !i.pull_request)
        .collect();
    all.sort_by_key(|i| i.number);
    Ok(all)
}

fn add(dir: Option<&Path>, a: &FollowupsAddArgs) -> CmdResult {
    let text = read_input(&a.from)?;
    let verdict = Verdict::parse(&text).map_err(|e| {
        BosunError::usage(
            format!("the verdict is not valid: {e}"),
            "pass the reviewer's verdict JSON (the `bosun review-pack` skeleton filled in)",
        )
    })?;
    if verdict.pr != a.pr {
        return Err(BosunError::usage(
            format!("the verdict is for PR #{}, not #{}", verdict.pr, a.pr),
            format!(
                "pass the verdict of PR #{} or name PR #{}",
                a.pr, verdict.pr
            ),
        ));
    }
    let app = App::load(dir)?;
    let gh = app.gh.as_ref();
    kill_switch(gh)?;
    let pull = gh.get_pull(a.pr)?.ok_or_else(|| {
        BosunError::error(
            format!("PR #{} does not exist", a.pr),
            "check the number; `gh pr list` lists the open PRs",
        )
    })?;
    let flow = flow_at(gh, &pull.base_sha)?;
    let found = non_blocking(&verdict, flow.block_on_blocker);
    let texts: Vec<String> = found.iter().map(|f| item_text(a.pr, f)).collect();
    super::refuse_secret("a finding", &texts.join("\n"))?;
    if texts.is_empty() {
        let issue = rolling(gh)?.map(|i| i.number);
        return Ok(Outcome::data(
            json!({"pr": a.pr, "issue": issue, "created": false, "added": 0, "skipped": 0, "items": []}),
        ));
    }
    let (issue, created) = match rolling(gh)? {
        Some(i) => (i, false),
        None => {
            gh.ensure_label(LABEL, "c5def5", "non-blocking review findings (D-028)")?;
            let mine = gh.create_issue(TITLE, HEADER, &[LABEL])?;
            // A parallel run may have opened one too: the oldest stays, the newer closes.
            match rolling(gh)? {
                Some(keep) if keep.number != mine.number => {
                    gh.add_comment(mine.number, &format!("duplicate of #{}", keep.number))?;
                    gh.update_issue(
                        mine.number,
                        IssuePatch {
                            state: Some(PatchState::Closed),
                            state_reason: Some("not_planned".into()),
                            ..IssuePatch::default()
                        },
                    )?;
                    (keep, false)
                }
                _ => (mine, true),
            }
        }
    };
    let fresh = write_items(gh, &issue, &texts)?;
    let duplicates: Vec<u64> = rolling_all(gh)?
        .iter()
        .map(|i| i.number)
        .filter(|n| *n != issue.number)
        .collect();
    Ok(Outcome::data(json!({
        "pr": a.pr, "issue": issue.number, "created": created, "duplicates": duplicates,
        "added": fresh.len(), "skipped": texts.len() - fresh.len(), "items": fresh,
    })))
}

/// Read-modify-write the issue body: drop ticked items, append `texts`, refuse a body over
/// [`MAX_BODY`], then read the issue back and, when a parallel add lost this write, redo it once
/// from the fresh body. Returns the items that were new.
fn write_items(
    gh: &dyn Github,
    first: &Issue,
    texts: &[String],
) -> Result<Vec<String>, BosunError> {
    let number = first.number;
    let mut fresh = Vec::new();
    for attempt in 0..2 {
        // Read right before the write, so a box a human ticked meanwhile is not overwritten.
        let issue = gh.get_issue(number)?.ok_or_else(|| {
            BosunError::error(
                format!("follow-up issue #{number} disappeared"),
                "run `bosun followups add` again",
            )
        })?;
        let (body, new) = append(issue.body_text(), texts);
        if new.is_empty() && attempt == 0 {
            return Ok(new);
        }
        if body.chars().count() > MAX_BODY {
            return Err(BosunError::limit(
                format!(
                    "follow-up issue #{number} would pass {MAX_BODY} characters: nothing was added"
                ),
                format!(
                    "pick up and tick the open items of #{number} (`bosun followups list`), then run `bosun followups add` again"
                ),
            ));
        }
        if !new.is_empty() {
            fresh = new;
            gh.update_issue(
                number,
                IssuePatch {
                    body: Some(body),
                    ..IssuePatch::default()
                },
            )?;
        }
        // Another add may have read the old body at the same time: check that ours is in.
        let back = gh.get_issue(number)?.ok_or_else(|| {
            BosunError::error(
                format!("follow-up issue #{number} disappeared"),
                "run `bosun followups add` again",
            )
        })?;
        let have = open_items(back.body_text());
        if fresh.iter().all(|t| have.contains(&t.as_str())) {
            return Ok(fresh);
        }
    }
    Err(BosunError::conflict(
        format!("follow-up issue #{number} changed during the write and the items may be missing"),
        "run `bosun followups add` again with the same verdict (items already there are skipped)",
    ))
}

fn list(dir: Option<&Path>) -> CmdResult {
    let app = App::load(dir)?;
    let gh = app.gh.as_ref();
    let Some(issue) = rolling(gh)? else {
        return Ok(Outcome::data(
            json!({"issue": null, "open": 0, "ticked": 0, "items": []}),
        ));
    };
    let body = issue.body_text();
    let items: Vec<_> = open_items(body)
        .into_iter()
        .map(|t| json!({"text": t, "pr": item_pr(t)}))
        .collect();
    let ticked = existing(body).len() - items.len();
    Ok(Outcome::data(
        json!({"issue": issue.number, "open": items.len(), "ticked": ticked, "items": items}),
    ))
}
