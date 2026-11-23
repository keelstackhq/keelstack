//! Desired vs live (pure; no I/O) for GitHub (`diff_github`) and Cloudflare
//! (`diff_cloudflare`). Items use the same area/kind/key scheme as `hoist explain`, and
//! `source` is the desired element's source.

use crate::change::{LiveToken, deploy_token_name, live_deploy_token};
use crate::cloudflare::{CfToken, LiveCloudflare};
use crate::config::Managed;
use crate::desired::Desired;
use crate::github::{EnvBranches, Fetch, LiveGithub, LiveVariable, RepoSettings, Ruleset};
use crate::repo::Sourced;
use crate::report::{Action, HumanStep, Item};
use crate::timeutil::{now_secs, parse_rfc3339};

struct Ctx {
    area: &'static str,
    managed: Managed,
    items: Vec<Item>,
}

impl Ctx {
    fn push(&mut self, kind: &str, key: &str, action: Action, reason: String, source: &str) {
        // Externally managed resources are never created or updated by hoist: that is drift.
        let action = match (self.managed, action) {
            (Managed::External, Action::Create | Action::Update) => Action::Drift,
            (_, a) => a,
        };
        self.items.push(Item {
            area: self.area.to_string(),
            kind: kind.to_string(),
            key: key.to_string(),
            action,
            reason,
            source: source.to_string(),
            verify: None,
        });
    }

