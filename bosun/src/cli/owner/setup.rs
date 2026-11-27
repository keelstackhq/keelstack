//! `bosun owner setup` (B2 T2): the gate App, the CI signer and everything around them in one
//! idempotent run on the owner's computer.
//!
//! Security rules (plan B2, binding): a private key (the App's, the signer's) exists only in
//! memory as a [`Secret`] (redacting `Debug`, wiped on drop, so also on every error path); it
//! reaches GitHub only as a sealed box made with the environment's public key; it is never
//! printed, never in argv or the environment of a child, never in the repo. The one file bosun
//! may delete is the owner's `--key-file`, and only with `--delete-key-file`. The branch is
//! pushed with a plain push (never forced); there is no PR and no merge. The run then waits
//! (30 minutes at most) for the owner to merge PR A, which an agent opens with `bosun gate-app
//! advance`, and runs `hoist apply --area github` (B3 T2), so the one run covers that step too.
//!
//! Every step reads what exists first and skips it. Nothing already set is overwritten, except
//! that a new App replaces the key of an App this run created, and a signer secret with no
//! enrolled key (so no one can use it) is replaced with a key that is enrolled.

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

use super::super::SetupArgs;
use crate::app::App;
use crate::cli::keygen;
use crate::github::{AppCredentials, EnvBranches, Github};
use crate::output::{BosunError, CmdResult, Outcome};

type Result<T> = std::result::Result<T, BosunError>;

const BRANCH: &str = "bosun/gate-app";
const APP_ENV: &str = "bosun-app";
const SIGNER_ENV: &str = "bosun-signer";
const CLIENT_VAR: &str = "BOSUN_APP_CLIENT_ID";
const APP_SECRET: &str = "BOSUN_APP_KEY";
const SIGNER_SECRET: &str = "BOSUN_SIGNER_KEY";
const GATE_APP: &str = ".bosun/gate-app.toml";
const APPROVERS: &str = ".bosun/approvers.toml";
const OWNER_PC: &str = "owner-computer";
const AGENT: &str = "agent";

struct Step {
    step: &'static str,
    device: &'static str,
    status: &'static str,
    detail: String,
}

impl Step {
    fn json(&self) -> Value {
        json!({"step": self.step, "device": self.device, "status": self.status,
            "detail": self.detail})
    }
}

/// `git` in the repo root; nothing it prints is passed on unfiltered.
struct Git {
    root: PathBuf,
    remote: String,
}

/// Drop `user:password@` from any URL in `s` (a remote may carry a token).
fn scrub(s: &str) -> String {
    let mut out = String::new();
    for word in s.split_inclusive(char::is_whitespace) {
        match word.find("://") {
            Some(i) if word[i + 3..].contains('@') => {
                let rest = &word[i + 3..];
                let at = rest.rfind('@').unwrap_or(0);
                out.push_str(&word[..i + 3]);
                out.push_str("[redacted]@");
                out.push_str(&rest[at + 1..]);
            }
            _ => out.push_str(word),
        }
    }
    out
}

