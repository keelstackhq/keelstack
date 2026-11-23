//! `[search_console]` of org.toml (spec §16.2-§16.4). Reads are GET only against the bridge
//! Worker; `apply` verifies the domain through a Cloudflare TXT record, then adds the property
//! and its sitemaps. Without a bridge URL or token the items are `unknown`.

use super::{SectionPlan, item, rejected, step};
use crate::auth::from_env;
use crate::error::HoistError;
use crate::github::Fetch;
use crate::http::{Fetched, Http, HttpError, Method, Service, seg};
use crate::report::{Action, HumanStep, Item};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub domain: String,
    /// The Cloudflare zone for the verification TXT record.
    pub zone: String,
    #[serde(default)]
    pub sitemaps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchConsoleCfg {
    /// Else env `HOIST_SEARCH_CONSOLE_BRIDGE`.
    pub bridge: Option<String>,
    #[serde(default)]
    pub sites: Vec<Site>,
}

/// `zone` equals or is a parent of `domain`; sitemaps are https URLs on the domain or below.
pub(super) fn validate(cfg: &SearchConsoleCfg) -> Result<(), String> {
    for s in &cfg.sites {
        if !super::under(&s.domain, &s.zone) {
            return Err(format!(
                "[search_console] site `{}`: zone `{}` must equal or be a parent of the domain",
                s.domain, s.zone
            ));
        }
        for url in &s.sitemaps {
            let host = url
                .strip_prefix("https://")
                .map(|r| r.split(['/', '?', '#', ':']).next().unwrap_or(""));
            if !host.is_some_and(|h| super::under(h, &s.domain)) {
                return Err(format!(
                    "[search_console] sitemap `{url}` must be an https:// URL on `{}` or a subdomain",
                    s.domain
                ));
            }
        }
    }
    Ok(())
}

/// Why the bridge cannot be used: no URL (`search_console.bridge`, else
/// `HOIST_SEARCH_CONSOLE_BRIDGE`) or no `HOIST_SEARCH_CONSOLE_TOKEN`.
pub(super) fn unavailable(cfg: &SearchConsoleCfg) -> Option<String> {
    if bridge_url(cfg).is_none() {
        return Some(
            "no Search Console bridge (set search_console.bridge or HOIST_SEARCH_CONSOLE_BRIDGE)"
                .into(),
        );
    }
    from_env("HOIST_SEARCH_CONSOLE_TOKEN")
        .is_none()
        .then(|| "no Search Console bridge token (set HOIST_SEARCH_CONSOLE_TOKEN)".to_string())
}

/// The bridge carries a bearer token: https, or plain http only on loopback.
pub(super) fn check_bridge(cfg: &SearchConsoleCfg) -> Result<(), String> {
    let Some(url) = bridge_url(cfg) else {
        return Ok(());
    };
    if url.starts_with("https://") {
        return Ok(());
    }
    let host = url.strip_prefix("http://").map(|r| {
        let auth = r.split(['/', '?', '#']).next().unwrap_or("");
        match auth.strip_prefix('[') {
            Some(v6) => format!("[{}", v6.split(']').next().unwrap_or("")) + "]",
            None => auth.split(':').next().unwrap_or("").to_string(),
        }
    });
    match host.as_deref() {
        Some("127.0.0.1" | "localhost" | "[::1]") => Ok(()),
        _ => Err(format!(
            "search_console.bridge (or HOIST_SEARCH_CONSOLE_BRIDGE) must be an https:// URL (http:// only for 127.0.0.1, localhost or [::1]); got `{url}`"
        )),
    }
}

fn bridge_url(cfg: &SearchConsoleCfg) -> Option<String> {
    std::env::var("HOIST_SEARCH_CONSOLE_BRIDGE")
        .ok()
        .or_else(|| cfg.bridge.clone())
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
}

pub(super) fn client(cfg: &SearchConsoleCfg, verbose: bool) -> Option<Http> {
    let url = bridge_url(cfg)?;
    let token = from_env("HOIST_SEARCH_CONSOLE_TOKEN")?;
    Some(Http::with_base(Service::SearchConsole, token, &url).verbose(verbose))
}

/// What `apply` does for one site.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SiteWrite {
    domain: String,
    zone: String,
    verify: bool,
    property: bool,
    sitemaps: Vec<String>,
}

impl SiteWrite {
    pub(crate) fn needs_cloudflare(&self) -> bool {
        self.verify
    }

    pub(crate) fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.verify {
            parts.push("verification".to_string());
        }
        if self.property {
            parts.push("property".into());
        }
        parts.extend(self.sitemaps.iter().map(|s| format!("sitemap {s}")));
        format!(
            "create search_console/{} ({})",
            self.domain,
            parts.join(", ")
        )
    }
}

