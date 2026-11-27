//! The `crates` live probe (spec §17.5): `GET /api/v1/me/tokens`.

use super::{
    Ctx, LiveProbe, LiveResult, day_of, day_str, expiry_drift, find_ref, last_used, token_for,
};
use crate::http::{Fetched, Http, Service};
use crate::system::Credential;
use serde_json::Value;

const FIX: &str = "set CARGO_REGISTRY_TOKEN to a crates.io API token";
const REFUSED: &str = "crates.io refused to list tokens with an API token (403); check the token at https://crates.io/settings/tokens";

pub struct Probe<'a> {
    ctx: &'a Ctx<'a>,
}

impl<'a> Probe<'a> {
    pub fn new(ctx: &'a Ctx<'a>) -> Probe<'a> {
        Probe { ctx }
    }
}

impl LiveProbe for Probe<'_> {
    fn provider(&self) -> &str {
        "crates"
    }

    fn probe(&self, cred: &Credential) -> LiveResult {
        let Some(tok) = token_for(cred, &["CARGO_REGISTRY_TOKEN"]) else {
            return LiveResult::Unknown(format!("no crates.io token: {FIX}"));
        };
        if cred
            .provider_ref
            .as_deref()
            .is_none_or(|r| r.trim().is_empty())
        {
            return LiveResult::Unknown(
                "set provider_ref to the crates.io token id or name".into(),
            );
        }
        let http = Http::new(Service::Crates, tok).verbose(self.ctx.verbose);
        let body = match http.get_json("/api/v1/me/tokens") {
            Ok(Fetched::Ok(v)) => v,
            Ok(Fetched::Forbidden(_) | Fetched::NotFound) => {
                return LiveResult::Unknown(REFUSED.into());
            }
            Err(crate::http::HttpError::Unauthorized { .. }) => {
                return LiveResult::Unknown(REFUSED.into());
            }
            Err(e) => return LiveResult::Unknown(format!("{e}: {FIX}")),
        };
        let tokens = body["api_tokens"].as_array().cloned().unwrap_or_default();
        let found = match find_ref(cred, &tokens, |t| {
            let id = t["id"]
                .as_u64()
                .map(|n| n.to_string())
                .or_else(|| t["id"].as_str().map(str::to_string))
                .unwrap_or_default();
            (id, t["name"].as_str().unwrap_or("").to_string())
        }) {
            Ok(t) => t,
            Err(why) => return LiveResult::Unknown(why),
        };
        let Some(t) = found else {
            return LiveResult::Drift("token not found in the crates.io token list".into());
        };
        let expires = t["expired_at"].as_str().and_then(day_of);
        let mut drift: Vec<String> = expiry_drift(cred, expires, self.ctx.today)
            .into_iter()
            .collect();
        let live: Vec<&str> = ["endpoint_scopes", "crate_scopes"]
            .iter()
            .filter_map(|k| t[*k].as_array())
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let missing: Vec<&str> = cred
            .scopes
            .iter()
            .map(String::as_str)
            .filter(|s| !live.contains(s))
            .collect();
        if !missing.is_empty() {
            drift.push(format!(
                "declared scopes not on the token: {}",
                missing.join(", ")
            ));
        }
        if !drift.is_empty() {
            return LiveResult::Drift(drift.join("; "));
        }
        LiveResult::Ok(format!(
            "token {} expires {}, {}",
            t["name"].as_str().unwrap_or("?"),
            expires.map_or_else(|| "never".to_string(), day_str),
            last_used(t["last_used_at"].as_str())
        ))
    }
}
