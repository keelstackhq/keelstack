//! The `[cloudflare]` section of org.toml (spec §16.3, §16.4): zones, DNS records, Pages domains.
//! hoist never creates zones and never deletes records.

use super::{SectionPlan, item, rejected, step, under};
use crate::auth::cloudflare_token;
use crate::error::HoistError;
use crate::github::Fetch;
use crate::http::{Fetched, Http, HttpError, Method, Service, seg};
use crate::report::Action;
use serde::Deserialize;
use serde_json::{Value, json};

const DASHBOARD: &str = "https://dash.cloudflare.com";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    #[serde(rename = "type")]
    pub ty: String,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub proxied: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    pub name: String,
    #[serde(default)]
    pub records: Vec<Record>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PagesDomain {
    pub project: String,
    pub domain: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CloudflareCfg {
    pub account_id: Option<String>,
    #[serde(default)]
    pub zones: Vec<Zone>,
    #[serde(default)]
    pub pages_domains: Vec<PagesDomain>,
}

/// `account_id` from the file, else env `CLOUDFLARE_ACCOUNT_ID`.
pub(crate) fn account_id(cfg: &CloudflareCfg) -> Option<String> {
    cfg.account_id
        .clone()
        .or_else(|| std::env::var("CLOUDFLARE_ACCOUNT_ID").ok())
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
}

pub(super) fn validate(cfg: &CloudflareCfg) -> Result<(), String> {
    for z in &cfg.zones {
        for r in &z.records {
            if !["A", "AAAA", "CNAME", "TXT"].contains(&r.ty.as_str()) {
                return Err(format!(
                    "[cloudflare] record `{}`: type `{}` must be A, AAAA, CNAME or TXT",
                    r.name, r.ty
                ));
            }
            if !under(&r.name, &z.name) {
                return Err(format!(
                    "[cloudflare] record `{}` is not in zone `{}`",
                    r.name, z.name
                ));
            }
            if r.ty == "TXT" && r.proxied {
                return Err(format!(
                    "[cloudflare] TXT record `{}` cannot be proxied",
                    r.name
                ));
            }
        }
    }
    if !cfg.pages_domains.is_empty() && account_id(cfg).is_none() {
        return Err(
            "[cloudflare] pages_domains need account_id (or env CLOUDFLARE_ACCOUNT_ID)".into(),
        );
    }
    Ok(())
}

/// A client when a token is set (`unknown` items otherwise).
pub(super) fn client(verbose: bool) -> Option<Http> {
    cloudflare_token()
        .ok()
        .map(|t| Http::new(Service::Cloudflare, t).verbose(verbose))
}

/// One write of `apply`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Op {
    Create {
        zone_id: String,
        rec: Record,
    },
    Patch {
        zone_id: String,
        id: String,
        rec: Record,
    },
    AddDomain {
        account: String,
        project: String,
        domain: String,
    },
}

impl Op {
    pub(crate) fn label(&self) -> String {
        match self {
            Op::Create { rec, .. } => {
                format!(
                    "create cloudflare/dns_record {} {} {}",
                    rec.ty, rec.name, rec.content
                )
            }
            Op::Patch { rec, .. } => {
                format!(
                    "update cloudflare/dns_record {} {} {}",
                    rec.ty, rec.name, rec.content
                )
            }
            Op::AddDomain {
                project, domain, ..
            } => format!("create cloudflare/pages_domain {project}/{domain}"),
        }
    }
}

fn unknown_reason(f: Fetched, what: &str) -> String {
    match f {
        Fetched::Forbidden(h) => format!("cannot read {what}: {h}"),
        _ => format!("cannot read {what}: not found (404)"),
    }
}