impl Git {
    fn raw(&self, args: &[&str], stdin: Option<&str>, env: &[(&str, &OsStr)]) -> Result<Output> {
        let mut c = Command::new("git");
        c.arg("-C").arg(&self.root).args(args);
        c.env("GIT_TERMINAL_PROMPT", "0");
        for (k, v) in env {
            c.env(k, v);
        }
        c.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| {
            BosunError::error(
                format!("cannot run git: {}", e.kind()),
                "install git and run `bosun owner setup` inside the repo",
            )
        })?;
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            let _ = pipe.write_all(text.as_bytes());
        }
        child.wait_with_output().map_err(|e| {
            BosunError::error(
                format!("git {} failed: {}", args.first().unwrap_or(&""), e.kind()),
                "rerun `bosun owner setup`",
            )
        })
    }

    /// Trimmed stdout of a git command that must succeed.
    fn out(&self, args: &[&str], stdin: Option<&str>, env: &[(&str, &OsStr)]) -> Result<String> {
        let o = self.raw(args, stdin, env)?;
        if !o.status.success() {
            let why = String::from_utf8_lossy(&o.stderr);
            let why = scrub(why.lines().next().unwrap_or("").trim());
            return Err(BosunError::error(
                format!("git {} failed: {why}", args.first().unwrap_or(&"")),
                "check the repo and the remote, then rerun `bosun owner setup` (it resumes)",
            ));
        }
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    /// Trimmed stdout, `None` when the command fails or prints nothing.
    fn opt(&self, args: &[&str]) -> Option<String> {
        let o = self.raw(args, None, &[]).ok()?;
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        (o.status.success() && !s.is_empty()).then_some(s)
    }

    fn commit_of(&self, reference: &str) -> Option<String> {
        self.opt(&[
            "rev-parse",
            "--verify",
            "-q",
            &format!("{reference}^{{commit}}"),
        ])
    }

    fn show(&self, commit: &str, path: &str) -> Option<String> {
        let o = self
            .raw(&["show", &format!("{commit}:{path}")], None, &[])
            .ok()?;
        o.status
            .success()
            .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    }

    /// A commit on top of `parent` that sets `files`; the work tree and the real index are not
    /// touched (a private index file is built and removed).
    fn commit_files(&self, parent: &str, files: &[(&str, &str)], message: &str) -> Result<String> {
        let idx = self.out(
            &["rev-parse", "--git-path", "bosun-gate-app.index"],
            None,
            &[],
        )?;
        let idx = if Path::new(&idx).is_absolute() {
            PathBuf::from(idx)
        } else {
            self.root.join(idx)
        };
        let result = (|| {
            let env: [(&str, &OsStr); 1] = [("GIT_INDEX_FILE", idx.as_os_str())];
            self.out(&["read-tree", parent], None, &env)?;
            for (path, text) in files {
                let blob = self.out(&["hash-object", "-w", "--stdin"], Some(text), &[])?;
                self.out(
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        &format!("100644,{blob},{path}"),
                    ],
                    None,
                    &env,
                )?;
            }
            let tree = self.out(&["write-tree"], None, &env)?;
            let who = self.opt(&["config", "--get", "user.email"]).is_some()
                && self.opt(&["config", "--get", "user.name"]).is_some();
            let mut args = vec![];
            if !who {
                args.extend([
                    "-c",
                    "user.name=bosun",
                    "-c",
                    "user.email=bosun@users.noreply.github.com",
                ]);
            }
            args.extend(["commit-tree", &tree, "-p", parent, "-m", message]);
            self.out(&args, None, &[])
        })();
        let _ = std::fs::remove_file(&idx);
        result
    }

    fn set_branch(&self, sha: &str, old: Option<&str>) -> Result<()> {
        let name = format!("refs/heads/{BRANCH}");
        let mut args = vec!["update-ref", name.as_str(), sha];
        if let Some(old) = old {
            args.push(old);
        }
        self.out(&args, None, &[]).map(|_| ())
    }

    /// The sha of the branch on the remote; `None` when it is not there or the remote cannot be
    /// read (the push then says why).
    fn remote_sha(&self) -> Option<String> {
        let line = self.opt(&[
            "ls-remote",
            "--heads",
            "--",
            &self.remote,
            &format!("refs/heads/{BRANCH}"),
        ])?;
        line.split_whitespace().next().map(str::to_string)
    }

    /// A plain push: no `+`, no `--force`.
    fn push(&self) -> Result<()> {
        let spec = format!("refs/heads/{BRANCH}:refs/heads/{BRANCH}");
        let o = self.raw(&["push", "-q", "--", &self.remote, &spec], None, &[])?;
        if o.status.success() {
            return Ok(());
        }
        let all = String::from_utf8_lossy(&o.stderr);
        let rejected = all.contains("[rejected]")
            && (all.contains("non-fast-forward") || all.contains("fetch first"));
        let why = scrub(
            all.lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim(),
        );
        let message = format!("git push of {BRANCH} to {} failed: {why}", self.remote);
        let fix = format!(
            "everything on GitHub is set up; fix the remote or the branch, then rerun `bosun owner setup` (it only pushes). bosun never forces: if the remote {BRANCH} diverged, delete it there"
        );
        Err(if rejected {
            BosunError::conflict(message, fix)
        } else {
            BosunError::error(message, fix)
        })
    }
}

