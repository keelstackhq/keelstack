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
            // Cloudflare holds one record per type, name and content: two declared records that
            // differ only in `proxied` cannot both exist (a create would be rejected).
            if r.ty != "TXT"
                && z.records.iter().any(|o| {
                    o.ty == r.ty
                        && name_key(&o.name) == name_key(&r.name)
                        && norm(&o.ty, &o.content) == norm(&r.ty, &r.content)
                        && o.proxied != r.proxied
                })
            {
                return Err(format!(
                    "[cloudflare] record {} `{}` {} is declared twice with different `proxied`; Cloudflare holds one such record: declare it once",
                    r.ty, r.name, r.content
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

/// A TXT value as DNS serves it: `"a" "b"` (chunks of at most 255 bytes, each quoted) is the
/// value `ab`; a value that is not a run of quoted chunks stays as it is (outer quotes off).
fn txt_value(s: &str) -> String {
    let t = s.trim();
    let mut out = String::new();
    let mut chars = t.chars().peekable();
    let mut chunks = 0;
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        match chars.next() {
            None if chunks > 0 => return out,
            Some('"') => {}
            _ => return unquote(s).to_string(),
        }
        let mut closed = false;
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some(e) => out.push(e),
                    None => return unquote(s).to_string(),
                },
                '"' => {
                    closed = true;
                    break;
                }
                c => out.push(c),
            }
        }
        if !closed {
            return unquote(s).to_string();
        }
        chunks += 1;
    }
}

