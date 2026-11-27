//! `system.toml` (spec §17, D-031): the external systems a repo uses and every credential it
//! holds, by name and metadata only (never values). `hoist system init|check|explain`.

pub mod check;
pub mod explain;
pub mod init;
pub mod live;

use crate::error::HoistError;
use crate::timeutil::{add_months, parse_date};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

pub const FILE: &str = "system.toml";

static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9._-]*$").unwrap());
static DURATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([1-9][0-9]{0,4})([dwmy])$").unwrap());

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SystemFile {
    pub version: i64,
    pub system: SystemMeta,
    #[serde(default, rename = "provider", skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<Provider>,
    #[serde(default, rename = "credential", skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<Credential>,
    #[serde(default, rename = "trust", skip_serializing_if = "Vec::is_empty")]
    pub trusts: Vec<Trust>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SystemMeta {
    pub name: String,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub capability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub console: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clis: Vec<Cli>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cli {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mise: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredKind {
    ApiToken,
    DeployToken,
    AppKey,
    SaKey,
    SshKey,
    AgeKey,
    Other,
}

impl CredKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CredKind::ApiToken => "api_token",
            CredKind::DeployToken => "deploy_token",
            CredKind::AppKey => "app_key",
            CredKind::SaKey => "sa_key",
            CredKind::SshKey => "ssh_key",
            CredKind::AgeKey => "age_key",
            CredKind::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Active,
    Deprecated,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    pub id: String,
    pub provider: String,
    pub kind: CredKind,
    pub role: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_via: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_ttl: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate_every: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Rotation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub storage: Vec<Storage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumers: Vec<Consumer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_ref: Option<String>,
    #[serde(default, skip_serializing_if = "is_active")]
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyless_exempt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

fn is_active(s: &Status) -> bool {
    *s == Status::Active
}

impl Credential {
    /// The secret names this credential stands for (`storage[].key`), sorted, unique.
    pub fn keys(&self) -> BTreeSet<&str> {
        self.storage.iter().map(|s| s.key.as_str()).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    Auto,
    Assisted,
    Manual,
}

impl Strategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Strategy::Auto => "auto",
            Strategy::Assisted => "assisted",
            Strategy::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rotation {
    pub strategy: Strategy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StorageKind {
    GhSecret,
    GhEnvSecret,
    Sops,
    FlySecret,
    WorkerSecret,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    pub kind: StorageKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    pub key: String,
}

impl Storage {
    /// Short human form: `gh-env:site/KEY`, `sops:path#KEY`, `fly:app/KEY`.
    pub fn label(&self) -> String {
        let o = |v: &Option<String>| v.clone().unwrap_or_default();
        match self.kind {
            StorageKind::GhSecret => format!("gh:{}", self.key),
            StorageKind::GhEnvSecret => format!("gh-env:{}/{}", o(&self.env), self.key),
            StorageKind::Sops => format!("sops:{}#{}", o(&self.path), self.key),
            StorageKind::FlySecret => format!("fly:{}/{}", o(&self.app), self.key),
            StorageKind::WorkerSecret => format!("worker:{}/{}", o(&self.app), self.key),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Consumer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
}

impl Consumer {
    /// `file:job` as in the workflow scan, for a workflow consumer.
    pub fn job_ref(&self) -> Option<String> {
        Some(format!(
            "{}:{}",
            self.workflow.as_ref()?,
            self.job.as_ref()?
        ))
    }

    pub fn label(&self) -> String {
        match (&self.runtime, self.job_ref()) {
            (Some(r), _) => r.clone(),
            (None, Some(j)) => j,
            (None, None) => String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrustKind {
    OidcTrustedPublishing,
    GithubApp,
    Wif,
}

impl TrustKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TrustKind::OidcTrustedPublishing => "oidc-trusted-publishing",
            TrustKind::GithubApp => "github-app",
            TrustKind::Wif => "wif",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Trust {
    pub provider: String,
    pub kind: TrustKind,
    pub subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
}

/// An expiry: a day (days since the epoch) or `never`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expiry {
    On(i64),
    Never,
}

pub fn parse_expiry(s: &str) -> Option<Expiry> {
    if s == "never" {
        Some(Expiry::Never)
    } else {
        parse_date(s).map(Expiry::On)
    }
}

/// `day + duration`: `<n>d|w|m|y` (`m`, `y` calendar months).
pub fn add_duration(day: i64, d: &str) -> Option<i64> {
    let c = DURATION.captures(d)?;
    let n: i64 = c[1].parse().ok()?;
    Some(match &c[2] {
        "d" => day + n,
        "w" => day + 7 * n,
        "m" if n <= 1200 => add_months(day, n),
        "y" if n <= 100 => add_months(day, 12 * n),
        _ => return None,
    })
}

fn err(file: &str, message: impl Into<String>) -> HoistError {
    HoistError::Config {
        file: file.to_string(),
        message: message.into(),
    }
}

impl SystemFile {
    /// Parse and validate; every error is exit 3 and names the file (and key or id).
    pub fn parse(text: &str, file: &str) -> Result<SystemFile, HoistError> {
        let sys: SystemFile = toml::from_str(text).map_err(|e| err(file, e.to_string()))?;
        sys.validate(file)?;
        Ok(sys)
    }

    pub fn load(path: &Path) -> Result<SystemFile, HoistError> {
        let file = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|e| {
            err(
                &file,
                format!("cannot read ({e}); run `hoist system init` to draft it"),
            )
        })?;
        SystemFile::parse(&text, &file)
    }

    fn validate(&self, file: &str) -> Result<(), HoistError> {
        if self.version != 1 {
            return Err(err(
                file,
                format!("`version` must be 1, not {}", self.version),
            ));
        }
        let mut seen = BTreeSet::new();
        for p in &self.providers {
            if !ID.is_match(&p.id) {
                return Err(err(
                    file,
                    format!("provider id `{}` must match [a-z0-9][a-z0-9._-]*", p.id),
                ));
            }
            if !seen.insert(p.id.as_str()) {
                return Err(err(file, format!("provider `{}` is declared twice", p.id)));
            }
        }
        let providers = seen;
        let mut ids = BTreeSet::new();
        for c in &self.credentials {
            let at = format!("credential `{}`", c.id);
            if !ID.is_match(&c.id) {
                return Err(err(
                    file,
                    format!("{at}: id must match [a-z0-9][a-z0-9._-]*"),
                ));
            }
            if !ids.insert(c.id.as_str()) {
                return Err(err(file, format!("{at} is declared twice")));
            }
            if !providers.contains(c.provider.as_str()) {
                return Err(err(
                    file,
                    format!("{at}: provider `{}` is not a [[provider]] id", c.provider),
                ));
            }
            for (k, v) in [("created_at", &c.created_at)] {
                if let Some(v) = v
                    && parse_date(v).is_none()
                {
                    return Err(err(
                        file,
                        format!("{at}: `{k}` `{v}` is not a YYYY-MM-DD day"),
                    ));
                }
            }
            if let Some(v) = &c.expires_at
                && parse_expiry(v).is_none()
            {
                return Err(err(
                    file,
                    format!("{at}: `expires_at` `{v}` is not a YYYY-MM-DD day or `never`"),
                ));
            }
            for (k, v) in [("max_ttl", &c.max_ttl), ("rotate_every", &c.rotate_every)] {
                if let Some(v) = v
                    && add_duration(0, v).is_none()
                {
                    return Err(err(
                        file,
                        format!("{at}: `{k}` `{v}` is not a duration like 90d, 2w, 6m, 1y"),
                    ));
                }
            }
            for s in &c.storage {
                let need: &[(&str, &Option<String>)] = match s.kind {
                    StorageKind::GhSecret => &[("repo", &s.repo)],
                    StorageKind::GhEnvSecret => &[("repo", &s.repo), ("env", &s.env)],
                    StorageKind::Sops => &[("path", &s.path)],
                    StorageKind::FlySecret | StorageKind::WorkerSecret => &[("app", &s.app)],
                };
                if let Some((k, _)) = need.iter().find(|(_, v)| v.is_none()) {
                    return Err(err(
                        file,
                        format!("{at}: storage `{}` needs `{k}`", s.label()),
                    ));
                }
                if s.key.is_empty() {
                    return Err(err(file, format!("{at}: storage `key` is empty")));
                }
            }
            for u in &c.consumers {
                let workflow = u.workflow.is_some() && u.job.is_some();
                let ok = match &u.runtime {
                    Some(_) => u.workflow.is_none() && u.job.is_none() && u.environment.is_none(),
                    None => workflow,
                };
                if !ok {
                    return Err(err(
                        file,
                        format!(
                            "{at}: a consumer is either {{workflow, job, environment?}} or {{runtime}}"
                        ),
                    ));
                }
            }
        }
        for t in &self.trusts {
            if !providers.contains(t.provider.as_str()) {
                return Err(err(
                    file,
                    format!(
                        "trust `{}`: provider `{}` is not a [[provider]] id",
                        t.subject, t.provider
                    ),
                ));
            }
        }
        Ok(())
    }

    pub fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }
}

/// `path:line` of the `id = "<id>"` line inside a `[[credential]]` block, else the file.
pub(crate) fn source_of(text: &str, file: &str, id: &str) -> String {
    let needle = format!("id = \"{id}\"");
    let mut in_cred = false;
    for (n, l) in text.lines().enumerate() {
        let t = l.trim_start();
        if t.starts_with('[') {
            in_cred = t.starts_with("[[credential]]");
        } else if in_cred && t.starts_with(&needle) {
            return format!("{file}:{}", n + 1);
        }
    }
    file.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_add_days_weeks_and_calendar_months() {
        let d = parse_date("2026-01-31").unwrap();
        assert_eq!(add_duration(d, "90d"), Some(d + 90));
        assert_eq!(add_duration(d, "2w"), Some(d + 14));
        assert_eq!(add_duration(d, "1m"), parse_date("2026-02-28"));
        assert_eq!(add_duration(d, "1y"), parse_date("2027-01-31"));
        for bad in ["0d", "90", "d", "1h", "90 d", "-1d"] {
            assert_eq!(add_duration(d, bad), None, "{bad}");
        }
    }

    #[test]
    fn expiry_is_a_day_or_never() {
        assert_eq!(parse_expiry("never"), Some(Expiry::Never));
        assert!(parse_expiry("2027-02-30").is_none());
    }
}
