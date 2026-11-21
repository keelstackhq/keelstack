//! GitHub writes (spec §6): one `Change` becomes one or two API calls. Every URL segment that
//! comes from config or the repo goes through `seg()`.

use crate::auth::Secret;
use crate::change::{Change, Reviewers};
use crate::desired::{ACTIONS_APP_ID, ADMIN_ROLE_ID, Desired, RULESET_NAME, TAG_RULESET_NAME};
use crate::error::HoistError;
use crate::github::plan_limited;
use crate::http::{Fetched, Http, HttpError, Method, seg};
use crate::seal::put_home;
use serde_json::{Map, Value, json};

/// Apply one GitHub change.
pub fn execute_github(http: &Http, d: &Desired, change: &Change) -> Result<(), HoistError> {
    let repo = format!("/repos/{}/{}", seg(&d.repo.owner), seg(&d.repo.name));
    match change {
        Change::RepoSettings { patch } => {
            // Immutable releases are their own endpoint: PUT enables, DELETE disables.
            let mut patch = patch.clone();
            if let Some(on) = patch.remove("immutable_releases").and_then(|v| v.as_bool()) {
                let method = if on { Method::Put } else { Method::Delete };
                http.send_json(method, &format!("{repo}/immutable-releases"), None)?;
            }
            if patch.is_empty() {
                return Ok(());
            }
            send(http, Method::Patch, &repo, &Value::Object(patch))
        }
        Change::Ruleset {
            existing_id,
            branch: _,
            required_checks,
            merge_queue,
        } => {
            let body = ruleset_body(required_checks, *merge_queue);
            let sent = match existing_id {
                Some(id) => {
                    let path = format!("{repo}/rulesets/{}", seg(&id.to_string()));
                    http.send_json(Method::Put, &path, Some(&body))
                }
                None => http.send_json(Method::Post, &format!("{repo}/rulesets"), Some(&body)),
            };
            match sent {
                Ok(_) => Ok(()),
                Err(HttpError::Rejected {
                    status: 403,
                    message,
                    ..
                }) if plan_limited(&message) => Err(HoistError::PlanLimited(message)),
                Err(err) => Err(err.into()),
            }
        }
        Change::TagRuleset {
            existing_id,
            patterns,
        } => write_tag_ruleset(http, &repo, *existing_id, patterns),
        Change::ActionsPermissions { default } => send(
            http,
            Method::Put,
            &format!("{repo}/actions/permissions/workflow"),
            &json!({ "default_workflow_permissions": default }),
        ),
        Change::Environment {
            name,
            branches,
            write_env,
            exists,
            release,
            reviewers,
        } => {
            let env = format!("{repo}/environments/{}", seg(name));
            // No branches: no restriction at all (`null`), as for an environment made by hand.
            let mut body = if branches.is_empty() {
                json!({"deployment_branch_policy": null})
            } else {
                json!({"deployment_branch_policy":{
                    "protected_branches": false,
                    "custom_branch_policies": true
                }})
            };
            if let (true, Reviewers::Users(ids)) = (*release, reviewers) {
                // The release environment gates publishing: its reviewers approve it.
                let users: Vec<Value> = ids.iter().map(|i| json!({"type":"User","id":i})).collect();
                body["reviewers"] = Value::Array(users);
            }
            if *write_env && *exists {
                carry_back(http, &env, &mut body)?;
            }
            let mut limited = None;
            let put = if *write_env {
                http.send_json(Method::Put, &env, Some(&body))
            } else {
                Ok(crate::http::Written {
                    status: 200,
                    body: None,
                })
            };
            match put {
                Ok(_) => {}
                // The plan has no required reviewers (private repo): still create the
                // environment and its branch policy, then report the gap. Only a 422 that
                // says so; any other 422 is a real error.
                Err(HttpError::Rejected {
                    status: 422,
                    message,
                    ..
                }) if *release && reviewers_unavailable(&message) => {
                    body.as_object_mut().map(|o| o.remove("reviewers"));
                    send(http, Method::Put, &env, &body)?;
                    limited = Some(message);
                }
                Err(err) => return Err(err.into()),
            }
            let path = format!("{env}/deployment-branch-policies");
            for branch in branches {
                let policy = json!({"name": branch, "type": "branch"});
                match http.send_json(Method::Post, &path, Some(&policy)) {
                    Ok(_) => {}
                    // 303: the policy exists (GitHub redirects to it); writes never follow it.
                    Err(HttpError::Rejected { status: 303, .. }) => {}
                    Err(HttpError::Rejected {
                        status: 422,
                        ref message,
                        ..
                    }) if message.to_lowercase().contains("already exists") => {}
                    Err(err) => return Err(err.into()),
                }
            }
            limited.map_or(Ok(()), |m| Err(HoistError::PlanLimited(m)))
        }
        Change::Variable {
            name,
            value,
            exists,
        } => {
            let body = json!({"name": name, "value": value});
            if *exists {
                send(
                    http,
                    Method::Patch,
                    &format!("{repo}/actions/variables/{}", seg(name)),
                    &body,
                )
            } else {
                send(
                    http,
                    Method::Post,
                    &format!("{repo}/actions/variables"),
                    &body,
                )
            }
        }
        Change::AccountIdSecret => {
            let account = d.cloudflare.account_id.as_ref().ok_or_else(|| {
                HoistError::Usage(
                    "no Cloudflare account id to write into CLOUDFLARE_ACCOUNT_ID; set \
                     [cloudflare] account_id in hoist.toml or CLOUDFLARE_ACCOUNT_ID"
                        .into(),
                )
            })?;
            put_home(
                http,
                d,
                "CLOUDFLARE_ACCOUNT_ID",
                &Secret::from(account.value.as_str()),
            )
        }
        _ => Err(HoistError::Usage("not a GitHub change".into())),
    }
}

