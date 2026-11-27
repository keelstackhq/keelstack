//! `hoist secrets rotate <credential-id>` (spec §17.3): rotate a `system.toml` credential.
//!
//! `assisted` and `manual` make no request and write nothing: exit 2 with the human steps.
//! `auto` (Cloudflare API tokens, Fly deploy tokens) is dual-phase: create the new token, test
//! it, write it to every storage, and only then revoke the old one. Every check that can fail
//! cheaply runs before the first write and exits 3; after it, failures are report items with
//! exit 1 and the old token is never revoked unless the new value is stored everywhere.
//! A token value lives only in a `Secret`: never argv, stdout, stderr, JSON or a log.

use super::stamp::{set_keys, write_atomic};
use super::{
    CredKind, Credential, FILE, Provider, Status, Storage, StorageKind, Strategy, SystemFile,
    add_duration, source_of,
};
use crate::auth::{Secret, cloudflare_token, fly_token, github_token};
use crate::error::HoistError;
use crate::fly::{fly_command, valid_app};
use crate::http::{Fetched, Http, HttpError, Method, Service, seg};
use crate::report::{Action, HumanStep, Item, Report};
use crate::seal::{put_env_secret, put_repo_secret};
use crate::timeutil::{civil_from_days, format_rfc3339, now_secs};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

/// A fresh token may need a moment before the provider reports it active.
const TEST_ATTEMPTS: u32 = 3;
/// The legacy rotation's secret (`hoist secrets rotate CLOUDFLARE_API_TOKEN`).
const LEGACY: &str = "CLOUDFLARE_API_TOKEN";
const DEFAULT_TTL: &str = "90d";

/// Rotate `name` when `dir/system.toml` exists and declares a credential with that id;
/// `None` otherwise (the caller runs the legacy rotation).
pub fn try_rotate(dir: &Path, name: &str, verbose: bool) -> Result<Option<Report>, HoistError> {
    let path = dir.join(FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let shown = path.display().to_string();
    // An unreadable or invalid system.toml never breaks the legacy rotation of its own secret.
    let loaded = std::fs::read_to_string(&path)
        .map_err(|e| HoistError::Config {
            file: shown.clone(),
            message: format!("cannot read ({e})"),
        })
        .and_then(|text| SystemFile::parse(&text, &shown).map(|sys| (text, sys)));
    let (text, sys) = match loaded {
        Ok(v) => v,
        Err(_) if name == LEGACY => return Ok(None),
        Err(e) => return Err(e),
    };
    let Some(cred) = sys.credentials.iter().find(|c| c.id == name) else {
        return Ok(None);
    };
    if cred.status != Status::Active {
        return Err(HoistError::Usage(format!(
            "credential `{name}` is not active (status {:?}); only active credentials are rotated",
            cred.status
        )));
    }
    let ctx = Ctx {
        dir,
        path: &path,
        shown: &shown,
        text: &text,
        sys: &sys,
        cred,
        verbose,
    };
    let strategy = cred
        .rotation
        .as_ref()
        .map_or(Strategy::Manual, |r| r.strategy);
    match strategy {
        Strategy::Auto => auto(&ctx).map(Some),
        s => Ok(Some(human(&ctx, s))),
    }
}

struct Ctx<'a> {
    dir: &'a Path,
    path: &'a Path,
    shown: &'a str,
    text: &'a str,
    sys: &'a SystemFile,
    cred: &'a Credential,
    verbose: bool,
}

fn report() -> Report {
    Report {
        command: "secrets rotate".into(),
        items: Vec::new(),
        human_steps: Vec::new(),
        exit_code: 0,
        text: None,
        hint: None,
    }
}

