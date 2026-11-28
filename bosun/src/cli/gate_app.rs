//! `bosun gate-app advance` (B3 T1): the gate-App transition (spec §15, steps 3 to 7), one step
//! per run. The state is read from the repo and GitHub every time, nothing is remembered:
//!
//! 1. PR A: `splice set bosun_app=1`, `[checks] mode = "dual"` with the App's id and slug,
//!    and a pinned `hoist.toml` (built on the `bosun/gate-app` commit of `bosun owner setup`
//!    when that branch exists);
//! 2. after PR A merged: wait until the App's check runs are seen on a probe PR;
//! 3. then the owner's `hoist apply --area github` (named in `next`, never run here);
//! 4. once the ruleset pins the three gates to the App: PR B (`mode = "checks"`,
//!    `[approvals] signer = true`), which the CI signer signs under `owner_delegation = "all"`.
//!
//! PRs are drafts (as `bosun pin --pr`); `bosun pr --pr N --ready` turns on auto-merge. This
//! command never merges, tags or forces a push (D13). `--dry-run` writes nothing.

use std::path::Path;

use serde_json::{Value, json};

use super::pin_pr::git;
use super::{AdvanceArgs, GateAppCmd};
use crate::app::{App, default_branch, kill_switch};
use crate::github::{Github, IssueState};
use crate::output::{BosunError, CmdResult, Outcome};

const GATE_APP: &str = ".bosun/gate-app.toml";
const CONFIG: &str = ".bosun/bosun.toml";
const HOIST: &str = "hoist.toml";
const CHANNEL_LOCK: &str = ".splice/channels/bosun.toml";
const BRANCH_A: &str = "bosun/gate-app";
const BRANCH_B: &str = "bosun/gate-app-checks";
const GATES: [&str; 3] = ["risk-check", "review-verdict", "main-open"];
/// GitHub Actions' App id: `ci-ok` stays pinned to it.
const ACTIONS_ID: u64 = 15368;
const OWNER_PC: &str = "owner-computer";
const AGENT: &str = "agent";
const HOIST_APPLY: &str = "hoist apply --area github --yes";

type Result<T> = std::result::Result<T, BosunError>;

pub fn run(dir: Option<&Path>, cmd: &GateAppCmd) -> CmdResult {
    match cmd {
        GateAppCmd::Advance(a) => advance(dir, a),
    }
}

/// The phases, in order; the one a run is at is the first not done.
const PHASES: [(&str, &str); 4] = [
    ("pr-a", AGENT),
    ("probe", AGENT),
    ("hoist-apply", OWNER_PC),
    ("pr-b", AGENT),
];

fn detail(step: &str) -> &'static str {
    match step {
        "pr-a" => {
            "PR A: bosun_app=1, [checks] mode = \"dual\" with the App, hoist.toml pinned to it"
        }
        "probe" => "the App's check runs are seen on a probe PR",
        "hoist-apply" => "the owner pins the ruleset: `hoist apply --area github`",
        _ => "PR B: [checks] mode = \"checks\" and [approvals] signer = true",
    }
}

/// The step list when phase `at` has `status`: earlier ones are done, later ones pending.
fn steps(at: usize, status: &str) -> Vec<Value> {
    PHASES
        .iter()
        .enumerate()
        .map(|(i, (step, device))| {
            let st = match i.cmp(&at) {
                std::cmp::Ordering::Less => "done",
                std::cmp::Ordering::Equal => status,
                std::cmp::Ordering::Greater => "pending",
            };
            json!({"step": step, "device": device, "status": st, "detail": detail(step)})
        })
        .collect()
}

/// The text of `.bosun/bosun.toml` on the default branch, parsed (missing: empty).
fn read_config(gh: &dyn Github, base: &str) -> Result<toml::Table> {
    let text = gh.file_at(CONFIG, base)?.unwrap_or_default();
    text.parse().map_err(|_| {
        BosunError::error(
            format!("{CONFIG} on {base} is not valid TOML"),
            format!("fix {CONFIG} on {base}, then rerun"),
        )
    })
}

