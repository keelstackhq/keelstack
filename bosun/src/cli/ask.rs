//! `bosun ask <issue> <question>`: mention the owners in a comment and set `needs-human`.
//! There is no timeout and no automatic answer (spec section 8).

use std::path::Path;

use serde_json::json;

use super::AskArgs;
use crate::app::{App, default_branch, kill_switch, owner_view, set_state};
use crate::cli::risk_check::OWNERS_PATH;
use crate::core::records::Record;
use crate::core::risk::OWNER_TEAM_FIX;
use crate::core::state::State;
use crate::output::{BosunError, CmdResult, Outcome};

pub fn run(dir: Option<&Path>, args: &AskArgs) -> CmdResult {
    super::refuse_secret("the question", &args.question)?;
    let app = App::load(dir)?;
    let (gh, n) = (app.gh.as_ref(), args.issue);
    kill_switch(gh)?;
    let issue = gh.get_issue(n)?.ok_or_else(|| {
        BosunError::error(
            format!("issue #{n} does not exist"),
            "check the number; `bosun status` lists the tasks",
        )
    })?;
    // The owners are read from the default branch.
    let branch = default_branch(gh)?;
    let view = owner_view(gh, &branch)?;
    if view.is_empty() {
        return Err(BosunError::error(
            format!("no owners to ask: {OWNERS_PATH} is missing or empty on {branch}"),
            format!(
                "fill {OWNERS_PATH} with `[[owner]]` tables (`github = \"login\"` or `team = \"org/slug\"`) and merge it"
            ),
        ));
    }
    let mentions = view.mentions();
    let record = Record::Ask {
        session: app.ctx.session.clone(),
        question: args.question.clone(),
    };
    let body = format!(
        "{} {}\n\n{}",
        mentions.join(" "),
        args.question,
        record.to_line()
    );
    let comment = gh.add_comment(n, &body)?;
    set_state(gh, n, &issue, State::NeedsHuman)?;
    // An unfinished task must not spend CI minutes: its open PR goes back to draft.
    let drafted = super::pr::draft_task_prs(&app, n);
    let mut data = json!({"issue": n, "comment_id": comment.id, "owners": view.users,
        "owner_teams": view.teams, "owner_teams_unreadable": view.unreadable});
    if !view.unreadable.is_empty() {
        data["owner_teams_fix"] = json!(OWNER_TEAM_FIX);
    }
    drafted.add_to(&mut data);
    Ok(Outcome {
        data,
        next: drafted.next,
    })
}