fn day_string(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

// ---- assisted / manual ----

/// Where a person creates the provider's tokens: the declared console, else a known default.
fn console(p: Option<&Provider>, id: &str, kind: CredKind) -> String {
    if let Some(c) = p.and_then(|p| p.console.clone()) {
        return c;
    }
    match id {
        "github" if kind == CredKind::AppKey => "https://github.com/settings/apps",
        "github" => "https://github.com/settings/tokens",
        "npm" => "https://www.npmjs.com/settings/~/tokens",
        "crates" => "https://crates.io/settings/tokens",
        "google" | "gcp" | "gsc" => "https://console.cloud.google.com/iam-admin/serviceaccounts",
        "fly" => "https://fly.io/dashboard",
        "cloudflare" => "https://dash.cloudflare.com/profile/api-tokens",
        _ => "",
    }
    .to_string()
}

fn store_hint(s: &Storage) -> String {
    match s.kind {
        StorageKind::GhSecret | StorageKind::GhEnvSecret => {
            format!("`hoist secrets set {}`", s.key)
        }
        StorageKind::Sops => format!(
            "`hoist secrets edit {}`",
            s.path.clone().unwrap_or_default()
        ),
        StorageKind::FlySecret => format!("`fly secrets set {}`", s.key),
        StorageKind::WorkerSecret => format!("`wrangler secret put {}`", s.key),
    }
}

fn human(ctx: &Ctx, strategy: Strategy) -> Report {
    let c = ctx.cred;
    let url = console(ctx.sys.provider(&c.provider), &c.provider, c.kind);
    let step = |id: String, title: String| HumanStep {
        id,
        title,
        url: url.clone(),
        status: "pending".into(),
    };
    let mut steps = Vec::new();
    for (n, s) in c.rotation.iter().flat_map(|r| &r.steps).enumerate() {
        steps.push(step(format!("rotate:{}:{}", c.id, n + 1), s.clone()));
    }
    for (n, s) in c.storage.iter().enumerate() {
        steps.push(step(
            format!("store:{}:{}", c.id, n + 1),
            format!("store the new value in {} ({})", s.label(), store_hint(s)),
        ));
    }
    steps.push(step(
        format!("stamp:{}", c.id),
        format!(
            "record it: hoist system stamp {} --created YYYY-MM-DD --expires YYYY-MM-DD",
            c.id
        ),
    ));
    let mut r = report();
    r.items.push(Item {
        area: "system".into(),
        kind: "rotation".into(),
        key: c.id.clone(),
        action: Action::Pending,
        reason: format!("{}: human steps", strategy.as_str()),
        source: source_of(ctx.text, ctx.shown, &c.id),
        verify: None,
    });
    r.human_steps = steps;
    r.exit_code = 2;
    r
}

// ---- auto ----

/// What the old token is and where it lives, per provider.
#[allow(clippy::large_enum_variant)]
enum Prov {
    Cloudflare {
        http: Http,
        /// `/user/tokens` or `/accounts/<id>/tokens`.
        base: String,
        old_id: String,
        name: String,
        policies: Vec<Value>,
        /// The old token's `condition` (request.ip) and `not_before`, copied so a rotation
        /// never widens a token.
        condition: Option<Value>,
        not_before: Option<Value>,
    },
    Fly {
        app: String,
        old_id: String,
        /// The ids listed in preflight, to tell the new token's id apart afterwards.
        before: Option<BTreeSet<String>>,
    },
}

/// The new token: its id when known, its value, and the `fly tokens list` before it.
struct New {
    id: Option<String>,
    value: Secret,
}

struct Out {
    r: Report,
    id: String,
    source: String,
}

impl Out {
    fn push(&mut self, area: &str, kind: &str, key: &str, action: Action, reason: String) {
        self.r.items.push(Item {
            area: area.into(),
            kind: kind.into(),
            key: key.into(),
            action,
            reason,
            source: self.source.clone(),
            verify: None,
        });
    }

    /// The rotation did not complete: exit 1.
    fn fail(mut self, why: &str) -> Report {
        let id = self.id.clone();
        self.push(
            "system",
            "rotation",
            &id,
            Action::Update,
            format!("failed: {why}"),
        );
        self.r.exit_code = 1;
        self.r
    }

    fn manual_revoke(&mut self, area: &str, what: &str, id: &str, why: &str, url: &str) {
        self.push(
            area,
            "api_token",
            &self.id.clone(),
            Action::Update,
            format!("failed: revoking {what} {id}: {why}"),
        );
        self.r.human_steps.push(HumanStep {
            id: format!("revoke-token:{id}"),
            title: format!("revoke {area} token {id} in the console"),
            url: url.into(),
            status: "pending".into(),
        });
        self.r.exit_code = 1;
    }
}

fn usage(msg: impl Into<String>) -> HoistError {
    HoistError::Usage(msg.into())
}

fn valid_key(k: &str) -> bool {
    !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `owner/name` of a storage's repo.
fn repo_of(s: &Storage) -> Result<(&str, &str), HoistError> {
    s.repo
        .as_deref()
        .and_then(|r| r.split_once('/'))
        .filter(|(o, n)| !o.is_empty() && !n.is_empty() && !n.contains('/'))
        .ok_or_else(|| usage(format!("storage {}: `repo` must be owner/name", s.label())))
}

fn auto(ctx: &Ctx) -> Result<Report, HoistError> {
    let c = ctx.cred;
    // Preflight: nothing below writes. Every error here exits 3.
    let fly = match c.provider.as_str() {
        "cloudflare" => false,
        "fly" => true,
        _ => {
            return Err(usage(
                "auto rotation supports cloudflare and fly; use assisted",
            ));
        }
    };
    if c.storage.is_empty() {
        return Err(usage(format!(
            "credential `{}` declares no storage; a rotated value would have nowhere to go",
            c.id
        )));
    }
    for s in &c.storage {
        if !matches!(
            s.kind,
            StorageKind::GhSecret | StorageKind::GhEnvSecret | StorageKind::FlySecret
        ) {
            return Err(usage(format!(
                "storage {} cannot be written by hoist; set rotation.strategy = \"assisted\"",
                s.label()
            )));
        }
        if !valid_key(&s.key) {
            return Err(usage(format!(
                "storage {}: `key` must be letters, digits and `_`",
                s.label()
            )));
        }
        match s.kind {
            StorageKind::FlySecret => {
                if !s.app.as_deref().is_some_and(valid_app) {
                    return Err(usage(format!("storage {}: invalid Fly app", s.label())));
                }
            }
            _ => {
                repo_of(s)?;
            }
        }
    }
    let reference = c
        .provider_ref
        .as_deref()
        .map(|r| {
            r.trim()
                .strip_prefix("token id ")
                .unwrap_or(r.trim())
                .trim()
        })
        .filter(|r| !r.is_empty())
        .ok_or_else(|| {
            usage(format!(
                "credential `{}` has no provider_ref; auto rotation needs the old token's id (or \
                 name) to find and revoke it. Set it with `hoist system stamp {} --created \
                 YYYY-MM-DD --ref <id>`",
                c.id, c.id
            ))
        })?;
    let gh = if c.storage.iter().any(|s| s.kind != StorageKind::FlySecret) {
        Some(Http::new(Service::Github, github_token()?).verbose(ctx.verbose))
    } else {
        None
    };
    let today = now_secs().div_euclid(86_400);
    let ttl = c
        .max_ttl
        .as_deref()
        .or(c.rotate_every.as_deref())
        .unwrap_or(DEFAULT_TTL);
    let expiry = add_duration(today, ttl).ok_or_else(|| usage(format!("bad duration `{ttl}`")))?;
    let mut prov = if fly {
        fly_token()?;
        let app = c
            .storage
            .iter()
            .find(|s| s.kind == StorageKind::FlySecret)
            .and_then(|s| s.app.clone())
            .or_else(|| {
                c.resources
                    .iter()
                    .find_map(|r| r.strip_prefix("app:").map(str::to_string))
            })
            .filter(|a| valid_app(a))
            .ok_or_else(|| {
                usage(format!(
                    "credential `{}`: no Fly app (a fly-secret storage `app` or resources \
                     `app:<name>`)",
                    c.id
                ))
            })?;
        let rows = fly_rows(&app, ctx.dir);
        let old_id = resolve_fly_id(reference, rows.as_deref())?;
        Prov::Fly {
            before: rows.map(|r| r.into_iter().map(|(id, _)| id).collect()),
            app,
            old_id,
        }
    } else {
        find_cloudflare(reference, ctx.verbose)?
    };

    // First write: create the new token.
    let mut out = Out {
        r: report(),
        id: c.id.clone(),
        source: source_of(ctx.text, ctx.shown, &c.id),
    };
    let new = match prov.create(ctx, expiry - today, expiry) {
        Ok(n) => n,
        Err(why) => return Ok(out.fail(&format!("creating the new token: {why}"))),
    };
    // Test it before it replaces anything: a dead token is taken back, nothing was stored.
    if let Err(why) = prov.test(&new.value) {
        let note = prov.revoke_new(ctx, &new, &mut out);
        return Ok(out.fail(&format!("testing the new token: {why}{note}")));
    }
    // Set it everywhere.
    let mut written: Vec<String> = Vec::new();
    for s in &c.storage {
        if let Err(why) = set_storage(ctx, gh.as_ref(), s, &new.value) {
            let rest: Vec<String> = c
                .storage
                .iter()
                .map(Storage::label)
                .filter(|l| !written.contains(l))
                .collect();
            if written.is_empty() {
                let note = prov.revoke_new(ctx, &new, &mut out);
                return Ok(out.fail(&format!(
                    "writing {}: {why}; nothing was written, the old token is untouched{note}",
                    s.label()
                )));
            }
            return Ok(out.fail(&format!(
                "writing {}: {why}; the new value is stored in {} but not in {}; both tokens \
                 are kept (new token {}): fix the storage and rotate again",
                s.label(),
                written.join(", "),
                rest.join(", "),
                new.id.as_deref().unwrap_or("id unknown"),
            )));
        }
        written.push(s.label());
        out.push(
            if s.kind == StorageKind::FlySecret {
                "fly"
            } else {
                "github"
            },
            "secret",
            &s.label(),
            Action::Ok,
            "set".into(),
        );
    }
    let tokens = if fly { "fly" } else { "cloudflare" };
    out.push(
        tokens,
        "api_token",
        &c.id,
        Action::Create,
        match &new.id {
            Some(id) => format!("created {id}"),
            None => "created (id unknown)".into(),
        },
    );
    // Revoke the old one only now.
    let old = prov.old_id().to_string();
    match prov.revoke_old(ctx) {
        Ok(()) => out.push(
            tokens,
            "api_token",
            &c.id,
            Action::Ok,
            format!("revoked {old}"),
        ),
        Err(why) => {
            let url = console(ctx.sys.provider(&c.provider), &c.provider, c.kind);
            out.manual_revoke(tokens, "token", &old, &why, &url);
        }
    }
    // Stamp system.toml.
    let created = day_string(today);
    let expires = day_string(expiry);
    let via = format!("hoist secrets rotate {}", c.id);
    let new_ref = new.id.as_ref().map(|id| format!("token id {id}"));
    let mut pairs = vec![
        ("created_at", created.as_str()),
        ("expires_at", expires.as_str()),
        ("created_via", via.as_str()),
    ];
    pairs.extend(new_ref.as_deref().map(|r| ("provider_ref", r)));
    // Re-read just before stamping: the file may have changed during the rotation.
    let stamped = std::fs::read_to_string(ctx.path)
        .map_err(|e| HoistError::Io(format!("cannot read: {e}")))
        .and_then(|fresh| set_keys(&fresh, ctx.shown, &c.id, &pairs))
        .and_then(|t| {
            write_atomic(ctx.path, &t).map_err(|e| HoistError::Io(format!("cannot write: {e}")))
        });
    match stamped {
        Ok(()) => out.push(
            "system",
            "stamp",
            &c.id,
            Action::Update,
            format!("created_at {created}, expires_at {expires}"),
        ),
        Err(err) => {
            out.push(
                "system",
                "stamp",
                &c.id,
                Action::Update,
                format!("failed: updating {FILE}: {err}"),
            );
            out.r.human_steps.push(HumanStep {
                id: format!("stamp:{}", c.id),
                title: format!(
                    "record it: hoist system stamp {} --created {created} --expires {expires}",
                    c.id
                ),
                url: String::new(),
                status: "pending".into(),
            });
            out.r.exit_code = 1;
        }
    }
    if new.id.is_none() {
        out.push(
            "system",
            "provider_ref",
            &c.id,
            Action::Unknown,
            format!(
                "the new token's id is unknown; provider_ref still names the old one. Set it: \
                 hoist system stamp {} --created {created} --ref <new id>",
                c.id
            ),
        );
    }
    Ok(out.r)
}

/// Write `value` into one storage. The value goes to GitHub sealed, to Fly over stdin.
fn set_storage(ctx: &Ctx, gh: Option<&Http>, s: &Storage, value: &Secret) -> Result<(), String> {
    match s.kind {
        StorageKind::GhSecret => {
            let (o, n) = repo_of(s).map_err(|e| e.to_string())?;
            let gh = gh.ok_or("no GitHub client")?;
            put_repo_secret(gh, (o, n), &s.key, value).map_err(|e| e.to_string())
        }
        StorageKind::GhEnvSecret => {
            let (o, n) = repo_of(s).map_err(|e| e.to_string())?;
            let env = s.env.as_deref().unwrap_or_default();
            let gh = gh.ok_or("no GitHub client")?;
            put_env_secret(gh, (o, n), env, &s.key, value).map_err(|e| e.to_string())
        }
        StorageKind::FlySecret => {
            let app = s.app.as_deref().unwrap_or_default();
            let mut child = fly_command(&["secrets", "import", "-a", app], ctx.dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("could not run `fly secrets import`: {e}"))?;
            let mut stdin = child.stdin.take().expect("piped stdin");
            let sent = writeln!(stdin, "{}={}", s.key, value.expose());
            drop(stdin);
            let status = child
                .wait()
                .map_err(|e| format!("`fly secrets import`: {e}"))?;
            if sent.is_err() || !status.success() {
                return Err(format!("`fly secrets import` failed ({status})"));
            }
            Ok(())
        }
        _ => Err("this storage cannot be written by hoist".into()),
    }
}

/// Find the old Cloudflare token by id or name: the user's tokens, then the account's.
fn find_cloudflare(reference: &str, verbose: bool) -> Result<Prov, HoistError> {
    let http = Http::new(Service::Cloudflare, cloudflare_token()?).verbose(verbose);
    let mut bases = vec!["/user/tokens".to_string()];
    if let Some(acc) = crate::auth::from_env("CLOUDFLARE_ACCOUNT_ID") {
        bases.push(format!("/accounts/{}/tokens", seg(acc.expose())));
    }
    for (n, base) in bases.iter().enumerate() {
        let list = match http.get_all(base, None)? {
            Fetched::Ok(Value::Array(list)) => list,
            Fetched::Ok(_) => continue,
            Fetched::NotFound | Fetched::Forbidden(_) if n > 0 => continue,
            Fetched::NotFound => {
                return Err(usage(format!("Cloudflare has no GET {base} (404)")));
            }
            Fetched::Forbidden(hint) => {
                return Err(usage(format!(
                    "Cloudflare refused GET {base} (403): {hint}. The token needs permission to \
                     list API tokens"
                )));
            }
        };
        let str_of = |t: &Value, k: &str| t.get(k).and_then(Value::as_str).map(str::to_string);
        let Some(old) = list.iter().find(|t| {
            str_of(t, "id").as_deref() == Some(reference)
                || str_of(t, "name").as_deref() == Some(reference)
        }) else {
            continue;
        };
        let policies: Vec<Value> = old
            .get("policies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|p| {
                let groups: Vec<Value> = p
                    .get("permission_groups")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|g| g.get("id").cloned())
                    .map(|id| json!({ "id": id }))
                    .collect();
                json!({
                    "effect": p.get("effect").cloned().unwrap_or(json!("allow")),
                    "resources": p.get("resources").cloned().unwrap_or(json!({})),
                    "permission_groups": groups,
                })
            })
            .collect();
        if policies.is_empty() {
            return Err(usage(format!(
                "the Cloudflare token `{reference}` lists no policies to copy; rotate it by hand \
                 (set rotation.strategy = \"assisted\")"
            )));
        }
        return Ok(Prov::Cloudflare {
            base: base.clone(),
            old_id: str_of(old, "id").unwrap_or_default(),
            name: str_of(old, "name").unwrap_or_else(|| reference.to_string()),
            condition: old.get("condition").filter(|c| !c.is_null()).cloned(),
            not_before: old.get("not_before").filter(|c| !c.is_null()).cloned(),
            policies,
            http,
        });
    }
    Err(usage(format!(
        "no Cloudflare token with id or name `{reference}` in GET /user/tokens{}; fix \
         provider_ref (`hoist system stamp <id> --created YYYY-MM-DD --ref <id>`)",
        if bases.len() > 1 {
            " or the account's"
        } else {
            ""
        }
    )))
}

