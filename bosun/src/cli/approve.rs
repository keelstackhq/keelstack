//! `bosun approve`: lists the PRs that wait for an owner. The owner approves with their own
//! comment `/bosun approve <head>` (posted by this command on their machine, or by hand in the
//! GitHub web or mobile app), a GitHub review, or a signed approval (`--sign-key`; spec section 8).
//! Chat approvals are retired (D-025): the owner's answer in a chat form is never an approval.
//! Every GitHub call uses `BOSUN_OWNER_TOKEN`, never `GITHUB_TOKEN`: the comment must come from
//! a plain user token, which agent sessions do not have. `--list` only reads: without the owner
//! token it reads with no token at all and lists what it can. The owner names the head sha of each
//! PR (`<pr>@<sha>`, from `--list`), so a push after the listing is never approved unseen.

use std::path::Path;

use serde_json::{Value, json};

use super::ApproveArgs;
use crate::app::App;
use crate::app::{Result, default_ref, kill_switch, main_red, owner_view};
use crate::cli::carry_over::unreadable_withdrawers;
use crate::cli::pr_no_ci::provider_default;
use crate::cli::risk_check::{
    autonomous_clean, autonomous_inputs, decisive_review, dismissed_review, enrolled_keys,
    owner_comment, quorum_clean, quorum_policy_at, review_models,
};
use crate::config::{CONFIG_FILE, CiProvider, Config};
use crate::core::approve::{
    APPROVERS_PATH, AUTONOMOUS_LINE, BOOTSTRAP_END, DELEGATION_LINE, Form, Policy, QUORUM_LINE,
    Readiness, Role, Run, Stat, TRUSTED_CREATOR, api_url_ok, approve_command, batch_message,
    bootstrap_line, ci_green, in_cloud, only_r_ci, parse_ready_summary, parse_target, policy_line,
    readiness, signed_comment, signed_message, signed_message_autonomous, signed_message_bootstrap,
    signed_message_v2, summary_readiness,
};
use crate::core::ask;
use crate::core::records::{PrHead, Record};
use crate::core::risk;
use crate::ctx::Ctx;
use crate::gates::main_open::{is_mainfix, judge};
use crate::github::{Github, IssueState, LiveGithub, Pull, b64};
use crate::hooks::answers;
use crate::output::{BosunError, CmdResult, Outcome};
use crate::registry;

const TOKEN_VAR: &str = "BOSUN_OWNER_TOKEN";
const MAIN_OPEN_WORKFLOW: &str = ".github/workflows/main-open.yml";

pub(crate) struct Ready {
    pub(crate) pr: u64,
    pub(crate) title: String,
    pub(crate) head_sha: String,
    pub(crate) base_sha: String,
    pub(crate) risk: Vec<String>,
    pub(crate) files: Vec<String>,
    /// The newest Actions `review-verdict` description on the head.
    pub(crate) verdict: String,
}

/// GitHub's compare API lists at most this many files and does not say when it cut the list.
const COMPARE_FILES_CAP: usize = 300;

/// Why a PR is not ready when an owner's newest decisive review on the head requests changes.
const WITHDRAWN: &str = "an owner's review requests changes on this head; that owner approves again with a GitHub review, or another owner approves with a GitHub review";

/// The same when that review was dismissed.
const WITHDRAWN_DISMISSED: &str = "an owner's request for changes on this head was dismissed; it needs a fresh plain owner comment (`bosun approve <pr>@<head>`, with the owner token), a signature bound to the dismissed review (`bosun approve <pr>@<head> --sign-key <pem>`), or a GitHub review from that owner, or a GitHub review from another owner";

/// Why an owner's approval of the head is withdrawn, if it is, and whether a fresh plain owner
/// comment ends it (only after a dismissal). A request for changes wins.
/// The logins whose withdrawals count: the listed owners plus, while a team is unreadable, the
/// logins that may be its members.
fn withdrawal_logins(
    view: &crate::core::risk::OwnerView,
    reviews: &[crate::github::Review],
) -> Vec<String> {
    let mut owners = view.logins();
    owners.extend(unreadable_withdrawers(view, reviews));
    owners
}

fn withdrawn(gh: &dyn Github, p: &Pull) -> Result<Option<(&'static str, bool)>> {
    let view = owner_view(gh, &p.base_sha)?;
    let reviews = gh.list_reviews(p.number)?;
    // While a team is unreadable, a withdrawal by a login that may be a member blocks too; such
    // a login's approval never counts (only withdrawals are read from this list).
    let owners = withdrawal_logins(&view, &reviews);
    let states: Vec<_> = owners
        .iter()
        .map(|o| decisive_review(&reviews, o, &p.head_sha))
        .collect();
    let has = |s: &str| states.iter().any(|x| x.as_deref() == Some(s));
    Ok(if has("CHANGES_REQUESTED") {
        Some((WITHDRAWN, false))
    } else if has("DISMISSED") {
        Some((WITHDRAWN_DISMISSED, true))
    } else {
        None
    })
}

/// What to do instead in a Claude cloud session.
const CLOUD_FIX: &str = "the owner approves with their own `/bosun approve <sha>` comment on the PR (GitHub web or mobile app) or a GitHub review, or runs `bosun approve` on their own machine; an orchestrator never records a chat answer as an approval (D-025)";

/// Shown with every list: an approval comment counts only when posted with a plain user token, as
/// this command does (an owner review counts too). The Claude GitHub tool posts through an app and
/// adds a footer, so its comments never count.
const LIST_HINT: &str = "the owner approves with their own `/bosun approve <sha>` comment on the PR or a GitHub review, or runs data.approve_command (bosun approve) on their own machine; chat is never an approval (D-025), and an unsigned /bosun approve comment posted through Claude's GitHub tool never counts";

