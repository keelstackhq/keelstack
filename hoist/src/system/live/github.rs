//! The `github` live probe (spec §17.5). `probe` checks a GitHub PAT's own expiry (GitHub shows
//! it only to the token itself); `sweep` reads the names and `updated_at` of the Actions secrets
//! of every repo and environment that any credential's storage names, for credentials of every
//! provider.

use super::{
    Ctx, LiveProbe, LiveResult, Sweep, day_of, day_str, env_names, expiry_drift, no_token,
    own_token, unknown_err,
};
use crate::auth::{from_env, github_token};
use crate::http::{Fetched, Http, HttpError, Service, seg};
use crate::system::{CredKind, Credential, Status, StorageKind, SystemFile, add_duration};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const EXPIRY_HEADER: &str = "github-authentication-token-expiration";

pub struct Probe<'a> {
    sys: &'a SystemFile,
    today: i64,
    verbose: bool,
}

impl<'a> Probe<'a> {
    pub fn new(ctx: &Ctx<'a>) -> Probe<'a> {
        Probe {
            sys: ctx.sys,
            today: ctx.today,
            verbose: ctx.verbose,
        }
    }
}

/// Where Actions secrets live: a repo, or one of its environments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Scope {
    Repo(String),
    Env(String, String),
}

impl Scope {
    fn of(s: &crate::system::Storage) -> Option<Scope> {
        let repo = s.repo.clone()?;
        match s.kind {
            StorageKind::GhSecret => Some(Scope::Repo(repo)),
            StorageKind::GhEnvSecret => Some(Scope::Env(repo, s.env.clone()?)),
            _ => None,
        }
    }

    fn repo(&self) -> &str {
        match self {
            Scope::Repo(r) | Scope::Env(r, _) => r,
        }
    }

    fn path(&self) -> String {
        let repo = self
            .repo()
            .split('/')
            .map(seg)
            .collect::<Vec<_>>()
            .join("/");
        match self {
            Scope::Repo(_) => format!("/repos/{repo}/actions/secrets"),
            Scope::Env(_, env) => {
                format!("/repos/{repo}/environments/{}/secrets", seg(env))
            }
        }
    }

    /// `gh:<repo>` or `gh-env:<env>`, the source shown for an undeclared secret.
    fn label(&self) -> String {
        match self {
            Scope::Repo(r) => format!("gh:{r}"),
            Scope::Env(_, e) => format!("gh-env:{e}"),
        }
    }
}

/// name -> `updated_at` day (None when absent or unparsable).
type Listed = BTreeMap<String, Option<i64>>;

fn list(http: &Http, scope: &Scope) -> Result<Listed, String> {
    let hint = format!("GITHUB_TOKEN needs Secrets: read on {}", scope.repo());
    match http.get_all(&scope.path(), Some("secrets")) {
        Ok(Fetched::Ok(v)) => {
            let items = v.get("secrets").unwrap_or(&v);
            Ok(items
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    let name = s.get("name")?.as_str()?.to_string();
                    let day = s
                        .get("updated_at")
                        .or_else(|| s.get("created_at"))
                        .and_then(Value::as_str)
                        .and_then(day_of);
                    Some((name, day))
                })
                .collect())
        }
        Ok(Fetched::NotFound | Fetched::Forbidden(_)) => Err(hint),
        Err(e @ HttpError::Unauthorized { .. }) => match unknown_err(&e, &hint) {
            LiveResult::Unknown(m) => Err(m),
            _ => Err(hint),
        },
        Err(e) => Err(format!("{e}: {hint}")),
    }
}

