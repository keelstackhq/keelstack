//! The `[github]` section of org.toml (spec §16.3, §16.4). Reads are GET only. The one write is
//! the provision environment, and only inside GitHub Actions with an app installation token.

use super::{SectionPlan, item, rejected, step};
use crate::auth::github_token;
use crate::error::HoistError;
use crate::http::{Fetched, Http, HttpError, Method, Service, seg};
use crate::report::Action;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provision {
    pub repo: String,
    #[serde(default = "provision")]
    pub environment: String,
    #[serde(default = "main")]
    pub branch: String,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub variables: Vec<String>,
}

fn provision() -> String {
    "provision".into()
}

fn main() -> String {
    "main".into()
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudEnvironment {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubCfg {
    pub org: String,
    #[serde(default)]
    pub apps: Vec<String>,
    pub provision: Option<Provision>,
    pub cloud_environment: Option<CloudEnvironment>,
}

pub(super) fn validate(cfg: &GithubCfg) -> Result<(), String> {
    if cfg.org.trim().is_empty() {
        return Err("[github] org must not be empty".into());
    }
    Ok(())
}

/// A client when a token is set (`unknown` items otherwise).
pub(super) fn client(verbose: bool) -> Option<Http> {
    github_token()
        .ok()
        .map(|t| Http::new(Service::Github, t).verbose(verbose))
}

/// Writes need an app installation token inside Actions (spec §16.4).
fn can_write(http: &Http) -> bool {
    std::env::var("GITHUB_ACTIONS").is_ok_and(|v| v == "true") && http.app_installation()
}

const WRITE_HINT: &str = "hint: the GitHub environment is written only inside GitHub Actions with an app installation token (GITHUB_ACTIONS=true, ghs_ token): run `hoist org apply --area github --yes` from the provision repo's workflow";

/// Create or fix the provision environment: one deployment branch, no reviewers.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EnvWrite {
    org: String,
    repo: String,
    env: String,
    branch: String,
    /// Live deployment branch policies `(id, name, type)`.
    policies: Vec<(u64, String, String)>,
}

impl EnvWrite {
    pub(crate) fn label(&self) -> String {
        format!(
            "write github/environment {}/{} (deployment branch {}, no reviewers)",
            self.repo, self.env, self.branch
        )
    }
}

fn why(f: Fetched, what: &str) -> String {
    match f {
        Fetched::Forbidden(h) => format!("cannot read {what}: {h}"),
        _ => format!("cannot read {what}: not found (404)"),
    }
}

/// Every item that needs the org, the repo or the token, `unknown` with the reason.
fn unknown_rest(p: &mut SectionPlan<EnvWrite>, cfg: &GithubCfg, reason: &str) {
    let mut add = |kind: &str, key: String| {
        p.items
            .push(item("github", kind, &key, Action::Unknown, reason));
    };
    for a in &cfg.apps {
        add("app", a.clone());
    }
    if let Some(pv) = &cfg.provision {
        add("repo", format!("{}/{}", cfg.org, pv.repo));
        let env = format!("{}/{}", pv.repo, pv.environment);
        add("environment", env.clone());
        for n in &pv.secrets {
            add("env_secret", format!("{env}/{n}"));
        }
        for n in &pv.variables {
            add("env_variable", format!("{env}/{n}"));
        }
    }
}

pub(crate) fn read(
    cfg: &GithubCfg,
    http: Option<&Http>,
) -> Result<SectionPlan<EnvWrite>, HoistError> {
    let mut p = SectionPlan::default();
    if let Some(c) = &cfg.cloud_environment {
        // No API reads it (H4): always listed, never changes the exit code.
        p.items.push(item(
            "github",
            "cloud_environment",
            &c.name,
            Action::Unknown,
            "no API reads it; confirm it by hand",
        ));
        p.steps.push(step(
            format!("github:cloud_environment:{}", c.name),
            format!(
                "confirm the cloud environment `{}` has the network secrets and allowed hosts (hoist cannot read it)",
                c.name
            ),
            "https://claude.ai/code",
            "confirm",
        ));
    }
    let org = &cfg.org;
    let Some(http) = http else {
        let reason = "no GitHub token (set GITHUB_TOKEN or GH_TOKEN, or run `gh auth login`)";
        p.items
            .push(item("github", "org", org, Action::Unknown, reason));
        unknown_rest(&mut p, cfg, reason);
        return Ok(p);
    };
    match http.get_json(&format!("/orgs/{}", seg(org)))? {
        Fetched::Ok(_) => p
            .items
            .push(item("github", "org", org, Action::Ok, "present")),
        Fetched::NotFound => {
            p.items.push(item(
                "github",
                "org",
                org,
                Action::Pending,
                "organization not found",
            ));
            p.steps.push(step(
                format!("github:org:{org}"),
                format!("create the GitHub organization {org} (hoist verifies it exists)"),
                "https://github.com/account/organizations/new",
                "pending",
            ));
            unknown_rest(&mut p, cfg, &format!("organization `{org}` not found"));
            return Ok(p);
        }
        other => {
            let reason = why(other, &format!("organization `{org}`"));
            p.items
                .push(item("github", "org", org, Action::Unknown, &reason));
            unknown_rest(&mut p, cfg, &reason);
            return Ok(p);
        }
    }
    read_apps(&mut p, cfg, http)?;
    if let Some(pv) = &cfg.provision {
        read_provision(&mut p, cfg, pv, http)?;
    }
    Ok(p)
}

fn read_apps(
    p: &mut SectionPlan<EnvWrite>,
    cfg: &GithubCfg,
    http: &Http,
) -> Result<(), HoistError> {
    if cfg.apps.is_empty() {
        return Ok(());
    }
    let path = format!("/orgs/{}/installations", seg(&cfg.org));
    let list = match http.get_all(&path, Some("installations"))? {
        Fetched::Ok(Value::Array(l)) => l,
        other => {
            let reason = why(other, "app installations");
            for a in &cfg.apps {
                p.items
                    .push(item("github", "app", a, Action::Unknown, &reason));
            }
            return Ok(());
        }
    };
    for slug in &cfg.apps {
        let selection = list
            .iter()
            .find(|i| i.get("app_slug").and_then(Value::as_str) == Some(slug))
            .map(|i| i.get("repository_selection").and_then(Value::as_str));
        let reason = match selection {
            Some(Some("all")) => {
                p.items.push(item(
                    "github",
                    "app",
                    slug,
                    Action::Ok,
                    "installed on all repositories",
                ));
                continue;
            }
            Some(_) => "installed on selected repositories only",
            None => "not installed",
        };
        p.items
            .push(item("github", "app", slug, Action::Pending, reason));
        p.steps.push(step(
            format!("github:app:{slug}"),
            format!(
                "install the GitHub App {slug} on all repositories of {} (hoist verifies the installation)",
                cfg.org
            ),
            &format!(
                "https://github.com/organizations/{}/settings/installations",
                cfg.org
            ),
            "pending",
        ));
    }
    Ok(())
}

fn read_provision(
    p: &mut SectionPlan<EnvWrite>,
    cfg: &GithubCfg,
    pv: &Provision,
    http: &Http,
) -> Result<(), HoistError> {
    let org = &cfg.org;
    let repo_key = format!("{org}/{}", pv.repo);
    let repo_path = format!("/repos/{}/{}", seg(org), seg(&pv.repo));
    let env_key = format!("{}/{}", pv.repo, pv.environment);
    let skip = |p: &mut SectionPlan<EnvWrite>, reason: &str| {
        p.items.push(item(
            "github",
            "environment",
            &env_key,
            Action::Unknown,
            reason,
        ));
        for n in &pv.secrets {
            let key = format!("{env_key}/{n}");
            p.items
                .push(item("github", "env_secret", &key, Action::Unknown, reason));
        }
        for n in &pv.variables {
            let key = format!("{env_key}/{n}");
            p.items.push(item(
                "github",
                "env_variable",
                &key,
                Action::Unknown,
                reason,
            ));
        }
    };
    match http.get_json(&repo_path)? {
        Fetched::Ok(_) => p
            .items
            .push(item("github", "repo", &repo_key, Action::Ok, "present")),
        Fetched::NotFound => {
            p.items.push(item(
                "github",
                "repo",
                &repo_key,
                Action::Pending,
                "repository not found",
            ));
            p.steps.push(step(
                format!("github:repo:{repo_key}"),
                format!("create the repository {repo_key} (hoist verifies it exists)"),
                &format!("https://github.com/organizations/{org}/repositories/new"),
                "pending",
            ));
            skip(p, &format!("repository `{repo_key}` not found"));
            return Ok(());
        }
        other => {
            let reason = why(other, &format!("repository `{repo_key}`"));
            p.items
                .push(item("github", "repo", &repo_key, Action::Unknown, &reason));
            skip(p, &reason);
            return Ok(());
        }
    }
    let env_path = format!("{repo_path}/environments/{}", seg(&pv.environment));
    let mut policies = Vec::new();
    let mut exists = false;
    let (action, reason) = match http.get_json(&env_path)? {
        Fetched::NotFound => (Action::Create, "environment is missing".to_string()),
        Fetched::Ok(env) => {
            exists = true;
            let mut problems = Vec::new();
            let dbp = env.get("deployment_branch_policy");
            let flag = |k: &str| dbp.and_then(|d| d.get(k)).and_then(Value::as_bool);
            if flag("custom_branch_policies") != Some(true)
                || flag("protected_branches") != Some(false)
            {
                problems.push("deployment branches are not limited to custom policies");
            }
            let reviewers = env
                .get("protection_rules")
                .and_then(Value::as_array)
                .is_some_and(|r| {
                    r.iter().any(|x| {
                        x.get("type").and_then(Value::as_str) == Some("required_reviewers")
                    })
                });
            if reviewers {
                problems.push("required reviewers are set");
            }
            match http.get_all(
                &format!("{env_path}/deployment-branch-policies"),
                Some("branch_policies"),
            )? {
                Fetched::Ok(Value::Array(list)) => {
                    policies = list
                        .iter()
                        .filter_map(|x| {
                            Some((
                                x.get("id")?.as_u64()?,
                                x.get("name")?.as_str()?.to_string(),
                                x.get("type")
                                    .and_then(Value::as_str)
                                    .unwrap_or("branch")
                                    .to_string(),
                            ))
                        })
                        .collect();
                    if !(policies.len() == 1
                        && policies[0].1 == pv.branch
                        && policies[0].2 == "branch")
                    {
                        problems.push("deployment branches are not exactly the one branch");
                    }
                }
                other => {
                    let reason = why(other, "deployment branch policies");
                    p.items.push(item(
                        "github",
                        "environment",
                        &env_key,
                        Action::Unknown,
                        reason,
                    ));
                    return read_names(p, pv, http, &env_path, &env_key);
                }
            }
            if problems.is_empty() {
                (Action::Ok, "present".to_string())
            } else {
                (Action::Update, problems.join("; "))
            }
        }
        other => {
            let reason = why(other, &format!("environment `{env_key}`"));
            skip(p, &reason);
            return Ok(());
        }
    };
    p.items
        .push(item("github", "environment", &env_key, action, reason));
    if action != Action::Ok {
        if can_write(http) {
            p.writes.push(EnvWrite {
                org: org.clone(),
                repo: pv.repo.clone(),
                env: pv.environment.clone(),
                branch: pv.branch.clone(),
                policies,
            });
        } else {
            p.hint = Some(WRITE_HINT.into());
        }
    }
    if !exists {
        let reason = "environment does not exist yet";
        for n in &pv.secrets {
            let key = format!("{env_key}/{n}");
            p.items
                .push(item("github", "env_secret", &key, Action::Unknown, reason));
        }
        for n in &pv.variables {
            let key = format!("{env_key}/{n}");
            p.items.push(item(
                "github",
                "env_variable",
                &key,
                Action::Unknown,
                reason,
            ));
        }
        return Ok(());
    }
    read_names(p, pv, http, &env_path, &env_key)
}

/// Names of the environment's secrets and variables: absent ones are human steps.
fn read_names(
    p: &mut SectionPlan<EnvWrite>,
    pv: &Provision,
    http: &Http,
    env_path: &str,
    env_key: &str,
) -> Result<(), HoistError> {
    for (kind, list, wanted) in [
        ("env_secret", "secrets", &pv.secrets),
        ("env_variable", "variables", &pv.variables),
    ] {
        if wanted.is_empty() {
            continue;
        }
        let names: Result<Vec<String>, String> =
            match http.get_all(&format!("{env_path}/{list}"), Some(list))? {
                Fetched::Ok(Value::Array(l)) => Ok(l
                    .iter()
                    .filter_map(|x| x.get("name").and_then(Value::as_str).map(str::to_string))
                    .collect()),
                other => Err(why(other, &format!("environment {list}"))),
            };
        for n in wanted {
            let key = format!("{env_key}/{n}");
            match &names {
                Err(reason) => p
                    .items
                    .push(item("github", kind, &key, Action::Unknown, reason)),
                Ok(have) if have.contains(n) => {
                    p.items
                        .push(item("github", kind, &key, Action::Ok, "present"))
                }
                Ok(_) => {
                    p.items
                        .push(item("github", kind, &key, Action::Pending, "not set"));
                    p.steps.push(step(
                        format!("github:{kind}:{key}"),
                        format!(
                            "set the {} {n} in environment {} (hoist verifies the name; it never reads or writes a value)",
                            kind.trim_start_matches("env_"),
                            pv.environment
                        ),
                        "",
                        "pending",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// PUT the environment, ensure its one branch policy, delete the others.
pub(crate) fn apply(
    http: &Http,
    writes: &[EnvWrite],
) -> Result<Vec<crate::report::Item>, HoistError> {
    let mut failed = Vec::new();
    for w in writes {
        let env = format!(
            "/repos/{}/{}/environments/{}",
            seg(&w.org),
            seg(&w.repo),
            seg(&w.env)
        );
        let body = json!({"deployment_branch_policy": {
            "protected_branches": false, "custom_branch_policies": true}});
        let label = w.label();
        let put = http.send_json(Method::Put, &env, Some(&body));
        if let Some(f) = rejected("github", &label, put)? {
            failed.push(f);
            continue;
        }
        let path = format!("{env}/deployment-branch-policies");
        if !w
            .policies
            .iter()
            .any(|(_, n, t)| *n == w.branch && t == "branch")
        {
            let policy = json!({"name": w.branch, "type": "branch"});
            let post = match http.send_json(Method::Post, &path, Some(&policy)) {
                // 303: the policy exists; writes never follow the redirect.
                Err(HttpError::Rejected { status: 303, .. }) => continue_ok(),
                r => r,
            };
            failed.extend(rejected("github", &label, post)?);
        }
        for (id, name, ty) in &w.policies {
            if *name == w.branch && ty == "branch" {
                continue;
            }
            let del = http.send_json(Method::Delete, &format!("{path}/{id}"), None);
            failed.extend(rejected("github", &label, del)?);
        }
    }
    Ok(failed)
}

fn continue_ok() -> Result<crate::http::Written, HttpError> {
    Ok(crate::http::Written {
        status: 303,
        body: None,
    })
}