    fn unknown(&mut self, kind: &str, key: &str, why: &str, source: &str) {
        self.push(kind, key, Action::Unknown, why.to_string(), source);
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

fn quoted(list: &[String]) -> String {
    if list.is_empty() {
        return "none".to_string();
    }
    list.iter()
        .map(|c| format!("`{c}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn setting(
    cx: &mut Ctx,
    live: &Fetch<RepoSettings>,
    key: &str,
    want: &Sourced<String>,
    get: impl Fn(&RepoSettings) -> Option<String>,
) {
    match live.as_got() {
        Err(why) => cx.unknown("repo_setting", key, why, &want.source),
        Ok(s) => match get(s) {
            None => cx.unknown(
                "repo_setting",
                key,
                &format!("cannot read {key}: the token cannot see it (needs admin access)"),
                &want.source,
            ),
            Some(have) if have == want.value => {
                cx.push("repo_setting", key, Action::Ok, have, &want.source)
            }
            Some(have) => cx.push(
                "repo_setting",
                key,
                Action::Update,
                format!("live {have}, want {}", want.value),
                &want.source,
            ),
        },
    }
}

fn applies_to(rs: &Ruleset, branch: &str) -> bool {
    let explicit = format!("refs/heads/{branch}");
    rs.include
        .iter()
        .any(|i| i == "~DEFAULT_BRANCH" || *i == explicit)
}

/// The ruleset's (reason, source) when it is in sync; `hoist explain` prints the same.
/// Source: the first required check's source, or `default` when there is none.
pub fn ruleset_summary(d: &Desired) -> (String, String) {
    let want = &d.github.ruleset;
    match want.required_checks.first() {
        None => (
            "no required checks inferred; ruleset not managed".to_string(),
            "default".to_string(),
        ),
        Some(first) => {
            let pins: Vec<String> = want
                .check_pins
                .iter()
                .map(|p| match &p.app {
                    crate::config::CheckApp::Slug(s) => format!("`{}` from app `{s}`", p.context),
                    crate::config::CheckApp::Id(i) => format!("`{}` from app id {i}", p.context),
                })
                .collect();
            let pinned = if pins.is_empty() {
                String::new()
            } else {
                format!("; pinned: {}", pins.join(", "))
            };
            (
                format!("required checks {}{pinned}", quoted(&wanted_checks(d))),
                first.source.clone(),
            )
        }
    }
}

/// The ruleset item's reason when the GitHub plan refuses rulesets (Free, private repo): what
/// stays unenforced, and the fix. Classic branch protection is refused there too.
pub fn free_plan_reason(d: &Desired) -> String {
    let want = &d.github.ruleset;
    let (queue, queue_fix) = if want.merge_queue.value {
        (
            ", merge queue",
            "; a merge queue also needs an organization repo (a private one on GitHub \
             Enterprise Cloud)",
        )
    } else {
        ("", "")
    };
    format!(
        "NOT enforced: required checks {}{queue}; GitHub Free: private repos get no rulesets, \
         branch protection or merge queue; upgrade the plan (GitHub Pro for a personal repo, \
         Team for an organization) or make the repo public{queue_fix}",
        quoted(&wanted_checks(d))
    )
}

/// A pinned required check whose App id is unresolved is an error (exit 3), never skipped.
pub fn ensure_pins_resolved(d: &Desired) -> Result<(), crate::error::HoistError> {
    match d.github.ruleset.check_pins.iter().find(|p| p.id.is_none()) {
        Some(p) => Err(crate::error::HoistError::Usage(format!(
            "required check `{}` is pinned to an App that is not resolved; check the slug, or \
             pin it with `integration_id`",
            p.context
        ))),
        None => Ok(()),
    }
}

fn ruleset(cx: &mut Ctx, d: &Desired, l: &LiveGithub) {
    let want = &d.github.ruleset;
    let name = &want.name;
    let (in_sync, source) = ruleset_summary(d);
    if want.required_checks.is_empty() {
        cx.push("ruleset", name, Action::Ok, in_sync, &source);
        return;
    }
    let rulesets = match l.rulesets.as_got() {
        Ok(r) => r,
        Err(why) if crate::github::plan_limited(why) => {
            return cx.push(
                "ruleset",
                name,
                Action::Pending,
                free_plan_reason(d),
                &source,
            );
        }
        Err(why) => return cx.unknown("ruleset", name, why, &source),
    };
    let Some(live) = rulesets.iter().find(|r| &r.name == name) else {
        let mut reason = format!("missing; would require {}", quoted(&wanted_checks(d)));
        if let Ok(true) = l.classic_protection.as_got().copied() {
            reason = format!(
                "missing; classic branch protection exists on `{}`",
                want.branch
            );
        }
        return cx.push("ruleset", name, Action::Create, reason, &source);
    };
    let mut problems = Vec::new();
    if let Some(mode) = live.enforcement.as_deref().filter(|m| *m != "active") {
        problems.push(format!("enforcement `{mode}`, want `active`"));
    }
    if !applies_to(live, &want.branch) {
        problems.push(format!("does not target default branch `{}`", want.branch));
    }
    let mut have = live.required_checks.clone();
    let mut need = wanted_checks(d);
    have.sort();
    need.sort();
    if have != need {
        problems.push(format!(
            "required checks: live {}, want {}",
            quoted(&have),
            quoted(&need)
        ));
    }
    // A pinned check's source must match (a plain-string check keeps whatever source it has).
    for pin in &want.check_pins {
        let Some(id) = pin.id else {
            // Never dropped silently: `ensure_pins_resolved` makes this an error before a write.
            problems.push(format!("`{}`: its app is not resolved", pin.context));
            continue;
        };
        let have = live.check_pins.iter().find(|(c, _)| *c == pin.context);
        if have.map(|(_, i)| *i) != Some(id) && live.required_checks.contains(&pin.context) {
            let was = have.map_or("any".to_string(), |(_, i)| format!("app id {i}"));
            let slug = match &pin.app {
                crate::config::CheckApp::Slug(s) => format!("app `{s}` (id {id})"),
                crate::config::CheckApp::Id(_) => format!("app id {id}"),
            };
            problems.push(format!("`{}`: live source {was}, want {slug}", pin.context));
        }
    }
    if live.merge_queue != want.merge_queue.value {
        problems.push(format!(
            "merge queue: live {}, want {}",
            on_off(live.merge_queue),
            on_off(want.merge_queue.value)
        ));
    }
    let missing: Vec<String> = ["deletion", "non_fast_forward"]
        .iter()
        .filter(|t| !live.rule_types.iter().any(|h| h == *t))
        .map(|t| t.to_string())
        .collect();
    if !missing.is_empty() {
        problems.push(format!("rules: missing {}", quoted(&missing)));
    }
    // Bypass actors: only `[github] admin_bypass = true` wants one (the admin role, in
    // `pull_request` mode). hoist adds it (never under --adopt: a bypass loosens) and never
    // removes a live actor: an unwanted one is drift, kept by every update.
    let admin = crate::desired::ADMIN_BYPASS;
    let mut drift = Vec::new();
    if want.admin_bypass && !live.bypass.iter().any(|b| b == admin) {
        let missing = "bypass actors: missing the repository admin role (pull_request)";
        if d.adopt {
            drift.push(format!(
                "{missing}; not written: adding a bypass loosens the ruleset (--adopt never \
                 loosens)"
            ));
        } else {
            problems.push(missing.to_string());
        }
    }
    let unwanted: Vec<String> = live
        .bypass
        .iter()
        .filter(|b| !(want.admin_bypass && *b == admin))
        .cloned()
        .collect();
    if !unwanted.is_empty() {
        let hint = if unwanted.iter().any(|b| b == admin) {
            ", or set `[github] admin_bypass = true` to keep the admin role"
        } else {
            ""
        };
        drift.push(format!(
            "bypass actors not wanted: {}; hoist never removes one: remove it in the ruleset \
             settings{hint}",
            quoted(&unwanted)
        ));
    }
    if problems.is_empty() && drift.is_empty() {
        cx.push("ruleset", name, Action::Ok, in_sync, &source);
    } else if problems.is_empty() {
        cx.push("ruleset", name, Action::Drift, drift.join("; "), &source);
    } else if d.adopt && !live.unmodelled.is_empty() {
        // hoist's update writes the target `branch`: adopting must not drop another one.
        let mut why = vec![format!(
            "{}; not written: an update would drop {} (--adopt never loosens)",
            problems.join("; "),
            live.unmodelled.join(", ")
        )];
        why.extend(drift);
        cx.push("ruleset", name, Action::Drift, why.join("; "), &source);
    } else {
        // The update fixes `problems`; what is drift stays (the next check reports it).
        problems.extend(drift);
        cx.push(
            "ruleset",
            name,
            Action::Update,
            problems.join("; "),
            &source,
        );
    }
}

/// The `hoist: tags` ruleset: release tags only the GitHub Actions app may create, move or
/// delete. Other rulesets are never read into this item.
fn tag_ruleset(cx: &mut Ctx, d: &Desired, l: &LiveGithub) {
    let Some(want) = &d.github.tag_ruleset else {
        return;
    };
    let (name, source, refs) = (&want.name, &want.source, want.refs());
    let who = if want.deploy_key() {
        "the repo's write deploy keys (`hoist: release tags`)"
    } else {
        "github-actions"
    };
    let in_sync = format!(
        "tags {} may be created, moved and deleted only by {who}",
        quoted(&refs)
    );
    let rulesets = match l.rulesets.as_got() {
        Ok(r) => r,
        Err(why) if crate::github::plan_limited(why) => {
            return cx.push(
                "tag_ruleset",
                name,
                Action::Pending,
                free_plan_tag_reason(&refs),
                source,
            );
        }
        Err(why) => return cx.unknown("tag_ruleset", name, why, source),
    };
    let Some(live) = rulesets.iter().find(|r| &r.name == name) else {
        return cx.push(
            "tag_ruleset",
            name,
            Action::Create,
            format!("missing; would protect tags {}", quoted(&refs)),
            source,
        );
    };
    let mut problems = Vec::new();
    if let Some(mode) = live.enforcement.as_deref().filter(|m| *m != "active") {
        problems.push(format!("enforcement `{mode}`, want `active`"));
    }
    if live.target.as_deref().is_some_and(|t| t != "tag") {
        problems.push("target is not tags".to_string());
    }
    // hoist's update reads the live ruleset and writes back what the owner added (extra tags,
    // rules, bypass actors); only the parts below are hoist's.
    let missing_refs: Vec<String> = refs
        .iter()
        .filter(|r| !live.include.contains(r))
        .cloned()
        .collect();
    if !missing_refs.is_empty() {
        problems.push(format!("tags: missing {}", quoted(&missing_refs)));
    }
    // An exclude can unprotect a release tag: an update clears it.
    if !live.exclude.is_empty() {
        problems.push(format!("excludes {}", quoted(&live.exclude)));
    }
    let missing_rules: Vec<String> = crate::github_write::TAG_RULES
        .iter()
        .filter(|t| !live.rule_types.iter().any(|h| h == *t))
        .map(|t| t.to_string())
        .collect();
    if !missing_rules.is_empty() {
        problems.push(format!("rules: missing {}", quoted(&missing_rules)));
    }
    // The Actions app is hoist's own actor (Integration 15368): in `deploy_key` mode it is
    // removed; any other actor is the owner's and stays.
    let app = format!("Integration:{}:", crate::desired::ACTIONS_APP_ID);
    let ours = format!("{app}always");
    let keys = "DeployKey:";
    let apps: Vec<&String> = live.bypass.iter().filter(|b| b.starts_with(&app)).collect();
    if want.deploy_key() {
        if !apps.is_empty() {
            problems.push(
                "bypass actors: the GitHub Actions app is still listed; `tags_bypass = \
                 \"deploy_key\"` replaces it with the deploy keys"
                    .to_string(),
            );
        }
        match live.bypass.iter().find(|b| b.starts_with(keys)) {
            None => problems
                .push("bypass actors: missing the deploy keys (`hoist: release tags`)".to_string()),
            Some(b) if b != crate::desired::DEPLOY_KEY_BYPASS => problems.push(format!(
                "bypass actors: the deploy keys have mode `{}`, want `always`",
                b.rsplit(':').next().unwrap_or("?")
            )),
            Some(_) => {}
        }
    } else {
        match apps.first() {
            None => problems.push("bypass actors: missing the GitHub Actions app".to_string()),
            Some(b) if **b != ours => problems.push(format!(
                "bypass actors: the GitHub Actions app has mode `{}`, want `always`",
                b.trim_start_matches(&app)
            )),
            Some(_) if apps.len() > 1 => problems.push(format!(
                "bypass actors: the GitHub Actions app is listed {} times",
                apps.len()
            )),
            Some(_) => {}
        }
    }
    let extra_refs: Vec<String> = live
        .include
        .iter()
        .filter(|i| !refs.contains(i))
        .cloned()
        .collect();
    let extra_rules: Vec<String> = live
        .rule_types
        .iter()
        .filter(|t| !crate::github_write::TAG_RULES.contains(&t.as_str()))
        .cloned()
        .collect();
    let extra_bypass: Vec<String> = live
        .bypass
        .iter()
        .filter(|b| !b.starts_with(&app) && !(want.deploy_key() && b.starts_with(keys)))
        .cloned()
        .collect();
    let mut kept = Vec::new();
    if !extra_refs.is_empty() {
        kept.push(format!("tags {}", quoted(&extra_refs)));
    }
    if !extra_rules.is_empty() {
        kept.push(format!("rules {}", quoted(&extra_rules)));
    }
    if !extra_bypass.is_empty() {
        kept.push(format!("bypass actors {}", quoted(&extra_bypass)));
    }
    if problems.is_empty() {
        let mut in_sync = in_sync;
        if !extra_bypass.is_empty() {
            in_sync = format!(
                "tags {} may be created, moved and deleted only by {who} and the bypass actors \
                 the owner added",
                quoted(&refs)
            );
        }
        if !kept.is_empty() {
            in_sync = format!("{in_sync}; owner-added, kept: {}", kept.join("; "));
        }
        cx.push("tag_ruleset", name, Action::Ok, in_sync, source);
    } else if d.adopt && !live.exclude.is_empty() {
        // The update keeps every owner addition but clears excludes: adopting writes nothing.
        let why = format!(
            "{}; not written: an update would drop excludes {} (--adopt never loosens)",
            problems.join("; "),
            quoted(&live.exclude)
        );
        cx.push("tag_ruleset", name, Action::Drift, why, source);
    } else {
        cx.push(
            "tag_ruleset",
            name,
            Action::Update,
            problems.join("; "),
            source,
        );
    }
}

/// `tags_bypass = "deploy_key"`: the write deploy key `hoist: release tags` and its private key
/// in the release environment's secret `RELEASE_TAG_KEY`. Both or neither: hoist never guesses
/// which key a lone secret holds, nor recovers a lost private key. Every write deploy key
/// bypasses the tag ruleset (GitHub's `DeployKey` actor has no id), so another one is drift.
fn tag_deploy_key(cx: &mut Ctx, d: &Desired, l: &LiveGithub) {
    use crate::desired::{TAG_KEY_SECRET, TAG_KEY_TITLE};
    let Some(want) = d.github.tag_ruleset.as_ref().filter(|t| t.deploy_key()) else {
        return;
    };
    let source = "hoist.toml:github.tags_bypass";
    let env = &want.key_env;
    let (keys, secret) = match (&l.deploy_keys, &l.tag_key_secret) {
        (Some(k), Some(s)) => (k, s),
        _ => return cx.unknown("tag_deploy_key", TAG_KEY_TITLE, "not read", source),
    };
    let keys = match keys.as_got() {
        Ok(k) => k,
        Err(why) => return cx.unknown("tag_deploy_key", TAG_KEY_TITLE, why, source),
    };
    let secret = match secret.as_got() {
        Ok(s) => *s,
        Err(why) => return cx.unknown("tag_deploy_key", TAG_KEY_TITLE, why, source),
    };
    let ours: Vec<_> = keys.iter().filter(|k| k.title == TAG_KEY_TITLE).collect();
    let others: Vec<String> = keys
        .iter()
        .filter(|k| k.title != TAG_KEY_TITLE && !k.read_only)
        .map(|k| k.title.clone())
        .collect();
    let fix = "delete the deploy key (Settings → Deploy keys) and the secret, then run `hoist \
               apply` again";
    let (mut action, mut reason) = match (ours.as_slice(), secret) {
        ([], false) => (
            Action::Create,
            format!(
                "missing; would add the write deploy key `{TAG_KEY_TITLE}` and seal its private \
                 key into secret {TAG_KEY_SECRET} of environment `{env}`"
            ),
        ),
        ([], true) => (
            Action::Drift,
            format!(
                "secret {TAG_KEY_SECRET} is in environment `{env}` but there is no deploy key \
                 `{TAG_KEY_TITLE}`: hoist cannot tell which key it holds; delete the secret \
                 and run `hoist apply` again"
            ),
        ),
        ([_], false) => (
            Action::Drift,
            format!(
                "deploy key `{TAG_KEY_TITLE}` exists but secret {TAG_KEY_SECRET} is missing in \
                 environment `{env}`: its private key is gone; delete the deploy key \
                 (Settings → Deploy keys) and run `hoist apply` again"
            ),
        ),
        ([k], true) if k.read_only => (
            Action::Drift,
            format!("deploy key `{TAG_KEY_TITLE}` is read-only, so it cannot push tags; {fix}"),
        ),
        ([_], true) => (
            Action::Ok,
            format!(
                "write deploy key `{TAG_KEY_TITLE}` present; its private key is in secret \
                 {TAG_KEY_SECRET} (environment `{env}`)"
            ),
        ),
        (_, _) => (
            Action::Drift,
            format!(
                "{} deploy keys are titled `{TAG_KEY_TITLE}`: hoist cannot tell which one \
                 secret {TAG_KEY_SECRET} holds; {fix}",
                ours.len()
            ),
        ),
    };
    if !others.is_empty() {
        reason = format!(
            "{reason}; other write deploy keys can also create, move and delete release tags: \
             {}; delete them or make them read-only",
            quoted(&others)
        );
        if action == Action::Ok {
            action = Action::Drift;
        }
    }
    cx.push("tag_deploy_key", TAG_KEY_TITLE, action, reason, source);
}

/// The tag ruleset item's reason when the GitHub plan refuses rulesets (Free, private repo).
/// The human step of a plan-limited ruleset (`plan-limit:rulesets`): the plan, not hoist, keeps
/// branch protection off, so `check` and `apply` exit 2 instead of a silent 0.
pub fn plan_limit_step(d: &Desired) -> HumanStep {
    HumanStep {
        id: "plan-limit:rulesets".to_string(),
        title: "branch protection is not enforced: this GitHub plan gives a private repo no \
                rulesets (required checks, tag rules); upgrade the plan (GitHub Pro for a \
                personal repo, Team for an organization) or make the repo public, then run \
                `hoist apply`"
            .to_string(),
        url: format!(
            "https://github.com/{}/{}/settings/rules",
            d.repo.owner, d.repo.name
        ),
        status: "pending".to_string(),
    }
}

/// Add [`plan_limit_step`] once.
pub fn add_plan_limit_step(steps: &mut Vec<HumanStep>, d: &Desired) {
    let step = plan_limit_step(d);
    if !steps.iter().any(|s| s.id == step.id) {
        steps.push(step);
    }
}

pub fn free_plan_tag_reason(refs: &[String]) -> String {
    format!(
        "NOT enforced: tags {}; GitHub Free: private repos get no rulesets; upgrade the plan \
         (GitHub Pro for a personal repo, Team for an organization) or make the repo public",
        quoted(refs)
    )
}

fn wanted_checks(d: &Desired) -> Vec<String> {
    d.github
        .ruleset
        .required_checks
        .iter()
        .map(|c| c.value.clone())
        .collect()
}

/// What hoist can do about a missing secret.
enum Producer {
    /// Hoist writes it.
    Yes,
    /// Hoist could, but the precondition cannot be checked (reason).
    Blind(String),
    /// A human sets it.
    No,
}

/// Secrets hoist writes itself: `CLOUDFLARE_ACCOUNT_ID`, and `CLOUDFLARE_API_TOKEN` when the
/// Cloudflare token list was read (hoist never mints blind). Both need a Cloudflare target,
/// an account id and `managed = "hoist"` for Cloudflare.
fn producer(d: &Desired, cf: Option<&LiveCloudflare>, name: &str) -> Producer {
    let c = &d.cloudflare;
    let ready = c.managed == Managed::Hoist
        && c.account_id.is_some()
        && !(c.pages.is_empty() && c.workers.is_empty());
    match name {
        "CLOUDFLARE_ACCOUNT_ID" if ready => Producer::Yes,
        "CLOUDFLARE_API_TOKEN" if ready => match cf.map(|l| l.tokens.as_got()) {
            Some(Ok(_)) => Producer::Yes,
            Some(Err(why)) => Producer::Blind(why.to_string()),
            None => Producer::Blind("Cloudflare was not read".to_string()),
        },
        _ => Producer::No,
    }
}

/// Under `--adopt`, the ids of this repo's existing deploy tokens: a new one is then not
/// minted (adopt revokes none, and two would leave the live one unclear); `hoist secrets
/// rotate` mints it and revokes them.
fn adopt_leftovers(d: &Desired, cf: Option<&LiveCloudflare>) -> Vec<String> {
    let name = deploy_token_name(d);
    match (d.adopt, cf.map(|l| l.tokens.as_got())) {
        (true, Some(Ok(tokens))) => tokens
            .iter()
            .filter(|t| t.name == name)
            .map(|t| t.id.clone())
            .collect(),
        _ => Vec::new(),
    }
}

/// Days before expiry at which the minted token needs rotation.
const ROTATE_WITHIN_DAYS: i64 = 30;

/// Why the active hoist token needs `hoist secrets rotate`, or `None` when it is fine.
/// Pure: `now` is Unix seconds.
pub fn rotation_reason(token: &CfToken, now: i64) -> Option<String> {
    if token.status != "active" {
        return Some(format!("token status is `{}`", token.status));
    }
    let at = token.expires_on.as_deref().and_then(parse_rfc3339)?;
    let days = (at - now).div_euclid(86_400);
    match days {
        d if d < 0 => Some(format!("expired {} days ago", -d)),
        d if d <= ROTATE_WITHIN_DAYS => Some(format!("expires in {d} days")),
        _ => None,
    }
}

/// The `cloudflare/api_token` item, tied to the `CLOUDFLARE_API_TOKEN` secret so that it
/// agrees with what `apply` can do: `create` only pairs with a planned `DeployToken` change;
/// a token that needs rotation is a human step (`hoist secrets rotate`), never `update`.
fn deploy_token(
    d: &Desired,
    cf: Option<&LiveCloudflare>,
    secret_present: bool,
    secret_updated: Option<&str>,
    out: &mut Vec<Item>,
    steps: &mut Vec<HumanStep>,
) {
    let Some(cf) = cf else { return };
    let name = deploy_token_name(d);
    let item = |action: Action, reason: String| Item {
        area: "cloudflare".to_string(),
        kind: "api_token".to_string(),
        key: name.clone(),
        action,
        reason,
        source: "default".to_string(),
        verify: None,
    };
    let tokens = match cf.tokens.as_got() {
        Ok(t) => t,
        Err(why) => {
            if secret_present {
                out.push(item(Action::Unknown, why.to_string()));
            }
            return;
        }
    };
    let rotate_step = |title: String| HumanStep {
        id: "rotate:CLOUDFLARE_API_TOKEN".to_string(),
        title,
        url: "hoist secrets rotate".to_string(),
        status: "pending".to_string(),
    };
    if !secret_present {
        let left = adopt_leftovers(d, Some(cf));
        if !left.is_empty() {
            let ids = left.join(", ");
            out.push(item(
                Action::Pending,
                format!(
                    "missing, and deploy tokens {ids} exist; --adopt revokes none: run `hoist \
                     secrets rotate`"
                ),
            ));
            steps.push(rotate_step(format!(
                "mint the Cloudflare deploy token with `hoist secrets rotate` (it revokes the \
                 older tokens {ids})"
            )));
            return;
        }
        let reason = "missing; hoist would mint it and seal it into CLOUDFLARE_API_TOKEN";
        out.push(item(Action::Create, reason.to_string()));
        return;
    }
    match live_deploy_token(&name, tokens, secret_updated) {
        LiveToken::NoToken => out.push(item(
            Action::Ok,
            "CLOUDFLARE_API_TOKEN is set outside hoist; run `hoist secrets rotate` to let \
             hoist mint and manage it"
                .to_string(),
        )),
        LiveToken::Unclear(ids) => {
            let ids = ids.join(", ");
            out.push(item(
                Action::Pending,
                format!(
                    "cannot tell which hoist deploy token CLOUDFLARE_API_TOKEN holds (ids {ids}); \
                     run `hoist secrets rotate`"
                ),
            ));
            steps.push(rotate_step(format!(
                "rotate the Cloudflare deploy token: cannot tell which of tokens {ids} is live"
            )));
        }
        LiveToken::Live(t, newer) => {
            let note = if newer.is_empty() {
                String::new()
            } else {
                format!(
                    "; newer token(s) {} are not in CLOUDFLARE_API_TOKEN (`hoist secrets \
                     rotate` revokes them)",
                    newer.join(", ")
                )
            };
            match rotation_reason(t, now_secs()) {
                None => out.push(item(Action::Ok, format!("active{note}"))),
                Some(why) => {
                    out.push(item(
                        Action::Pending,
                        format!("{why}; run `hoist secrets rotate`{note}"),
                    ));
                    steps.push(rotate_step(
                        "rotate the Cloudflare deploy token".to_string(),
                    ));
                }
            }
        }
    }
}

fn branch_list(list: &[String]) -> String {
    if list.is_empty() {
        "no branches".to_string()
    } else {
        quoted(list)
    }
}

/// What an existing environment lacks of the wanted deployment branch and tag policies:
/// `None` when it has them (or the read did not show them). Extra live policies are named,
/// never removed.
fn branch_gap(want: &[String], want_tags: &[String], have: &EnvBranches) -> Option<String> {
    let all = || {
        let mut parts = Vec::new();
        if !want.is_empty() {
            parts.push(branch_list(want));
        }
        if !want_tags.is_empty() {
            parts.push(format!("tag {}", quoted(want_tags)));
        }
        if parts.is_empty() {
            branch_list(want)
        } else {
            parts.join(" and ")
        }
    };
    let none_wanted = want.is_empty() && want_tags.is_empty();
    match (have, none_wanted) {
        (EnvBranches::Unrestricted, true) => None,
        (EnvBranches::Unrestricted, false) => Some(format!(
            "present, but any branch can deploy (no deployment branch policy); would restrict it \
             to {}",
            all()
        )),
        (EnvBranches::Protected, _) => Some(format!(
            "present, but only protected branches may deploy; would set the deployment branch \
             policy to {}",
            all()
        )),
        (EnvBranches::Custom(live, live_tags), true) => {
            let mut what = branch_list(live);
            if !live_tags.is_empty() {
                what = format!("{what} and tag {}", quoted(live_tags));
            }
            Some(format!(
                "present, but deployments are limited to {what}; hoist.toml wants no branch \
                 restriction"
            ))
        }
        (EnvBranches::Custom(live, live_tags), false) => {
            let missing: Vec<String> = want.iter().filter(|b| !live.contains(b)).cloned().collect();
            let missing_tags: Vec<String> = want_tags
                .iter()
                .filter(|t| !live_tags.contains(t))
                .cloned()
                .collect();
            if missing.is_empty() && missing_tags.is_empty() {
                return None;
            }
            let mut extra: Vec<String> =
                live.iter().filter(|b| !want.contains(b)).cloned().collect();
            let extra_tags: Vec<String> = live_tags
                .iter()
                .filter(|t| !want_tags.contains(t))
                .cloned()
                .collect();
            let mut lacks = Vec::new();
            if !missing.is_empty() {
                lacks.push(branch_list(&missing));
            }
            if !missing_tags.is_empty() {
                lacks.push(format!("tag {}", quoted(&missing_tags)));
            }
            let mut kept_parts = Vec::new();
            if !extra.is_empty() {
                kept_parts.push(branch_list(&std::mem::take(&mut extra)));
            }
            if !extra_tags.is_empty() {
                kept_parts.push(format!("tag {}", quoted(&extra_tags)));
            }
            let kept = if kept_parts.is_empty() {
                String::new()
            } else {
                format!("; also allowed (kept): {}", kept_parts.join(" and "))
            };
            Some(format!(
                "present, but the deployment branch policy lacks {}{kept}",
                lacks.join(" and ")
            ))
        }
    }
}

/// An environment that exists: `ok`, or `update` when its branch policy lacks a wanted branch.
fn environment_policy(cx: &mut Ctx, d: &Desired, l: &LiveGithub, w: &Sourced<String>) {
    let name = &w.value;
    let source = match d.github.env_settings.get(name) {
        Some(s) if s.branches.is_some() => &s.source,
        _ => &w.source,
    };
    match l.env_branches.get(name) {
        Some(Fetch::Unknown(why)) => cx.unknown("environment", name, why, source),
        Some(Fetch::Got(have)) => {
            match branch_gap(&d.env_branches(name), &d.env_tags(name), have) {
                // --adopt never rewrites an existing environment.
                Some(reason) if d.adopt => cx.push(
                    "environment",
                    name,
                    Action::Drift,
                    format!(
                        "{reason}; not written (--adopt never rewrites an existing environment)"
                    ),
                    source,
                ),
                Some(reason) => cx.push("environment", name, Action::Update, reason, source),
                None => cx.push(
                    "environment",
                    name,
                    Action::Ok,
                    "present".to_string(),
                    source,
                ),
            }
        }
        None => cx.push(
            "environment",
            name,
            Action::Ok,
            "present".to_string(),
            source,
        ),
    }
}

/// `s` as one shell word: unchanged when it is made only of safe characters, else single-quoted.
fn sh_quote(s: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "_-./:@%+=,".contains(c);
    if !s.is_empty() && s.chars().all(safe) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The `gh variable set` command(s) for a missing variable: one per environment when only
/// jobs with an `environment:` read it, else the repository-level one.
fn set_variable_commands(d: &Desired, name: &str, envs: &[String]) -> String {
    let slug = sh_quote(&format!("{}/{}", d.repo.owner, d.repo.name));
    let name = sh_quote(name);
    if envs.is_empty() {
        return format!("gh variable set {name} --repo {slug} --body '<value>'");
    }
    envs.iter()
        .map(|e| {
            let e = sh_quote(e);
            format!("gh variable set {name} --env {e} --repo {slug} --body '<value>'")
        })
        .collect::<Vec<_>>()
        .join(" ; ")
}

fn set_variable_step(d: &Desired, name: &str, envs: &[String]) -> HumanStep {
    let (owner, repo) = (&d.repo.owner, &d.repo.name);
    HumanStep {
        id: format!("variable:{name}"),
        title: format!(
            "set variable {name}: {} (or give its value in hoist.toml `[github.variables]` and \
             run `hoist apply`)",
            set_variable_commands(d, name, envs)
        ),
        url: format!("https://github.com/{owner}/{repo}/settings/variables/actions"),
        status: "pending".to_string(),
    }
}

/// Each variable the workflows read or `hoist.toml` lists: present, to set from hoist.toml
/// (`create`, `update`), or a human step.
fn variables(cx: &mut Ctx, d: &Desired, l: &LiveGithub, steps: &mut Vec<HumanStep>) {
    let g = &d.github;
    for w in &g.variables {
        let name = &w.value;
        let want = g.variable_values.get(name).map(|v| &v.value);
        let have = match l.variables.as_got() {
            Err(why) => {
                cx.unknown("variable", name, why, &w.source);
                continue;
            }
            Ok(have) => have,
        };
        // A repository variable shadows an organization one of the same name.
        let live: Option<&LiveVariable> = have
            .iter()
            .find(|v| &v.name == name && !v.org)
            .or_else(|| have.iter().find(|v| &v.name == name));
        let note = |reason: String| match &l.org_variables_note {
            Some(n) => format!("{reason}; {n}"),
            None => reason,
        };
        // The value's source (hoist.toml) when it sets one, else where a workflow reads it.
        let source = g
            .variable_values
            .get(name)
            .map_or_else(|| w.source.clone(), |v| v.source.clone());
        match (want, live) {
            (Some(v), Some(x)) if x.value == *v => {
                let scope = if x.org {
                    " (organization variable)"
                } else {
                    ""
                };
                cx.push(
                    "variable",
                    name,
                    Action::Ok,
                    format!("present{scope}"),
                    &source,
                )
            }
            (Some(v), Some(x)) if !x.org => cx.push(
                "variable",
                name,
                Action::Update,
                format!("live \"{}\", want \"{v}\"", x.value),
                &source,
            ),
            (Some(v), Some(x)) => cx.push(
                "variable",
                name,
                Action::Create,
                format!(
                    "the organization variable is \"{}\"; would set a repository variable \
                     \"{v}\" that overrides it",
                    x.value
                ),
                &source,
            ),
            (Some(_), None) => cx.push(
                "variable",
                name,
                Action::Create,
                note("missing; would set it from hoist.toml".to_string()),
                &source,
            ),
            (None, Some(x)) => {
                let scope = if x.org {
                    " (organization variable)"
                } else {
                    ""
                };
                cx.push(
                    "variable",
                    name,
                    Action::Ok,
                    format!("present{scope}"),
                    &source,
                )
            }
            (None, None) if g.optional_variables.contains(name) => cx.push(
                "optional_variable",
                name,
                Action::Ok,
                "optional; not set (`gh variable set` or `[github.variables]` if used)".to_string(),
                &source,
            ),
            (None, None) => {
                // A job with an `environment:` reads an environment variable of that name.
                let home = g.variable_homes.iter().find(|h| &h.name == name);
                let envs: &[String] = match home {
                    Some(h) if h.envless.is_empty() => &h.envs,
                    _ => &[],
                };
                let mut present = Vec::new();
                let mut unreadable = None;
                for env in envs {
                    match l.env_variables.get(env) {
                        Some(Fetch::Got(vars)) if vars.iter().any(|v| &v.name == name) => {
                            present.push(env.clone())
                        }
                        Some(Fetch::Unknown(why)) => unreadable = Some(why.clone()),
                        _ => {}
                    }
                }
                if !present.is_empty() {
                    cx.push(
                        "variable",
                        name,
                        Action::Ok,
                        format!("present (environment {})", present.join(", ")),
                        &source,
                    );
                    continue;
                }
                if let Some(why) = unreadable {
                    cx.unknown("variable", name, &why, &source);
                    continue;
                }
                cx.push(
                    "variable",
                    name,
                    Action::Pending,
                    note(format!(
                        "missing; no value in hoist.toml: set it with `{}`, or add it to \
                         `[github.variables]`",
                        set_variable_commands(d, name, envs)
                    )),
                    &source,
                );
                steps.push(set_variable_step(d, name, envs));
            }
        }
    }
}

/// Diff the desired GitHub state against what was read. Returns the items plus the human
/// steps (missing GitHub Apps), sorted by (area, kind, key).
pub fn diff_github(
    d: &Desired,
    l: &LiveGithub,
    cf: Option<&LiveCloudflare>,
) -> (Vec<Item>, Vec<HumanStep>) {
    let g = &d.github;
    let mut cx = Ctx {
        area: "github",
        managed: g.managed,
        items: Vec::new(),
    };
    let mut steps = Vec::new();

    if let Some(v) = &g.settings.visibility {
        setting(&mut cx, &l.settings, "visibility", v, |s| {
            Some(s.visibility.clone())
        });
    }
    let flag = |b: bool| b.to_string();
    let want = Sourced {
        value: flag(g.settings.allow_auto_merge.value),
        source: g.settings.allow_auto_merge.source.clone(),
    };
    setting(&mut cx, &l.settings, "allow_auto_merge", &want, |s| {
        s.allow_auto_merge.map(flag)
    });
    let want = Sourced {
        value: flag(g.settings.delete_branch_on_merge.value),
        source: g.settings.delete_branch_on_merge.source.clone(),
    };
    setting(&mut cx, &l.settings, "delete_branch_on_merge", &want, |s| {
        s.delete_branch_on_merge.map(flag)
    });

    if let Some(w) = &g.settings.immutable_releases {
        let want = Sourced {
            value: flag(w.value),
            source: w.source.clone(),
        };
        setting(&mut cx, &l.settings, "immutable_releases", &want, |s| {
            s.immutable_releases.map(flag)
        });
    }

    ruleset(&mut cx, d, l);
    tag_ruleset(&mut cx, d, l);
    tag_deploy_key(&mut cx, d, l);

    let want = &g.actions_permissions;
    match l.actions_permissions.as_got() {
        Err(why) => cx.unknown("actions_permissions", "default", why, &want.source),
        Ok(have) if *have == want.value => cx.push(
            "actions_permissions",
            "default",
            Action::Ok,
            have.clone(),
            &want.source,
        ),
        Ok(have) => cx.push(
            "actions_permissions",
            "default",
            Action::Update,
            format!("live {have}, want {}", want.value),
            &want.source,
        ),
    }

    // The release environment is an item of the `release` area (diff_release), not listed twice.
    let mut environments: Vec<Sourced<String>> = Vec::new();
    for e in &g.environments {
        if d.release_environment() == Some(e.value.as_str()) {
            continue;
        }
        if d.env_managed(&e.value) {
            environments.push(e.clone());
        } else {
            let source = g
                .env_settings
                .get(&e.value)
                .map_or(&e.source, |s| &s.source);
            cx.push(
                "environment",
                &e.value,
                Action::Ok,
                "not managed (`manage = false` in hoist.toml); hoist leaves it alone".to_string(),
                source,
            );
        }
    }
    for w in &environments {
        match l.environments.as_got() {
            Err(why) => cx.unknown("environment", &w.value, why, &w.source),
            Ok(have) if have.contains(&w.value) => environment_policy(&mut cx, d, l, w),
            Ok(_) => cx.push(
                "environment",
                &w.value,
                Action::Create,
                "missing; would create the environment".to_string(),
                &w.source,
            ),
        }
    }
    variables(&mut cx, d, l, &mut steps);
    for e in &g.unresolved_environments {
        cx.unknown(
            "environment",
            &e.value,
            "the environment name is an expression, known only at run time: list the names in \
             hoist.toml `[github.environments.<name>]` so hoist creates them",
            &e.source,
        );
    }
    for w in &g.secrets {
        let name = &w.value;
        if name == "CLOUDFLARE_API_TOKEN"
            && !matches!(producer(d, cf, name), Producer::No)
            && let Ok(have) = l.secrets.as_got()
        {
            deploy_token(
                d,
                cf,
                have.contains(name),
                l.api_token_updated_at.as_deref(),
                &mut cx.items,
                &mut steps,
            );
        }
        // secret_scope = "environment": a job without an environment cannot get it.
        if !d.envless(name).is_empty() {
            let jobs = d.envless(name).join(", ");
            cx.push(
                "secret",
                name,
                Action::Drift,
                format!(
                    "read by {jobs} with no `environment:`; secret_scope = \"environment\" keeps \
                     secrets in environments only: give the job an environment"
                ),
                &w.source,
            );
            continue;
        }
        // The release-tags deploy key's secret is the `tag_deploy_key` item.
        if name == crate::desired::TAG_KEY_SECRET
            && g.tag_ruleset.as_ref().is_some_and(|t| t.deploy_key())
        {
            continue;
        }
        // A secret hoist produces (Cloudflare token, account id) is never optional.
        if d.github.optional_secrets.contains(name) && matches!(producer(d, cf, name), Producer::No)
        {
            match l.secrets.as_got() {
                Err(why) => cx.unknown("optional_secret", name, why, &w.source),
                Ok(have) => {
                    let reason = if have.contains(name) {
                        "optional; present".to_string()
                    } else {
                        format!("optional; not set (`hoist secrets set {name}` if used)")
                    };
                    cx.push("optional_secret", name, Action::Ok, reason, &w.source);
                }
            }
            continue;
        }
        match l.secrets.as_got() {
            Err(why) => cx.unknown("secret", name, why, &w.source),
            Ok(have) if have.contains(name) => {
                cx.push("secret", name, Action::Ok, "present".to_string(), &w.source)
            }
            Ok(_) => {
                let produced = producer(d, cf, name);
                let homes = d.homes(name);
                let hint = if !homes.is_empty() {
                    format!("missing in environment(s) {}", homes.join(", "))
                } else if name == "CLOUDFLARE_API_TOKEN" && matches!(produced, Producer::Yes) {
                    "missing; `hoist apply` mints a scoped deploy token".to_string()
                } else {
                    format!("missing; set with `hoist secrets set {name}`")
                };
                let reason = match &l.org_secrets_note {
                    Some(note) => format!("{hint}; {note}"),
                    None => hint,
                };
                let left = match name.as_str() {
                    "CLOUDFLARE_API_TOKEN" => adopt_leftovers(d, cf),
                    _ => Vec::new(),
                };
                match produced {
                    // The rotate step comes with the `cloudflare/api_token` item.
                    Producer::Yes if !left.is_empty() => cx.push(
                        "secret",
                        name,
                        Action::Pending,
                        "missing; --adopt does not mint beside older deploy tokens: run `hoist \
                         secrets rotate`"
                            .to_string(),
                        &w.source,
                    ),
                    Producer::Yes => cx.push("secret", name, Action::Create, reason, &w.source),
                    Producer::Blind(why) => cx.unknown(
                        "secret",
                        name,
                        &format!("cannot list Cloudflare tokens: {why}; hoist will not mint blind"),
                        &w.source,
                    ),
                    Producer::No => {
                        // Hoist cannot produce this value: a human sets it.
                        cx.push("secret", name, Action::Pending, reason, &w.source);
                        steps.push(HumanStep {
                            id: format!("secret:{name}"),
                            title: format!("set secret {name}"),
                            url: format!("hoist secrets set {name}"),
                            status: "pending".to_string(),
                        });
                    }
                }
            }
        }
    }

    // secret_scope = "environment": a call that inherits secrets into a workflow hoist cannot
    // read; which secrets it needs, and in which environments, is unknown.
    if d.github.env_scope {
        for job in &d.github.opaque_inherits {
            cx.push(
                "workflow_call",
                job,
                Action::Drift,
                "passes `secrets: inherit` to a workflow in another repository: hoist cannot see \
                 which secrets it reads, and secret_scope = \"environment\" passes no repository \
                 secrets; name them in the called job's environment, or pass them explicitly"
                    .to_string(),
                "hoist.toml:github.secret_scope",
            );
        }
    }

    // secret_scope = "environment": a `secrets` use hoist cannot name has no known environment.
    if d.github.env_scope {
        for job in &d.github.unresolved_secrets {
            cx.push(
                "secret_reference",
                job,
                Action::Drift,
                format!(
                    "unresolvable secret reference in {job}: name each secret as `secrets.NAME` \
                     so hoist can place it in the job's environment"
                ),
                "hoist.toml:github.secret_scope",
            );
        }
    }

    // secret_scope = "environment": a repository copy of a secret kept in an environment.
    for name in &l.repo_strays {
        let env = d.homes(name).join(", ");
        cx.push(
            "secret",
            &format!("{name} (repository)"),
            Action::Pending,
            format!(
                "a repository secret; secret_scope = \"environment\" keeps it in environment \
                 {env}: delete the repository copy"
            ),
            "hoist.toml:github.secret_scope",
        );
        steps.push(HumanStep {
            id: format!("repo-secret:{name}"),
            title: format!(
                "delete repository secret {name} (Settings → Secrets and variables → Actions); \
                 it is kept in environment {env}"
            ),
            url: String::new(),
            status: "pending".to_string(),
        });
    }

    for app in &g.apps {
        let slug = &app.value;
        match l.apps.as_got() {
            Err(why) => cx.unknown("app", slug, why, &app.source),
            Ok(have) if have.contains(slug) => cx.push(
                "app",
                slug,
                Action::Ok,
                "installed".to_string(),
                &app.source,
            ),
            Ok(_) => {
                cx.push(
                    "app",
                    slug,
                    Action::Pending,
                    format!("GitHub App `{slug}` is not installed on this repo"),
                    &app.source,
                );
                steps.push(HumanStep {
                    id: format!("app:{slug}"),
                    title: format!(
                        "install the {slug} GitHub App on {}/{}",
                        d.repo.owner, d.repo.name
                    ),
                    // An app named in hoist.toml has its own install page; Renovate's is its
                    // public page.
                    url: if app.source.starts_with("hoist.toml") {
                        format!("https://github.com/apps/{slug}/installations/new")
                    } else {
                        format!("https://github.com/apps/{slug}")
                    },
                    status: "pending".to_string(),
                });
            }
        }
    }

    if cx.items.iter().any(|i| {
        matches!(i.kind.as_str(), "ruleset" | "tag_ruleset") && i.action == Action::Pending
    }) {
        add_plan_limit_step(&mut steps, d);
    }
    cx.items
        .sort_by(|a, b| (&a.area, &a.kind, &a.key).cmp(&(&b.area, &b.kind, &b.key)));
    (cx.items, steps)
}

/// Diff the desired Cloudflare state against what was read. Items are sorted by
/// (area, kind, key); no human steps arise in M1, the shape matches `diff_github`.
pub fn diff_cloudflare(d: &Desired, l: &LiveCloudflare) -> (Vec<Item>, Vec<HumanStep>) {
    let cf = &d.cloudflare;
    let mut cx = Ctx {
        area: "cloudflare",
        managed: cf.managed,
        items: Vec::new(),
    };
    for p in &cf.pages {
        let name = &p.project.value;
        let live = l.pages.iter().find(|(n, _)| n == name).map(|(_, f)| f);
        let live = match live.map(Fetch::as_got) {
            None => Err("not read"),
            Some(r) => r,
        };
        match live {
            Err(why) => {
                cx.unknown("pages_project", name, why, &p.project.source);
                if let Some(dom) = &p.domain {
                    cx.unknown("custom_domain", &dom.value, why, &dom.source);
                }
            }
            Ok(None) => {
                cx.push(
                    "pages_project",
                    name,
                    Action::Create,
                    "missing; would create the Pages project".to_string(),
                    &p.project.source,
                );
                if let Some(dom) = &p.domain {
                    cx.push(
                        "custom_domain",
                        &dom.value,
                        Action::Create,
                        format!("missing; Pages project `{name}` does not exist yet"),
                        &dom.source,
                    );
                }
            }
            Ok(Some(proj)) => {
                if proj.production_branch == d.default_branch {
                    cx.push(
                        "pages_project",
                        name,
                        Action::Ok,
                        format!("production branch `{}`", proj.production_branch),
                        &p.project.source,
                    );
                } else {
                    let why = format!(
                        "production branch: live `{}`, want `{}`",
                        proj.production_branch, d.default_branch
                    );
                    // --adopt does not change what an existing site deploys as production.
                    let (action, why) = match d.adopt {
                        true => (Action::Drift, format!("{why}; not written (--adopt)")),
                        false => (Action::Update, why),
                    };
                    cx.push("pages_project", name, action, why, &p.project.source);
                }
                if let Some(dom) = &p.domain {
                    if proj.domains.contains(&dom.value) {
                        cx.push(
                            "custom_domain",
                            &dom.value,
                            Action::Ok,
                            "attached".to_string(),
                            &dom.source,
                        );
                    } else {
                        cx.push(
                            "custom_domain",
                            &dom.value,
                            Action::Create,
                            format!("missing; would attach to Pages project `{name}`"),
                            &dom.source,
                        );
                    }
                }
            }
        }
    }
    for w in &cf.workers {
        let live = l
            .workers
            .iter()
            .find(|(n, _)| *n == w.value)
            .map(|(_, f)| f);
        match live.map(Fetch::as_got) {
            None => cx.unknown("workers_script", &w.value, "not read", &w.source),
            Some(Err(why)) => cx.unknown("workers_script", &w.value, why, &w.source),
            Some(Ok(true)) => cx.push(
                "workers_script",
                &w.value,
                Action::Ok,
                "deployed".to_string(),
                &w.source,
            ),
            Some(Ok(false)) => cx.push(
                "workers_script",
                &w.value,
                Action::Pending,
                "created by the first deploy".to_string(),
                &w.source,
            ),
        }
    }
    cx.items.extend(crate::r2::diff(d, l));
    cx.items
        .sort_by(|a, b| (&a.area, &a.kind, &a.key).cmp(&(&b.area, &b.kind, &b.key)));
    (cx.items, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::sh_quote;

    #[test]
    fn sh_quote_leaves_safe_words_and_quotes_the_rest() {
        assert_eq!(sh_quote("acme/x"), "acme/x");
        assert_eq!(sh_quote("prod env"), "'prod env'");
        assert_eq!(sh_quote("a;rm -rf x"), "'a;rm -rf x'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote("$(x)"), "'$(x)'");
        assert_eq!(sh_quote(""), "''");
    }
}
