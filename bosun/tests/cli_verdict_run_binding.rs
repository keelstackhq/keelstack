//! A verdict record counts only when it is bound to a run of the verdict workflow (issue #39,
//! spec section 8): the run is `.github/workflows/verdict.yml`, event `issue_comment`, on the
//! default branch; the attempt was live when the record was written and ran `bosun verdict
//! apply`; one record names a (run, attempt); and the verdict comment the record applied yields
//! the record. Anything else fails closed. Seeded records get a valid run from the fake.

mod common;

use common::fake::{FakeGithub, verdict_jobs_with, verdict_run_json};
use common::{bosun, one_object, run, run_env};
use serde_json::{Value, json};

const NOW: &str = "2026-10-22T10:00:00Z";
const BASE: &str = "base0000";
const HEAD: &str = "sha1";
const BOT: &str = "github-actions[bot]";
const RULES: &str = r#"
[[rule]]
id = "R-ci-review"
why = "r"
approval = "two-verdicts"
paths = [".github/workflows/**"]
"#;

fn world() -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "Closes #7");
    fake.issue(7, "task", &[], "");
    fake.content(
        "governance/owners.toml",
        BASE,
        "[[owner]]\ngithub = \"ada\"\n",
    );
    fake.content(".bosun/risk.toml", BASE, RULES);
    fake.pull_file(
        1,
        ".github/workflows/other.yml",
        "modified",
        1,
        0,
        Some("@@ -0,0 +1,1 @@\n+x"),
    );
    fake
}

fn line(session: &str, head: &str, approved: bool, extra: &str) -> String {
    format!(
        "Review verdict: x.\n\n{{\"bosun\":\"verdict\",\"session\":\"{session}\",\"head_sha\":\"{head}\",\"approved\":{approved},\"confidence\":0.95,\"major\":0{extra}}}"
    )
}

/// Seed a record of `session`; the fake binds it to a fresh valid run. Returns (run, applied).
fn seed(fake: &FakeGithub, session: &str) -> (u64, u64) {
    fake.comment_by(1, BOT, &line(session, HEAD, true, ""));
    let id = fake.comments_of(1).last().unwrap().id;
    (common::fake::DEFAULT_RUN + id, 900_000 + id)
}

fn check(fake: &FakeGithub) -> i32 {
    run(fake, "session_test", NOW, &["risk-check", "1"]).0
}

/// Two seeded verdicts; `f` changes the run of the second. The exit code of risk-check.
fn second_run(f: impl FnOnce(&mut Value)) -> i32 {
    let fake = world();
    seed(&fake, "s1");
    let (r2, _) = seed(&fake, "s2");
    let mut v = verdict_run_json(r2);
    f(&mut v);
    fake.verdict_run(r2, v);
    check(&fake)
}

/// Two seeded verdicts; `f` changes the second's applied comment.
fn second_applied(f: impl FnOnce(&mut common::fake::FakeComment)) -> i32 {
    let fake = world();
    seed(&fake, "s1");
    let (_, a2) = seed(&fake, "s2");
    f(fake
        .state
        .lock()
        .unwrap()
        .applied_comments
        .get_mut(&a2)
        .unwrap());
    check(&fake)
}

#[test]
fn a_record_of_a_valid_verdict_run_counts() {
    assert_eq!(second_run(|_| {}), 0);
}

#[test]
fn every_run_is_read_once() {
    let fake = world();
    let (r1, _) = seed(&fake, "s1");
    seed(&fake, "s2");
    assert_eq!(check(&fake), 0);
    let reads = |p: String| {
        fake.requests()
            .iter()
            .filter(|r| r.path.ends_with(&p))
            .count()
    };
    assert_eq!(reads(format!("/actions/runs/{r1}")), 1);
    assert_eq!(reads(format!("/actions/runs/{r1}/attempts/1")), 1);
}

#[test]
fn two_records_naming_one_run_and_attempt_neither_counts() {
    let fake = world();
    let (r, a) = seed(&fake, "s1");
    // A second record of another session names the same run, attempt and applied comment.
    let extra = format!(",\"run\":{r},\"attempt\":1,\"applied\":{a}");
    fake.comment_by(1, BOT, &line("s2", HEAD, true, &extra));
    assert_eq!(check(&fake), 5);
    // Neither counts, so a third valid record is one verdict, not two.
    seed(&fake, "s3");
    assert_eq!(check(&fake), 5);
}