fn parse_gate_app(text: &str) -> Option<(u64, String)> {
    let t: toml::Table = text.parse().ok()?;
    let id = u64::try_from(t.get("id")?.as_integer()?).ok()?;
    Some((id, t.get("slug")?.as_str()?.to_string()))
}

fn advance(dir: Option<&Path>, a: &AdvanceArgs) -> CmdResult {
    if a.remote.starts_with('-') || a.branch.as_deref().is_some_and(|b| b.starts_with('-')) {
        return Err(BosunError::usage(
            "--remote and --branch must be names, not options",
            "pass a git remote name and a branch name",
        ));
    }
    let app = App::load(dir)?;
    let gh: &dyn Github = &*app.gh;
    kill_switch(gh)?;
    let root = app.ctx.dir.clone();
    let base = default_branch(gh)?;
    if git(&root, &["rev-parse", "--git-dir"]).is_err() {
        return Err(BosunError::usage(
            "this is not a git repo",
            "run `bosun gate-app advance` inside the repo (or pass -C <repo>)",
        ));
    }
    // Remote-tracking refs only: the branch of `owner setup` lives on the remote.
    let _ = git(&root, &["fetch", "-q", &a.remote]);

    let branch_a = a.branch.clone().unwrap_or_else(|| BRANCH_A.into());
    let (id, slug) = find_gate_app(&root, gh, &base, &a.remote, &branch_a)?;
    let cfg = read_config(gh, &base)?;
    let mode = cfg
        .get("checks")
        .and_then(|c| c.get("mode"))
        .and_then(toml::Value::as_str)
        .unwrap_or("statuses");
    let a_done = ["dual", "checks"].contains(&mode)
        && cfg
            .get("checks")
            .and_then(|c| c.get("app_id"))
            .and_then(toml::Value::as_integer)
            == i64::try_from(id).ok();
    let signer = cfg
        .get("approvals")
        .and_then(|c| c.get("signer"))
        .and_then(toml::Value::as_bool)
        == Some(true);
    let delegated = cfg
        .get("approvals")
        .and_then(|c| c.get("owner_delegation"))
        .and_then(toml::Value::as_str)
        == Some("all");
    let repo = app.ctx.repo.to_string();
    let open = gh.list_pulls(None, IssueState::Open)?;
    let open_pr = |branch: &str| {
        open.iter()
            .find(|p| p.head_ref == branch && p.head_repo.eq_ignore_ascii_case(&repo))
            .map(|p| p.number)
    };

    let mut data = json!({"dry_run": a.dry_run, "app_id": id, "slug": slug});
    let finish = |mut data: Value, at: usize, status: &str, left: Vec<String>| {
        data["step"] = json!(PHASES.get(at).map_or("done", |p| p.0));
        data["status"] = json!(status);
        data["steps"] = json!(steps(at, status));
        data["left"] = json!(left);
        data
    };

    // Everything merged.
    if a_done && mode == "checks" && signer {
        let mut d = finish(data, 4, "done", vec![]);
        d["steps"] = json!(steps(4, "done"));
        return Ok(Outcome::data(d));
    }

    // 1) PR A.
    if !a_done {
        if let Some(n) = open_pr(&branch_a) {
            data["branch"] = json!(branch_a);
            data["pr"] = json!({"number": n});
            let left = vec![format!(
                "Approve PR #{n} (owner: it enrolls the CI signer key and pins hoist.toml); then rerun `bosun gate-app advance`."
            )];
            return Ok(Outcome::data(finish(data, 0, "waiting", left)));
        }
        if a.dry_run {
            data["branch"] = json!(branch_a);
            return Ok(Outcome::data(finish(
                data,
                0,
                "would-do",
                vec![format!("open PR A from {branch_a}")],
            )));
        }
        let title = "chore(bosun): turn on the gate App (dual)";
        let body = format!(
            "Step 3 and 5 of the transition (spec §15): `splice set bosun_app=1`, `[checks] mode = \"dual\"` with App {id} (`{slug}`), and hoist.toml's required checks pinned to it.\n\nAfter it merges: `bosun gate-app advance` waits for the App's runs on a probe PR, then names the owner's `{HOIST_APPLY}`."
        );
        let edit = |root: &Path| -> Result<Vec<String>> {
            let mut notes = Vec::new();
            if root.join(CHANNEL_LOCK).exists() {
                let argv: Vec<String> = [
                    "-C",
                    &root.to_string_lossy(),
                    "--yes",
                    "--json",
                    "set",
                    "--channel",
                    "bosun",
                    "bosun_app=1",
                ]
                .map(String::from)
                .to_vec();
                super::init::run_splice(&argv)?;
            } else {
                notes.push(format!(
                    "no {CHANNEL_LOCK}: `splice set bosun_app=1` skipped"
                ));
            }
            edit_file(root, CONFIG, |t| {
                let t = set_key(t, "checks", "mode", "\"dual\"");
                let t = set_key(&t, "checks", "app_id", &id.to_string());
                set_key(&t, "checks", "app_slug", &format!("\"{slug}\""))
            })?;
            if root.join(HOIST).exists() {
                edit_file(root, HOIST, |t| pin_hoist(t, &slug))?;
            } else {
                notes.push(format!("no {HOIST}: nothing to pin"));
            }
            Ok(notes)
        };
        let (pull, notes) = open_pr_on(&app, &base, &a.remote, &branch_a, title, &body, edit)?;
        data["branch"] = json!(branch_a);
        data["pr"] = json!({"number": pull.0, "head_sha": pull.1});
        let mut left = vec![
            format!(
                "Run `bosun pr --pr {} --ready` (gates, ready, auto-merge); never merge it.",
                pull.0
            ),
            "The owner approves PR A once (it enrolls the signer key).".to_string(),
        ];
        left.extend(notes);
        return Ok(Outcome::data(finish(data, 0, "created", left)));
    }

    // 2) The App's runs on a probe PR.
    if !runs_seen(gh, &base, id)? {
        let left = vec![
            "Open any small PR (or wait for one): the App's risk-check run must appear on it, then rerun `bosun gate-app advance`.".to_string(),
        ];
        return Ok(Outcome::data(finish(data, 1, "waiting", left)));
    }

    // 3) The ruleset pins the gates to the App.
    let pinned = gh.ruleset_pins(&base)?.is_some_and(|pins| {
        GATES
            .iter()
            .all(|g| pins.iter().any(|(c, i)| c == g && *i == Some(id)))
    });
    if !pinned {
        let left = vec![format!(
            "The owner runs `{HOIST_APPLY}` from a checkout of main (`bosun owner setup` does it after PR A merges), then rerun `bosun gate-app advance`."
        )];
        return Ok(Outcome::data(finish(data, 2, "waiting-owner", left))
            .with_next("Bash", json!({"command": HOIST_APPLY, "device": OWNER_PC})));
    }

    // 4) PR B.
    let branch_b = a.branch.clone().unwrap_or_else(|| BRANCH_B.into());
    if let Some(n) = open_pr(&branch_b) {
        data["branch"] = json!(branch_b);
        data["pr"] = json!({"number": n});
        let left = vec![signer_left(n, delegated)];
        return Ok(Outcome::data(finish(data, 3, "waiting", left)));
    }
    if a.dry_run {
        data["branch"] = json!(branch_b);
        return Ok(Outcome::data(finish(
            data,
            3,
            "would-do",
            vec![format!("open PR B from {branch_b}")],
        )));
    }
    let title = "chore(bosun): gate App check runs only, turn on the CI signer";
    let body = format!(
        "Step 7 of the transition (spec §15): `[checks] mode = \"checks\"` and `[approvals] signer = true`. The ruleset already pins the gates to App {id} (`{slug}`), so statuses can stop.\n\n{}",
        if delegated {
            "`owner_delegation = \"all\"`: the CI signer signs this PR once the required checks and the verdicts are clean."
        } else {
            "`owner_delegation` is not `all`: the owner approves this PR."
        }
    );
    let edit = |root: &Path| -> Result<Vec<String>> {
        edit_file(root, CONFIG, |t| {
            let t = set_key(t, "checks", "mode", "\"checks\"");
            set_key(&t, "approvals", "signer", "true")
        })?;
        Ok(vec![])
    };
    let (pull, notes) = open_pr_on(&app, &base, &a.remote, &branch_b, title, &body, edit)?;
    data["branch"] = json!(branch_b);
    data["pr"] = json!({"number": pull.0, "head_sha": pull.1});
    let mut left = vec![
        format!(
            "Run `bosun pr --pr {} --ready` (gates, ready, auto-merge); never merge it.",
            pull.0
        ),
        signer_left(pull.0, delegated),
    ];
    left.extend(notes);
    Ok(Outcome::data(finish(data, 3, "created", left)))
}