/// The old Fly token's id: `reference` when it is an id in the list, else the one token of that
/// name. Anything else (no list, no match, several matches) is a usage error before any write.
fn resolve_fly_id(
    reference: &str,
    rows: Option<&[(String, String)]>,
) -> Result<String, HoistError> {
    let fix = "set provider_ref to the Fly token id (`fly tokens list`)";
    let Some(rows) = rows else {
        return Err(usage(format!(
            "cannot list Fly tokens to find `{reference}`; {fix}"
        )));
    };
    if rows.iter().any(|(id, _)| id == reference) {
        return Ok(reference.to_string());
    }
    let named: Vec<&str> = rows
        .iter()
        .filter(|(_, n)| !n.is_empty() && n == reference)
        .map(|(id, _)| id.as_str())
        .collect();
    match named[..] {
        [id] => Ok(id.to_string()),
        [] => Err(usage(format!(
            "no Fly token with id or name `{reference}`; {fix}"
        ))),
        _ => Err(usage(format!(
            "provider_ref `{reference}` matches {} Fly tokens; {fix}",
            named.len()
        ))),
    }
}

/// `(id, name)` of every token `fly tokens list` shows, or `None` when it cannot be read.
fn fly_rows(app: &str, dir: &Path) -> Option<Vec<(String, String)>> {
    let out = fly_command(&["tokens", "list", "-a", app, "--json"], dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    let arr = v.get("tokens").unwrap_or(&v).as_array()?;
    Some(
        arr.iter()
            .filter_map(|t| {
                let id = t.get("id").and_then(Value::as_str)?;
                let name = t.get("name").and_then(Value::as_str).unwrap_or_default();
                Some((id.to_string(), name.to_string()))
            })
            .collect(),
    )
}

fn fly_list(app: &str, dir: &Path) -> Option<BTreeSet<String>> {
    fly_rows(app, dir).map(|r| r.into_iter().map(|(id, _)| id).collect())
}

impl Prov {
    fn old_id(&self) -> &str {
        match self {
            Prov::Cloudflare { old_id, .. } | Prov::Fly { old_id, .. } => old_id,
        }
    }

    fn create(&mut self, ctx: &Ctx, days: i64, expiry: i64) -> Result<New, String> {
        match self {
            Prov::Cloudflare {
                http,
                base,
                name,
                policies,
                condition,
                not_before,
                ..
            } => {
                // Not `PUT <base>/<id>/value` (roll): it invalidates the old value at once,
                // which would break every consumer until the new value is stored. A second
                // token keeps the old one valid until it is revoked.
                let mut body = json!({
                    "name": name,
                    "policies": policies,
                    "expires_on": format_rfc3339(expiry * 86_400),
                });
                for (key, v) in [("condition", condition), ("not_before", not_before)] {
                    if let Some(v) = v {
                        body[key] = v.clone();
                    }
                }
                let written = http
                    .send_json(Method::Post, base, Some(&body))
                    .map_err(|e| e.to_string())?;
                let mut resp = written.body.unwrap_or(Value::Null);
                let id = resp
                    .pointer("/result/id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let value = match resp.pointer_mut("/result/value").map(Value::take) {
                    Some(Value::String(v)) => Some(Secret::from(v)),
                    _ => None,
                };
                drop(resp);
                match (id, value) {
                    (Some(id), Some(value)) => Ok(New {
                        id: Some(id),
                        value,
                    }),
                    (Some(id), None) => {
                        let _ = cf_revoke(http, base, &id);
                        Err(format!(
                            "Cloudflare returned no token value (token {id} revoked)"
                        ))
                    }
                    _ => Err(
                        "Cloudflare returned no token id; a token named like the old one \
                              may exist: revoke it in the dashboard"
                            .into(),
                    ),
                }
            }
            Prov::Fly { app, before, .. } => {
                let before = before.clone();
                let expiry = format!("{}h", days * 24);
                let out = fly_command(
                    &["tokens", "create", "deploy", "-a", app, "--expiry", &expiry],
                    ctx.dir,
                )
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .map_err(|e| format!("could not run `fly tokens create`: {e}"))?;
                if !out.status.success() {
                    return Err(format!("`fly tokens create` failed ({})", out.status));
                }
                let value = String::from_utf8(out.stdout)
                    .map_err(|_| "`fly tokens create` printed no text".to_string())?;
                let value = Secret::from(value.trim().to_string());
                if value.expose().is_empty() {
                    return Err("`fly tokens create` printed no token".into());
                }
                let id = match (before, fly_list(app, ctx.dir)) {
                    (Some(b), Some(a)) => {
                        let added: Vec<&String> = a.difference(&b).collect();
                        (added.len() == 1).then(|| added[0].clone())
                    }
                    _ => None,
                };
                Ok(New { id, value })
            }
        }
    }

    /// Check the new value works at the provider; up to `TEST_ATTEMPTS` tries.
    fn test(&self, value: &Secret) -> Result<(), String> {
        let (http, path) = match self {
            Prov::Cloudflare { http, base, .. } => (
                Http::with_base_like(http, value.clone()),
                if base == "/user/tokens" {
                    crate::cloudflare::VERIFY_PATH.to_string()
                } else {
                    format!("{base}/verify")
                },
            ),
            Prov::Fly { app, .. } => (
                Http::new(Service::Fly, value.clone()),
                format!("/apps/{}", seg(app)),
            ),
        };
        let mut last = String::new();
        for attempt in 1..=TEST_ATTEMPTS {
            if attempt > 1 {
                http.pause(1);
            }
            last = match http.get_json(&path) {
                Ok(Fetched::Ok(v)) => {
                    let status = v.pointer("/result/status").and_then(Value::as_str);
                    if matches!(self, Prov::Cloudflare { .. }) && status != Some("active") {
                        format!("status `{}`", status.unwrap_or("missing"))
                    } else {
                        return Ok(());
                    }
                }
                Ok(Fetched::NotFound) => "not found (404)".into(),
                Ok(Fetched::Forbidden(hint)) => hint,
                Err(HttpError::Unauthorized { .. }) => "rejected (401)".into(),
                Err(e) => return Err(e.to_string()),
            };
        }
        Err(format!(
            "the new token did not pass after {TEST_ATTEMPTS} attempts (GET {path}: {last})"
        ))
    }

    /// Take the new token back after a failure; the returned note says what to do if that failed.
    fn revoke_new(&self, ctx: &Ctx, new: &New, out: &mut Out) -> String {
        let Some(id) = &new.id else {
            out.r.human_steps.push(HumanStep {
                id: format!("revoke-new:{}", ctx.cred.id),
                title: "revoke the newly created token in the provider console".into(),
                url: String::new(),
                status: "pending".into(),
            });
            return "; the new token's id is unknown: revoke it by hand".into();
        };
        let done = match self {
            Prov::Cloudflare { http, base, .. } => {
                cf_revoke(http, base, id).map_err(|e| e.to_string())
            }
            Prov::Fly { .. } => fly_revoke(ctx, id),
        };
        match done {
            Ok(()) => format!("; the new token {id} was revoked"),
            Err(why) => {
                let url = console(
                    ctx.sys.provider(&ctx.cred.provider),
                    &ctx.cred.provider,
                    ctx.cred.kind,
                );
                out.r.human_steps.push(HumanStep {
                    id: format!("revoke-token:{id}"),
                    title: format!("revoke token {id} in the provider console"),
                    url,
                    status: "pending".into(),
                });
                format!("; revoking the new token {id} failed ({why}): revoke it by hand")
            }
        }
    }

    fn revoke_old(&self, ctx: &Ctx) -> Result<(), String> {
        match self {
            Prov::Cloudflare {
                http, base, old_id, ..
            } => cf_revoke(http, base, old_id).map_err(|e| e.to_string()),
            Prov::Fly { old_id, .. } => fly_revoke(ctx, old_id),
        }
    }
}

/// Delete a Cloudflare token; a 404 means it is already gone.
fn cf_revoke(http: &Http, base: &str, id: &str) -> Result<(), HttpError> {
    match http.send_json(Method::Delete, &format!("{base}/{}", seg(id)), None) {
        Ok(_) | Err(HttpError::Rejected { status: 404, .. }) => Ok(()),
        Err(e) => Err(e),
    }
}

fn fly_revoke(ctx: &Ctx, id: &str) -> Result<(), String> {
    let status = fly_command(&["tokens", "revoke", id], ctx.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("could not run `fly tokens revoke`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`fly tokens revoke` failed ({status})"))
    }
}