#[test]
fn a_run_of_another_workflow_event_or_branch_does_not_count() {
    assert_eq!(
        second_run(|v| v["path"] = json!(".github/workflows/risk.yml")),
        5
    );
    assert_eq!(second_run(|v| v["event"] = json!("pull_request")), 5);
    assert_eq!(second_run(|v| v["head_branch"] = json!("feature")), 5);
}

#[test]
fn a_comment_before_the_attempt_or_after_it_ended_does_not_count() {
    assert_eq!(
        second_run(|v| v["run_started_at"] = json!("2026-10-23T00:00:00Z")),
        5
    );
    assert_eq!(
        second_run(|v| v["updated_at"] = json!("2026-10-22T09:59:59Z")),
        5
    );
}

#[test]
fn a_run_still_in_progress_counts_but_not_a_queued_or_waiting_one() {
    let live = |s: &'static str| {
        move |v: &mut Value| {
            v["status"] = json!(s);
            v["updated_at"] = json!("2026-10-22T09:59:59Z");
        }
    };
    assert_eq!(second_run(live("in_progress")), 0);
    assert_eq!(second_run(live("queued")), 5);
    assert_eq!(second_run(live("waiting")), 5);
}

#[test]
fn a_run_with_a_missing_field_does_not_count() {
    for key in [
        "path",
        "event",
        "head_branch",
        "run_started_at",
        "updated_at",
        "status",
    ] {
        let code = second_run(|v| {
            v.as_object_mut().unwrap().remove(key);
        });
        assert_eq!(code, 5, "{key}");
    }
}

#[test]
fn an_unreadable_run_or_attempt_or_job_list_does_not_count() {
    for tail in ["", "/attempts/1", "/attempts/1/jobs"] {
        for status in [404, 500] {
            let fake = world();
            seed(&fake, "s1");
            let (r2, _) = seed(&fake, "s2");
            fake.fail_path(&format!("/actions/runs/{r2}{tail}"), status);
            assert_eq!(check(&fake), 5, "{tail} {status}");
        }
    }
}

#[test]
fn an_attempt_the_record_names_must_exist() {
    let fake = world();
    seed(&fake, "s1");
    let (r, a) = seed(&fake, "s2");
    let id = fake.comments_of(1).last().unwrap().id;
    let body = fake
        .comments_of(1)
        .last()
        .unwrap()
        .body
        .replace("\"attempt\":1", "\"attempt\":2");
    let _ = (r, a, id);
    fake.last_comment(|c| c.body = body);
    assert_eq!(check(&fake), 5);
}

const WIDE: (&str, &str) = ("2000-01-01T00:00:00Z", "2099-12-31T00:00:00Z");

/// Two seeded verdicts; the second's run lists the recording step `name(applied)` with this
/// conclusion and window. The exit code of risk-check.
fn second_step(name: impl FnOnce(u64) -> String, conclusion: &str, win: (&str, &str)) -> i32 {
    let fake = world();
    seed(&fake, "s1");
    let (r2, a2) = seed(&fake, "s2");
    fake.verdict_jobs(
        r2,
        1,
        verdict_jobs_with(&name(a2), conclusion, win.0, win.1),
    );
    check(&fake)
}

fn record_step(a: u64) -> String {
    format!("bosun verdict record 1 {a}")
}

#[test]
fn an_attempt_whose_record_step_did_not_succeed_does_not_count() {
    assert_eq!(second_step(record_step, "success", WIDE), 0);
    // A refused or no-op apply never yields a successful record step: it is skipped or fails.
    for conclusion in ["skipped", "failure", "cancelled", ""] {
        assert_eq!(
            second_step(record_step, conclusion, WIDE),
            5,
            "{conclusion:?}"
        );
    }
    // The old step name is no record step.
    let old = |_| "bosun verdict apply".to_string();
    assert_eq!(second_step(old, "success", WIDE), 5);
    let fake = world();
    seed(&fake, "s1");
    let (r2, _) = seed(&fake, "s2");
    fake.verdict_jobs(r2, 1, json!([{"name": "other", "steps": []}]));
    assert_eq!(check(&fake), 5, "no verdict job");
}

#[test]
fn a_record_outside_the_record_steps_window_does_not_count() {
    // The record was created at NOW.
    assert_eq!(second_step(record_step, "success", (NOW, NOW)), 0);
    let late = ("2026-10-22T10:00:01Z", "2026-10-22T10:05:00Z");
    assert_eq!(second_step(record_step, "success", late), 5, "before");
    let early = ("2026-10-22T09:00:00Z", "2026-10-22T09:59:59Z");
    assert_eq!(second_step(record_step, "success", early), 5, "after");
    assert_eq!(
        second_step(record_step, "success", ("", "")),
        5,
        "no window"
    );
}

