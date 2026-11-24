//! GitHub writes (spec §6): one `Change` becomes one or two API calls. Every URL segment that
//! comes from config or the repo goes through `seg()`.

use crate::auth::Secret;
use crate::change::{Change, Reviewers};
use crate::config::TagsBypass;
use crate::desired::{
    ACTIONS_APP_ID, ADMIN_ROLE_ID, Desired, RULESET_NAME, TAG_KEY_SECRET, TAG_KEY_TITLE,
    TAG_RULESET_NAME,
};
use crate::error::HoistError;
use crate::github::plan_limited;
use crate::http::{Fetched, Http, HttpError, Method, seg};
use crate::report::HumanStep;
use crate::seal::{put_env_secret, put_home};
use serde_json::{Map, Value, json};

/// Apply one GitHub change. `Some(step)` is a human step instead of a write: the organization
/// disallows deploy keys, so the tag deploy key (and its ruleset) wait for a person.
pub fn execute_github(
    http: &Http,
    d: &Desired,
    change: &Change,
) -> Result<Option<HumanStep>, HoistError> {
    if let Change::TagDeployKey { env } = change {
        let repo = format!("/repos/{}/{}", seg(&d.repo.owner), seg(&d.repo.name));
        return write_tag_key(http, d, &repo, env);
    }
    execute_github_plain(http, d, change).map(|()| None)
}