/// What a read of the world found.
struct State {
    local: Option<String>,
    parent: String,
    gate_app: Option<(u64, String)>,
    key_ids: Vec<String>,
    signer_enrolled: bool,
    envs_fix: [bool; 2],
    var_set: bool,
    app_secret: bool,
    signer_secret: bool,
}

fn parse_gate_app(text: &str) -> Option<(u64, String)> {
    let t: toml::Table = text.parse().ok()?;
    let id = u64::try_from(t.get("id")?.as_integer()?).ok()?;
    Some((id, t.get("slug")?.as_str()?.to_string()))
}

fn read_state(git: &Git, gh: &dyn Github) -> Result<State> {
    let local = git.commit_of(&format!("refs/heads/{BRANCH}"));
    let tracking = git.commit_of(&format!("refs/remotes/{}/{BRANCH}", git.remote));
    let base = [
        format!("refs/remotes/{}/main", git.remote),
        "refs/heads/main".to_string(),
        "HEAD".to_string(),
    ]
    .iter()
    .find_map(|r| git.commit_of(r))
    .ok_or_else(|| {
        BosunError::error(
            "the repo has no commit to build the branch on",
            "commit once on main, then rerun `bosun owner setup`",
        )
    })?;
    let parent = local.clone().or(tracking).unwrap_or(base.clone());
    // The App is recorded on the branch, or already on main (the PR was merged).
    let gate_app = [&parent, &base]
        .iter()
        .find_map(|c| git.show(c, GATE_APP))
        .and_then(|t| parse_gate_app(&t));
    // Enrolled on the branch, or already on main (the PR was merged): a stale local branch
    // must never make a rerun replace a working signer key.
    let mut key_ids: Vec<String> = Vec::new();
    let mut signer_enrolled = false;
    for (i, c) in [&parent, &base].iter().enumerate() {
        let text = git.show(c, APPROVERS).unwrap_or_default();
        let Ok(table) = text.parse::<toml::Table>() else {
            if i == 0 {
                return Err(BosunError::error(
                    format!("{APPROVERS} is not valid TOML"),
                    format!("fix {APPROVERS}, then rerun"),
                ));
            }
            continue;
        };
        let keys = table
            .get("key")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten();
        for k in keys {
            if let Some(id) = k.get("id").and_then(toml::Value::as_str) {
                key_ids.push(id.to_string());
            }
            signer_enrolled |= k.get("role").and_then(toml::Value::as_str) == Some("signer");
        }
    }

    let fix = |env: &str| -> Result<bool> {
        match gh.environment_branches(env)? {
            EnvBranches::Forbidden => Err(BosunError::needs_human(
                format!("the token cannot read environment {env}"),
                "use a token with Administration (repo) write, or ask the repo admin to run `bosun owner setup`",
            )),
            EnvBranches::Custom(v) if v == [("main".to_string(), "branch".to_string())] => {
                gh.environment_has_reviewers(env)
            }
            EnvBranches::Custom(v) if !v.is_empty() => Err(BosunError::needs_human(
                format!("environment {env} allows other branches than main"),
                format!(
                    "in the repo settings, Environments, {env}, remove every deployment branch except main, then rerun"
                ),
            )),
            _ => Ok(true),
        }
    };
    let envs_fix = [fix(APP_ENV)?, fix(SIGNER_ENV)?];
    let has = |env: &str, kind: &str, name: &str| -> Result<bool> {
        gh.exists(&format!("/environments/{env}/{kind}/{name}"))?
            .ok_or_else(|| {
                BosunError::needs_human(
                    format!("the token cannot read {kind} of environment {env}"),
                    "use a token with Secrets, Variables and Environments access",
                )
            })
    };
    Ok(State {
        local,
        parent,
        gate_app,
        key_ids,
        signer_enrolled,
        envs_fix,
        var_set: has(APP_ENV, "variables", CLIENT_VAR)?,
        app_secret: has(APP_ENV, "secrets", APP_SECRET)?,
        signer_secret: has(SIGNER_ENV, "secrets", SIGNER_SECRET)?,
    })
}