fn site_id(domain: &str) -> String {
    format!("sc-domain:{domain}")
}

fn why(f: &Fetched, what: &str) -> String {
    match f {
        Fetched::Forbidden(h) => format!("cannot read {what}: {h}"),
        _ => format!("cannot read {what}: not found (404)"),
    }
}

fn is_verified(list: &Value, domain: &str) -> bool {
    let wanted = format!("dns://{domain}").to_lowercase();
    list.get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|w| {
            let id = w.get("id").and_then(Value::as_str).unwrap_or("");
            let id = id.replace("%3A", ":").replace("%2F", "/").to_lowercase();
            let site = w.get("site");
            let ident = site
                .and_then(|s| s.get("identifier"))
                .and_then(Value::as_str);
            let ty = site.and_then(|s| s.get("type")).and_then(Value::as_str);
            id == wanted
                || (ident.is_some_and(|i| i.eq_ignore_ascii_case(domain))
                    && ty.is_none_or(|t| t == "INET_DOMAIN"))
        })
}

pub(crate) fn read(
    cfg: &SearchConsoleCfg,
    http: Option<&Http>,
) -> Result<SectionPlan<SiteWrite>, HoistError> {
    let mut p = SectionPlan::default();
    let Some(http) = http else {
        let reason = unavailable(cfg).unwrap_or_default();
        for s in &cfg.sites {
            for (kind, key) in site_keys(s) {
                p.items
                    .push(item("search_console", kind, &key, Action::Unknown, &reason));
            }
        }
        return Ok(p);
    };
    if cfg.sites.is_empty() {
        return Ok(p);
    }
    let verified = match http.get_json("/sv/webResource")? {
        Fetched::Ok(v) => Fetch::Got(v),
        other => Fetch::Unknown(why(&other, "verified sites")),
    };
    let sites = match http.get_json("/wm/sites")? {
        Fetched::Ok(v) => Fetch::Got(v),
        other => Fetch::Unknown(why(&other, "Search Console properties")),
    };
    for s in &cfg.sites {
        let id = site_id(&s.domain);
        let mut w = SiteWrite {
            domain: s.domain.clone(),
            zone: s.zone.clone(),
            verify: false,
            property: false,
            sitemaps: Vec::new(),
        };
        let mut known = true;
        let mut push = |p: &mut SectionPlan<SiteWrite>, kind: &str, key: &str, r: Fetch<bool>| {
            let (action, reason) = match r {
                Fetch::Got(true) => (Action::Ok, "present".to_string()),
                Fetch::Got(false) => (Action::Create, "not present".to_string()),
                Fetch::Unknown(r) => {
                    known = false;
                    (Action::Unknown, r)
                }
            };
            p.items
                .push(item("search_console", kind, key, action, reason));
            matches!(action, Action::Create)
        };
        let v = match &verified {
            Fetch::Got(l) => Fetch::Got(is_verified(l, &s.domain)),
            Fetch::Unknown(r) => Fetch::Unknown(r.clone()),
        };
        w.verify = push(&mut p, "verification", &s.domain, v);
        let present = match &sites {
            Fetch::Got(l) => Fetch::Got(
                l.get("siteEntry")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|e| e.get("siteUrl").and_then(Value::as_str) == Some(&id)),
            ),
            Fetch::Unknown(r) => Fetch::Unknown(r.clone()),
        };
        let property_here = present == Fetch::Got(true);
        w.property = push(&mut p, "property", &id, present);
        // The sitemaps of a property that does not exist yet are not readable: all missing.
        let listed = if property_here {
            match http.get_json(&format!("/wm/sites/{}/sitemaps", seg(&id)))? {
                Fetched::Ok(v) => Fetch::Got(Some(v)),
                Fetched::NotFound => Fetch::Got(None),
                other => Fetch::Unknown(why(&other, &format!("sitemaps of `{id}`"))),
            }
        } else if matches!(sites, Fetch::Got(_)) {
            Fetch::Got(None)
        } else {
            Fetch::Unknown("cannot read the property list".into())
        };
        for url in &s.sitemaps {
            let r = match &listed {
                Fetch::Got(v) => Fetch::Got(v.as_ref().is_some_and(|v| {
                    v.get("sitemap")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .any(|m| m.get("path").and_then(Value::as_str) == Some(url))
                })),
                Fetch::Unknown(r) => Fetch::Unknown(r.clone()),
            };
            if push(&mut p, "sitemap", url, r) {
                w.sitemaps.push(url.clone());
            }
        }
        if known && (w.verify || w.property || !w.sitemaps.is_empty()) {
            p.writes.push(w);
        }
    }
    Ok(p)
}

fn site_keys(s: &Site) -> Vec<(&'static str, String)> {
    let mut v = vec![
        ("verification", s.domain.clone()),
        ("property", site_id(&s.domain)),
    ];
    v.extend(s.sitemaps.iter().map(|u| ("sitemap", u.clone())));
    v
}

