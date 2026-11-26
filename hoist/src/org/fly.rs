//! `[fly]` of org.toml (spec §16.7, D-029): the Fly.io org, its apps, app secrets (names only)
//! and custom-domain certificates, through the Machines HTTP API (no `fly` CLI). Reads are GET
//! only. `apply` creates missing apps and certificates; it never sets secret values, never
//! deletes and never moves machines. A missing token, a 401 or a 403 makes the items `unknown`.

use super::{SectionPlan, item, rejected, step};
use crate::auth::from_env;
use crate::error::HoistError;
use crate::github::Fetch;
use crate::http::{Fetched, Http, HttpError, Method, Service, seg};
use crate::report::{Action, Item};
use serde::Deserialize;
use serde_json::{Value, json};

const DASHBOARD: &str = "https://fly.io/dashboard";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct App {
    pub name: String,
    /// Primary region (three letters, e.g. `fra`): verified against the app's machines.
    pub region: Option<String>,
    /// Secret names that must exist on the app; values are never read or written.
    #[serde(default)]
    pub secrets: Vec<String>,
    /// Custom domains that need a certificate.
    #[serde(default)]
    pub certificates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlyCfg {
    /// The Fly organization slug.
    pub org: String,
    #[serde(default)]
    pub apps: Vec<App>,
}

pub(super) fn validate(cfg: &FlyCfg) -> Result<(), String> {
    if !crate::fly::valid_app(&cfg.org) {
        return Err(format!(
            "[fly] org `{}` must be a Fly organization slug (letters, digits, `-`)",
            cfg.org
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for a in &cfg.apps {
        if !crate::fly::valid_app(&a.name) || a.name.contains(['.', '_']) {
            return Err(format!(
                "[fly] app `{}` must be a Fly app name (lower-case letters, digits, `-`)",
                a.name
            ));
        }
        if !seen.insert(a.name.as_str()) {
            return Err(format!("[fly] app `{}` is declared twice", a.name));
        }
        if let Some(r) = &a.region
            && !(r.len() == 3 && r.chars().all(|c| c.is_ascii_lowercase()))
        {
            return Err(format!(
                "[fly] app `{}`: region `{r}` must be a three-letter Fly region code (e.g. fra)",
                a.name
            ));
        }
        for s in &a.secrets {
            let ok = s
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !ok {
                return Err(format!(
                    "[fly] app `{}`: secret name `{s}` must match [A-Za-z_][A-Za-z0-9_]*",
                    a.name
                ));
            }
        }
        for h in &a.certificates {
            let ok = h.contains('.')
                && h.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '*'))
                && !h.starts_with('.')
                && !h.ends_with('.');
            if !ok {
                return Err(format!(
                    "[fly] app `{}`: certificate `{h}` must be a host name like `api.example.dev`",
                    a.name
                ));
            }
        }
    }
    Ok(())
}

/// A client when `FLY_API_TOKEN` is set (`unknown` items otherwise).
pub(super) fn client(verbose: bool) -> Option<Http> {
    from_env("FLY_API_TOKEN").map(|t| Http::new(Service::Fly, t).verbose(verbose))
}

/// One write of `apply` (apps before certificates).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Write {
    App { org: String, name: String },
    Cert { app: String, hostname: String },
}

impl Write {
    pub(crate) fn label(&self) -> String {
        match self {
            Write::App { org, name } => format!("create fly/app {name} (org {org})"),
            Write::Cert { app, hostname } => format!("create fly/certificate {app}/{hostname}"),
        }
    }
}

/// `Got(None)` is a 404; 401, 403 and transport failures are `Unknown`.
fn fetch(http: &Http, path: &str, what: &str) -> Fetch<Option<Value>> {
    match http.get_json(path) {
        Ok(Fetched::Ok(v)) => Fetch::Got(Some(v)),
        Ok(Fetched::NotFound) => Fetch::Got(None),
        Ok(Fetched::Forbidden(h)) => Fetch::Unknown(format!("cannot read {what}: {h}")),
        Err(HttpError::Unauthorized { .. }) => Fetch::Unknown(format!(
            "cannot read {what}: Fly.io rejected the token (401); check FLY_API_TOKEN"
        )),
        Err(e) => Fetch::Unknown(format!("cannot read {what}: {e}")),
    }
}

/// The array under `key` (or the body itself when it is an array).
fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.as_array()
        .or_else(|| v.get(key).and_then(Value::as_array))
        .map_or(&[][..], Vec::as_slice)
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn cert_ready(c: &Value) -> bool {
    text(c, "status").is_none_or(|s| {
        ["ready", "issued", "active"]
            .iter()
            .any(|ok| s.eq_ignore_ascii_case(ok))
    })
}

pub(crate) fn read(cfg: &FlyCfg, http: Option<&Http>) -> Result<SectionPlan<Write>, HoistError> {
    let mut p = SectionPlan::default();
    let org = cfg.org.as_str();
    let Some(http) = http else {
        let why = "no Fly.io token (set FLY_API_TOKEN)";
        p.items.push(item("fly", "org", org, Action::Unknown, why));
        for a in &cfg.apps {
            p.items
                .push(item("fly", "app", &a.name, Action::Unknown, why));
            for k in unreadable_children(a) {
                p.items.push(item("fly", k.0, &k.1, Action::Unknown, why));
            }
        }
        p.steps.push(step(
            "fly:token:FLY_API_TOKEN".into(),
            "create an org-scoped Fly token (`fly tokens create org`) and give it to the sandbox \
             as FLY_API_TOKEN (hoist verifies it by reading the org's apps)"
                .into(),
            DASHBOARD,
            "confirm",
        ));
        return Ok(p);
    };
    let apps = match fetch(
        http,
        &format!("/apps?org_slug={}", seg(org)),
        "the Fly apps",
    ) {
        Fetch::Got(Some(v)) => Fetch::Got(Some(list(&v, "apps").to_vec())),
        Fetch::Got(None) => Fetch::Got(None),
        Fetch::Unknown(why) => Fetch::Unknown(why),
    };
    let live = match apps {
        Fetch::Got(Some(l)) => {
            p.items
                .push(item("fly", "org", org, Action::Ok, "apps are readable"));
            l
        }
        Fetch::Got(None) => {
            p.items.push(item(
                "fly",
                "org",
                org,
                Action::Pending,
                "org not found (404)",
            ));
            p.steps.push(step(
                format!("fly:org:{org}"),
                format!(
                    "create the Fly organization `{org}` (hoist verifies it by listing its apps)"
                ),
                DASHBOARD,
                "pending",
            ));
            for a in &cfg.apps {
                p.items.push(item(
                    "fly",
                    "app",
                    &a.name,
                    Action::Unknown,
                    "the Fly organization does not exist yet",
                ));
            }
            return Ok(p);
        }
        Fetch::Unknown(why) => {
            p.items.push(item("fly", "org", org, Action::Unknown, &why));
            for a in &cfg.apps {
                p.items
                    .push(item("fly", "app", &a.name, Action::Unknown, &why));
                for k in unreadable_children(a) {
                    p.items.push(item("fly", k.0, &k.1, Action::Unknown, &why));
                }
            }
            return Ok(p);
        }
    };
    for a in &cfg.apps {
        let exists = live
            .iter()
            .any(|l| text(l, "name") == Some(a.name.as_str()));
        let name = a.name.as_str();
        if !exists {
            p.items.push(item(
                "fly",
                "app",
                name,
                Action::Create,
                format!("missing in org {org}"),
            ));
            p.writes.push(Write::App {
                org: org.into(),
                name: name.into(),
            });
            // Certificates need only the app; secrets wait for it (a re-run after apply).
            for h in &a.certificates {
                p.items.push(item(
                    "fly",
                    "certificate",
                    &format!("{name}/{h}"),
                    Action::Create,
                    "missing (the app is created first)",
                ));
                p.writes.push(Write::Cert {
                    app: name.into(),
                    hostname: h.clone(),
                });
            }
            for s in &a.secrets {
                p.items.push(item(
                    "fly",
                    "secret",
                    &format!("{name}/{s}"),
                    Action::Unknown,
                    "the app does not exist yet",
                ));
            }
            continue;
        }
        p.items
            .push(item("fly", "app", name, Action::Ok, "present"));
        if let Some(region) = &a.region {
            region_item(&mut p, http, name, region);
        }
        if !a.secrets.is_empty() {
            secrets_items(&mut p, http, a);
        }
        if !a.certificates.is_empty() {
            cert_items(&mut p, http, a);
        }
    }
    Ok(p)
}

fn unreadable_children(a: &App) -> Vec<(&'static str, String)> {
    let mut v = Vec::new();
    if a.region.is_some() {
        v.push(("region", a.name.clone()));
    }
    v.extend(
        a.secrets
            .iter()
            .map(|s| ("secret", format!("{}/{s}", a.name))),
    );
    v.extend(
        a.certificates
            .iter()
            .map(|h| ("certificate", format!("{}/{h}", a.name))),
    );
    v
}

/// The primary region is set by the first deploy (`fly.toml`); hoist checks the machines.
fn region_item(p: &mut SectionPlan<Write>, http: &Http, app: &str, region: &str) {
    match fetch(
        http,
        &format!("/apps/{}/machines", seg(app)),
        "the machines",
    ) {
        Fetch::Unknown(why) => p
            .items
            .push(item("fly", "region", app, Action::Unknown, why)),
        Fetch::Got(None) => p.items.push(item(
            "fly",
            "region",
            app,
            Action::Unknown,
            "cannot read the machines: not found (404)",
        )),
        Fetch::Got(Some(v)) => {
            let regions: Vec<&str> = list(&v, "machines")
                .iter()
                .filter_map(|m| text(m, "region"))
                .collect();
            if regions.is_empty() || regions.contains(&region) {
                let why = if regions.is_empty() {
                    format!("no machines yet; the first deploy sets {region}")
                } else {
                    format!("machines run in {region}")
                };
                p.items.push(item("fly", "region", app, Action::Ok, why));
            } else {
                p.items.push(item(
                    "fly",
                    "region",
                    app,
                    Action::Pending,
                    format!("no machine in {region} (found {})", regions.join(", ")),
                ));
                p.steps.push(step(
                    format!("fly:region:{app}"),
                    format!(
                        "run machines of `{app}` in {region} (`primary_region` in fly.toml, then \
                         `fly deploy`); hoist verifies it by reading the machines' regions"
                    ),
                    &format!("https://fly.io/apps/{app}"),
                    "pending",
                ));
            }
        }
    }
}

fn secrets_items(p: &mut SectionPlan<Write>, http: &Http, a: &App) {
    let app = a.name.as_str();
    let have = match fetch(
        http,
        &format!("/apps/{}/secrets", seg(app)),
        "the app secrets",
    ) {
        Fetch::Got(Some(v)) => Fetch::Got(
            list(&v, "secrets")
                .iter()
                .filter_map(|s| text(s, "name").or_else(|| s.as_str()).map(str::to_string))
                .collect::<Vec<_>>(),
        ),
        Fetch::Got(None) => Fetch::Unknown("cannot read the app secrets: not found (404)".into()),
        Fetch::Unknown(why) => Fetch::Unknown(why),
    };
    for s in &a.secrets {
        let key = format!("{app}/{s}");
        match &have {
            Fetch::Unknown(why) => p
                .items
                .push(item("fly", "secret", &key, Action::Unknown, why)),
            Fetch::Got(names) if names.contains(s) => {
                p.items
                    .push(item("fly", "secret", &key, Action::Ok, "set (name only)"))
            }
            Fetch::Got(_) => {
                p.items
                    .push(item("fly", "secret", &key, Action::Pending, "not set"));
                p.steps.push(step(
                    format!("fly:secret:{key}"),
                    format!(
                        "set `{s}` on `{app}` (`fly secrets set {s} -a {app}` reads the value from \
                         a prompt or stdin, never put it in the command line; hoist verifies the \
                         name, never the value)"
                    ),
                    &format!("https://fly.io/apps/{app}/secrets"),
                    "pending",
                ));
            }
        }
    }
}

fn cert_items(p: &mut SectionPlan<Write>, http: &Http, a: &App) {
    let app = a.name.as_str();
    let certs = fetch(
        http,
        &format!("/apps/{}/certificates", seg(app)),
        "the certificates",
    );
    for h in &a.certificates {
        let key = format!("{app}/{h}");
        match &certs {
            Fetch::Unknown(why) => {
                p.items
                    .push(item("fly", "certificate", &key, Action::Unknown, why))
            }
            Fetch::Got(None) => p.items.push(item(
                "fly",
                "certificate",
                &key,
                Action::Unknown,
                "cannot read the certificates: not found (404)",
            )),
            Fetch::Got(Some(v)) => {
                let live = list(v, "certificates")
                    .iter()
                    .find(|c| text(c, "hostname").is_some_and(|x| x.eq_ignore_ascii_case(h)));
                match live {
                    None => {
                        p.items
                            .push(item("fly", "certificate", &key, Action::Create, "missing"));
                        p.writes.push(Write::Cert {
                            app: app.into(),
                            hostname: h.clone(),
                        });
                    }
                    Some(c) if cert_ready(c) => {
                        p.items
                            .push(item("fly", "certificate", &key, Action::Ok, "issued"))
                    }
                    Some(c) => {
                        p.items.push(item(
                            "fly",
                            "certificate",
                            &key,
                            Action::Pending,
                            format!(
                                "not issued yet (status {})",
                                text(c, "status").unwrap_or("?")
                            ),
                        ));
                        p.steps.push(step(
                            format!("fly:certificate:{key}"),
                            format!(
                                "point DNS of `{h}` at `{app}` (hoist writes Cloudflare records \
                                 from org.toml; run `fly certs check {h} -a {app}` for the targets); \
                                 hoist verifies the certificate status"
                            ),
                            &format!("https://fly.io/apps/{app}/certificates"),
                            "pending",
                        ));
                    }
                }
            }
        }
    }
}

/// Create the missing apps, then the certificates; the items returned are the failed writes.
pub(crate) fn apply(http: &Http, writes: &[Write]) -> Result<Vec<Item>, HoistError> {
    let mut failed = Vec::new();
    let apps = writes.iter().filter(|w| matches!(w, Write::App { .. }));
    let certs = writes.iter().filter(|w| matches!(w, Write::Cert { .. }));
    for w in apps.chain(certs) {
        let r = match w {
            Write::App { org, name } => http.send_json(
                Method::Post,
                "/apps",
                Some(&json!({"app_name": name, "org_slug": org})),
            ),
            Write::Cert { app, hostname } => http.send_json(
                Method::Post,
                &format!("/apps/{}/certificates/acme", seg(app)),
                Some(&json!({"hostname": hostname})),
            ),
        };
        failed.extend(rejected("fly", &w.label(), r)?);
    }
    Ok(failed)
}
