//! The `npm` live probe (spec §17.5): `GET /-/npm/v1/tokens` lists the account's tokens with a
//! masked prefix, never the value.

use super::{
    Ctx, LiveProbe, LiveResult, day_of, day_str, expiry_drift, find_ref, no_token, ref_words,
    token_for, unknown_err,
};
use crate::http::{Fetched, Http, Service};
use crate::system::Credential;
use serde_json::Value;

const FIX: &str = "set NPM_TOKEN to a token that can list tokens (npm token list)";
const MAX_PAGES: usize = 10;

pub struct Probe<'a> {
    ctx: &'a Ctx<'a>,
}

impl<'a> Probe<'a> {
    pub fn new(ctx: &'a Ctx<'a>) -> Probe<'a> {
        Probe { ctx }
    }

    /// Every listed token, following `urls.next` when it is a relative path (max 10 pages).
    fn list(&self, http: &Http) -> Result<Vec<Value>, LiveResult> {
        let mut path = "/-/npm/v1/tokens".to_string();
        let mut all = Vec::new();
        for _ in 0..MAX_PAGES {
            let body = match http.get_json(&path) {
                Ok(Fetched::Ok(v)) => v,
                Ok(Fetched::NotFound) => {
                    return Err(LiveResult::Unknown(format!(
                        "npm registry has no token list (404): {FIX}"
                    )));
                }
                Ok(Fetched::Forbidden(m)) => {
                    return Err(LiveResult::Unknown(format!(
                        "npm registry refused to list tokens ({m}): {FIX}"
                    )));
                }
                Err(e) => return Err(unknown_err(&e, FIX)),
            };
            if let Some(objects) = body["objects"].as_array() {
                all.extend(objects.iter().cloned());
            }
            match body["urls"]["next"].as_str() {
                Some(next) if next.starts_with('/') && !next.starts_with("//") => {
                    path = next.to_string();
                }
                _ => break,
            }
        }
        Ok(all)
    }
}

impl LiveProbe for Probe<'_> {
    fn provider(&self) -> &str {
        "npm"
    }

    fn probe(&self, cred: &Credential) -> LiveResult {
        let Some(tok) = token_for(cred, &["NPM_TOKEN"]) else {
            return no_token(cred, &format!("no npm token: {FIX}"));
        };
        if cred
            .provider_ref
            .as_deref()
            .is_none_or(|r| r.trim().is_empty())
        {
            return LiveResult::Unknown(
                "set provider_ref to the npm token key (npm token list)".into(),
            );
        }
        let http = Http::new(Service::Npm, tok).verbose(self.ctx.verbose);
        let tokens = match self.list(&http) {
            Ok(t) => t,
            Err(r) => return r,
        };
        let words = ref_words(cred);
        let by_ref = match find_ref(cred, &tokens, |t| {
            (
                t["key"].as_str().unwrap_or("").to_string(),
                t["name"].as_str().unwrap_or("").to_string(),
            )
        }) {
            Ok(t) => t,
            Err(why) => return LiveResult::Unknown(why),
        };
        // A masked prefix only counts for a long word (>= 8 chars) next to a masked value of
        // at least 4 chars; the word may be longer than the mask, never shorter than 8.
        let found = by_ref.or_else(|| {
            tokens.iter().find(|t| {
                let masked = masked(t);
                masked.len() >= 4
                    && words
                        .iter()
                        .any(|w| w.len() >= 8 && (w.starts_with(masked) || masked.starts_with(w)))
            })
        });
        let Some(t) = found else {
            return LiveResult::Drift("token not found in the npm token list".into());
        };
        let readonly = t["readonly"].as_bool().unwrap_or(false);
        let mut drift = Vec::new();
        if cred.role == "publish" && readonly {
            drift.push("publish token is read-only at npm".to_string());
        }
        if cred.role == "read" && !readonly {
            drift.push("read token is not read-only at npm".to_string());
        }
        if let Some(exp) = t["expires"].as_str().filter(|e| !e.is_empty()) {
            drift.extend(expiry_drift(cred, day_of(exp), self.ctx.today));
        }
        if !drift.is_empty() {
            return LiveResult::Drift(drift.join("; "));
        }
        let cidr: Vec<&str> = t["cidr_whitelist"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let created = t["created"]
            .as_str()
            .and_then(day_of)
            .map_or_else(|| "unknown".to_string(), day_str);
        LiveResult::Ok(format!(
            "token {} created {created}, readonly {readonly}, cidr {}",
            masked(t),
            if cidr.is_empty() {
                "any".to_string()
            } else {
                cidr.join(",")
            }
        ))
    }
}

/// The masked token prefix without its trailing ellipsis.
fn masked(t: &Value) -> &str {
    t["token"]
        .as_str()
        .unwrap_or("")
        .trim_end_matches('…')
        .trim_end_matches("...")
}