fn pick_signer_id(taken: &[String]) -> String {
    let mut id = "signer-ci".to_string();
    let mut n = 1;
    while taken.contains(&id) {
        n += 1;
        id = format!("signer-ci-{n}");
    }
    id
}

/// `approvers` with the block appended, checked to be valid TOML.
fn enrolled(approvers: &str, block: &str) -> Result<String> {
    let sep = if approvers.is_empty() {
        ""
    } else if approvers.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let joined = format!("{approvers}{sep}{block}");
    joined.parse::<toml::Table>().map_err(|_| {
        BosunError::error(
            format!("{APPROVERS} has `key` that is not a list of [[key]] tables"),
            format!("fix {APPROVERS} so each key is a [[key]] table, then rerun"),
        )
    })?;
    Ok(joined)
}

pub fn run(dir: Option<&Path>, a: &SetupArgs) -> CmdResult {
    if a.remote.starts_with('-') {
        return Err(BosunError::usage(
            "--remote must be a git remote name or URL, not an option",
            "pass the name of the remote that receives the branch, such as origin",
        ));
    }
    let app = App::load(dir)?;
    let git = Git {
        root: app.ctx.dir.clone(),
        remote: a.remote.clone(),
    };
    if git.opt(&["rev-parse", "--git-dir"]).is_none() {
        return Err(BosunError::usage(
            "this is not a git repo",
            "run `bosun owner setup` inside the repo (or pass -C <repo>)",
        ));
    }
    let gh: &dyn Github = &*app.gh;
    let st = read_state(&git, gh)?;
    let manual = a.app.manual;

    let need_creds = st.gate_app.is_none() || !st.var_set || !st.app_secret;
    let fresh = st.gate_app.is_none();
    let need_signer = !(st.signer_secret && st.signer_enrolled);
    let signer_id = pick_signer_id(&st.key_ids);

    if !a.dry_run {
        if need_creds && !manual && st.gate_app.is_some() {
            return Err(BosunError::needs_human(
                format!(
                    "the App is known but its key or client id is not set in the environment {APP_ENV}, and bosun cannot read the key back"
                ),
                "rerun with `bosun owner setup --manual --app-id <id> --client-id <Iv1...> --slug <slug> --key-file <pem> --delete-key-file` (generate a new key in the App settings first)",
            ));
        }
        if fresh && !manual && (st.var_set || st.app_secret) {
            return Err(BosunError::needs_human(
                format!(
                    "the environment {APP_ENV} already has the App key or client id but no App is recorded"
                ),
                "rerun with `--manual` and the App's id, client id, slug and key; bosun never replaces a key it cannot prove it should",
            ));
        }
    }

    let remote_before = git.remote_sha();
    let mut steps: Vec<Step> = Vec::new();

    if a.dry_run {
        let app_status = if need_creds { "would-do" } else { "skipped" };
        let detail = match (&st.gate_app, need_creds) {
            (None, _) if manual => "take the App you made by hand".to_string(),
            (None, _) => "create the App in your browser (manifest flow)".to_string(),
            (Some((id, _)), true) => format!(
                "App {id} is known; its key and client id need `--manual` (the key cannot be read back)"
            ),
            (Some((id, _)), false) => format!("App {id} is recorded and set up"),
        };
        steps.push(Step {
            step: "app",
            device: OWNER_PC,
            status: app_status,
            detail,
        });
        let envs_todo = st.envs_fix.iter().any(|f| *f);
        steps.push(Step {
            step: "environments",
            device: OWNER_PC,
            status: if envs_todo { "would-do" } else { "skipped" },
            detail: format!(
                "{APP_ENV} and {SIGNER_ENV}: deployment branch main only, no reviewers"
            ),
        });
        steps.push(Step {
            step: "variable-and-app-secret",
            device: OWNER_PC,
            status: if need_creds { "would-do" } else { "skipped" },
            detail: format!("{CLIENT_VAR} and the sealed {APP_SECRET} in {APP_ENV}"),
        });
        steps.push(Step {
            step: "signer-key",
            device: OWNER_PC,
            status: if need_signer { "would-do" } else { "skipped" },
            detail: format!(
                "generate in memory, seal {SIGNER_SECRET} into {SIGNER_ENV}, enroll in {APPROVERS}"
            ),
        });
        steps.push(Step {
            step: "delete-keys",
            device: OWNER_PC,
            status: if need_creds || need_signer { "would-do" } else { "skipped" },
            detail: "wipe every private key from memory; delete the key file only with --delete-key-file".into(),
        });
        let will_commit = fresh || need_signer;
        let push = will_commit || st.local.is_some() && st.local != remote_before;
        steps.push(Step {
            step: "push-branch",
            device: OWNER_PC,
            status: if push { "would-do" } else { "skipped" },
            detail: format!("plain push of {BRANCH} to {}", a.remote),
        });
        steps.extend(follow_ups());
        steps.extend(apply_steps("would-do"));
        return Ok(Outcome::data(json!({
            "dry_run": true,
            "app_id": st.gate_app.as_ref().map(|g| g.0),
            "slug": st.gate_app.as_ref().map(|g| g.1.clone()),
            "branch": BRANCH,
            "pushed": false,
            "steps": steps.iter().map(Step::json).collect::<Vec<_>>(),
        })));
    }

    // 1) The App.
    let mut creds: Option<AppCredentials> = None;
    let mut key_file_used = false;
    if need_creds {
        let (c, _) = super::credentials(dir, Some(&app), &a.app)?;
        if let Some((id, slug)) = &st.gate_app
            && (c.id != *id || c.slug != *slug)
        {
            return Err(BosunError::usage(
                format!("the App given is not the recorded App {id} ({slug})"),
                "pass the id, slug and key of the App named in .bosun/gate-app.toml",
            ));
        }
        let plain = |s: &str, extra: &[u8]| {
            !s.is_empty()
                && s.len() <= 100
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
        };
        if !plain(&c.slug, b"-") || !plain(&c.client_id, b"._-") {
            return Err(BosunError::error(
                "the App's slug or client id has an unexpected form",
                "rerun with `--manual` and the values from the App's settings page",
            ));
        }
        key_file_used = a.app.key_file.is_some();
        creds = Some(c);
    }
    let ids = match (&creds, &st.gate_app) {
        (Some(c), _) => Some((c.id, c.slug.clone())),
        (None, Some((i, s))) => Some((*i, s.clone())),
        (None, None) => None,
    };
    let client_id = creds.as_ref().map(|c| c.client_id.clone());
    let mut parent = st.parent.clone();
    let mut local = st.local.clone();
    let mut committed = false;
    if fresh && let Some(c) = &creds {
        let text = format!("id = {}\nslug = \"{}\"\n", c.id, c.slug);
        let sha = git.commit_files(
            &parent,
            &[(GATE_APP, &text)],
            "chore(bosun): record the gate App",
        )?;
        git.set_branch(&sha, local.as_deref())?;
        parent = sha.clone();
        local = Some(sha);
        committed = true;
    }
    steps.push(Step {
        step: "app",
        device: OWNER_PC,
        status: if need_creds { "done" } else { "skipped" },
        detail: match (&creds, &st.gate_app) {
            (Some(c), _) => format!("App {} ({}) recorded on {BRANCH}", c.id, c.slug),
            (None, Some((id, slug))) => format!("App {id} ({slug}) is recorded"),
            (None, None) => String::new(),
        },
    });

    // 2) Environments: main only, no reviewers.
    let mut changed = false;
    for (env, fix) in [APP_ENV, SIGNER_ENV].iter().zip(st.envs_fix) {
        if fix {
            gh.put_environment_main_only(env)?;
            changed = true;
        }
    }
    steps.push(Step {
        step: "environments",
        device: OWNER_PC,
        status: if changed { "done" } else { "skipped" },
        detail: format!("{APP_ENV} and {SIGNER_ENV}: deployment branch main only, no reviewers"),
    });

    // 3) The variable and the App key, sealed.
    let mut set_any = false;
    let mut app_secret_put = false;
    if let Some(c) = &creds {
        if !st.var_set || fresh {
            gh.put_environment_variable(APP_ENV, CLIENT_VAR, &c.client_id)?;
            set_any = true;
        }
        if fresh || !st.app_secret {
            gh.put_environment_secret(APP_ENV, APP_SECRET, &c.key)?;
            app_secret_put = true;
            set_any = true;
        }
    }
    steps.push(Step {
        step: "variable-and-app-secret",
        device: OWNER_PC,
        status: if set_any { "done" } else { "skipped" },
        detail: format!("{CLIENT_VAR} and the sealed {APP_SECRET} in {APP_ENV}"),
    });

    // 4) The signer key: generated here, sealed into its environment, its public half enrolled.
    let mut signer_public = None;
    if need_signer {
        let key = keygen::new_key()?;
        let block = keygen::key_block(&signer_id, &key.public, "signer");
        let text = git.show(&parent, APPROVERS).unwrap_or_default();
        let joined = enrolled(&text, &block)?;
        gh.put_environment_secret(SIGNER_ENV, SIGNER_SECRET, &key.pem)?;
        let sha = git.commit_files(
            &parent,
            &[(APPROVERS, &joined)],
            "chore(bosun): enroll the CI signer key",
        )?;
        git.set_branch(&sha, local.as_deref())?;
        local = Some(sha);
        committed = true;
        signer_public = Some(key.public.clone());
    }
    steps.push(Step {
        step: "signer-key",
        device: OWNER_PC,
        status: if need_signer { "done" } else { "skipped" },
        detail: format!(
            "{SIGNER_SECRET} sealed into {SIGNER_ENV}; public key enrolled in {APPROVERS}"
        ),
    });

    // 5) Keys: the ones held were only in memory; the owner's key file goes only on request.
    let held = creds.is_some() || need_signer;
    drop(creds);
    let mut detail = "no private key was written to disk".to_string();
    if a.delete_key_file
        && key_file_used
        && app_secret_put
        && let Some(p) = &a.app.key_file
    {
        std::fs::remove_file(p).map_err(|e| {
            BosunError::error(
                format!("cannot delete {}: {}", p.display(), e.kind()),
                "delete the key file yourself: the App key is already stored",
            )
        })?;
        detail = format!("{} deleted on request", p.display());
    }
    steps.push(Step {
        step: "delete-keys",
        device: OWNER_PC,
        status: if held { "done" } else { "skipped" },
        detail,
    });

    // 6) Push the branch (plain).
    let tip = local.clone();
    let mut pushed = false;
    if let Some(tip) = &tip
        && (committed || remote_before.as_deref() != Some(tip.as_str()))
    {
        git.push()?;
        pushed = true;
    }
    steps.push(Step {
        step: "push-branch",
        device: OWNER_PC,
        status: if pushed { "done" } else { "skipped" },
        detail: format!("plain push of {BRANCH} to {}", a.remote),
    });
    steps.extend(follow_ups());
    steps.extend(wait_and_apply(&app, &git)?);

    let mut data = json!({
        "dry_run": false,
        "app_id": ids.as_ref().map(|i| i.0),
        "slug": ids.as_ref().map(|i| i.1.clone()),
        "branch": BRANCH,
        "pushed": pushed,
        "steps": steps.iter().map(Step::json).collect::<Vec<_>>(),
    });
    if let Some(c) = client_id {
        data["client_id"] = json!(c);
    }
    if let Some(p) = signer_public {
        data["signer_id"] = json!(signer_id);
        data["signer_public_key"] = json!(p);
    }
    Ok(Outcome::data(data))
}