fn signer_left(n: u64, delegated: bool) -> String {
    if delegated {
        format!(
            "PR #{n}: the CI signer signs it (owner_delegation = all) when the checks and verdicts are clean. It edits `.bosun/**`, so the owner approves it on GitHub (follow-up: let the signer sign it)."
        )
    } else {
        format!("PR #{n}: owner_delegation is not `all`, so the owner approves it.")
    }
}

/// The App's id and slug from `.bosun/gate-app.toml`: on the default branch, else on the
/// `owner setup` branch (local or fetched), else in the work tree.
fn find_gate_app(
    root: &Path,
    gh: &dyn Github,
    base: &str,
    remote: &str,
    branch: &str,
) -> Result<(u64, String)> {
    let mut texts = vec![gh.file_at(GATE_APP, base)?];
    for r in [
        format!("refs/heads/{branch}"),
        format!("refs/remotes/{remote}/{branch}"),
    ] {
        texts.push(git(root, &["show", &format!("{r}:{GATE_APP}")]).ok());
    }
    texts.push(std::fs::read_to_string(root.join(GATE_APP)).ok());
    texts
        .into_iter()
        .flatten()
        .find_map(|t| parse_gate_app(&t))
        .ok_or_else(|| {
            BosunError::needs_human(
                format!("{GATE_APP} is not on {base} or on {branch}: the gate App is not set up"),
                "the owner runs `keel owner` (or `bosun owner setup`) on their computer (it records the App and pushes bosun/gate-app), then rerun",
            )
            .with_data(json!({"step": "owner-setup", "status": "needs-human"}))
        })
}