#[test]
fn a_record_naming_another_prs_or_comments_run_does_not_count() {
    // A real, successful run of PR 2's verdict (or of another comment) names its own step.
    let other_pr = |a| format!("bosun verdict record 2 {a}");
    assert_eq!(second_step(other_pr, "success", WIDE), 5, "another PR");
    let other_comment = |a| format!("bosun verdict record 1 {}", a + 1);
    assert_eq!(second_step(other_comment, "success", WIDE), 5, "comment");
}

#[test]
fn a_named_comment_apply_would_not_record_as_this_session_does_not_count() {
    // A Bot's comment with a reviewer marker: apply records `bot:<login>`, never `s2`.
    let bot_comment = |c: &mut common::fake::FakeComment| {
        c.user = BOT.into();
        c.user_type = Some("Bot".into());
    };
    let fake = world();
    fake.permission(BOT, "none");
    seed(&fake, "s1");
    let (_, a2) = seed(&fake, "s2");
    bot_comment(
        fake.state
            .lock()
            .unwrap()
            .applied_comments
            .get_mut(&a2)
            .unwrap(),
    );
    assert_eq!(check(&fake), 5, "bot author");
    // A non-reviewer (no write access) author.
    let fake = world();
    fake.permission("mallory", "read");
    seed(&fake, "s1");
    let (_, a2) = seed(&fake, "s2");
    let mut st = fake.state.lock().unwrap();
    st.applied_comments.get_mut(&a2).unwrap().user = "mallory".into();
    drop(st);
    assert_eq!(check(&fake), 5, "non-reviewer");
}