/// What happens after the push: an agent opens the PR, the owner approves it on GitHub.
fn follow_ups() -> Vec<Step> {
    vec![
        Step {
            step: "open-pr",
            device: AGENT,
            status: "pending",
            detail: format!("open a PR from {BRANCH} (bosun never merges this branch)"),
        },
        Step {
            step: "approve-enrollment",
            device: OWNER_PC,
            status: "pending",
            detail: "approve the PR yourself on GitHub: a PR cannot enroll its own key".into(),
        },
    ]
}

const WAIT_DEFAULT_SECS: u64 = 1800;
const POLL_DEFAULT_MS: u64 = 15_000;

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

fn apply_steps(status: &'static str) -> Vec<Step> {
    vec![
        Step {
            step: "wait-pr-a",
            device: OWNER_PC,
            status,
            detail: format!(
                "wait for PR A (branch {BRANCH}) to merge, at most {} minutes",
                WAIT_DEFAULT_SECS / 60
            ),
        },
        Step {
            step: "hoist-apply",
            device: OWNER_PC,
            status,
            detail: "run `hoist apply --area github --yes` to pin the ruleset to the App".into(),
        },
    ]
}

/// Has PR A (head `bosun/gate-app` of this repo) merged, or is the App already on main?
fn pr_a_merged(app: &App) -> Result<bool> {
    let gh: &dyn Github = &*app.gh;
    let repo = app.ctx.repo.to_string();
    let merged = gh
        .list_pulls(None, crate::github::IssueState::All)?
        .iter()
        .any(|p| {
            p.head_ref == BRANCH
                && (p.merged || p.merged_at.is_some())
                && p.head_repo.eq_ignore_ascii_case(&repo)
        });
    if merged {
        return Ok(true);
    }
    let base = crate::app::default_branch(gh)?;
    Ok(gh.file_at(GATE_APP, &base)?.is_some())
}

