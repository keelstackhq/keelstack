//! The `google` live probe (spec §17.5): the keys of a service account from the IAM API.

use super::{Ctx, LiveProbe, LiveResult, day_of, day_str, expiry_drift, ref_matches, unknown_err};
use crate::auth::from_env;
use crate::http::{Fetched, Http, Service, seg};
use crate::system::Credential;
use serde_json::Value;

const FIX: &str = "set GOOGLE_OAUTH_ACCESS_TOKEN (gcloud auth print-access-token)";
const SA_SUFFIX: &str = ".iam.gserviceaccount.com";

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
        "google"
    }

    fn answers(&self, id: &str) -> bool {
        matches!(id, "google" | "gcp" | "gsc")
    }

    fn probe(&self, cred: &Credential) -> LiveResult {
        let Some(tok) = from_env("GOOGLE_OAUTH_ACCESS_TOKEN") else {
            return LiveResult::Unknown(format!("no Google token: {FIX}"));
        };
        let Some((email, project)) = cred.principal.as_deref().and_then(service_account) else {
            return LiveResult::Unknown("set principal to the service account email".into());
        };
        let http = Http::new(Service::Google, tok).verbose(self.ctx.verbose);
        let path = format!(
            "/v1/projects/{}/serviceAccounts/{}/keys?keyTypes=USER_MANAGED",
            seg(&project),
            seg(&email)
        );
        let body = match http.get_json(&path) {
            Ok(Fetched::Ok(v)) => v,
            Ok(Fetched::NotFound) => {
                return LiveResult::Unknown(format!(
                    "Google IAM does not know {email} (404): set principal to the service account email"
                ));
            }
            Ok(Fetched::Forbidden(m)) => {
                return LiveResult::Unknown(format!(
                    "Google IAM refused to list keys ({m}): {FIX} for an account that may list keys"
                ));
            }
            Err(e) => return unknown_err(&e, FIX),
        };
        let keys: Vec<(&str, &Value)> = body["keys"]
            .as_array()
            .map(|a| a.iter().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .filter(|k| k["keyType"].as_str().is_none_or(|t| t == "USER_MANAGED"))
            .map(|k| {
                (
                    k["name"]
                        .as_str()
                        .unwrap_or("")
                        .rsplit('/')
                        .next()
                        .unwrap_or(""),
                    k,
                )
            })
            .collect();
        let has_ref = cred
            .provider_ref
            .as_deref()
            .is_some_and(|r| !r.trim().is_empty());
        let (id, key) = if has_ref {
            match keys.iter().find(|(id, _)| ref_matches(cred, id, "")) {
                Some(k) => *k,
                None => return LiveResult::Drift("key not found for the service account".into()),
            }
        } else if keys.len() == 1 {
            keys[0]
        } else {
            return LiveResult::Unknown("set provider_ref to the key id".into());
        };
        let mut drift = Vec::new();
        if key["disabled"].as_bool().unwrap_or(false) {
            drift.push("key is disabled".to_string());
        }
        let valid_until = key["validBeforeTime"]
            .as_str()
            .filter(|t| !t.starts_with("9999-"))
            .and_then(day_of);
        drift.extend(expiry_drift(cred, valid_until, self.ctx.today));
        let others = keys
            .iter()
            .filter(|(i, k)| *i != id && !k["disabled"].as_bool().unwrap_or(false))
            .count();
        if others > 0 {
            drift.push(format!(
                "{others} other user-managed key{} (old keys not revoked)",
                if others == 1 { "" } else { "s" }
            ));
        }
        if !drift.is_empty() {
            return LiveResult::Drift(drift.join("; "));
        }
        LiveResult::Ok(format!(
            "key {id} valid until {}",
            valid_until.map_or_else(|| "never".to_string(), day_str)
        ))
    }
}

/// `(email, project)` of a principal like `sa name@proj.iam.gserviceaccount.com`.
fn service_account(principal: &str) -> Option<(String, String)> {
    let mut p = principal.trim();
    for prefix in ["service account ", "sa "] {
        if p.len() > prefix.len()
            && p.is_char_boundary(prefix.len())
            && p[..prefix.len()].eq_ignore_ascii_case(prefix)
        {
            p = p[prefix.len()..].trim();
        }
    }
    let (name, domain) = p.split_once('@')?;
    let project = domain.strip_suffix(SA_SUFFIX)?;
    (!name.is_empty() && !project.is_empty() && !p.contains(char::is_whitespace))
        .then(|| (p.to_string(), project.to_string()))
}