/// A PUT resets what it omits: read the existing environment and keep its wait timer,
/// self-review rule and reviewers (a `reviewers` already in `body` is the release environment's
/// resolved approvers and wins). An unreadable environment is not rewritten blind.
fn carry_back(http: &Http, env: &str, body: &mut Value) -> Result<(), HoistError> {
    let live = match http.get_json(env)? {
        Fetched::Ok(v) => v,
        Fetched::NotFound => return Ok(()),
        Fetched::Forbidden(hint) => {
            return Err(HoistError::Usage(format!(
                "cannot read {env} before changing it ({hint}); hoist will not rewrite an \
                 environment blind, since that could reset its reviewers: fix the token's \
                 Environments permission"
            )));
        }
    };
    let rules = live["protection_rules"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for rule in &rules {
        match rule["type"].as_str() {
            Some("wait_timer") => {
                if let Some(n) = rule["wait_timer"].as_u64() {
                    body["wait_timer"] = json!(n);
                }
            }
            Some("required_reviewers") => {
                if let Some(b) = rule["prevent_self_review"].as_bool() {
                    body["prevent_self_review"] = json!(b);
                }
                if body.get("reviewers").is_none() {
                    let who: Vec<Value> = rule["reviewers"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|r| {
                            Some(json!({"type": r["type"].as_str()?, "id": r["reviewer"]["id"].as_u64()?}))
                        })
                        .collect();
                    if !who.is_empty() {
                        body["reviewers"] = Value::Array(who);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Whether a 422 on the environment PUT says the plan allows no required reviewers.
fn reviewers_unavailable(message: &str) -> bool {
    let m = message.to_lowercase();
    m.contains("reviewer") || m.contains("protection rule")
}

/// Who approves the release environment, resolved in the apply preflight before any write:
/// the `[release] reviewers` logins (`GET /users/<login>`), else the token's user
/// (`GET /user`), unless that is a bot: a bot must not be the sole approver.
pub fn resolve_reviewers(http: &Http, logins: &[String]) -> Result<Reviewers, HoistError> {
    if logins.is_empty() {
        return match http.get_json("/user")? {
            Fetched::Ok(v) if v["id"].is_u64() => {
                let login = v["login"].as_str().unwrap_or_default();
                if v["type"] == "Bot" || login.ends_with("[bot]") {
                    Ok(Reviewers::HumanNeeded)
                } else {
                    Ok(Reviewers::Users(vec![v["id"].as_u64().unwrap_or_default()]))
                }
            }
            _ => Err(HoistError::Usage(
                "cannot name a required reviewer for the release environment: GET /user did not \
                 return a user; apply with a user token (an App installation token has no user) \
                 or set [release] reviewers in hoist.toml"
                    .into(),
            )),
        };
    }
    let mut ids = Vec::new();
    for login in logins {
        match http.get_json(&format!("/users/{}", seg(login)))? {
            Fetched::Ok(v) if v["id"].is_u64() => ids.push(v["id"].as_u64().unwrap_or_default()),
            _ => {
                return Err(HoistError::Usage(format!(
                    "[release] reviewers: `{login}` is not a GitHub user this token can read \
                     (GET /users/{login}); fix the login in hoist.toml"
                )));
            }
        }
    }
    Ok(Reviewers::Users(ids))
}

fn send(http: &Http, method: Method, path: &str, body: &Value) -> Result<(), HoistError> {
    http.send_json(method, path, Some(body))?;
    Ok(())
}

/// The merge queue settings hoist writes.
pub fn merge_queue_parameters() -> Value {
    json!({
        "merge_method": "MERGE",
        "grouping_strategy": "ALLGREEN",
        "max_entries_to_build": 5,
        "min_entries_to_merge": 1,
        "max_entries_to_merge": 5,
        "min_entries_to_merge_wait_minutes": 5,
        "check_response_timeout_minutes": 60
    })
}

/// The rule types of the `hoist: tags` ruleset, in the order a new one lists them.
pub const TAG_RULES: [&str; 3] = ["creation", "update", "deletion"];

fn actions_app_actor() -> Value {
    json!({
        "actor_id": ACTIONS_APP_ID,
        "actor_type": "Integration",
        "bypass_mode": "always"
    })
}

/// The `hoist: tags` ruleset: the GitHub Actions app (github-actions[bot]) bypasses the
/// creation, update and deletion rules on the matching tags.
fn tag_ruleset_body(patterns: &[String]) -> Value {
    let include: Vec<String> = patterns.iter().map(|p| format!("refs/tags/{p}")).collect();
    let rules: Vec<Value> = TAG_RULES.iter().map(|t| json!({ "type": t })).collect();
    json!({
        "name": TAG_RULESET_NAME,
        "target": "tag",
        "enforcement": "active",
        "conditions": {"ref_name": {"include": include, "exclude": []}},
        "rules": rules,
        "bypass_actors": [actions_app_actor()]
    })
}

/// Read-merge-write of an existing `hoist: tags` ruleset: the PUT replaces `rules`,
/// `conditions` and `bypass_actors` whole, so the live ones are written back with only hoist's
/// parts changed: enforcement `active`, target `tag`, the wanted tag includes added, excludes
/// cleared, missing rules added (live rules keep their parameters), and the Actions app's
/// bypass mode `always`. Owner-added includes, rules and bypass actors stay.
fn merged_tag_ruleset_body(live: &Value, patterns: &[String]) -> Value {
    let fresh = tag_ruleset_body(patterns);
    let list = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
    let mut include = list(live.pointer("/conditions/ref_name/include"));
    for want in list(fresh.pointer("/conditions/ref_name/include")) {
        if !include.contains(&want) {
            include.push(want);
        }
    }
    let mut rules = list(live.get("rules"));
    for want in TAG_RULES {
        if !rules.iter().any(|r| r["type"] == want) {
            rules.push(json!({ "type": want }));
        }
    }
    let mut bypass = list(live.get("bypass_actors"));
    let is_app = |b: &Value| {
        b["actor_type"] == "Integration" && b["actor_id"].as_u64() == Some(ACTIONS_APP_ID)
    };
    match bypass.iter_mut().find(|b| is_app(b)) {
        Some(b) => *b = actions_app_actor(),
        None => bypass.push(actions_app_actor()),
    }
    let mut conditions = live
        .get("conditions")
        .filter(|c| c.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    conditions["ref_name"] = json!({"include": include, "exclude": []});
    json!({
        "name": TAG_RULESET_NAME,
        "target": "tag",
        "enforcement": "active",
        "conditions": conditions,
        "rules": rules,
        "bypass_actors": bypass
    })
}

/// The `hoist: tags` write: a POST for a new ruleset; for an existing one a GET first, then a
/// PUT of [`merged_tag_ruleset_body`]. A ruleset gone since the plan is created again; an
/// unreadable one is not rewritten blind.
fn write_tag_ruleset(
    http: &Http,
    repo: &str,
    existing_id: Option<u64>,
    patterns: &[String],
) -> Result<(), HoistError> {
    let create = || {
        http.send_json(
            Method::Post,
            &format!("{repo}/rulesets"),
            Some(&tag_ruleset_body(patterns)),
        )
    };
    let sent = match existing_id {
        None => create(),
        Some(id) => {
            let path = format!("{repo}/rulesets/{}", seg(&id.to_string()));
            match http.get_json(&path)? {
                Fetched::Ok(live) => {
                    let body = merged_tag_ruleset_body(&live, patterns);
                    http.send_json(Method::Put, &path, Some(&body))
                }
                Fetched::NotFound => create(),
                Fetched::Forbidden(hint) => {
                    return Err(HoistError::Usage(format!(
                        "cannot read {path} before changing it ({hint}); hoist will not \
                         rewrite the `{TAG_RULESET_NAME}` ruleset blind, since that could drop \
                         what the owner added: fix the token's Administration permission"
                    )));
                }
            }
        }
    };
    match sent {
        Ok(_) => Ok(()),
        Err(HttpError::Rejected {
            status: 403,
            message,
            ..
        }) if plan_limited(&message) => Err(HoistError::PlanLimited(message)),
        Err(err) => Err(err.into()),
    }
}

fn ruleset_body(required_checks: &[String], merge_queue: bool) -> Value {
    let checks: Vec<Value> = required_checks
        .iter()
        .map(|c| json!({ "context": c }))
        .collect();
    // No deletion and no force push of the default branch; status checks after them.
    let mut rules = vec![
        json!({"type": "deletion"}),
        json!({"type": "non_fast_forward"}),
        json!({
            "type": "required_status_checks",
            "parameters": {
                "strict_required_status_checks_policy": false,
                "required_status_checks": checks
            }
        }),
    ];
    if merge_queue {
        rules.push(json!({
            "type": "merge_queue",
            "parameters": merge_queue_parameters()
        }));
    }
    let mut body = Map::new();
    body.insert("name".into(), json!(RULESET_NAME));
    body.insert("target".into(), json!("branch"));
    body.insert("enforcement".into(), json!("active"));
    body.insert(
        "conditions".into(),
        json!({"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}}),
    );
    body.insert("rules".into(), Value::Array(rules));
    // The repository admin role may merge a PR past the rules (a mainfix), never push.
    body.insert(
        "bypass_actors".into(),
        json!([{
            "actor_id": ADMIN_ROLE_ID,
            "actor_type": "RepositoryRole",
            "bypass_mode": "pull_request"
        }]),
    );
    Value::Object(body)
}