/// Poll for the merge of PR A (`BOSUN_OWNER_WAIT_SECS`, default 30 minutes; every
/// `BOSUN_OWNER_POLL_MS`, default 15 s), then run `hoist apply --area github --yes`.
fn wait_and_apply(app: &App, git: &Git) -> Result<Vec<Step>> {
    let wait = env_u64("BOSUN_OWNER_WAIT_SECS", WAIT_DEFAULT_SECS);
    let poll = std::time::Duration::from_millis(env_u64("BOSUN_OWNER_POLL_MS", POLL_DEFAULT_MS));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait);
    let mut merged = pr_a_merged(app)?;
    while !merged && std::time::Instant::now() < deadline {
        std::thread::sleep(poll.min(deadline.saturating_duration_since(std::time::Instant::now())));
        merged = pr_a_merged(app)?;
    }
    if !merged {
        let mut v = apply_steps("pending");
        v[0].detail = "PR A is not merged yet: ask an agent to run `bosun gate-app advance`, approve PR A on GitHub, then rerun `bosun owner setup` (it only waits and applies)".to_string();
        return Ok(v);
    }
    // The pinned hoist.toml is on main now: bring a clean checkout of main up to date.
    let on_main = git.opt(&["rev-parse", "--abbrev-ref", "HEAD"]).as_deref() == Some("main");
    if on_main && git.opt(&["status", "--porcelain"]).is_none() {
        let _ = git.raw(
            &["pull", "-q", "--ff-only", "--", &git.remote, "main"],
            None,
            &[],
        );
    }
    let bin = std::env::var("BOSUN_HOIST")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "hoist".into());
    // The token reaches hoist through the inherited environment, never argv; its stdout is
    // not ours (one JSON object on stdout).
    let out = Command::new(&bin)
        .arg("-C")
        .arg(&git.root)
        .args(["apply", "--area", "github", "--yes"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| {
            BosunError::needs_human(
                format!("cannot start {bin}: {}", e.kind()),
                "install hoist (`npm install -g @keelstackhq/hoist`) or set BOSUN_HOIST, then run `hoist apply --area github --yes` from the checkout of main",
            )
        })?;
    if !out.status.success() {
        let last = String::from_utf8_lossy(&out.stdout);
        let last = scrub(last.lines().last().unwrap_or("").trim());
        return Err(BosunError::error(
            format!(
                "hoist apply --area github failed (exit {:?}) {last}",
                out.status.code()
            ),
            "fix the hoist error above, then run `hoist apply --area github --yes` from the checkout of main (everything else is set up)",
        ));
    }
    let mut v = apply_steps("done");
    v[0].detail = "PR A is merged".into();
    v[1].detail = "hoist apply --area github --yes ran".into();
    Ok(v)
}