#[derive(Default)]
pub(crate) struct Board {
    pub(crate) ready: Vec<Ready>,
    waiting: Vec<(u64, &'static str)>,
    /// Waiting PRs whose dismissed withdrawal a fresh `bosun approve` comment ends: (pr, head, base).
    dismissed: Vec<(u64, String, String)>,
    /// What a tokenless listing could not read: (pr, or `None` for the PR list; why).
    pub(crate) skipped: Vec<(Option<u64>, String)>,
}

/// Why a tokenless listing may miss PRs, and the fix.
const SKIPPED_FIX: &str = "these were read without a token (a private repo, or GitHub's 60 requests an hour, refuse them); set BOSUN_OWNER_TOKEN for a full list";

/// Put `p` in `waiting` when an owner's approval of it is withdrawn.
fn hold(gh: &dyn Github, p: &Pull, board: &mut Board) -> Result<bool> {
    let Some((why, by_comment)) = withdrawn(gh, p)? else {
        return Ok(false);
    };
    board.waiting.push((p.number, why));
    if by_comment {
        let (head, base) = (p.head_sha.clone(), p.base_sha.clone());
        board.dismissed.push((p.number, head, base));
    }
    Ok(true)
}

/// The open PRs whose risk-check waits for an owner, split into ready and waiting, by number.
/// Statuses, check runs and files all describe the head sha read with the PR. With `lenient`
/// (a tokenless listing) a failed read skips that PR, or the whole list, into `skipped`.
pub(crate) fn board(gh: &dyn Github, ctx: &Ctx, lenient: bool) -> Result<Board> {
    let mut pulls = match gh.list_pulls(None, IssueState::Open) {
        Ok(p) => p,
        Err(e) if lenient => {
            let why = format!("cannot list the open PRs: {}", e.message);
            let skipped = vec![(None, why)];
            return Ok(Board {
                skipped,
                ..Board::default()
            });
        }
        Err(e) => return Err(e),
    };
    pulls.sort_by_key(|p| p.number);
    if !lenient {
        return board_of(gh, ctx, pulls);
    }
    let mut all = Board::default();
    for p in pulls {
        let n = p.number;
        match board_of(gh, ctx, vec![p]) {
            Ok(b) => {
                all.ready.extend(b.ready);
                all.waiting.extend(b.waiting);
                all.dismissed.extend(b.dismissed);
            }
            Err(e) => all.skipped.push((Some(n), e.message)),
        }
    }
    Ok(all)
}

/// The verdict line of `p`'s newest ready summary of the head by a writer or the gate App. Only a
/// hint that the author finished `--ready`: the readiness is recomputed from trusted data.
fn summary_hint(gh: &dyn Github, p: &Pull) -> Result<Option<String>> {
    for c in gh.list_comments(p.number)?.iter().rev() {
        if !parse_ready_summary(&c.body, &p.head_sha) {
            continue;
        }
        let bot = crate::github::gate::record_author(&c.user).eq_ignore_ascii_case(TRUSTED_CREATOR);
        if bot || matches!(gh.permission(&c.user)?.as_deref(), Some("admin" | "write")) {
            return Ok(Some(summary_readiness(Some(&c.body)).1));
        }
    }
    Ok(None)
}

/// No-CI readiness of `p`, judged now with the base's rules (never the summary's words): high
/// risk with no approval of this head waits for the owner.
fn no_ci_readiness(gh: &dyn Github, ctx: &Ctx, p: &Pull) -> Result<Readiness> {
    Ok(match crate::cli::risk_check::evaluate_with(gh, ctx, p) {
        Ok(_) => Readiness::NotWaiting,
        Err(e)
            if e.exit == crate::output::Exit::NeedsHuman
                && e.data.as_ref().is_some_and(|d| d["level"] == "high") =>
        {
            let d = e.data.unwrap_or_default();
            let ids = d["matches"].as_array().into_iter().flatten();
            Readiness::Ready(
                ids.filter_map(|m| m["rule"].as_str().map(String::from))
                    .collect(),
            )
        }
        Err(e) => return Err(e),
    })
}

/// The board of `pulls`, in their order. A PR whose base has no CI is read from its ready summary.
fn board_of(gh: &dyn Github, ctx: &Ctx, pulls: Vec<Pull>) -> Result<Board> {
    let mut board = Board::default();
    for p in pulls {
        if provider_default(gh)? == CiProvider::None {
            let Some(verdict) = summary_hint(gh, &p)? else {
                continue;
            };
            if let Readiness::Ready(risk) = no_ci_readiness(gh, ctx, &p)?
                && !hold(gh, &p, &mut board)?
            {
                board.ready.push(ready_of(gh, p, risk, verdict)?);
            }
            continue;
        }
        let statuses: Vec<Stat> = crate::github::gate::gate_history(gh, &p.head_sha)?
            .into_iter()
            .map(|s| Stat {
                context: s.context,
                state: s.state,
                description: s.description,
                creator: s.creator,
            })
            .collect();
        let runs: Vec<Run> = gh
            .check_runs(&p.head_sha)?
            .into_iter()
            .map(|r| Run {
                id: r.id,
                name: r.name,
                conclusion: r.conclusion,
                at: r.completed_at.or(r.started_at).unwrap_or_default(),
                app: r.app,
            })
            .collect();
        match readiness(&statuses, &runs) {
            Readiness::NotWaiting => {}
            Readiness::Waiting(why) => board.waiting.push((p.number, why)),
            Readiness::Ready(_) if hold(gh, &p, &mut board)? => {}
            Readiness::Ready(risk) => board.ready.push(Ready {
                verdict: (statuses.iter())
                    .find(|s| s.context == "review-verdict" && s.creator == TRUSTED_CREATOR)
                    .map(|s| s.description.clone())
                    .unwrap_or_default(),
                pr: p.number,
                title: p.title.clone(),
                files: gh
                    .compare_files(&p.base_sha, &p.head_sha)?
                    .into_iter()
                    .map(|f| f.filename)
                    .collect(),
                head_sha: p.head_sha,
                base_sha: p.base_sha,
                risk,
            }),
        }
    }
    Ok(board)
}

fn ready_of(gh: &dyn Github, p: Pull, risk: Vec<String>, verdict: String) -> Result<Ready> {
    let files = gh.compare_files(&p.base_sha, &p.head_sha)?;
    Ok(Ready {
        verdict,
        pr: p.number,
        title: p.title.clone(),
        files: files.into_iter().map(|f| f.filename).collect(),
        head_sha: p.head_sha,
        base_sha: p.base_sha,
        risk,
    })
}

pub fn run(dir: Option<&Path>, args: &ApproveArgs) -> CmdResult {
    if !args.list && args.main_open.is_none() && args.prs.is_empty() {
        return Err(BosunError::usage(
            "approve needs PR@HEAD arguments, --list or --main-open PR",
            "run `bosun approve --list`, then the command in its data.approve_command",
        ));
    }
    if args.via_signer {
        return via_signer(dir, args);
    }
    if let Some(key) = &args.sign_key {
        return sign(dir, args, key);
    }
    let mut targets = Vec::new();
    for a in &args.prs {
        targets.push(parse_target(a).ok_or_else(|| {
            BosunError::usage(
                format!("{a:?} is not <pr>@<40-hex head sha>"),
                "run `bosun approve --list` and run the command in its data.approve_command",
            )
        })?);
    }
    let cloud = in_cloud(
        std::env::var("CCR_AGENT_PROXY_ENABLED").ok().as_deref(),
        std::env::var("CLAUDE_CODE_REMOTE_ENVIRONMENT_TYPE")
            .ok()
            .as_deref(),
    );
    if cloud && !args.list {
        return Err(BosunError::needs_human(
            "a Claude cloud session's agent proxy rewrites GitHub writes (app attribution and a footer), so an approval from here never counts",
            CLOUD_FIX,
        ));
    }
    if args.list && !std::env::var(TOKEN_VAR).is_ok_and(|t| !t.trim().is_empty()) {
        // Read-only and tokenless: no secret is sent, so any BOSUN_GITHUB_API will do.
        let ctx = Ctx::load(dir)?;
        let gh = LiveGithub::anonymous(&ctx.repo.owner, &ctx.repo.name);
        return exec(&gh, args, targets, &ctx, cloud, true);
    }
    let token = std::env::var(TOKEN_VAR)
        .ok()
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| {
            BosunError::needs_human(
                format!("the owner's token is missing: {TOKEN_VAR} is not set"),
                format!(
                    "run bosun approve on your own machine (a local shell or a local Claude Code session) with {TOKEN_VAR} set; see docs/PROGRESS.md, Blocked 9. It never uses GITHUB_TOKEN"
                ),
            )
        })?;
    let token = token.trim().to_string();
    let api = crate::github::http::api_base();
    if !api_url_ok(&api) {
        return Err(BosunError::error(
            "BOSUN_GITHUB_API is not https://, so the owner token is not sent",
            "unset BOSUN_GITHUB_API (or set it to an https:// URL) in the owner's environment",
        ));
    }
    let ctx = Ctx::load(dir)?;
    let gh = LiveGithub::with_token(&token, &ctx.repo.owner, &ctx.repo.name);
    redact(&token, exec(&gh, args, targets, &ctx, cloud, false))
}