/// Are the App's check runs on a recent PR or on the default branch?
fn runs_seen(gh: &dyn Github, base: &str, id: u64) -> Result<bool> {
    let mut shas: Vec<String> = gh
        .list_pulls(None, IssueState::All)?
        .into_iter()
        .take(20)
        .map(|p| p.head_sha)
        .collect();
    shas.push(base.to_string());
    for sha in shas {
        if gh.check_runs_all(&sha)?.iter().any(|r| r.app_id == id) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Make `branch` (the remote's, the local one, else a new one from the default branch), run
/// `edit` on it, commit, push (plain) and open the draft PR. Returns `((number, head sha),
/// notes)`; the checkout ends where it began. A clean tree is required (as for `bosun pin --pr`).
fn open_pr_on(
    app: &App,
    base: &str,
    remote: &str,
    branch: &str,
    title: &str,
    body: &str,
    edit: impl FnOnce(&Path) -> Result<Vec<String>>,
) -> Result<((u64, String), Vec<String>)> {
    let root = app.ctx.dir.clone();
    if !git(&root, &["status", "--porcelain"])?.is_empty() {
        return Err(BosunError::conflict(
            "the working tree has changes",
            "commit or stash them, then rerun",
        ));
    }
    let orig = git(&root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let local = git(
        &root,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok();
    let tracking = format!("refs/remotes/{remote}/{branch}");
    let on_remote = git(&root, &["rev-parse", "--verify", "-q", &tracking]).is_ok();
    let from_base = format!("refs/remotes/{remote}/{base}");
    let start = if git(&root, &["rev-parse", "--verify", "-q", &from_base]).is_ok() {
        from_base
    } else {
        base.to_string()
    };
    let created = !local;
    let switched = if local {
        git(&root, &["switch", "-q", branch])
    } else if on_remote {
        git(&root, &["switch", "-q", "-c", branch, &tracking])
    } else {
        git(&root, &["switch", "-q", "-c", branch, &start])
    };
    let back = |root: &Path| {
        let _ = git(root, &["switch", "-q", "-f", &orig]);
        if created {
            let _ = git(root, &["branch", "-q", "-D", branch]);
        }
    };
    switched?;
    let done = (|| -> Result<Vec<String>> {
        let notes = edit(&root)?;
        git(&root, &["add", "-A"])?;
        if git(&root, &["diff", "--cached", "--quiet"]).is_err() {
            git(&root, &["commit", "-q", "-m", title])?;
        } else if !on_remote {
            return Err(BosunError::error(
                format!("nothing to change on {branch}: the files already say it"),
                "check .bosun/bosun.toml and hoist.toml on the default branch, then rerun",
            ));
        }
        git(&root, &["push", "-q", "-u", remote, branch])?;
        Ok(notes)
    })();
    let notes = match done {
        Ok(n) => n,
        Err(e) => {
            back(&root);
            return Err(e);
        }
    };
    let _ = git(&root, &["switch", "-q", &orig]);
    let pull = app
        .gh
        .create_pull(title, branch, base, body, true)
        .map_err(|e| BosunError {
            fix: format!(
                "{branch} is pushed: open its PR by hand with `gh pr create --draft --head {branch} --base {base}`"
            ),
            ..e
        })?;
    Ok(((pull.number, pull.head_sha), notes))
}

fn edit_file(root: &Path, rel: &str, f: impl FnOnce(&str) -> String) -> Result<()> {
    let path = root.join(rel);
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let new = f(&old);
    if new.parse::<toml::Table>().is_err() {
        return Err(BosunError::error(
            format!("{rel} would not be valid TOML after the edit"),
            format!("fix {rel} by hand, then rerun"),
        ));
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&path, new).map_err(|e| {
        BosunError::error(
            format!("cannot write {rel}: {e}"),
            "check the file permissions, then rerun",
        )
    })
}

/// The number of unclosed `[` on a line (strings hold no brackets in the keys we edit).
fn depth(line: &str) -> i32 {
    let mut d = 0;
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => break,
            '[' if !quoted => d += 1,
            ']' if !quoted => d -= 1,
            _ => {}
        }
    }
    d
}

/// `text` with `key = value` set inside `[table]` (the table or the key is added when missing).
/// `value` is TOML text; a multi-line array value is replaced whole.
pub(crate) fn set_key(text: &str, table: &str, key: &str, value: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let header = format!("[{table}]");
    let new_line = format!("{key} = {value}");
    let Some(h) = lines.iter().position(|l| l.trim() == header) else {
        let mut out = text.trim_end().to_string();
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        return format!("{out}{header}\n{new_line}\n");
    };
    let end = (h + 1..lines.len())
        .find(|&i| lines[i].trim_start().starts_with('['))
        .unwrap_or(lines.len());
    let is_key = |l: &str| {
        l.trim_start()
            .strip_prefix(key)
            .is_some_and(|r| r.trim_start().starts_with('='))
    };
    let mut out: Vec<String> = Vec::new();
    match (h + 1..end).find(|&i| is_key(lines[i])) {
        Some(i) => {
            let mut j = i;
            let mut open = depth(lines[i].split_once('=').map_or("", |p| p.1));
            while open > 0 && j + 1 < lines.len() {
                j += 1;
                open += depth(lines[j]);
            }
            out.extend(lines[..i].iter().map(|l| l.to_string()));
            out.push(new_line);
            out.extend(lines[j + 1..].iter().map(|l| l.to_string()));
        }
        None => {
            out.extend(lines[..=h].iter().map(|l| l.to_string()));
            out.push(new_line);
            out.extend(lines[h + 1..].iter().map(|l| l.to_string()));
        }
    }
    out.join("\n") + "\n"
}

/// `hoist.toml` with `[github] required_checks` pinned: `ci-ok` to GitHub Actions, the three
/// gates to the App `slug` (added when missing), anything else kept as it was.
fn pin_hoist(text: &str, slug: &str) -> String {
    let parsed: toml::Table = text.parse().unwrap_or_default();
    let existing: Vec<toml::Value> = parsed
        .get("github")
        .and_then(|g| g.get("required_checks"))
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let context = |v: &toml::Value| {
        v.as_str()
            .or_else(|| v.get("context").and_then(toml::Value::as_str))
            .map(str::to_string)
    };
    let mut lines: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for v in &existing {
        let Some(c) = context(v) else { continue };
        seen.push(c.clone());
        lines.push(if GATES.contains(&c.as_str()) {
            format!("  {{ context = \"{c}\", app = \"{slug}\" }},")
        } else if c == "ci-ok" {
            format!("  {{ context = \"ci-ok\", integration_id = {ACTIONS_ID} }},")
        } else if v.is_str() {
            format!("  \"{c}\",")
        } else {
            let app = v.get("app").and_then(toml::Value::as_str);
            let iid = v.get("integration_id").and_then(toml::Value::as_integer);
            match (app, iid) {
                (Some(a), _) => format!("  {{ context = \"{c}\", app = \"{a}\" }},"),
                (_, Some(i)) => format!("  {{ context = \"{c}\", integration_id = {i} }},"),
                _ => format!("  \"{c}\","),
            }
        });
    }
    for g in GATES {
        if !seen.iter().any(|c| c == g) {
            lines.push(format!("  {{ context = \"{g}\", app = \"{slug}\" }},"));
        }
    }
    let value = format!("[\n{}\n]", lines.join("\n"));
    set_key(text, "github", "required_checks", &value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_key_replaces_adds_and_appends() {
        let t = "a = 1\n\n[checks]\nmode = \"statuses\"\n\n[x]\nmode = 2\n";
        let r = set_key(t, "checks", "mode", "\"dual\"");
        assert!(r.contains("[checks]\nmode = \"dual\"") && r.contains("[x]\nmode = 2"));
        let r = set_key(&r, "checks", "app_id", "7");
        assert!(r.parse::<toml::Table>().unwrap()["checks"]["app_id"].as_integer() == Some(7));
        let r = set_key("a = 1\n", "approvals", "signer", "true");
        assert_eq!(r, "a = 1\n\n[approvals]\nsigner = true\n");
    }

    #[test]
    fn pin_hoist_replaces_a_multiline_array_and_keeps_the_rest() {
        let t =
            "[github]\nrequired_checks = [\n  \"ci-ok\",\n  \"other\",\n]\nmerge_queue = false\n";
        let r = pin_hoist(t, "gate");
        let p: toml::Table = r.parse().unwrap();
        let rc = p["github"]["required_checks"].as_array().unwrap();
        assert_eq!(rc.len(), 5, "{r}");
        assert_eq!(rc[0]["integration_id"].as_integer(), Some(15368));
        assert_eq!(rc[1].as_str(), Some("other"));
        assert_eq!(p["github"]["merge_queue"].as_bool(), Some(false));
    }
}