/// A record name as compared: DNS names are case-insensitive; no trailing dot.
fn name_key(name: &str) -> String {
    name.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Content as compared: trimmed, unquoted; IPs in canonical form; hostnames lower-cased
/// without a trailing dot. TXT stays exact, its quoted chunks joined (`"a" "b"` is `ab`).
fn norm(ty: &str, s: &str) -> String {
    if ty == "TXT" {
        return txt_value(s);
    }
    let s = unquote(s);
    match ty {
        "A" => s
            .parse::<std::net::Ipv4Addr>()
            .map_or(s.into(), |a| a.to_string()),
        "AAAA" => s
            .parse::<std::net::Ipv6Addr>()
            .map_or(s.into(), |a| a.to_string()),
        "CNAME" | "NS" | "MX" | "PTR" | "SRV" => s.trim_end_matches('.').to_ascii_lowercase(),
        _ => s.to_string(),
    }
}

enum Match {
    Present,
    /// Live record id and the reason text.
    Patch(String, String),
    Create,
    /// A repeat of an earlier identical declared record: not reported again.
    Twin,
}

/// Whether each declared record repeats an earlier identical one (spec §16.4: "Identical declared
/// records are one"): same type, name (any case, no trailing dot), normalised content and, except
/// for TXT, `proxied`. Records that differ in `proxied` are two wishes, never twins.
fn twins(declared: &[Record]) -> Vec<bool> {
    let key = |r: &Record| {
        (
            r.ty.clone(),
            name_key(&r.name),
            norm(&r.ty, &r.content),
            r.ty != "TXT" && r.proxied,
        )
    };
    (0..declared.len())
        .map(|i| declared[..i].iter().any(|e| key(e) == key(&declared[i])))
        .collect()
}

/// Pairs each declared record with a live one, per (name, type), so no live record is used twice.
/// Pass 1: a live record with the same normalised content (and proxied, except TXT) is `Present`;
/// one with only proxied different is patched. Pass 2 (not TXT): a declared record still
/// unmatched takes the first live same-type record no declared record claimed and patches its
/// content; with none left it is a `Create`. TXT values coexist: no match is a `Create`.
/// Undeclared live records are never touched.
fn match_records(
    declared: &[Record],
    live: &std::collections::HashMap<String, &Vec<Live>>,
) -> Vec<Match> {
    let none = Vec::new();
    let live_of = |r: &Record| -> Vec<&Live> {
        live.get(&name_key(&r.name))
            .copied()
            .unwrap_or(&none)
            .iter()
            .filter(|l| l.ty == r.ty)
            .collect()
    };
    let mut claimed: std::collections::HashSet<String> = std::collections::HashSet::new();
    // A twin claims nothing, so one live record is claimed once.
    let mut out: Vec<Option<Match>> = twins(declared)
        .into_iter()
        .map(|t| t.then_some(Match::Twin))
        .collect();
    for pass in 0..2 {
        for (i, r) in declared.iter().enumerate() {
            if out[i].is_some() {
                continue;
            }
            let cands: Vec<&Live> = live_of(r)
                .into_iter()
                .filter(|l| !claimed.contains(&l.id))
                .collect();
            let want = norm(&r.ty, &r.content);
            let hit = if pass == 0 {
                let same = |l: &&&Live| norm(&l.ty, &l.content) == want;
                cands
                    .iter()
                    .find(|l| same(l) && (r.ty == "TXT" || l.proxied == r.proxied))
                    .or_else(|| cands.iter().find(same))
                    .copied()
            } else if r.ty == "TXT" {
                None
            } else {
                cands.first().copied()
            };
            let Some(l) = hit else {
                if pass == 1 {
                    out[i] = Some(Match::Create);
                }
                continue;
            };
            claimed.insert(l.id.clone());
            let content_same = norm(&l.ty, &l.content) == want;
            let mut why = Vec::new();
            if !content_same {
                why.push(format!(
                    "content: live {}, want {}",
                    unquote(&l.content),
                    r.content
                ));
            }
            if r.ty != "TXT" && l.proxied != r.proxied {
                why.push(format!("proxied: live {}, want {}", l.proxied, r.proxied));
            }
            out[i] = Some(if why.is_empty() {
                Match::Present
            } else {
                Match::Patch(l.id.clone(), why.join("; "))
            });
        }
    }
    out.into_iter()
        .map(|m| m.unwrap_or(Match::Create))
        .collect()
}

fn records(http: &Http, zone_id: &str, name: &str) -> Result<Fetch<Vec<Live>>, HoistError> {
    // Queried by the name as compared (lower case, no trailing dot), as the reads are keyed.
    let path = format!(
        "/zones/{}/dns_records?name={}",
        seg(zone_id),
        seg(&name_key(name))
    );
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
                .any(|r| r.ty == "TXT" && norm("TXT", &r.content) == norm("TXT", content))
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
            // A twin is the first record's repeat: one unknown item, not two.
            for (r, _) in z.records.iter().zip(twins(&z.records)).filter(|(_, t)| !t) {
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
        // Keyed by the DNS name as compared (any case, no trailing dot), as twins are: one read
        // per name.
        let mut lives: std::collections::HashMap<String, Fetch<Vec<Live>>> =
            std::collections::HashMap::new();
        if id.is_some() {
            for r in &z.records {
                if let std::collections::hash_map::Entry::Vacant(e) = lives.entry(name_key(&r.name))
                {
                    e.insert(records(http, id.as_deref().unwrap_or_default(), &r.name)?);
                }
            }
        }
        // Names whose read failed are absent here and reported unknown below.
        let by_name: std::collections::HashMap<String, &Vec<Live>> = lives
            .iter()
            .filter_map(|(k, v)| match v {
                Fetch::Got(l) => Some((k.clone(), l)),
                Fetch::Unknown(_) => None,
            })
            .collect();
        let plans = match_records(&z.records, &by_name);
        let mut reported: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (i, r) in z.records.iter().enumerate() {
            let key = record_key(r);
            let twin = matches!(plans[i], Match::Twin);
            if !twin {
                reported.insert(key.clone());
            }
            let unreadable =
                id.is_none() || matches!(lives.get(&name_key(&r.name)), Some(Fetch::Unknown(_)));
            if twin && unreadable {
                // The first record already says why it is unknown.
                continue;
            }
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
            if let Some(Fetch::Unknown(why)) = lives.get(&name_key(&r.name)) {
                p.items
                    .push(item("cloudflare", "dns_record", &key, Action::Unknown, why));
                continue;
            }
            let m = &plans[i];
            // A twin spelled exactly like an earlier record has the same key: one item, not two.
            // A twin spelled differently (another case) keeps its own `ok` item.
            if twin && !reported.insert(key.clone()) {
                continue;
            }
            let (action, reason, op) = match m {
                Match::Twin => (
                    Action::Ok,
                    "repeat of an identical declared record; claims nothing".to_string(),
                    None,
                ),
                Match::Present => (Action::Ok, "present".to_string(), None),
                Match::Patch(l, reason) => (
                    Action::Update,
                    reason.clone(),
                    Some(Op::Patch {
                        zone_id: zid.clone(),
                        id: l.clone(),
                        rec: r.clone(),
                    }),
                ),
                Match::Create => (
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_values_join_their_quoted_chunks() {
        assert_eq!(txt_value(r#""a" "b""#), "ab");
        assert_eq!(txt_value(r#""v=spf1 -all""#), "v=spf1 -all");
        assert_eq!(txt_value("v=spf1 -all"), "v=spf1 -all");
        assert_eq!(txt_value(r#""a\"b" "c""#), "a\"bc");
        // Not a run of quoted chunks: as it is (only matching outer quotes would go).
        assert_eq!(txt_value(r#""a" b"#), r#""a" b"#);
        assert_eq!(txt_value(r#""a"#), r#""a"#);
    }

    #[test]
    fn twins_key_on_name_case_and_proxied() {
        let r = |name: &str, proxied: bool| Record {
            ty: "A".into(),
            name: name.into(),
            content: "1.1.1.1".into(),
            proxied,
        };
        assert_eq!(
            twins(&[r("www.x.dev", false), r("WWW.x.dev.", false)]),
            [false, true]
        );
        assert_eq!(
            twins(&[r("www.x.dev", false), r("www.x.dev", true)]),
            [false, false]
        );
    }
}