fn execute_github_plain(http: &Http, d: &Desired, change: &Change) -> Result<(), HoistError> {
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
            admin_bypass,
        } => {
            crate::diff::ensure_pins_resolved(d)?;
            let pins: Vec<(String, u64)> = d
                .github
                .ruleset
                .check_pins
                .iter()
                .filter_map(|p| Some((p.context.clone(), p.id?)))
                .collect();
            let mut fresh = ruleset_body(required_checks, *merge_queue, *admin_bypass);
            pin_checks(&mut fresh, &pins);
            let merge = |live: &Value| {
                let mut body =
                    merged_ruleset_body(live, required_checks, *merge_queue, *admin_bypass);
                pin_checks(&mut body, &pins);
                body
            };
            write_ruleset(http, &repo, *existing_id, RULESET_NAME, &fresh, merge)
        }
        Change::TagRuleset {
            existing_id,
            patterns,
            bypass,
        } => {
            let fresh = tag_ruleset_body(patterns, *bypass);
            let merge = |live: &Value| merged_tag_ruleset_body(live, patterns, *bypass);
            write_ruleset(http, &repo, *existing_id, TAG_RULESET_NAME, &fresh, merge)
        }
        Change::ActionsPermissions { default } => send(
            http,
            Method::Put,
            &format!("{repo}/actions/permissions/workflow"),
            &json!({ "default_workflow_permissions": default }),
        ),
        Change::Environment {
            name,
            branches,
            tags,
            write_env,
            exists,
            release,
            reviewers,
        } => {
            let env = format!("{repo}/environments/{}", seg(name));
            // No branches: no restriction at all (`null`), as for an environment made by hand.
            let mut body = if branches.is_empty() && tags.is_empty() {
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
            let all = branches
                .iter()
                .map(|b| (b, "branch"))
                .chain(tags.iter().map(|t| (t, "tag")));
            for (branch, kind) in all {
                let policy = json!({"name": branch, "type": kind});
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

/// GitHub's `DeployKey` actor: every write deploy key of the repo; it takes no id (null).
fn deploy_keys_actor() -> Value {
    json!({
        "actor_id": null,
        "actor_type": "DeployKey",
        "bypass_mode": "always"
    })
}

fn admin_actor() -> Value {
    json!({
        "actor_id": ADMIN_ROLE_ID,
        "actor_type": "RepositoryRole",
        "bypass_mode": "pull_request"
    })
}

/// The bypass actor hoist itself wrote before `tags_bypass = "deploy_key"`: the GitHub
/// Actions app (`Integration` 15368). Every other actor was added by the owner.
fn is_actions_app(b: &Value) -> bool {
    b["actor_type"] == "Integration" && b["actor_id"].as_u64() == Some(ACTIONS_APP_ID)
}

fn tag_bypass_actor(bypass: TagsBypass) -> Value {
    match bypass {
        TagsBypass::Actions => actions_app_actor(),
        TagsBypass::DeployKey => deploy_keys_actor(),
    }
}

/// The `hoist: tags` ruleset: the bypass actor (the GitHub Actions app, or with
/// `tags_bypass = "deploy_key"` the repo's write deploy keys) bypasses the creation, update and
/// deletion rules on the matching tags.
fn tag_ruleset_body(patterns: &[String], bypass: TagsBypass) -> Value {
    let include: Vec<String> = patterns.iter().map(|p| format!("refs/tags/{p}")).collect();
    let rules: Vec<Value> = TAG_RULES.iter().map(|t| json!({ "type": t })).collect();
    json!({
        "name": TAG_RULESET_NAME,
        "target": "tag",
        "enforcement": "active",
        "conditions": {"ref_name": {"include": include, "exclude": []}},
        "rules": rules,
        "bypass_actors": [tag_bypass_actor(bypass)]
    })
}

/// Read-merge-write of an existing `hoist: tags` ruleset: the PUT replaces `rules`,
/// `conditions` and `bypass_actors` whole, so the live ones are written back with only hoist's
/// parts changed: enforcement `active`, target `tag`, the wanted tag includes added, excludes
/// cleared, missing rules added (live rules keep their parameters), and hoist's bypass actor
/// written once in mode `always`. In `actions` mode that is the Actions app (a duplicate is
/// dropped); in `deploy_key` mode the deploy keys, and the Actions app (hoist's own earlier
/// actor) is removed. Owner-added includes, rules and bypass actors stay.
fn merged_tag_ruleset_body(live: &Value, patterns: &[String], bypass: TagsBypass) -> Value {
    let fresh = tag_ruleset_body(patterns, bypass);
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
    let mut actors = list(live.get("bypass_actors"));
    let first_app = actors.iter().position(is_actions_app);
    let is_keys = |b: &Value| b["actor_type"] == "DeployKey";
    match bypass {
        TagsBypass::Actions => {
            // Keep the app's place in the list, once.
            let mut seen = false;
            actors.retain(|b| {
                if !is_actions_app(b) {
                    return true;
                }
                let keep = !seen;
                seen = true;
                keep
            });
            match first_app {
                Some(i) => actors[i] = actions_app_actor(),
                None => actors.push(actions_app_actor()),
            }
        }
        TagsBypass::DeployKey => {
            actors.retain(|b| !is_actions_app(b) && !is_keys(b));
            actors.push(deploy_keys_actor());
        }
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
        "bypass_actors": actors
    })
}

/// A ruleset write: a POST of `fresh` for a new ruleset; for an existing one a GET first, then
/// a PUT of `merge(live)` (never a blind full replace). A ruleset gone since the plan is
/// created again; an unreadable one is not rewritten.
fn write_ruleset(
    http: &Http,
    repo: &str,
    existing_id: Option<u64>,
    name: &str,
    fresh: &Value,
    merge: impl Fn(&Value) -> Value,
) -> Result<(), HoistError> {
    let create = || http.send_json(Method::Post, &format!("{repo}/rulesets"), Some(fresh));
    let sent = match existing_id {
        None => create(),
        Some(id) => {
            let path = format!("{repo}/rulesets/{}", seg(&id.to_string()));
            match http.get_json(&path)? {
                Fetched::Ok(live) => http.send_json(Method::Put, &path, Some(&merge(&live))),
                Fetched::NotFound => create(),
                Fetched::Forbidden(hint) => {
                    return Err(HoistError::Usage(format!(
                        "cannot read {path} before changing it ({hint}); hoist will not \
                         rewrite the `{name}` ruleset blind, since that could drop what the \
                         owner added: fix the token's Administration permission"
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

/// Whether a rejected `POST /keys` says deploy keys are not allowed here.
fn deploy_keys_disallowed(message: &str) -> bool {
    message.to_lowercase().contains("deploy key")
}

/// The human step id for an organization that disallows deploy keys.
pub const DEPLOY_KEYS_POLICY: &str = "deploy-keys-policy";

fn deploy_keys_policy_step(d: &Desired) -> HumanStep {
    HumanStep {
        id: DEPLOY_KEYS_POLICY.to_string(),
        title: format!(
            "allow deploy keys for {}/{} (organization settings), then re-run keel up",
            d.repo.owner, d.repo.name
        ),
        url: format!(
            "https://github.com/organizations/{}/settings",
            seg(&d.repo.owner)
        ),
        status: "pending".to_string(),
    }
}

/// `tags_bypass = "deploy_key"`: a new ed25519 key (in memory only), added as the write
/// deploy key `hoist: release tags`, its private key sealed into secret `RELEASE_TAG_KEY` of
/// environment `env`. The environment's public key is read first, so a missing environment
/// writes nothing; a failed seal deletes the key just added (by the id the POST returned).
fn write_tag_key(
    http: &Http,
    d: &Desired,
    repo: &str,
    env: &str,
) -> Result<Option<HumanStep>, HoistError> {
    let probe = format!("{repo}/environments/{}/secrets/public-key", seg(env));
    match http.get_json(&probe)? {
        Fetched::Ok(_) => {}
        Fetched::NotFound => {
            return Err(HoistError::Usage(format!(
                "environment `{env}` has no Actions public key (404): create the environment \
                 (`[release] environment`) and run `hoist apply` again; no deploy key was added"
            )));
        }
        Fetched::Forbidden(hint) => {
            return Err(HoistError::Usage(format!(
                "cannot read {probe} ({hint}): the token needs Secrets and Environments write \
                 access; no deploy key was added"
            )));
        }
    }
    let key = crate::sshkey::generate()?;
    let body = json!({"title": TAG_KEY_TITLE, "key": key.public, "read_only": false});
    let added = match http.send_json(Method::Post, &format!("{repo}/keys"), Some(&body)) {
        Ok(added) => added,
        // The organization (or enterprise) disallows deploy keys: a settings change only a
        // person can make, not a failed write.
        Err(HttpError::Rejected {
            status: 422,
            ref message,
            ..
        }) if deploy_keys_disallowed(message) => return Ok(Some(deploy_keys_policy_step(d))),
        Err(err) => return Err(err.into()),
    };
    let id = added
        .body
        .as_ref()
        .and_then(|b| b.get("id"))
        .and_then(Value::as_u64);
    let sealed = put_env_secret(
        http,
        (&d.repo.owner, &d.repo.name),
        env,
        TAG_KEY_SECRET,
        &key.private,
    );
    drop(key);
    let Err(err) = sealed else {
        return Ok(None);
    };
    // A key whose private half is lost is useless (and drift): take it back.
    let undone = match id {
        Some(id) => http
            .send_json(
                Method::Delete,
                &format!("{repo}/keys/{}", seg(&id.to_string())),
                None,
            )
            .is_ok(),
        None => false,
    };
    let after = if undone {
        "the deploy key was deleted again; run `hoist apply` again"
    } else {
        "delete the deploy key `hoist: release tags` (Settings → Deploy keys) and run `hoist \
         apply` again"
    };
    Err(HoistError::Network(format!(
        "sealing {TAG_KEY_SECRET} into environment `{env}` failed: {err}; {after}"
    )))
}

fn ruleset_body(required_checks: &[String], merge_queue: bool, admin_bypass: bool) -> Value {
    let mut rules = vec![
        json!({"type": "deletion"}),
        json!({"type": "non_fast_forward"}),
        status_checks_rule(required_checks, None),
    ];
    if merge_queue {
        rules.push(queue_rule());
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
    // Only with `[github] admin_bypass = true`: the repository admin role may merge a PR past
    // the rules (a mainfix), never push.
    let actors = if admin_bypass {
        vec![admin_actor()]
    } else {
        Vec::new()
    };
    body.insert("bypass_actors".into(), Value::Array(actors));
    Value::Object(body)
}

/// Set `integration_id` on each pinned check of the body's `required_status_checks` rule
/// (spec §6.1); unpinned entries keep whatever they have.
fn pin_checks(body: &mut Value, pins: &[(String, u64)]) {
    let Some(rules) = body.get_mut("rules").and_then(Value::as_array_mut) else {
        return;
    };
    for rule in rules
        .iter_mut()
        .filter(|r| r["type"] == "required_status_checks")
    {
        let Some(entries) = rule
            .pointer_mut("/parameters/required_status_checks")
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        for e in entries {
            if let Some((_, id)) = pins.iter().find(|(c, _)| e["context"] == c.as_str()) {
                e["integration_id"] = json!(id);
            }
        }
    }
}

fn queue_rule() -> Value {
    json!({
        "type": "merge_queue",
        "parameters": merge_queue_parameters()
    })
}

/// The `required_status_checks` rule requiring `checks`. With the live rule, its other
/// parameters (a strict policy) and each kept check's entry (a pinned source) stay.
fn status_checks_rule(checks: &[String], live: Option<&Value>) -> Value {
    let live_checks: Vec<Value> = live
        .and_then(|r| r.pointer("/parameters/required_status_checks"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let entries: Vec<Value> = checks
        .iter()
        .map(|c| {
            live_checks
                .iter()
                .find(|l| l["context"] == c.as_str())
                .cloned()
                .unwrap_or_else(|| json!({ "context": c }))
        })
        .collect();
    let mut rule = live.cloned().unwrap_or_else(|| {
        json!({
            "type": "required_status_checks",
            "parameters": {"strict_required_status_checks_policy": false}
        })
    });
    if !rule["parameters"].is_object() {
        rule["parameters"] = json!({"strict_required_status_checks_policy": false});
    }
    rule["parameters"]["required_status_checks"] = Value::Array(entries);
    rule
}

/// Read-merge-write of an existing `hoist: default branch` ruleset (as for `hoist: tags`):
/// the live rules, conditions and bypass actors are written back with only hoist's parts
/// changed: enforcement `active`, target `branch`, `~DEFAULT_BRANCH` included when the live
/// includes miss the default branch, `deletion` and `non_fast_forward` added when missing, the
/// required checks set (a live strict policy and pinned sources kept), the merge queue rule
/// added (hoist's settings) or removed as wanted (a live one keeps its settings), and the admin
/// role added when `admin_bypass` asks. Owner-added rules, includes, excludes and bypass
/// actors stay: hoist never removes an actor.
fn merged_ruleset_body(
    live: &Value,
    required_checks: &[String],
    merge_queue: bool,
    admin_bypass: bool,
) -> Value {
    let list = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
    let mut rules = Vec::new();
    let has = |t: &str, rules: &Vec<Value>| rules.iter().any(|r| r["type"] == t);
    for rule in list(live.get("rules")) {
        match rule["type"].as_str() {
            Some("required_status_checks") => {
                rules.push(status_checks_rule(required_checks, Some(&rule)));
            }
            Some("merge_queue") if !merge_queue => {}
            _ => rules.push(rule),
        }
    }
    for t in ["deletion", "non_fast_forward"] {
        if !has(t, &rules) {
            rules.push(json!({ "type": t }));
        }
    }
    if !has("required_status_checks", &rules) {
        rules.push(status_checks_rule(required_checks, None));
    }
    if merge_queue && !has("merge_queue", &rules) {
        rules.push(queue_rule());
    }
    let mut include = list(live.pointer("/conditions/ref_name/include"));
    let default = json!("~DEFAULT_BRANCH");
    if !include.contains(&default) {
        include.push(default);
    }
    let exclude = list(live.pointer("/conditions/ref_name/exclude"));
    let mut conditions = live
        .get("conditions")
        .filter(|c| c.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    conditions["ref_name"] = json!({"include": include, "exclude": exclude});
    let mut actors = list(live.get("bypass_actors"));
    let is_admin = |b: &Value| {
        b["actor_type"] == "RepositoryRole"
            && b["actor_id"].as_u64() == Some(ADMIN_ROLE_ID)
            && b["bypass_mode"] == "pull_request"
    };
    if admin_bypass && !actors.iter().any(is_admin) {
        actors.push(admin_actor());
    }
    json!({
        "name": RULESET_NAME,
        "target": "branch",
        "enforcement": "active",
        "conditions": conditions,
        "rules": rules,
        "bypass_actors": actors
    })
}