fn exec(
    gh: &dyn Github,
    args: &ApproveArgs,
    mut targets: Vec<(u64, String)>,
    ctx: &Ctx,
    cloud: bool,
    tokenless: bool,
) -> CmdResult {
    let now = &ctx.now.to_string();
    if args.list {
        let mut b = board(gh, ctx, tokenless)?;
        // Held by the owner at this head (PostToolUse answers): not asked again until it moves.
        let (held, ready): (Vec<Ready>, Vec<Ready>) = std::mem::take(&mut b.ready)
            .into_iter()
            .partition(|r| answers::held(&ctx.dir, &ctx.repo.to_string(), r.pr, &r.head_sha));
        b.ready = ready;
        let heads: Vec<(u64, String)> =
            b.ready.iter().map(|r| (r.pr, r.head_sha.clone())).collect();
        let items: Vec<(u64, String, String)> = b
            .ready
            .iter()
            .map(|r| (r.pr, r.head_sha.clone(), r.title.clone()))
            .collect();
        let (message_id, message, message_pairs) = match batch_message(&items) {
            Some(m) => (json!(m.id), json!(m.text), json!(m.pairs)),
            None => (Value::Null, Value::Null, Value::Null),
        };
        let mut data = json!({
            "message_id": message_id,
            "message": message,
            "message_pairs": message_pairs,
            "ready": b.ready.iter().map(ready_json).collect::<Vec<_>>(),
            "held": held.iter().map(|r| json!({"pr": r.pr, "head_sha": r.head_sha}))
                .collect::<Vec<_>>(),
            "waiting": b.waiting.iter().map(|(pr, why)| json!({"pr": pr, "why": why}))
                .collect::<Vec<_>>(),
            "approve_command": approve_command(&heads),
            "cloud": cloud,
            "auth": if tokenless { "none" } else { "owner-token" },
            "skipped": b.skipped.iter().map(|(pr, why)| json!({"pr": pr, "why": why}))
                .collect::<Vec<_>>(),
            "hint": if cloud {
                format!("{LIST_HINT}; in a Claude cloud session an approval posted here never counts: {CLOUD_FIX}")
            } else {
                LIST_HINT.to_string()
            },
        });
        if !b.skipped.is_empty() {
            data["skipped_fix"] = json!(SKIPPED_FIX);
        }
        // Informational: a read error here is the board's to report, not this field's.
        if let Ok(view) = default_ref(gh).and_then(|r| owner_view(gh, &r))
            && !view.teams.is_empty()
        {
            data["owner_teams_unreadable"] = json!(view.unreadable);
            if !view.unreadable.is_empty() {
                data["owner_teams_fix"] = json!(risk::OWNER_TEAM_FIX);
            }
        }
        return Ok(ask_next(&b.ready, data));
    }
    kill_switch(gh)?;
    if let Some(n) = args.main_open {
        let head = post_main_open(gh, n)?;
        write_record(gh, &[], std::slice::from_ref(&head), now)?;
        return Ok(Outcome::data(
            json!({"main_open": {"pr": n, "head_sha": head.head_sha}}),
        ));
    }

    let b = board(gh, ctx, false)?;
    targets.sort_unstable();
    targets.dedup();
    let mut not_ready = Vec::new();
    for (n, sha) in &targets {
        let why = match b.ready.iter().find(|r| r.pr == *n) {
            Some(r) if r.head_sha == *sha => continue,
            Some(r) => format!("head changed (now {})", r.head_sha),
            None => match b.dismissed.iter().find(|d| d.0 == *n) {
                Some(d) if d.1 == *sha => continue,
                Some(d) => format!("head changed (now {})", d.1),
                None => why_not(gh, &b, *n)?,
            },
        };
        not_ready.push(json!({"pr": n, "why": why}));
    }
    if !not_ready.is_empty() {
        return Err(BosunError::conflict(
            "some of the PRs are not ready at the head you named; nothing was posted",
            "run `bosun approve --list` again and run the command in its data.approve_command",
        )
        .with_data(json!({"not_ready": not_ready})));
    }
    let mut approved = Vec::new();
    let mut heads = Vec::new();
    let mut failed = None;
    for (pr, sha) in targets {
        let base = b
            .ready
            .iter()
            .find(|r| r.pr == pr)
            .map(|r| &r.base_sha)
            .or_else(|| b.dismissed.iter().find(|d| d.0 == pr).map(|d| &d.2))
            .expect("checked above");
        match approve_one(gh, pr, &sha, base) {
            Ok((id, repost)) => {
                let mut entry = json!({"pr": pr, "head_sha": sha, "comment_id": id});
                if repost {
                    entry["repost"] = json!(true);
                }
                approved.push(entry);
                heads.push(PrHead { pr, head_sha: sha });
            }
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    // The comments that were posted are recorded even when a later one failed.
    let recorded = if heads.is_empty() {
        Ok(())
    } else {
        write_record(gh, &heads, &[], now)
    };
    if let Some(e) = failed {
        return Err(e.with_data(json!({"approved": approved})));
    }
    recorded.map_err(|e| e.with_data(json!({"approved": approved})))?;
    Ok(Outcome::data(json!({"approved": approved})))
}

/// Post `/bosun approve <sha>`, also when one that counts already exists: the new comment
/// re-runs risk-check if its run was lost. Returns the new comment's id and whether one existed.
fn approve_one(gh: &dyn Github, pr: u64, sha: &str, base: &str) -> Result<(u64, bool)> {
    let comments = gh.list_comments(pr)?;
    let existed = owner_view(gh, base)?
        .logins()
        .iter()
        .any(|o| owner_comment(&comments, o, sha).is_some());
    Ok((
        gh.add_comment(pr, &format!("/bosun approve {sha}"))?.id,
        existed,
    ))
}

/// Chat approvals are retired (D-025): no form, no `next`. Each ready item names the comment the
/// owner posts, and `data.owner_step` says so. Nothing ready, no `owner_step`.
fn ask_next(ready: &[Ready], mut data: Value) -> Outcome {
    let items: Vec<ask::Item> = (ready.iter())
        .map(|r| ask::Item {
            pr: r.pr,
            head: &r.head_sha,
        })
        .collect();
    for (i, item) in items.iter().enumerate() {
        data["ready"][i]["owner_comment"] = json!(ask::owner_comment(item.head));
    }
    if let Some(t) = ask::owner_step(&items) {
        data["owner_step"] = json!(t);
    }
    Outcome::data(data)
}

fn ready_json(r: &Ready) -> Value {
    json!({"pr": r.pr, "title": r.title, "head_sha": r.head_sha,
        "risk_from_status": r.risk, "verdict": r.verdict, "files": r.files,
        "files_truncated": r.files.len() >= COMPARE_FILES_CAP})
}

/// Why `n` is not in the ready list.
fn why_not(gh: &dyn Github, b: &Board, n: u64) -> Result<String> {
    if let Some((_, why)) = b.waiting.iter().find(|(pr, _)| *pr == n) {
        return Ok((*why).to_string());
    }
    Ok(match gh.get_pull(n)? {
        None => "not found".into(),
        Some(p) if p.state != "open" => "closed".into(),
        Some(_) => "not waiting for an owner".into(),
    })
}

/// `bosun gate main-open --pr N`, posted with the owner token: the same `judge`. Only for a PR
/// that changes the main-open workflow (the base workflow cannot judge it). Exit 4 when main is
/// red and the PR is no mainfix; exit 3 when the PR is closed, missing or changes no such file.
fn post_main_open(gh: &dyn Github, n: u64) -> Result<PrHead> {
    if crate::github::gate::checks_mode() {
        // The owner's token cannot write the gate App's check run (spec §15, O4).
        return Err(BosunError::conflict(
            "`bosun approve --main-open` cannot post main-open in [checks] mode \"checks\"",
            "re-run the main-open workflow (it posts main-open as the gate App)",
        ));
    }
    let pull = gh
        .get_pull(n)?
        .filter(|p| p.state == "open")
        .ok_or_else(|| {
            BosunError::conflict(
                format!("PR #{n} is not an open PR"),
                "name an open PR; `bosun approve --list` lists the PRs that wait for an owner",
            )
        })?;
    if !gh
        .pull_files(n)?
        .iter()
        .any(|f| f.filename == MAIN_OPEN_WORKFLOW)
    {
        return Err(BosunError::conflict(
            format!("PR #{n} does not change {MAIN_OPEN_WORKFLOW}"),
            "the main-open workflow posts this status for other PRs; re-run it, or wait for it",
        ));
    }
    let red = main_red(gh)?;
    let mainfix = red.is_some() && is_mainfix(gh, &pull)?;
    let v = judge(red, mainfix);
    let state = if v.pass { "success" } else { "failure" };
    let description = format!("{} (posted by bosun approve --main-open)", v.description);
    gh.create_status(&pull.head_sha, "main-open", state, &description, None)?;
    if v.pass {
        return Ok(PrHead {
            pr: n,
            head_sha: pull.head_sha,
        });
    }
    let red = red.unwrap_or_default();
    Err(BosunError::halted(
        v.description,
        format!("fix main with a bosun:kind/mainfix PR, or wait for #{red} to close"),
    )
    .with_data(json!({"pr": n, "head_sha": pull.head_sha, "main_red": red, "pass": false})))
}

/// One `approve` record on the registry issue: an audit hint, never authority.
fn write_record(gh: &dyn Github, prs: &[PrHead], main_open: &[PrHead], at: &str) -> Result<()> {
    let (issue, _) = registry::ensure(gh)?;
    let record = Record::Approve {
        prs: prs.to_vec(),
        main_open: main_open.to_vec(),
        at: at.into(),
    };
    gh.add_comment(issue, &record.to_line())?;
    Ok(())
}

/// Belt and braces: a server that echoes the token in a message or a title never gets it to
/// stdout or stderr.
fn redact(token: &str, result: CmdResult) -> CmdResult {
    let scrub = |s: &str| s.replace(token, "[redacted]");
    let scrub_json = |v: Value| serde_json::from_str(&scrub(&v.to_string())).unwrap_or(Value::Null);
    match result {
        Ok(mut o) => {
            // Only list mode sets `next` (the owner question); it is scrubbed like `data`.
            o.data = scrub_json(o.data);
            for n in &mut o.next {
                n.args = scrub_json(n.args.take());
            }
            Ok(o)
        }
        Err(mut e) => {
            e.message = scrub(&e.message);
            e.fix = scrub(&e.fix);
            e.data = e.data.map(scrub_json);
            Err(e)
        }
    }
}

/// The workflow the CI signer runs from (D-017, keyless fast lane).
const SIGNER_WORKFLOW: &str = "signer.yml";

/// A session id the signer workflow accepts as `requester`: `^[A-Za-z0-9._/:-]{1,200}$`.
fn requester_ok(s: &str) -> bool {
    (1..=200).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/:-".contains(&b))
}

/// `bosun approve <pr>@<head> --via-signer`: asks the CI signer for an approval. Posts nothing;
/// the session dispatches the workflow with the call in `next` (D29).
fn via_signer(dir: Option<&Path>, args: &ApproveArgs) -> CmdResult {
    let (pr, pinned) = match args.prs.as_slice() {
        [target] => parse_target(target),
        _ => None,
    }
    .ok_or_else(|| {
        BosunError::usage(
            "--via-signer takes one <pr>@<40-hex head sha>: the head to approve",
            "run `bosun approve --list`, then `bosun approve <pr>@<head> --via-signer`",
        )
    })?;
    let app = App::load(dir)?;
    let gh = app.gh.as_ref();
    kill_switch(gh)?;
    let pull = gh
        .get_pull(pr)?
        .filter(|p| p.state == "open")
        .ok_or_else(|| {
            BosunError::conflict(format!("PR #{pr} is not an open PR"), "name an open PR")
        })?;
    if pinned != pull.head_sha {
        return Err(BosunError::conflict(
            format!(
                "PR #{pr} head changed (now {}); nothing was requested",
                pull.head_sha
            ),
            "check the new head, then ask again with `bosun approve <pr>@<head> --via-signer`",
        ));
    }
    // The reviewer-independence check in CI excludes the session that asked.
    let requester = app.ctx.session.trim().to_string();
    if !requester_ok(&requester) {
        return Err(BosunError::usage(
            format!("BOSUN_SESSION {requester:?} is empty or not a plain session id"),
            "set BOSUN_SESSION to this session's id (A-Z a-z 0-9 . _ / : -, at most 200 characters)",
        ));
    }
    let branch = crate::app::default_branch(gh)?;
    let (owner, repo) = (&app.ctx.repo.owner, &app.ctx.repo.name);
    let args = json!({"method": "run_workflow", "owner": owner, "repo": repo,
        "workflow_id": SIGNER_WORKFLOW, "ref": branch,
        "inputs": {"pr": pr.to_string(), "head": pinned, "requester": requester}});
    Ok(Outcome::data(json!({"pr": pr, "head_sha": pull.head_sha, "requested": true,
        "workflow": SIGNER_WORKFLOW, "ref": branch,
        "message": "nothing was posted: make the tool call in `next`, then run `bosun risk-check` on the PR once the workflow is done"}))
    .with_next("mcp__github__actions_run_trigger", args))
}

/// What the head's `ci-ok` is for the CI signer: the newest check run of the gate App
/// (`[checks] mode = "checks"` is required), as
/// `ci_green` reads it (a commit status never counts). Returns (green, what was seen).
fn ci_ok_state(gh: &dyn Github, sha: &str) -> Result<(bool, String)> {
    let runs: Vec<Run> = gh
        .check_runs(sha)?
        .into_iter()
        .map(|r| Run {
            id: r.id,
            name: r.name,
            conclusion: r.conclusion,
            at: r.completed_at.or(r.started_at).unwrap_or_default(),
            // Only the gate App's run counts (`checks` mode is required for the signer): read as
            // the trusted creator for `ci_green`; every other creator, GitHub Actions too, is
            // nobody. A push workflow on any branch can post a github-actions `ci-ok`.
            app: match crate::github::gate::trusted_app_id() {
                Some(id) if r.app_id == id => crate::core::approve::TRUSTED_APP.into(),
                _ => String::new(),
            },
        })
        .collect();
    let seen = (runs.iter())
        .filter(|r| r.name == "ci-ok")
        .max_by(|a, b| (&a.at, a.id).cmp(&(&b.at, b.id)))
        .map_or("missing".to_string(), |r| {
            format!(
                "{} by {}",
                r.conclusion.as_deref().unwrap_or("not completed"),
                r.app
            )
        });
    Ok((ci_green(&runs), seen))
}

/// `bosun approve <pr>@<head> --sign-key <pem>`: the signer (the orchestrator session
/// holding an enrolled key, or the owner's machine) signs `signed_message` for the PR's current head and posts the comment with
/// the session's own token. It works in a Claude cloud session: the signature, not the author,
/// is what risk-check checks. The key is never printed or logged.
fn sign(dir: Option<&Path>, args: &ApproveArgs, key_path: &Path) -> CmdResult {
    // The owner approves the head they saw: a bare PR number would sign whatever was pushed
    // in between.
    let (pr, pinned) = match args.prs.as_slice() {
        [target] => parse_target(target),
        _ => None,
    }
    .ok_or_else(|| {
        BosunError::usage(
            "--sign-key takes one <pr>@<40-hex head sha>: the head the owner approved",
            "run `bosun approve --list` for the ready heads; the owner approves with a `/bosun approve <head_sha>` comment on the PR (D-025), and only an enrolled-key signer runs `bosun approve <pr>@<head> --sign-key <pem>` on the exact head",
        )
    })?;
    let api = crate::github::http::api_base();
    if !api_url_ok(&api) {
        return Err(BosunError::error(
            "BOSUN_GITHUB_API is not https://, so the head it reports is not trusted for signing",
            "unset BOSUN_GITHUB_API (or set it to an https:// URL)",
        ));
    }
    let pair = read_key(key_path)?;
    let app = App::load(dir)?;
    let gh = app.gh.as_ref();
    // Also for --print: a printed body could be posted by hand while bosun is halted.
    kill_switch(gh)?;
    let pull = gh
        .get_pull(pr)?
        .filter(|p| p.state == "open")
        .ok_or_else(|| {
            BosunError::conflict(format!("PR #{pr} is not an open PR"), "name an open PR")
        })?;
    let head = pull.head_sha.clone();
    if pinned != head {
        return Err(BosunError::conflict(
            format!("PR #{pr} head changed (now {head}); nothing was signed"),
            "show the owner the new head and sign it only on their word",
        ));
    }
    let role = match args.role.as_deref() {
        Some("delegate") => Role::Delegate,
        Some("signer") => Role::Signer,
        _ => Role::Owner,
    };
    match (&args.requester, role == Role::Signer) {
        (Some(r), true) if requester_ok(r) => {}
        (None, false) => {}
        (_, true) => {
            return Err(BosunError::usage(
                "--as signer needs --requester <session id> (A-Z a-z 0-9 . _ / : -, at most 200 characters)",
                "pass the session that asked: the signer workflow does it from its `requester` input",
            ));
        }
        (Some(_), false) => {
            return Err(BosunError::usage(
                "--requester goes with --as signer only",
                "drop --requester",
            ));
        }
    }
    if role == Role::Signer && !args.bootstrap {
        return Err(BosunError::usage(
            "--as signer signs only under the quorum (D-022) or autonomous (D-017) policy: it needs --bootstrap",
            "add --bootstrap (`[approvals] policy` at the PR's base must be \"quorum\" or \"autonomous\")",
        ));
    }
    if args.bootstrap && role == Role::Delegate {
        return Err(BosunError::usage(
            "--bootstrap signs with an owner key, not --as delegate",
            "drop --as delegate, or drop --bootstrap",
        ));
    }
    // D-022: a signature never counts on the quorum trust root (risk-check ignores it there), so
    // none is posted, whatever its form: the owner's own review or comment is the approval.
    let roots = crate::cli::risk_check::code_roots_at(gh, &pull.base_sha)?;
    let files = gh.pull_files(pr)?;
    let (root, cause) =
        crate::cli::risk_check::quorum_trust_root(gh, &files, &roots, (&pull.base_sha, &head))?;
    // D-027: owner delegation covers the trust root only for the quorum signature
    // (`--bootstrap`), which also checks the default branch still delegates (`lists_stricter`).
    if !args.bootstrap && crate::cli::risk_check::delegation_at(gh, &pull.base_sha)? {
        let paths: Vec<_> = (files.iter())
            .map(|f| (&*f.filename, f.previous_filename.as_deref()))
            .collect();
        let mut hit = risk::quorum_trust_paths(&paths, &roots);
        hit.extend(crate::core::approve::trust_files_changed_in(
            paths.iter().copied(),
            &roots,
        ));
        if !hit.is_empty() {
            hit.sort();
            hit.dedup();
            return Err(BosunError::error(
                format!(
                    "owner delegation (D-027) covers the trust root ({}) only with the quorum signature: nothing was signed",
                    hit.join(", ")
                ),
                "sign with --bootstrap (the autonomous form, after two clean verdicts on the head)",
            ));
        }
    }
    if !root.is_empty() {
        return Err(BosunError::error(
            format!(
                "quorum (D-022) never counts a signature on the trust root ({}{}): nothing was signed",
                root.join(", "),
                cause.map(|c| format!("; {c}")).unwrap_or_default()
            ),
            "the owner approves this PR with their own GitHub review or `/bosun approve <sha>` comment",
        ));
    }
    // D-022: a diff bosun cannot see may hide the trust root, as risk-check's R-unseen and the
    // bootstrap signing path (`autonomous_ok`) judge it; plain signing under quorum needs the same.
    if quorum_policy_at(gh, &pull.base_sha)? {
        let views: Vec<risk::FileView> = (files.iter())
            .map(|f| risk::FileView {
                path: f.filename.clone(),
                status: f.status.clone(),
                has_patch: f.patch.is_some(),
                changes: f.additions + f.deletions,
                same_blob: false,
            })
            .collect();
        if risk::unseen_diff(&views, pull.changed_files).is_some() {
            return Err(BosunError::error(
                "quorum (D-022) never counts a signature on a diff bosun cannot see (R-unseen): nothing was signed",
                "the owner approves this PR with their own GitHub review or `/bosun approve <sha>` comment",
            ));
        }
    }

    if role == Role::Signer {
        // Fail closed: the default branch (now) and the PR's base must both allow the signer.
        signer_allowed(gh, &crate::app::default_ref(gh)?, "the default branch")?;
        signer_allowed(gh, &pull.base_sha, "the PR's base")?;
    }
    let boot = match args.bootstrap {
        true => Some(bootstrap_mode(
            &app,
            &app.ctx.now.to_string()[..10],
            &pull,
            args.requester.as_deref(),
        )?),
        false => None,
    };
    if role == Role::Signer {
        let (green, ci_ok) = ci_ok_state(gh, &head)?;
        if !green {
            return Err(BosunError::error(
                format!(
                    "the head's ci-ok is not green ({ci_ok}; only the gate App's newest check run counts, never a status or GitHub Actions'); nothing was signed"
                ),
                "wait for ci-ok to pass on this head, then run the signer again",
            )
            .with_data(json!({"ci_ok": ci_ok, "head_sha": head})));
        }
    }
    // A request for changes, and any withdrawal for a delegate, cannot be signed away. After a
    // dismissal an owner key signs v2, bound to each dismissed owner review (spec section 8).
    let mut reviews = vec![None];
    match withdrawn(gh, &pull)? {
        Some((_, true)) if role == Role::Owner && boot.is_none() => {
            let all = gh.list_reviews(pr)?;
            let view = owner_view(gh, &pull.base_sha)?;
            reviews = withdrawal_logins(&view, &all)
                .iter()
                .filter_map(|o| dismissed_review(&all, o, &head))
                .map(Some)
                .collect();
            if reviews.is_empty() || reviews.contains(&Some(0)) {
                return Err(BosunError::conflict(
                    format!("PR #{pr}: the dismissed review cannot be named, nothing was signed"),
                    "the owner posts a plain `/bosun approve <head>` comment or submits a GitHub approving review",
                ));
            }
        }
        Some((why, _)) => {
            return Err(BosunError::conflict(
                format!("PR #{pr}: a signature would not count, nothing was signed: {why}"),
                "the owner submits a GitHub approving review (or, after a dismissal, posts a plain `/bosun approve <head>` comment); only an owner key signs after a dismissal, and a request for changes ends only with a review",
            ));
        }
        None => {}
    }
    let public = b64::encode(ring::signature::KeyPair::public_key(&pair).as_ref());
    let keys = enrolled_keys(gh).map_err(|why| {
        BosunError::error(
            format!("signed approvals are off: {why}"),
            format!(
                "fix {APPROVERS_PATH} on the default branch with a PR the owner approves on GitHub"
            ),
        )
    })?;
    let key = keys
        .into_iter()
        .find(|k| b64::decode(&k.ed25519).map(|b| b64::encode(&b)) == Some(public.clone()))
        .ok_or_else(|| {
            BosunError::error(
                format!("the key is not enrolled in {APPROVERS_PATH} on the default branch"),
                format!(
                    "enroll the public key {public} in {APPROVERS_PATH} with a PR the owner approves on GitHub"
                ),
            )
        })?;
    if key.role != role {
        return Err(BosunError::error(
            format!(
                "key {} is enrolled as {}, not {}; nothing was signed",
                key.id,
                key.role.name(),
                role.name()
            ),
            format!(
                "pass `--as {}`, or enroll the key with that role",
                key.role.name()
            ),
        ));
    }
    let key_id = key.id;
    let (owner, repo) = (&app.ctx.repo.owner, &app.ctx.repo.name);
    let policy = boot.as_ref().map(|b| b.2.name());
    let policy_source = boot.as_ref().map(|b| b.3);
    let mut signed = Vec::new();
    let note = boot.as_ref().map(|b| b.1.as_str());
    let until = boot.as_ref().and_then(|b| b.0.clone());
    for after in reviews {
        let (form, msg) = match (after, &boot) {
            (Some(r), _) => (Form::V2(r), signed_message_v2(owner, repo, pr, &head, r)),
            (None, Some((_, _, Policy::Autonomous | Policy::Quorum, _))) => (
                Form::Autonomous,
                signed_message_autonomous(owner, repo, pr, &head),
            ),
            (None, Some(_)) => (
                Form::Bootstrap,
                signed_message_bootstrap(owner, repo, pr, &head),
            ),
            (None, None) => (Form::V1, signed_message(role, owner, repo, pr, &head)),
        };
        let sig = b64::encode(pair.sign(msg.as_bytes()).as_ref());
        let body = signed_comment(&head, form, note, &key_id, &sig);
        signed.push((after, body));
    }
    if args.print {
        let all: Vec<Value> = (signed.iter())
            .map(|(after, body)| json!({"after_review": after, "body": body}))
            .collect();
        return Ok(Outcome::data(
            json!({"pr": pr, "head_sha": head, "key_id": key_id, "role": role.name(),
                "bootstrap_until": until, "policy": policy, "policy_source": policy_source, "body": signed[0].1, "bodies": all}),
        ));
    }
    let mut approved = Vec::new();
    for (after, body) in signed {
        // The comments already posted stay listed when a later one fails.
        let id = match gh.add_comment(pr, &body) {
            Ok(c) => c.id,
            Err(e) => return Err(e.with_data(json!({"approved": approved}))),
        };
        let mut entry = json!({"pr": pr, "head_sha": head, "key_id": key_id,
            "role": role.name(), "comment_id": id, "policy": policy,
            "policy_source": policy_source});
        if let Some(r) = after {
            entry["after_review"] = json!(r);
        }
        approved.push(entry);
    }
    Ok(Outcome::data(json!({"approved": approved})))
}

/// Refuses unless `[approvals]` and `[checks]` at `git_ref` let the CI signer sign.
fn signer_allowed(gh: &dyn Github, git_ref: &str, what: &str) -> Result<()> {
    let config = match gh.file_at(CONFIG_FILE, git_ref)? {
        Some(t) => Config::parse(&t, CONFIG_FILE)?,
        None => Config::default(),
    };
    let policy = config.approvals.policy();
    if !matches!(policy, Policy::Autonomous | Policy::Quorum) {
        return Err(BosunError::error(
            format!(
                "--as signer signs only under the quorum (D-022) or autonomous (D-017) policy; the policy at {what} is {}",
                policy.name()
            ),
            "set `[approvals] policy = \"quorum\"` or `\"autonomous\"` with a PR the owner approves (it counts once it is on the default branch and at the PR's base), or ask the owner",
        ));
    }
    if !config.approvals.signer {
        return Err(BosunError::error(
            format!(
                "the CI signer is off: `[approvals] signer` is not true at {what}; nothing was signed"
            ),
            "the owner decides: set `[approvals] signer = true` in .bosun/bosun.toml with a PR (R-weaken, owner-approved); the default is the orchestrator-held key (`--sign-key` as owner)",
        ));
    }
    let gate_app = config.checks.mode == crate::config::CheckMode::Checks
        && config.checks.app_id.is_some()
        && crate::github::gate::trusted_app_id() == config.checks.app_id;
    if !gate_app {
        return Err(BosunError::error(
            format!(
                "the CI signer needs `[checks] mode = \"checks\"` with the gate App at {what} (and in this checkout): in statuses mode any push workflow can post a ci-ok; nothing was signed"
            ),
            "set `[checks] mode = \"checks\"`, `app_id` and `app_slug` (spec section 15), or sign with the orchestrator-held key",
        ));
    }
    Ok(())
}

/// Bootstrap until, the signed line, the policy and who set it (`Approvals::policy_source`).
type Boot = (Option<String>, String, Policy, &'static str);

/// What a policy asks of a signature, one number per dimension, higher = stricter:
/// 1. how much stays with the owner (owner 3, owner-gated 2, orchestrator 1, autonomous and
///    quorum 0: they also sign gated PRs),
/// 2. independent verdicts required on the head (quorum 2; owner 2 too, nobody signs; autonomous
///    and owner-gated 1, an independent review; orchestrator 0),
/// 3. whether R-weaken still applies (quorum ignores it: 0).
///
/// Policies are not totally ordered: quorum is stricter in 2, autonomous in 3.
fn policy_dims(p: Policy) -> [u8; 3] {
    match p {
        Policy::Owner => [3, 2, 1],
        Policy::OwnerGated => [2, 1, 1],
        Policy::Orchestrator => [1, 0, 1],
        Policy::Autonomous => [0, 1, 1],
        Policy::Quorum => [0, 2, 0],
    }
}

/// True when the default branch's policy is stricter than the PR's base in ANY dimension; the
/// base policy then fails closed and may not sign.
fn default_stricter(main: Policy, base: Policy) -> bool {
    let (m, b) = (policy_dims(main), policy_dims(base));
    m.iter().zip(b.iter()).any(|(m, b)| m > b)
}

/// True when the default branch's `review_models`, `autonomous_allow`, `verdict_bots`,
/// `oneoff_allow` or `bosun_repo` is stricter than the PR
/// base's (`bosun_repo`: any difference, the effective repo with its default, since the pin
/// check trusts the repo it names): a set on the default branch that the base left unset or widened (unset `review_models`
/// counts any model; unset `autonomous_allow` is the built-in set). Compared as exact strings, so
/// a base entry the default branch does not list reads as looser (fail closed).
fn lists_stricter(main: &crate::config::Approvals, base: &crate::config::Approvals) -> bool {
    let models = main.review_models.as_ref().is_some_and(|m| {
        (base.review_models.as_ref()).is_none_or(|b| b.iter().any(|x| !m.contains(x)))
    });
    let builtin = || {
        risk::AUTONOMOUS_ALLOWED
            .iter()
            .map(|g| g.to_string())
            .collect()
    };
    let eff = |a: &crate::config::Approvals| -> Vec<String> {
        a.autonomous_allow.clone().unwrap_or_else(builtin)
    };
    let (m, b) = (eff(main), eff(base));
    // Lists where unset allows nothing: a base entry the default branch does not list is looser.
    let wider = |m: &Option<Vec<String>>, b: &Option<Vec<String>>| {
        (b.iter().flatten()).any(|g| !m.iter().flatten().any(|x| x == g))
    };
    let repo = |a: &crate::config::Approvals| {
        (a.bosun_repo.clone()).unwrap_or_else(|| crate::config::DEFAULT_BOSUN_REPO.to_string())
    };
    models
        // Owner delegation (D-027) the base has and the default branch dropped.
        || (base.delegated() && !main.delegated())
        || repo(main) != repo(base)
        || b.iter().any(|g| !m.contains(g))
        || wider(&main.verdict_bots, &base.verdict_bots)
        || wider(&main.oneoff_allow, &base.oneoff_allow)
}

/// How `--bootstrap` may sign for the PR at `head` today: the last day of the window (D-012)
/// and its note line, or (after it) the note line of the approval policy (D-014) and no date. The
/// setting is read at the PR's base, exactly as risk-check and the quorum trust root read it.
fn bootstrap_mode(app: &App, today: &str, pull: &Pull, requester: Option<&str>) -> Result<Boot> {
    let gh = app.gh.as_ref();
    let (pr, head) = (pull.number, pull.head_sha.as_str());
    let text = gh.file_at(CONFIG_FILE, &pull.base_sha)?;
    let approvals = match text {
        Some(t) => Config::parse(&t, CONFIG_FILE)?.approvals,
        None => Default::default(),
    };
    let (policy, src) = (approvals.policy(), approvals.policy_source());
    // Fail closed, for every policy: the default branch (now) must be at least as strict as the
    // PR's base, and must not end the D-012 window the base still has open.
    let main = match gh.file_at(CONFIG_FILE, &crate::app::default_ref(gh)?)? {
        Some(t) => Config::parse(&t, CONFIG_FILE)?.approvals,
        None => Default::default(),
    };
    let signs = |p: Policy| matches!(p, Policy::Autonomous | Policy::Quorum);
    let end_of = |a: &crate::config::Approvals| {
        (a.bootstrap_until.as_ref()).map(|u| u.clone().min(BOOTSTRAP_END.to_string()))
    };
    let kind = |p: Policy| {
        if signs(p) {
            "a signing policy"
        } else {
            p.name()
        }
    };
    let stricter = default_stricter(main.policy(), policy)
        || lists_stricter(&main, &approvals)
        || (signs(policy) && !signs(main.policy()))
        // A window the base has open today must not be shortened or dropped; one that already
        // ended is not "open", so the default branch dropping its stale date is fine.
        || (!signs(policy)
            && end_of(&approvals).is_some_and(|b| today <= b.as_str())
            && end_of(&main) < end_of(&approvals));
    if stricter {
        return Err(BosunError::error(
            format!(
                "the approvals at the PR's base ({}, bootstrap_until {}) are looser than on the default branch ({}, bootstrap_until {}); nothing was signed",
                kind(policy),
                end_of(&approvals).as_deref().unwrap_or("unset"),
                kind(main.policy()),
                end_of(&main).as_deref().unwrap_or("unset"),
            ),
            "the owner restores the settings on the default branch with a PR they approve, then run again",
        ));
    }
    if signs(policy) {
        autonomous_ok(app, pull, requester, policy)?;
        let line = if policy == Policy::Quorum && approvals.delegated() {
            DELEGATION_LINE
        } else if policy == Policy::Quorum {
            QUORUM_LINE
        } else {
            AUTONOMOUS_LINE
        };
        return Ok((None, line.into(), policy, src));
    }
    let end = approvals
        .bootstrap_until
        .map(|u| u.min(BOOTSTRAP_END.to_string()));
    if let Some(d) = end.as_ref().filter(|d| today <= d.as_str()) {
        return Ok((Some(d.clone()), bootstrap_line(d), policy, src));
    }
    let window = match &end {
        Some(d) => format!("bootstrap mode ended on {d} (D-012)"),
        None => format!(
            "bootstrap mode is off: `[approvals] bootstrap_until` is not set in {CONFIG_FILE} at the PR's base (D-012)"
        ),
    };
    let fix = "nothing was signed: the owner approves with their own `/bosun approve <sha>` comment or a GitHub review, or runs `bosun approve` on their own machine (D-012)";
    let refuse = |why: String| Err(BosunError::error(format!("{why}; {window}"), fix));
    let ok = match policy {
        Policy::Orchestrator | Policy::Autonomous | Policy::Quorum => true,
        Policy::Owner => return refuse("approval policy owner: only the owner approves".into()),
        Policy::OwnerGated => {
            let stats: Vec<Stat> = (crate::github::gate::gate_statuses(gh, head)?.into_iter())
                .map(|s| Stat {
                    context: s.context,
                    state: s.state,
                    description: s.description,
                    creator: s.creator,
                })
                .collect();
            let files: Vec<_> = gh.pull_files(pr)?;
            let paths: Vec<_> = (files.iter())
                .map(|f| (&*f.filename, f.previous_filename.as_deref()))
                .collect();
            only_r_ci(&stats) && risk::workflow_files(&paths).is_empty()
        }
    };
    if !ok {
        return refuse("approval policy owner-gated: only R-ci changes outside workflows are signed; the owner approves this PR".into());
    }
    Ok((None, policy_line(policy), policy, src))
}

/// D-017: signs only when no excluded path changed and a reviewer other than the task's leaseholders
/// and this signer has a clean verdict on the head. risk-check cannot see the signer's session, so
/// this is where signer != reviewer is enforced.
fn autonomous_ok(app: &App, pull: &Pull, requester: Option<&str>, policy: Policy) -> Result<()> {
    let gh = app.gh.as_ref();
    let fix = "nothing was signed: get the clean verdicts this names and sign again, or the owner approves with their own `/bosun approve <sha>` comment or a GitHub review";
    let files = gh.pull_files(pull.number)?;
    let paths: Vec<_> = (files.iter())
        .map(|f| (&*f.filename, f.previous_filename.as_deref()))
        .collect();
    let quorum = policy == Policy::Quorum;
    if quorum {
        // D-022: a diff bosun cannot see may hide the trust root (as risk-check's R-unseen).
        let views: Vec<risk::FileView> = (files.iter())
            .map(|f| risk::FileView {
                path: f.filename.clone(),
                status: f.status.clone(),
                has_patch: f.patch.is_some(),
                changes: f.additions + f.deletions,
                same_blob: false,
            })
            .collect();
        if risk::unseen_diff(&views, pull.changed_files).is_some() {
            return Err(BosunError::error(
                "quorum (D-022) never covers a diff bosun cannot see (R-unseen): the owner approves",
                "nothing was signed: the owner approves this PR (their GitHub review or `/bosun approve <sha>` comment)",
            ));
        }
        // D-022: the trust root is the owner's (a halt refused the signature at the top already).
        let roots = crate::cli::risk_check::code_roots_at(gh, &pull.base_sha)?;
        let (root, cause) = crate::cli::risk_check::quorum_trust_root(
            gh,
            &files,
            &roots,
            (&pull.base_sha, &pull.head_sha),
        )?;
        if !root.is_empty() {
            return Err(BosunError::error(
                format!(
                    "quorum (D-022) never covers the trust root ({}{}): the owner approves",
                    root.join(", "),
                    cause.map(|c| format!("; {c}")).unwrap_or_default()
                ),
                "nothing was signed: the owner approves this PR (their GitHub review or `/bosun approve <sha>` comment)",
            ));
        }
    }
    // The base rules, as risk-check reads them: a rule match in the deny set (R-gov) refuses too.
    let rules = match gh.file_at(".bosun/risk.toml", &pull.base_sha)? {
        Some(t) => risk::Rules::parse(&t)
            .map_err(|e| BosunError::error(format!(".bosun/risk.toml at base: {e}"), fix))?,
        None => risk::Rules::defaults(),
    };
    let changed: Vec<_> = (files.iter())
        .filter(|f| {
            !crate::core::touch::plan_tick_file(
                &f.filename,
                &f.status,
                f.previous_filename.as_deref(),
                f.patch.as_deref(),
            )
        })
        .map(|f| risk::ChangedFile {
            path: f.filename.clone(),
            previous_path: f.previous_filename.clone(),
            added_lines: f
                .patch
                .as_deref()
                .map(risk::added_lines)
                .unwrap_or_default(),
        })
        .collect();
    // R-weaken and capabilities, as risk-check computes them: a weakening is the owner's.
    let findings = if quorum {
        Vec::new()
    } else {
        crate::cli::risk_check::weaken_findings(gh, &files, &pull.base_sha, &pull.head_sha)?
    };
    if !findings.is_empty() {
        return Err(BosunError::error(
            format!(
                "autonomous mode (D-017) never covers a PR with R-weaken findings: the owner approves ({})",
                findings.join("; ")
            ),
            fix,
        ));
    }
    let mut capabilities = risk::capability_labels(&pull.labels);
    let mut one_off = false;
    for i in crate::core::meta::linked_issues_in(&pull.body, &app.ctx.repo.to_string()) {
        if let Some(issue) = gh.get_issue(i)?
            && let Some(meta) = crate::app::task_meta(&issue)
        {
            one_off |= crate::cli::risk_check::is_one_off(
                gh,
                &issue,
                &meta,
                &pull.base_sha,
                &app.ctx.config.project,
            )?;
            capabilities.extend(meta.capabilities);
        }
    }
    let mut matches = risk::evaluate(&rules, &changed, &[], &capabilities);
    let lines: u64 = files.iter().map(|f| f.additions + f.deletions).sum();
    matches.extend(risk::oneoff_size_match(one_off, lines));
    let allow = crate::cli::risk_check::autonomous_allow(gh, &pull.base_sha)?;
    let mut hit = risk::autonomous_excluded(&paths, &matches, allow.as_deref());
    let patches: Vec<_> = (files.iter())
        .map(|f| (&*f.filename, f.patch.as_deref()))
        .collect();
    hit.extend(risk::path_attributes(&patches));
    // The quorum ignores every exclusion but the trust root (checked above).
    if !quorum && !hit.is_empty() {
        return Err(BosunError::error(
            format!(
                "autonomous mode (D-017) never covers {}: the owner approves",
                hit.join(", ")
            ),
            fix,
        ));
    }
    // A decoy link (no open task the author leased) leaves no author to exclude: nothing counts.
    let inputs = autonomous_inputs(gh, pull, &app.ctx.repo.to_string())?;
    let models = review_models(gh, &pull.base_sha)?;
    let clean = inputs.map_or(vec![], |(comments, mut excluded)| {
        excluded.push(app.ctx.session.clone());
        // The CI signer's own session is no reviewer's: the session that asked is excluded too.
        excluded.extend(requester.map(String::from));
        if quorum {
            quorum_clean(&comments, &pull.head_sha, &excluded, models.as_deref())
        } else {
            autonomous_clean(&comments, &pull.head_sha, &excluded, models.as_deref())
        }
    });
    let min = if quorum { risk::QUORUM_VERDICTS } else { 1 };
    if clean.len() < min {
        let rule = if quorum {
            "quorum (D-022)"
        } else {
            "autonomous mode (D-017)"
        };
        return Err(BosunError::error(
            format!(
                "{rule} needs {min} clean verdict(s) from different sessions ({} counted) on head {}, from reviewers who are neither the author (a task lease holder) nor this session, and an open linked task (`Closes #N`) with a lease record by the PR author",
                clean.len(),
                pull.head_sha
            ),
            "get independent clean verdicts (approve, confidence >= min, no major finding) with `bosun verdict comment --from -` in reviewer sessions (without CI add `--sign-key <reviewer.pem>`), then sign again",
        ));
    }
    Ok(())
}

/// An ed25519 key from a PKCS#8 PEM (`openssl genpkey -algorithm ed25519`). Errors name the
/// path only, never the content.
pub(crate) fn read_key(path: &Path) -> Result<ring::signature::Ed25519KeyPair> {
    let bad = || {
        BosunError::error(
            format!(
                "{} is not an ed25519 PKCS#8 PEM private key",
                path.display()
            ),
            "pass the file `openssl genpkey -algorithm ed25519 -out key.pem` wrote",
        )
    };
    let text = std::fs::read_to_string(path).map_err(|e| {
        BosunError::error(
            format!("cannot read {}: {}", path.display(), e.kind()),
            "pass a readable PEM file to --sign-key",
        )
    })?;
    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            eprintln!(
                "bosun: approve: {} is readable by group or others; chmod 600 it",
                path.display()
            );
        }
    }
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some("-----BEGIN PRIVATE KEY-----") {
        return Err(bad());
    }
    let b64_body: String = lines
        .take_while(|l| *l != "-----END PRIVATE KEY-----")
        .collect();
    let der = b64::decode(&b64_body).ok_or_else(bad)?;
    ring::signature::Ed25519KeyPair::from_pkcs8_maybe_unchecked(&der).map_err(|_| bad())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quorum and autonomous are each stricter in a different dimension (quorum: two verdicts;
    /// autonomous: R-weaken still applies), so neither base may sign under the other default.
    #[test]
    fn default_stricter_in_any_dimension_refuses() {
        use Policy::*;
        for p in [Owner, OwnerGated, Orchestrator, Autonomous, Quorum] {
            assert!(!default_stricter(p, p), "{p:?} vs itself signs");
        }
        // Base quorum, default autonomous: autonomous still applies R-weaken.
        assert!(default_stricter(Autonomous, Quorum));
        // Base autonomous, default quorum: quorum needs two verdicts.
        assert!(default_stricter(Quorum, Autonomous));
        // Orchestrator signs without a verdict; the autonomous default asks for one.
        assert!(default_stricter(Autonomous, Orchestrator));
        assert!(default_stricter(Quorum, Orchestrator));
        // A base that signs nothing is never looser than the default.
        for m in [Owner, OwnerGated, Orchestrator, Autonomous, Quorum] {
            assert!(!default_stricter(m, Owner), "{m:?} vs owner base");
        }
        // The owner default keeps everything with the owner.
        assert!(default_stricter(Owner, Quorum));
        assert!(default_stricter(OwnerGated, Autonomous));
        // Default looser or equal in every dimension.
        assert!(!default_stricter(Orchestrator, OwnerGated));
        assert!(!default_stricter(Orchestrator, Owner));
        assert!(!default_stricter(OwnerGated, Owner));
    }

    fn lists(models: Option<&[&str]>, allow: Option<&[&str]>) -> crate::config::Approvals {
        let v = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
        crate::config::Approvals {
            review_models: models.map(v),
            autonomous_allow: allow.map(v),
            ..Default::default()
        }
    }

    #[test]
    fn default_stricter_compares_review_models_and_autonomous_allow() {
        let none = lists(None, None);
        assert!(!lists_stricter(&none, &none));
        // Default branch enrols models; a base that counts any model, or another one, is looser.
        let m = lists(Some(&["opus"]), None);
        assert!(lists_stricter(&m, &none));
        assert!(lists_stricter(&m, &lists(Some(&["opus", "haiku"]), None)));
        assert!(!lists_stricter(&m, &m));
        // A base that narrows further, or a default branch that sets nothing, is not looser.
        assert!(!lists_stricter(&lists(Some(&["opus", "sonnet"]), None), &m));
        assert!(!lists_stricter(&none, &m));
        // autonomous_allow: base widening beyond the default branch's set is looser.
        let narrow = lists(None, Some(&["docs/**"]));
        assert!(lists_stricter(&narrow, &none));
        assert!(lists_stricter(
            &narrow,
            &lists(None, Some(&["docs/**", "src/**"]))
        ));
        assert!(!lists_stricter(&narrow, &narrow));
        assert!(!lists_stricter(&none, &narrow));
        assert!(!lists_stricter(&narrow, &lists(None, Some(&[]))));
    }

    #[test]
    fn lists_stricter_compares_bosun_repo() {
        let repo = |r: Option<&str>| crate::config::Approvals {
            bosun_repo: r.map(String::from),
            ..Default::default()
        };
        assert!(!lists_stricter(&repo(None), &repo(None)));
        assert!(!lists_stricter(&repo(Some("a/b")), &repo(Some("a/b"))));
        assert!(lists_stricter(&repo(Some("a/b")), &repo(None)));
        assert!(lists_stricter(&repo(None), &repo(Some("a/b"))));
        assert!(lists_stricter(&repo(Some("a/b")), &repo(Some("c/d"))));
        assert!(!lists_stricter(
            &repo(Some(crate::config::DEFAULT_BOSUN_REPO)),
            &repo(None)
        ));
    }

    #[test]
    fn default_stricter_compares_verdict_bots_and_oneoff_allow() {
        let v = |l: &[&str]| Some(l.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let bots = |l: &[&str]| crate::config::Approvals {
            verdict_bots: v(l),
            ..Default::default()
        };
        let oneoff = |l: &[&str]| crate::config::Approvals {
            oneoff_allow: v(l),
            ..Default::default()
        };
        let none = crate::config::Approvals::default();
        // Unset allows nothing: a base that adds a bot or glob is looser than the default branch.
        assert!(lists_stricter(&none, &bots(&["claude[bot]"])));
        assert!(lists_stricter(
            &bots(&["a[bot]"]),
            &bots(&["a[bot]", "b[bot]"])
        ));
        assert!(lists_stricter(&bots(&["a[bot]"]), &bots(&["b[bot]"])));
        assert!(!lists_stricter(&bots(&["a[bot]"]), &bots(&["a[bot]"])));
        assert!(!lists_stricter(
            &bots(&["a[bot]", "b[bot]"]),
            &bots(&["a[bot]"])
        ));
        assert!(!lists_stricter(&bots(&["a[bot]"]), &none));
        assert!(!lists_stricter(&none, &bots(&[])));
        assert!(lists_stricter(&none, &oneoff(&["README.md"])));
        assert!(lists_stricter(
            &oneoff(&["a/**"]),
            &oneoff(&["a/**", "b/**"])
        ));
        assert!(!lists_stricter(&oneoff(&["a/**"]), &oneoff(&["a/**"])));
        assert!(!lists_stricter(&oneoff(&["a/**"]), &none));
    }
}