#[test]
fn an_unreadable_applied_comment_does_not_count_and_says_so() {
    let fake = world();
    seed(&fake, "s1");
    let (_, a2) = seed(&fake, "s2");
    fake.fail_path(&format!("/comments/{a2}"), 500);
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_SESSION", "session_test")
        .env("BOSUN_NOW", NOW)
        .args(["risk-check", "1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(5));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(&format!("applied comment {a2} is unreadable")),
        "{err}"
    );
}

#[test]
fn a_record_without_a_run_attempt_or_applied_comment_does_not_count() {
    for strip in ["run", "attempt", "applied"] {
        let fake = world();
        seed(&fake, "s1");
        seed(&fake, "s2");
        let key = format!("\"{strip}\":");
        let mut body = fake.comments_of(1).last().unwrap().body.clone();
        let at = body.find(&key).unwrap();
        let end = body[at..].find([',', '}']).unwrap() + at;
        let cut = if body[end..].starts_with('}') {
            at - 1
        } else {
            at
        };
        body.replace_range(cut..end + usize::from(body[end..].starts_with(',')), "");
        fake.last_comment(|c| c.body = body);
        assert_eq!(check(&fake), 5, "{strip}");
    }
    let fake = world();
    fake.raw_verdict_records();
    fake.comment_by(1, BOT, &line("s1", HEAD, true, ",\"run\":11"));
    fake.comment_by(1, BOT, &line("s2", HEAD, true, ""));
    assert_eq!(check(&fake), 5);
}

#[test]
fn the_applied_comment_must_be_the_reviewers_unedited_and_on_this_pr() {
    assert_eq!(second_applied(|_| {}), 0);
    assert_eq!(
        second_applied(|c| c.updated_at = "2026-10-22T00:00:00Z".into()),
        5,
        "edited"
    );
    assert_eq!(second_applied(|c| c.issue = 2), 5, "another PR");
    assert_eq!(
        second_applied(|c| c.created_at = "2026-10-22T09:00:00Z".into()),
        5,
        "after the run began"
    );
    assert_eq!(
        second_applied(|c| c.body = c.body.replace("bosun:reviewer s2", "bosun:reviewer s9")),
        5,
        "another session"
    );
    assert_eq!(
        second_applied(|c| c.body = c.body.replace("\"approve\"", "\"request-changes\"")),
        5,
        "another verdict"
    );
    assert_eq!(
        second_applied(|c| c.body = c.body.replace("sha1", "sha2")),
        5,
        "another head"
    );
    assert_eq!(
        second_applied(|c| c.body = "no verdict here".into()),
        5,
        "no verdict"
    );
    let fake = world();
    seed(&fake, "s1");
    let (_, a2) = seed(&fake, "s2");
    fake.state.lock().unwrap().applied_comments.remove(&a2);
    assert_eq!(check(&fake), 5, "the comment is gone");
}

#[test]
fn a_rejection_on_an_unverifiable_run_still_blocks() {
    let fake = world();
    seed(&fake, "s1");
    seed(&fake, "s2");
    // A newer rejection of s2 whose run cannot be read still withdraws its approval.
    fake.raw_verdict_records();
    fake.comment_by(
        1,
        BOT,
        &line("s2", HEAD, false, ",\"run\":99,\"attempt\":1,\"applied\":5"),
    );
    assert_eq!(check(&fake), 5);
}

// --- bosun verdict apply writes the binding ---

const WINDOW_START: &str = "2026-10-22T10:00:00Z";

fn reviewer_comment(fake: &FakeGithub, session: &str) -> u64 {
    let v = json!({"pr": 1, "head_sha": HEAD, "verdict": "approve", "confidence": 0.95, "findings": []});
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_SESSION", session)
        .args(["verdict", "comment", "--from", "-"])
        .write_stdin(v.to_string())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", one_object(&out.stdout));
    fake.comments_of(1).last().unwrap().id
}

/// Apply the reviewer comments of s1 and s2 as the workflow would (the record is written by
/// `github-actions[bot]`), with or without the run environment; the risk-check exit code.
fn applied_end_to_end(with_env: bool) -> i32 {
    let fake = world();
    fake.pull_as_issue(1, "change");
    fake.raw_verdict_records();
    for (n, session) in [(31u64, "s1"), (32, "s2")] {
        fake.verdict_run(n, {
            let mut v = verdict_run_json(n);
            v["run_started_at"] = json!(WINDOW_START);
            v
        });
        let id = reviewer_comment(&fake, session);
        fake.verdict_jobs(
            n,
            1,
            verdict_jobs_with(&record_step(id), "success", WIDE.0, WIDE.1),
        );
        let (run_id, attempt) = (n.to_string(), "1");
        let env: &[(&str, &str)] = if with_env {
            &[("BOSUN_RUN_ID", &run_id), ("BOSUN_RUN_ATTEMPT", attempt)]
        } else {
            &[]
        };
        let (code, v) = run_env(
            &fake,
            "verdict-workflow",
            NOW,
            &["verdict", "apply", &id.to_string()],
            env,
        );
        assert_eq!(code, 0, "{v}");
        fake.last_comment(|c| c.user = BOT.into());
        let record = fake.comments_of(1).last().unwrap().body.clone();
        assert_eq!(record.contains("\"applied\":"), with_env, "{record}");
    }
    check(&fake)
}

#[test]
fn the_record_apply_writes_counts_end_to_end_with_the_run_environment() {
    assert_eq!(applied_end_to_end(true), 0);
}

#[test]
fn the_record_apply_writes_without_the_run_environment_does_not_count() {
    assert_eq!(applied_end_to_end(false), 5);
}

// --- `bosun verdict apply --check` and `--require` ---

fn comment_of(fake: &FakeGithub, user: &str) -> u64 {
    let id = reviewer_comment(fake, "s1");
    fake.last_comment(|c| c.user = user.into());
    id
}

#[test]
fn check_writes_nothing_and_says_whether_the_record_step_runs() {
    let fake = world();
    fake.pull_as_issue(1, "change");
    let id = comment_of(&fake, "ada");
    let before = fake.comments_of(1).len();
    let (code, v) = run(
        &fake,
        "verdict-workflow",
        NOW,
        &["verdict", "apply", "--check", &id.to_string()],
    );
    assert_eq!((code, &v["data"]["accepted"]), (0, &json!(true)), "{v}");
    assert_eq!(fake.comments_of(1).len(), before, "nothing is written");
    // A refused comment: neutral for --check, an error for --require, no record either way.
    fake.permission("mallory", "read");
    let id = comment_of(&fake, "mallory");
    let (code, v) = run(
        &fake,
        "verdict-workflow",
        NOW,
        &["verdict", "apply", "--check", &id.to_string()],
    );
    assert_eq!((code, &v["data"]["accepted"]), (0, &json!(false)), "{v}");
    let n = fake.comments_of(1).len();
    let (code, v) = run(
        &fake,
        "verdict-workflow",
        NOW,
        &["verdict", "apply", "--require", &id.to_string()],
    );
    assert_eq!(code, 1, "{v}");
    assert_eq!(fake.comments_of(1).len(), n);
}