impl LiveProbe for Probe<'_> {
    fn provider(&self) -> &str {
        "github"
    }

    fn probe(&self, cred: &Credential) -> LiveResult {
        // Only names GitHub owns (`GH_TOKEN`, `GH_PAT`) are read; never an arbitrary env var.
        let key = env_names(cred, &[])
            .into_iter()
            .find(|k| from_env(k).is_some());
        let (Some(key), Some(own), CredKind::ApiToken) = (key, own_token(cred), cred.kind) else {
            let key = cred
                .storage
                .iter()
                .map(|s| s.key.as_str())
                .find(|k| *k != "GITHUB_TOKEN")
                .unwrap_or(&cred.id);
            return no_token(
                cred,
                &format!(
                    "the value is not in env; GitHub shows a PAT's expiry only to the token itself (run in a job that reads {key})"
                ),
            );
        };
        let http = Http::new(Service::Github, own).verbose(self.verbose);
        let fix = format!("check {key}");
        match http.get_json_header("/user", EXPIRY_HEADER) {
            Ok((Fetched::Ok(_), header)) => {
                let live = header.as_deref().and_then(day_of);
                match expiry_drift(cred, live, self.today) {
                    Some(why) => LiveResult::Drift(why),
                    None => LiveResult::Ok(format!(
                        "PAT expires {}",
                        live.map_or_else(|| "never".to_string(), day_str)
                    )),
                }
            }
            Ok((Fetched::NotFound, _)) => {
                LiveResult::Unknown(format!("GET /user not found: {fix}"))
            }
            Ok((Fetched::Forbidden(m), _)) => LiveResult::Unknown(format!("{m}: {fix}")),
            Err(e) => unknown_err(&e, &fix),
        }
    }

    fn sweep(&self) -> Vec<Sweep> {
        let creds = &self.sys.credentials;
        let scopes: BTreeSet<Scope> = creds
            .iter()
            .flat_map(|c| &c.storage)
            .filter_map(Scope::of)
            .collect();
        if scopes.is_empty() {
            return Vec::new();
        }
        let Ok(token) = github_token() else {
            return vec![Sweep {
                kind: "live",
                key: "github".into(),
                source: String::new(),
                result: LiveResult::Unknown(
                    "no GitHub token: set GITHUB_TOKEN with Secrets: read".into(),
                ),
            }];
        };
        let http = Http::new(Service::Github, token).verbose(self.verbose);
        let listed: BTreeMap<&Scope, Result<Listed, String>> =
            scopes.iter().map(|s| (s, list(&http, s))).collect();
        let mut out = Vec::new();
        for c in creds.iter().filter(|c| c.status == Status::Active) {
            if let Some(result) = self.storage_result(c, &listed) {
                out.push(Sweep {
                    kind: "live_storage",
                    key: c.id.clone(),
                    source: String::new(),
                    result,
                });
            }
        }
        for (scope, found) in &listed {
            let Ok(found) = found else { continue };
            let declared: BTreeSet<&str> = creds
                .iter()
                .flat_map(|c| &c.storage)
                .filter(|s| Scope::of(s).as_ref() == Some(*scope))
                .map(|s| s.key.as_str())
                .collect();
            for name in found.keys().filter(|n| !declared.contains(n.as_str())) {
                out.push(Sweep {
                    kind: "undeclared",
                    key: name.clone(),
                    source: scope.label(),
                    result: LiveResult::Drift(format!(
                        "secret exists at GitHub ({}) but no [[credential]] storage names it",
                        scope.label()
                    )),
                });
            }
        }
        out
    }
}

impl Probe<'_> {
    fn storage_result(
        &self,
        c: &Credential,
        listed: &BTreeMap<&Scope, Result<Listed, String>>,
    ) -> Option<LiveResult> {
        let (mut missing, mut stale, mut unknown, mut ok) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for s in &c.storage {
            let Some(scope) = Scope::of(s) else { continue };
            let label = s.label();
            match &listed[&scope] {
                Err(hint) => unknown.push(format!("{label}: {hint}")),
                Ok(found) => match found.get(&s.key) {
                    None => missing.push(label),
                    Some(day) => {
                        let set = day.map_or_else(
                            || "at an unknown date".to_string(),
                            |d| format!("set {}", day_str(d)),
                        );
                        let due =
                            c.rotate_every.as_deref().zip(*day).and_then(|(every, d)| {
                                add_duration(d, every).map(|due| (every, due))
                            });
                        match due {
                            Some((every, due)) if due <= self.today => stale.push(format!(
                                "{label} last {set}, older than rotate_every {every}"
                            )),
                            _ => ok.push(format!("{label} {set}")),
                        }
                    }
                },
            }
        }
        let mut drift = Vec::new();
        if !missing.is_empty() {
            drift.push(format!("missing {}", missing.join(", ")));
        }
        drift.extend(stale);
        if !drift.is_empty() {
            return Some(LiveResult::Drift(drift.join("; ")));
        }
        if !unknown.is_empty() {
            return Some(LiveResult::Unknown(unknown.join("; ")));
        }
        (!ok.is_empty()).then(|| LiveResult::Ok(ok.join("; ")))
    }
}
