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
    HEADER, LABEL, TITLE, append, existing, item_pr, item_text, non_blocking, open_items,
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
    Ok(gh
        .list_issues(&[LABEL], IssueState::Open)?
        .into_iter()
        .filter(|i| !i.pull_request)
        .min_by_key(|i| i.number))
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
            format!("pass the verdict of PR #{} or name PR #{}", a.pr, verdict.pr),
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
    let (body, fresh) = append(issue.body_text(), &texts);
    if !fresh.is_empty() {
        gh.update_issue(
            issue.number,
            IssuePatch {
                body: Some(body),
                ..IssuePatch::default()
            },
        )?;
    }
    Ok(Outcome::data(json!({
        "pr": a.pr, "issue": issue.number, "created": created,
        "added": fresh.len(), "skipped": texts.len() - fresh.len(), "items": fresh,
    })))
}

fn list(dir: Option<&Path>) -> CmdResult {
    let app = App::load(dir)?;
    let gh = app.gh.as_ref();
    let Some(issue) = rolling(gh)? else {
        return Ok(Outcome::data(json!({"issue": null, "open": 0, "ticked": 0, "items": []})));
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
