//! The `cloudflare` live probe (spec §17.5): `GET /user/tokens` (and the account's tokens when
//! `CLOUDFLARE_ACCOUNT_ID` is set) read once; the credential is matched by `provider_ref`.

use super::{
    Ctx, LiveProbe, LiveResult, day_of, day_str, expiry_drift, last_used, own_token, ref_matches,
    unknown_err,
};
use crate::auth::from_env;
use crate::http::{Fetched, Http, Service, seg};
use crate::system::Credential;
use serde_json::Value;
use std::cell::OnceCell;

const NO_TOKEN: &str = "no Cloudflare token: set CLOUDFLARE_API_TOKEN to a token that can read API tokens (User API Tokens: Read)";
const FIX: &str = "check CLOUDFLARE_API_TOKEN";

struct Tok {
    id: String,
    name: String,
    status: String,
    expires_on: Option<String>,
    issued_on: Option<String>,
    last_used_on: Option<String>,
    groups: Vec<String>,
}

pub struct Probe {
    http: Option<Http>,
    account: Option<String>,
    today: i64,
    tokens: OnceCell<Result<Vec<Tok>, LiveResult>>,
}

impl Probe {
    pub fn new(ctx: &Ctx) -> Probe {
        Probe {
            http: from_env("CLOUDFLARE_API_TOKEN")
                .map(|t| Http::new(Service::Cloudflare, t).verbose(ctx.verbose)),
            account: std::env::var("CLOUDFLARE_ACCOUNT_ID")
                .ok()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty()),
            today: ctx.today,
            tokens: OnceCell::new(),
        }
    }

    fn read(&self, http: &Http) -> Result<Vec<Tok>, LiveResult> {
        let mut paths = vec!["/user/tokens".to_string()];
        if let Some(a) = &self.account {
            paths.push(format!("/accounts/{}/tokens", seg(a)));
        }
        let mut all = Vec::new();
        let mut readable = false;
        for path in &paths {
            match http.get_all(path, None).map_err(|e| unknown_err(&e, FIX))? {
                Fetched::Ok(v) => {
                    readable = true;
                    let list = v.get("result").unwrap_or(&v);
                    all.extend(list.as_array().into_iter().flatten().filter_map(parse));
                }
                Fetched::NotFound | Fetched::Forbidden(_) => {}
            }
        }
        if readable {
            Ok(all)
        } else {
            Err(LiveResult::Unknown(format!(
                "cannot list API tokens: {FIX} (User API Tokens: Read)"
            )))
        }
    }
}

fn parse(v: &Value) -> Option<Tok> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    Some(Tok {
        id: s("id")?,
        name: s("name").unwrap_or_default(),
        status: s("status").unwrap_or_default(),
        expires_on: s("expires_on"),
        issued_on: s("issued_on"),
        last_used_on: s("last_used_on"),
        groups: v
            .get("policies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|p| p.get("permission_groups").and_then(Value::as_array))
            .flatten()
            .filter_map(|g| g.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect(),
    })
}

impl LiveProbe for Probe {
    fn provider(&self) -> &str {
        "cloudflare"
    }

    fn probe(&self, cred: &Credential) -> LiveResult {
        let Some(http) = &self.http else {
            return LiveResult::Unknown(NO_TOKEN.into());
        };
        let Some(reference) = cred
            .provider_ref
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
        else {
            return LiveResult::Unknown(
                "set provider_ref to the Cloudflare token id or its hoist: name".into(),
            );
        };
        let tokens = match self.tokens.get_or_init(|| self.read(http)) {
            Ok(t) => t,
            Err(unknown) => return unknown.clone(),
        };
        let Some(tok) = tokens.iter().find(|t| ref_matches(cred, &t.id, &t.name)) else {
            return LiveResult::Drift(format!(
                "token {reference} not found at Cloudflare (revoked or another account)"
            ));
        };
        let expires = tok.expires_on.as_deref().and_then(day_of);
        let mut why = Vec::new();
        if tok.status != "active" {
            why.push(format!("token status is {}, not active", tok.status));
        }
        why.extend(expiry_drift(cred, expires, self.today));
        let missing: Vec<&str> = cred
            .scopes
            .iter()
            .map(String::as_str)
            .filter(|s| !tok.groups.iter().any(|g| g.eq_ignore_ascii_case(s)))
            .collect();
        if !missing.is_empty() {
            why.push(format!(
                "permission groups missing at Cloudflare: {}",
                missing.join(", ")
            ));
        }
        if !why.is_empty() {
            return LiveResult::Drift(why.join("; "));
        }
        let mut detail = format!(
            "token {} active, expires {}, issued {}, {}",
            tok.name,
            expires.map_or_else(|| "never".to_string(), day_str),
            tok.issued_on
                .as_deref()
                .and_then(day_of)
                .map_or_else(|| "unknown".to_string(), day_str),
            last_used(tok.last_used_on.as_deref())
        );
        if self.verified_in_use(http, cred, &tok.id) {
            detail.push_str(", verified in use");
        }
        LiveResult::Ok(detail)
    }
}

impl Probe {
    /// Whether the credential's own value (when it is in env) is this token, per
    /// `GET /user/tokens/verify`. Any failure just means "not verified".
    fn verified_in_use(&self, http: &Http, cred: &Credential, id: &str) -> bool {
        let Some(own) = own_token(cred) else {
            return false;
        };
        let other = Http::with_base_like(http, own);
        matches!(
            other.get_json("/user/tokens/verify"),
            Ok(Fetched::Ok(v)) if v.pointer("/result/id").and_then(Value::as_str) == Some(id)
        )
    }
}