/// The id of zone `zone`: `Got(None)` when the token sees no such zone.
pub(crate) fn zone_id(http: &Http, zone: &str) -> Result<Fetch<Option<String>>, HoistError> {
    Ok(
        match http.get_json(&format!("/zones?name={}", seg(zone)))? {
            Fetched::Ok(v) => Fetch::Got(
                v.pointer("/result/0/id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            ),
            other => Fetch::Unknown(unknown_reason(other, &format!("zone `{zone}`"))),
        },
    )
}

struct Live {
    id: String,
    ty: String,
    content: String,
    proxied: bool,
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
}

fn records(http: &Http, zone_id: &str, name: &str) -> Result<Fetch<Vec<Live>>, HoistError> {
    let path = format!("/zones/{}/dns_records?name={}", seg(zone_id), seg(name));
    Ok(match http.get_all(&path, None)? {
        Fetched::Ok(Value::Array(list)) => Fetch::Got(
            list.iter()
                .filter_map(|r| {
                    Some(Live {
                        id: r.get("id")?.as_str()?.to_string(),
                        ty: r.get("type")?.as_str()?.to_string(),
                        content: r.get("content")?.as_str()?.to_string(),
                        proxied: r.get("proxied").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect(),
        ),
        other => Fetch::Unknown(unknown_reason(other, &format!("DNS records of `{name}`"))),
    })
}

/// Make sure a TXT record `name` = `content` exists in the zone: `Ok(true)` when it was
/// created, `Ok(false)` when present, the refusal when the POST was rejected.
pub(crate) fn ensure_txt(
    http: &Http,
    zone_id: &str,
    name: &str,
    content: &str,
) -> Result<Result<bool, HttpError>, HoistError> {
    match records(http, zone_id, name)? {
        Fetch::Unknown(why) => Err(HoistError::Usage(why)),
        Fetch::Got(live) => {
            if live
                .iter()
                .any(|r| r.ty == "TXT" && unquote(&r.content) == content)
            {
                return Ok(Ok(false));
            }
            let body = json!({"type": "TXT", "name": name, "content": content, "ttl": 1});
            match http.send_json(
                Method::Post,
                &format!("/zones/{}/dns_records", seg(zone_id)),
                Some(&body),
            ) {
                Ok(_) => Ok(Ok(true)),
                // A refused write is data for the caller; 401 and the rest are exit 3.
                Err(e @ HttpError::Rejected { .. }) => Ok(Err(e)),
                Err(e) => Err(e.into()),
            }
        }
    }
}

pub(crate) fn read(
    cfg: &CloudflareCfg,
    http: Option<&Http>,
) -> Result<SectionPlan<Op>, HoistError> {
    let mut p = SectionPlan::default();
    let record_key = |r: &Record| format!("{} {} {}", r.ty, r.name, r.content);
    let Some(http) = http else {
        let why = "no Cloudflare token (set CLOUDFLARE_API_TOKEN)";
        for z in &cfg.zones {
            p.items
                .push(item("cloudflare", "zone", &z.name, Action::Unknown, why));
            for r in &z.records {
                p.items.push(item(
                    "cloudflare",
                    "dns_record",
                    &record_key(r),
                    Action::Unknown,
                    why,
                ));
            }
        }
        for d in &cfg.pages_domains {
            let key = format!("{}/{}", d.project, d.domain);
            p.items.push(item(
                "cloudflare",
                "pages_domain",
                &key,
                Action::Unknown,
                why,
            ));
        }
        return Ok(p);
    };
    for z in &cfg.zones {
        let (id, why) = match zone_id(http, &z.name)? {
            Fetch::Got(Some(id)) => {
                p.items
                    .push(item("cloudflare", "zone", &z.name, Action::Ok, "present"));
                (Some(id), String::new())
            }
            Fetch::Got(None) => {
                p.items.push(item(
                    "cloudflare",
                    "zone",
                    &z.name,
                    Action::Pending,
                    "not visible to this token",
                ));
                p.steps.push(step(
                    format!("cloudflare:zone:{}", z.name),
                    format!(
                        "add site {} to Cloudflare and set its nameservers at the registrar (hoist verifies the zone is visible)",
                        z.name
                    ),
                    DASHBOARD,
                    "pending",
                ));
                (None, "zone is missing".to_string())
            }
            Fetch::Unknown(why) => {
                p.items
                    .push(item("cloudflare", "zone", &z.name, Action::Unknown, &why));
                (None, why)
            }
        };
        for r in &z.records {
            let key = record_key(r);
            let Some(zid) = &id else {
                p.items.push(item(
                    "cloudflare",
                    "dns_record",
                    &key,
                    Action::Unknown,
                    &why,
                ));
                continue;
            };
            let live = match records(http, zid, &r.name)? {
                Fetch::Got(l) => l,
                Fetch::Unknown(why) => {
                    p.items
                        .push(item("cloudflare", "dns_record", &key, Action::Unknown, why));
                    continue;
                }
            };
            let same_type: Vec<&Live> = live.iter().filter(|l| l.ty == r.ty).collect();
            let exact = same_type.iter().find(|l| unquote(&l.content) == r.content);
            // Only a live record that no declared record matches exactly may be rewritten.
            let spare = same_type.iter().find(|l| {
                !z.records
                    .iter()
                    .any(|d| d.ty == l.ty && d.name == r.name && d.content == unquote(&l.content))
            });
            let (action, reason, op) = match (exact, spare) {
                (Some(l), _) if r.ty == "TXT" || l.proxied == r.proxied => {
                    (Action::Ok, "present".to_string(), None)
                }
                (Some(l), _) => (
                    Action::Update,
                    format!("proxied: live {}, want {}", l.proxied, r.proxied),
                    Some(Op::Patch {
                        zone_id: zid.clone(),
                        id: l.id.clone(),
                        rec: r.clone(),
                    }),
                ),
                (None, Some(l)) if r.ty != "TXT" => (
                    Action::Update,
                    format!("content: live `{}`, want `{}`", l.content, r.content),
                    Some(Op::Patch {
                        zone_id: zid.clone(),
                        id: l.id.clone(),
                        rec: r.clone(),
                    }),
                ),
                _ => (
                    Action::Create,
                    "missing".to_string(),
                    Some(Op::Create {
                        zone_id: zid.clone(),
                        rec: r.clone(),
                    }),
                ),
            };
            p.items
                .push(item("cloudflare", "dns_record", &key, action, reason));
            p.writes.extend(op);
        }
    }
    let account = account_id(cfg).unwrap_or_default();
    for d in &cfg.pages_domains {
        let key = format!("{}/{}", d.project, d.domain);
        let path = format!(
            "/accounts/{}/pages/projects/{}/domains",
            seg(&account),
            seg(&d.project)
        );
        match http.get_all(&path, None)? {
            Fetched::Ok(Value::Array(list)) => {
                if list
                    .iter()
                    .any(|e| e.get("name").and_then(Value::as_str) == Some(d.domain.as_str()))
                {
                    p.items.push(item(
                        "cloudflare",
                        "pages_domain",
                        &key,
                        Action::Ok,
                        "present",
                    ));
                } else {
                    p.items.push(item(
                        "cloudflare",
                        "pages_domain",
                        &key,
                        Action::Create,
                        "domain is not attached",
                    ));
                    p.writes.push(Op::AddDomain {
                        account: account.clone(),
                        project: d.project.clone(),
                        domain: d.domain.clone(),
                    });
                }
            }
            Fetched::NotFound => {
                p.items.push(item(
                    "cloudflare",
                    "pages_domain",
                    &key,
                    Action::Pending,
                    format!("Pages project `{}` not found", d.project),
                ));
                p.steps.push(step(
                    format!("cloudflare:pages_domain:{key}"),
                    format!(
                        "create the Pages project {} (hoist apply in its repo does; hoist org check verifies the domain)",
                        d.project
                    ),
                    DASHBOARD,
                    "pending",
                ));
            }
            other => p.items.push(item(
                "cloudflare",
                "pages_domain",
                &key,
                Action::Unknown,
                unknown_reason(other, &format!("Pages domains of `{}`", d.project)),
            )),
        }
    }
    Ok(p)
}

/// Each write is one request; a refused one is a failed item and the rest still run.
pub(crate) fn apply(http: &Http, writes: &[Op]) -> Result<Vec<crate::report::Item>, HoistError> {
    let mut failed = Vec::new();
    for w in writes {
        let body_of = |r: &Record| {
            let mut b = json!({"type": r.ty, "name": r.name, "content": r.content, "ttl": 1});
            if r.ty != "TXT" {
                b["proxied"] = json!(r.proxied);
            }
            b
        };
        let result = match w {
            Op::Create { zone_id, rec } => http.send_json(
                Method::Post,
                &format!("/zones/{}/dns_records", seg(zone_id)),
                Some(&body_of(rec)),
            ),
            Op::Patch { zone_id, id, rec } => {
                let mut b = json!({"content": rec.content});
                if rec.ty != "TXT" {
                    b["proxied"] = json!(rec.proxied);
                }
                http.send_json(
                    Method::Patch,
                    &format!("/zones/{}/dns_records/{}", seg(zone_id), seg(id)),
                    Some(&b),
                )
            }
            Op::AddDomain {
                account,
                project,
                domain,
            } => http.send_json(
                Method::Post,
                &format!(
                    "/accounts/{}/pages/projects/{}/domains",
                    seg(account),
                    seg(project)
                ),
                Some(&json!({"name": domain})),
            ),
        };
        failed.extend(rejected("cloudflare", &w.label(), result)?);
    }
    Ok(failed)
}
