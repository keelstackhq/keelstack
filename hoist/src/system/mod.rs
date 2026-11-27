//! `system.toml` (spec §17, D-031): the external systems a repo uses and every credential it
//! holds, by name and metadata only (never values). `hoist system init|check|explain`.

pub mod catalog;
pub mod check;
pub mod explain;
pub mod init;
pub mod live;
pub mod rotate;
pub mod stamp;

use crate::error::HoistError;
use crate::timeutil::{add_months, parse_date};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

pub const FILE: &str = "system.toml";

fn temp_beside(path: &Path) -> std::path::PathBuf {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    path.with_file_name(format!(".{name}.hoist-tmp-{}", std::process::id()))
}

fn write_temp(tmp: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let _ = std::fs::remove_file(tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()
}

/// Replace `path` with `text` atomically: a temp file in the same directory, then a rename. The
/// existing file's mode is kept. A symlink is written through (the temp file sits next to the
/// resolved target and the link stays a link); a dangling link is an error.
pub(crate) fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let target = if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        std::fs::canonicalize(path)?
    } else {
        path.to_path_buf()
    };
    let tmp = temp_beside(&target);
    let done = write_temp(&tmp, text).and_then(|()| {
        if let Ok(m) = std::fs::metadata(&target) {
            std::fs::set_permissions(&tmp, m.permissions())?;
        }
        std::fs::rename(&tmp, &target)
    });
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

/// Create `path` with `text` atomically (temp file, then a hard link that fails when `path`
/// exists, even as a symlink); `false` when it already exists. Never overwrites.
pub(crate) fn create_atomic(path: &Path, text: &str) -> std::io::Result<bool> {
    let tmp = temp_beside(path);
    let done = write_temp(&tmp, text).and_then(|()| std::fs::hard_link(&tmp, path));
    let _ = std::fs::remove_file(&tmp);
    match done {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
}

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
    /// Ids (space-separated) of tokens an unfinished `auto` rotation minted and stored in some
    /// storages: the next rotation revokes them once the new value is stored everywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_ref: Option<String>,
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

#[cfg(test)]
mod atomic_tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_keeps_mode_and_leaves_no_temp() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("SYSTEM.md");
        std::fs::write(&p, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o640)).unwrap();
        }
        write_atomic(&p, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o640
            );
        }
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_writes_through_a_symlink_and_keeps_the_link() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("real")).unwrap();
        let t = d.path().join("real/doc.md");
        std::fs::write(&t, "old").unwrap();
        let l = d.path().join("SYSTEM.md");
        std::os::unix::fs::symlink(&t, &l).unwrap();
        write_atomic(&l, "new").unwrap();
        assert!(
            std::fs::symlink_metadata(&l)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&t).unwrap(), "new");
        let dangling = d.path().join("gone.md");
        std::os::unix::fs::symlink(d.path().join("nowhere"), &dangling).unwrap();
        assert!(write_atomic(&dangling, "x").is_err());
        assert!(
            std::fs::symlink_metadata(&dangling)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn create_atomic_never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("system.toml");
        assert!(create_atomic(&p, "one").unwrap());
        assert!(!create_atomic(&p, "two").unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "one");
        #[cfg(unix)]
        {
            let l = d.path().join("link.toml");
            std::os::unix::fs::symlink(d.path().join("nowhere"), &l).unwrap();
            assert!(!create_atomic(&l, "x").unwrap());
            assert!(!d.path().join("nowhere").exists());
        }
        // system.toml, plus the symlink on unix; no temp files.
        let n = if cfg!(unix) { 2 } else { 1 };
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), n);
    }
}