/// What `apply` did beyond the failed writes: items turned `pending` (verification still
/// propagating) with their human steps.
#[derive(Default)]
pub(crate) struct Outcome {
    pub failed: Vec<Item>,
    pub pending: Vec<Item>,
    pub steps: Vec<HumanStep>,
}

/// Apply per site, in order (spec §16.4). `cf` is the Cloudflare client for the TXT record.
pub(crate) fn apply(
    http: &Http,
    cf: Option<&Http>,
    writes: &[SiteWrite],
    sites: &[Site],
) -> Result<Outcome, HoistError> {
    let mut out = Outcome::default();
    for w in writes {
        let id = site_id(&w.domain);
        if w.verify {
            let Some(cf) = cf else {
                crate::auth::cloudflare_token()?;
                return Err(HoistError::Usage("no Cloudflare token".into()));
            };
            let body = json!({
                "site": {"type": "INET_DOMAIN", "identifier": w.domain},
                "verificationMethod": "DNS_TXT",
            });
            let token = match http.send_json(Method::Post, "/sv/token", Some(&body)) {
                Ok(written) => written
                    .body
                    .as_ref()
                    .and_then(|b| b.get("token"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                Err(e) => {
                    out.failed.extend(rejected(
                        "search_console",
                        &format!("verification token {}", w.domain),
                        Err(e),
                    )?);
                    continue;
                }
            };
            let Some(token) = token else {
                return Err(HoistError::Network(
                    "Search Console bridge answered /sv/token without a `token`".into(),
                ));
            };
            let zone = match super::cloudflare::zone_id(cf, &w.zone)? {
                Fetch::Got(Some(z)) => z,
                Fetch::Got(None) => {
                    return Err(HoistError::Usage(format!(
                        "Cloudflare zone `{}` is not visible to the token",
                        w.zone
                    )));
                }
                Fetch::Unknown(why) => return Err(HoistError::Usage(why)),
            };
            let created = match super::cloudflare::ensure_txt(cf, &zone, &w.domain, &token)? {
                Ok(created) => created,
                Err(e) => {
                    out.failed.extend(rejected(
                        "cloudflare",
                        &format!("TXT record {}", w.domain),
                        Err(e),
                    )?);
                    continue;
                }
            };
            let insert = http.send_json(
                Method::Post,
                "/sv/webResource?verificationMethod=DNS_TXT",
                Some(&json!({"site": {"type": "INET_DOMAIN", "identifier": w.domain}})),
            );
            match insert {
                // Propagation only explains a refusal when this run just created the record.
                Err(HttpError::Rejected { status, .. })
                    if created && (400..500).contains(&status) =>
                {
                    let (items, st) = propagating(w, sites);
                    out.pending.extend(items);
                    out.steps.push(st);
                    continue;
                }
                other => {
                    if let Some(failed) = rejected(
                        "search_console",
                        &format!("verification {}", w.domain),
                        other,
                    )? {
                        out.failed.push(failed);
                        continue;
                    }
                }
            }
        }
        if w.property {
            let r = http.send_json(Method::Put, &format!("/wm/sites/{}", seg(&id)), None);
            if let Some(failed) = rejected("search_console", &format!("property {id}"), r)? {
                out.failed.push(failed);
                continue;
            }
        }
        for url in &w.sitemaps {
            let r = http.send_json(
                Method::Put,
                &format!("/wm/sites/{}/sitemaps/{}", seg(&id), seg(url)),
                None,
            );
            out.failed
                .extend(rejected("search_console", &format!("sitemap {url}"), r)?);
        }
    }
    Ok(out)
}

/// The verification item and everything behind it, `pending` until DNS has propagated; one
/// human step (the dependent items point at it).
fn propagating(w: &SiteWrite, sites: &[Site]) -> (Vec<Item>, HumanStep) {
    let domain = &w.domain;
    let pending = |kind: &str, key: &str, reason: &str| {
        item("search_console", kind, key, Action::Pending, reason)
    };
    let mut items = vec![pending(
        "verification",
        domain,
        "Google refused the verification; the TXT record is still propagating",
    )];
    let dependent = "waiting for the domain verification";
    items.push(pending("property", &site_id(domain), dependent));
    if let Some(s) = sites.iter().find(|s| &s.domain == domain) {
        items.extend(s.sitemaps.iter().map(|u| pending("sitemap", u, dependent)));
    }
    let st = step(
        format!("search_console:verification:{domain}"),
        format!(
            "re-run `hoist org apply` in a few minutes (Google must see the TXT record of {domain}; hoist verifies it through the bridge)"
        ),
        "https://search.google.com/search-console",
        "pending",
    );
    (items, st)
}
