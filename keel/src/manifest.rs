use crate::cli::UpArgs;
use crate::error::KeelError;
use crate::pins::Pins;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILE: &str = "keel.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Token {
    #[serde(default = "default_secret")]
    pub secret: String,
    #[serde(default = "default_environment")]
    pub environment: String,
}

fn default_secret() -> String {
    "SPLICE_BOT_TOKEN".into()
}
fn default_environment() -> String {
    "splice-bot".into()
}

impl Default for Token {
    fn default() -> Token {
        Token {
            secret: default_secret(),
            environment: default_environment(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dns {
    Cloudflare,
    #[default]
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fly {
    pub app: String,
    pub region: String,
    /// Public hostname, no scheme.
    pub host: String,
    #[serde(default)]
    pub dns: Dns,
    /// The Cloudflare zone of `host`: `host` itself or a suffix of it at a label boundary.
    /// Default: the last two labels of `host`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    /// The host sends no mail: hoist adds SPF and DMARC records for it (`no_mail = true`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_mail: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deploy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fly: Option<Fly>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    R2,
    S3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backup {
    pub provider: Provider,
    #[serde(default = "default_retention")]
    pub retention_days: u32,
    /// The Cloudflare account id for the R2 bucket; default: env `CLOUDFLARE_ACCOUNT_ID`
    /// (not a secret).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

impl Backup {
    /// The R2 bucket name for the fly app: `<lowercased app>-backup`.
    pub fn bucket(app: &str) -> String {
        format!("{}-backup", app.to_lowercase())
    }
}

fn default_retention() -> u32 {
    30
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirmed {
    #[serde(default)]
    pub agent_env: bool,
    /// The host whose DNS records were added by hand (`--confirm dns`). Counts only while it
    /// equals `[deploy.fly] host`. An old `dns = true` reads as unset.
    #[serde(
        default,
        deserialize_with = "confirmed_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub dns: Option<String>,
    /// The fly app whose backup bucket was created by hand (`--confirm backup`). Counts only
    /// while it equals `[deploy.fly] app`. An old `backup = true` reads as unset.
    #[serde(
        default,
        deserialize_with = "confirmed_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub backup: Option<String>,
}

/// A string is the confirmed value; a boolean (the old format) is void.
fn confirmed_value<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum V {
        Str(String),
        Bool(#[allow(dead_code)] bool),
    }
    Ok(match V::deserialize(d)? {
        V::Str(s) => Some(s),
        V::Bool(_) => None,
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pieces: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default)]
    pub token: Token,
    #[serde(default)]
    pub confirmed: Confirmed,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deploy: Option<Deploy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<Backup>,
}

fn manifest_err(message: impl Into<String>) -> KeelError {
    KeelError::Manifest {
        file: FILE.into(),
        message: message.into(),
    }
}

impl Manifest {
    pub const DEFAULT_BRANCH: &'static str = "keel/bootstrap";

    /// `dir/keel.toml`, or the empty manifest when the file does not exist.
    pub fn load(dir: &Path) -> Result<Manifest, KeelError> {
        let path = dir.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Manifest::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
            Err(e) => Err(manifest_err(format!("cannot read it: {e}"))),
        }
    }

    pub fn parse(text: &str) -> Result<Manifest, KeelError> {
        let m: Manifest =
            toml::from_str(text).map_err(|e| manifest_err(e.message().trim().to_string()))?;
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<(), KeelError> {
        if let Some(f) = self.fly() {
            for (key, v) in [("app", &f.app), ("region", &f.region), ("host", &f.host)] {
                if v.trim().is_empty() {
                    return Err(manifest_err(format!(
                        "[deploy.fly] `{key}` must not be empty"
                    )));
                }
            }
            dns_name("host", &f.host)?;
            if !valid_app(&f.app) {
                return Err(manifest_err(format!(
                    "[deploy.fly] `app` {:?} must be 1-63 characters of lowercase a-z, 0-9 and `-`",
                    f.app
                )));
            }
            if let Some(z) = &f.zone {
                dns_name("zone", z)?;
            }
            if let Some(z) = &f.zone
                && !(f.host == *z || f.host.ends_with(&format!(".{z}")))
            {
                return Err(manifest_err(format!(
                    "[deploy.fly] `zone` {z:?} must be `host` {:?} or a parent domain of it, e.g. example.co.uk for app.example.co.uk",
                    f.host
                )));
            }
        }
        if let Some(b) = &self.backup {
            if self.fly().is_none() {
                return Err(manifest_err(
                    "[backup] needs [deploy.fly] (the backup secrets go to the fly app): add a [deploy.fly] table or remove [backup]",
                ));
            }
            if b.retention_days < 1 {
                return Err(manifest_err("[backup] `retention_days` must be at least 1"));
            }
            if let Some(id) = &b.account_id
                && !valid_account_id(id)
            {
                return Err(manifest_err(
                    "[backup] `account_id` must be the 32-character lowercase hex Cloudflare account id",
                ));
            }
            let bucket = Backup::bucket(&self.fly().expect("checked above").app);
            if b.provider == Provider::R2
                && !((3..=63).contains(&bucket.len())
                    && bucket
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'))
            {
                return Err(manifest_err(format!(
                    "[deploy.fly] `app` gives the R2 bucket name {bucket:?}; a bucket name is 3-63 characters of a-z, 0-9 and `-`: rename the app or use provider = \"s3\""
                )));
            }
        }
        Ok(())
    }

    /// The `[deploy.fly]` table, if any.
    pub fn fly(&self) -> Option<&Fly> {
        self.deploy.as_ref().and_then(|d| d.fly.as_ref())
    }

    pub fn exists(dir: &Path) -> bool {
        dir.join(FILE).is_file()
    }

    pub fn write(&self, dir: &Path) -> Result<(), KeelError> {
        if let Some(r) = &self.repo {
            validate_repo(r)?;
        }
        let text = toml::to_string(self).map_err(|e| manifest_err(e.to_string()))?;
        // Atomic: a temp file in the same directory, then a rename over keel.toml.
        let tmp = dir.join(format!(".{FILE}.{}.tmp", std::process::id()));
        let res = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, dir.join(FILE)));
        if let Err(e) = res {
            let _ = std::fs::remove_file(&tmp);
            return Err(manifest_err(format!("cannot write it: {e}")));
        }
        Ok(())
    }

    /// For the `rust`/`ts` templates without a `template_ref`, take the `hull` pin. Returns
    /// whether the manifest changed. A custom source gets no default: it needs an explicit ref.
    pub fn default_template_ref(&mut self, pins: &Pins) -> bool {
        let hull = matches!(self.template.as_deref(), Some("rust" | "ts"));
        match pins.version("hull") {
            Some(v) if hull && self.template_ref.is_none() => {
                self.template_ref = Some(v.to_string());
                true
            }
            _ => false,
        }
    }

    pub fn branch_name(&self) -> &str {
        self.branch.as_deref().unwrap_or(Self::DEFAULT_BRANCH)
    }

    /// Fold the flags into the manifest. A flag that disagrees with a value already in the
    /// manifest is a usage error naming the key. Returns whether the manifest changed.
    pub fn merge_flags(&mut self, args: &UpArgs) -> Result<bool, KeelError> {
        let mut changed = false;
        if let Some(r) = &args.repo {
            validate_repo(r)?;
        }
        changed |= merge_opt("repo", &mut self.repo, args.repo.as_deref())?;
        changed |= merge_opt("template", &mut self.template, args.template.as_deref())?;
        changed |= merge_opt(
            "template-ref",
            &mut self.template_ref,
            args.template_ref.as_deref(),
        )?;
        changed |= merge_opt("plan", &mut self.plan, args.plan.as_deref())?;
        if let Some(p) = &args.pieces {
            let pieces = parse_pieces(p)?;
            if self.pieces.is_empty() {
                self.pieces = pieces;
                changed = true;
            } else if self.pieces != pieces {
                return Err(conflict(
                    "pieces",
                    &self.pieces.join(","),
                    &pieces.join(","),
                ));
            }
        }
        for step in &args.confirm {
            match step.as_str() {
                "agent-env" => {
                    changed |= !self.confirmed.agent_env;
                    self.confirmed.agent_env = true;
                }
                "dns" => {
                    let Some(f) = self.fly() else {
                        return Err(needs_table("dns", "[deploy.fly]"));
                    };
                    let host = f.host.clone();
                    changed |= self.confirmed.dns.as_deref() != Some(&host);
                    self.confirmed.dns = Some(host);
                }
                "backup" => {
                    if self.backup.is_none() {
                        return Err(needs_table("backup", "[backup]"));
                    }
                    let app = self.fly().map(|f| f.app.clone());
                    let Some(app) = app else {
                        return Err(needs_table("backup", "[deploy.fly]"));
                    };
                    changed |= self.confirmed.backup.as_deref() != Some(&app);
                    self.confirmed.backup = Some(app);
                }
                other => {
                    return Err(KeelError::Usage(format!(
                        "--confirm {other}: unknown step; the steps that take it are `agent-env`, `dns` and `backup`"
                    )));
                }
            }
        }
        Ok(changed)
    }
}

/// A fly app name: `^[a-z0-9-]+$`, at most 63 characters.
pub fn valid_app(s: &str) -> bool {
    (1..=63).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// A Cloudflare account id: `^[0-9a-f]{32}$`.
pub fn valid_account_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// A DNS name for the manifest key `key`: lowercase labels of a-z, 0-9 and `-` (1-63 characters,
/// not starting or ending with `-`) separated by single dots, at least two labels, at most 253
/// characters in all.
fn dns_name(key: &str, v: &str) -> Result<(), KeelError> {
    let label_ok = |l: &str| {
        (1..=63).contains(&l.len())
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    };
    if v.len() <= 253 && v.split('.').count() >= 2 && v.split('.').all(label_ok) {
        return Ok(());
    }
    let hint = if v.bytes().any(|c| c.is_ascii_uppercase()) {
        " (use lowercase)"
    } else {
        ""
    };
    Err(manifest_err(format!(
        "[deploy.fly] `{key}` {v:?} is not a DNS name: expected lowercase labels of a-z, 0-9 and `-` separated by dots, at least two, e.g. app.example.com (no scheme, path, port, `@`, `?` or `#`){hint}"
    )))
}

/// `OWNER/NAME`: `^[A-Za-z0-9-]+/[A-Za-z0-9._-]+$` and no `..`.
pub fn validate_repo(repo: &str) -> Result<(), KeelError> {
    let ok = repo.split_once('/').is_some_and(|(owner, name)| {
        !owner.is_empty()
            && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !name.is_empty()
            && name != "."
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    }) && !repo.contains("..");
    if ok {
        Ok(())
    } else {
        Err(KeelError::Usage(format!(
            "--repo {repo:?}: expected OWNER/NAME (letters, digits, `-`; the name may also have `.` and `_`), e.g. keelstackhq/my-app"
        )))
    }
}

fn needs_table(step: &str, table: &str) -> KeelError {
    KeelError::Usage(format!(
        "--confirm {step} needs {table} in {FILE}: it records the host or app it was done for"
    ))
}

fn conflict(key: &str, have: &str, flag: &str) -> KeelError {
    KeelError::Usage(format!(
        "--{key} {flag} disagrees with `{key} = {have}` in {FILE}: change the flag or edit {FILE}"
    ))
}

fn merge_opt(key: &str, field: &mut Option<String>, flag: Option<&str>) -> Result<bool, KeelError> {
    match (field.as_deref(), flag) {
        (_, None) => Ok(false),
        (None, Some(f)) => {
            *field = Some(f.to_string());
            Ok(true)
        }
        (Some(have), Some(f)) if have == f => Ok(false),
        (Some(have), Some(f)) => Err(conflict(key, have, f)),
    }
}

/// `ID[:NAME],ID[:NAME]...`
pub fn parse_pieces(s: &str) -> Result<Vec<String>, KeelError> {
    let pieces: Vec<String> = s.split(',').map(|p| p.trim().to_string()).collect();
    if pieces.iter().any(|p| p.is_empty() || p.starts_with(':')) {
        return Err(KeelError::Usage(format!(
            "--pieces {s:?}: expected a comma list of ID or ID:NAME, e.g. rust-cli:my-cli"
        )));
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> UpArgs {
        UpArgs::default()
    }

    #[test]
    fn parses_full_manifest_with_defaults() {
        let m = Manifest::parse(
            "repo = \"o/n\"\ntemplate = \"rust\"\npieces = [\"rust-cli:x\"]\n[confirmed]\nagent_env = true\n",
        )
        .unwrap();
        assert_eq!(m.repo.as_deref(), Some("o/n"));
        assert_eq!(m.token.secret, "SPLICE_BOT_TOKEN");
        assert_eq!(m.token.environment, "splice-bot");
        assert!(m.confirmed.agent_env);
        assert_eq!(m.branch_name(), "keel/bootstrap");
    }

    #[test]
    fn unknown_key_is_a_manifest_error_naming_it() {
        let e = Manifest::parse("repo = \"o/n\"\nreop = 1\n").unwrap_err();
        assert!(matches!(e, KeelError::Manifest { .. }));
        assert!(e.to_string().contains("reop"), "{e}");
        let e = Manifest::parse("[token]\nsecrt = \"x\"\n").unwrap_err();
        assert!(e.to_string().contains("secrt"), "{e}");
    }

    const FLY: &str = "[deploy.fly]\napp = \"a\"\nregion = \"fra\"\nhost = \"app.example.com\"\n";

    #[test]
    fn deploy_and_backup_tables_parse_with_defaults_and_round_trip() {
        let m = Manifest::parse(&format!("{FLY}[backup]\nprovider = \"r2\"\n")).unwrap();
        let f = m.fly().unwrap();
        assert_eq!((f.app.as_str(), f.dns), ("a", Dns::None));
        let b = m.backup.as_ref().unwrap();
        assert_eq!((b.provider, b.retention_days), (Provider::R2, 30));
        assert_eq!((f.no_mail, b.account_id.as_deref()), (false, None));
        let m2 = Manifest::parse(&format!(
            "{FLY}no_mail = true\n[backup]\nprovider = \"r2\"\naccount_id = \"0123456789abcdef0123456789abcdef\"\n"
        ))
        .unwrap();
        assert!(m2.fly().unwrap().no_mail);
        assert_eq!(
            m2.backup.unwrap().account_id.as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        let dir = tempfile::tempdir().unwrap();
        m.write(dir.path()).unwrap();
        assert_eq!(Manifest::load(dir.path()).unwrap(), m);
        assert!(Manifest::parse("").unwrap().fly().is_none());
    }

    #[test]
    fn deploy_and_backup_errors_name_the_key() {
        for (text, want) in [
            (format!("{FLY}dns = \"route53\"\n"), "route53"),
            (format!("{FLY}bogus = \"x\"\n"), "bogus"),
            (format!("{FLY}zone = \"x.com\"\n"), "zone"),
            (format!("{FLY}zone = \"ample.com\"\n"), "zone"),
            (format!("{FLY}[backup]\nprovider = \"gcs\"\n"), "gcs"),
            (
                format!("{FLY}[backup]\nprovider = \"r2\"\nretention = 3\n"),
                "retention",
            ),
            (
                format!("{FLY}[backup]\nprovider = \"r2\"\nretention_days = 0\n"),
                "retention_days",
            ),
            ("[backup]\nprovider = \"r2\"\n".to_string(), "[deploy.fly]"),
            (
                format!(
                    "{}[backup]\nprovider = \"r2\"\n",
                    FLY.replace("\"a\"", "\"my_app\"")
                ),
                "deploy.fly",
            ),
            (format!("{FLY}no_mails = true\n"), "no_mails"),
            (
                format!("{FLY}[backup]\nprovider = \"r2\"\naccount_ids = \"x\"\n"),
                "account_ids",
            ),
            (
                "[deploy.fly]\napp = \"a\"\nregion = \"fra\"\n".to_string(),
                "host",
            ),
            (FLY.replace("app.example.com", "https://x.io"), "host"),
        ] {
            let e = Manifest::parse(&text).unwrap_err();
            assert!(matches!(e, KeelError::Manifest { .. }), "{text}");
            assert_eq!(e.exit_code(), 3);
            assert!(e.to_string().contains(want), "{text}: {e}");
        }
    }

    #[test]
    fn host_zone_app_and_account_id_are_validated_shapes() {
        let bad_names = [
            "app.example.com@evil.io",
            "app.example.com?x=1",
            "app.example.com#x",
            "app.example.com:443",
            "app.example.com/x",
            "app. example.com",
            "App.example.com",
            "app..example.com",
            "-app.example.com",
            "app-.example.com",
            "app.example.com.",
            "localhost",
        ];
        for h in bad_names {
            let e = Manifest::parse(&FLY.replace("app.example.com", h)).unwrap_err();
            assert_eq!(e.exit_code(), 3, "{h}");
            assert!(e.to_string().contains("host"), "{h}: {e}");
        }
        assert!(Manifest::parse(&FLY.replace("app.example.com", "a\\\"b.example.com")).is_err());
        let long = format!("{}.com", "a".repeat(64));
        assert!(Manifest::parse(&FLY.replace("app.example.com", &long)).is_err());
        let e = Manifest::parse(&FLY.replace("app.example.com", "App.example.com")).unwrap_err();
        assert!(e.to_string().contains("lowercase"), "{e}");
        for z in ["EXAMPLE.com", "example.com@x", "example com"] {
            let e = Manifest::parse(&format!("{FLY}zone = {z:?}\n")).unwrap_err();
            assert!(e.to_string().contains("zone"), "{z}: {e}");
        }
        for a in ["A", "a\"b", "a b", "a_b", "My-App"] {
            let t = FLY.replace("app = \"a\"", &format!("app = {a:?}"));
            let e = Manifest::parse(&t).unwrap_err();
            assert!(e.to_string().contains("`app`"), "{a}: {e}");
        }
        assert!(
            Manifest::parse(&FLY.replace("\"a\"", &format!("\"{}\"", "a".repeat(64)))).is_err()
        );
        for id in [
            "abc",
            "ABCDEF0123456789ABCDEF0123456789",
            "\\\"x",
            &"g".repeat(32),
        ] {
            let t = format!("{FLY}[backup]\nprovider = \"r2\"\naccount_id = {id:?}\n");
            let e = Manifest::parse(&t).unwrap_err();
            assert!(e.to_string().contains("account_id"), "{id}: {e}");
        }
        let id = "0123456789abcdef0123456789abcdef";
        let t = format!("{FLY}[backup]\nprovider = \"r2\"\naccount_id = \"{id}\"\n");
        assert!(Manifest::parse(&t).is_ok());
    }

    #[test]
    fn zone_is_the_host_or_a_parent_domain_at_a_label_boundary() {
        for ok in ["app.example.com", "example.com"] {
            let m = Manifest::parse(&format!("{FLY}zone = \"{ok}\"\n")).unwrap();
            assert_eq!(m.fly().unwrap().zone.as_deref(), Some(ok));
        }
        assert_eq!(Manifest::parse(FLY).unwrap().fly().unwrap().zone, None);
    }

    #[test]
    fn flags_fill_an_empty_manifest_and_round_trip() {
        let mut m = Manifest::default();
        let a = UpArgs {
            repo: Some("o/n".into()),
            template: Some("rust".into()),
            pieces: Some("rust-cli:my-cli, ts-lib".into()),
            ..args()
        };
        assert!(m.merge_flags(&a).unwrap());
        assert_eq!(m.pieces, ["rust-cli:my-cli", "ts-lib"]);
        let dir = tempfile::tempdir().unwrap();
        m.write(dir.path()).unwrap();
        assert_eq!(Manifest::load(dir.path()).unwrap(), m);
        // Same flags again: no change.
        assert!(!m.merge_flags(&a).unwrap());
    }

    #[test]
    fn disagreeing_flag_is_a_usage_error_naming_the_key() {
        let mut m = Manifest::parse("repo = \"o/n\"\ntemplate = \"rust\"\n").unwrap();
        let e = m
            .merge_flags(&UpArgs {
                template: Some("ts".into()),
                ..args()
            })
            .unwrap_err();
        assert!(matches!(e, KeelError::Usage(_)));
        assert!(e.to_string().contains("template"), "{e}");
        let e = m
            .merge_flags(&UpArgs {
                repo: Some("o/other".into()),
                ..args()
            })
            .unwrap_err();
        assert!(e.to_string().contains("repo"), "{e}");
    }

    #[test]
    fn pieces_conflict_and_bad_list() {
        let mut m = Manifest::parse("pieces = [\"a\"]\n").unwrap();
        let e = m
            .merge_flags(&UpArgs {
                pieces: Some("b".into()),
                ..args()
            })
            .unwrap_err();
        assert!(e.to_string().contains("pieces"), "{e}");
        assert!(parse_pieces("a,,b").is_err());
    }

    #[test]
    fn confirm_agent_env_is_recorded_and_others_rejected() {
        let mut m = Manifest::default();
        assert!(
            m.merge_flags(&UpArgs {
                confirm: vec!["agent-env".into()],
                ..args()
            })
            .unwrap()
        );
        assert!(m.confirmed.agent_env);
        // dns and backup need the table they confirm.
        for what in ["dns", "backup"] {
            let e = m
                .merge_flags(&UpArgs {
                    confirm: vec![what.into()],
                    ..args()
                })
                .unwrap_err();
            assert!(
                e.to_string().contains(&format!("--confirm {what} needs")),
                "{what}: {e}"
            );
        }
        let mut m = Manifest::parse(&format!("{FLY}[backup]\nprovider = \"r2\"\n")).unwrap();
        assert!(
            m.merge_flags(&UpArgs {
                confirm: vec!["dns".into(), "backup".into()],
                ..args()
            })
            .unwrap()
        );
        assert_eq!(m.confirmed.dns.as_deref(), Some("app.example.com"));
        assert_eq!(m.confirmed.backup.as_deref(), Some("a"));
        // Same again: nothing changes; the value round-trips as a string.
        assert!(
            !m.merge_flags(&UpArgs {
                confirm: vec!["dns".into()],
                ..args()
            })
            .unwrap()
        );
        let t = toml::to_string(&m).unwrap();
        assert!(
            t.contains("dns = \"app.example.com\"") && t.contains("backup = \"a\""),
            "{t}"
        );
        assert_eq!(Manifest::parse(&t).unwrap(), m);
        // A new host is a new confirmation.
        m.deploy.as_mut().unwrap().fly.as_mut().unwrap().host = "b.example.com".into();
        assert!(
            m.merge_flags(&UpArgs {
                confirm: vec!["dns".into()],
                ..args()
            })
            .unwrap()
        );
        assert_eq!(m.confirmed.dns.as_deref(), Some("b.example.com"));
        assert!(
            m.merge_flags(&UpArgs {
                confirm: vec!["merge".into()],
                ..args()
            })
            .is_err()
        );
    }

    #[test]
    fn an_old_boolean_confirmation_is_void() {
        let m =
            Manifest::parse("[confirmed]\nagent_env = true\ndns = true\nbackup = false\n").unwrap();
        assert!(m.confirmed.agent_env);
        assert_eq!((m.confirmed.dns, m.confirmed.backup), (None, None));
    }

    #[test]
    fn missing_file_loads_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Manifest::load(dir.path()).unwrap(), Manifest::default());
        assert!(!Manifest::exists(dir.path()));
    }

    #[test]
    fn repo_must_be_owner_slash_name() {
        for ok in ["o/n", "Key-stone/my_app.rs", "a1/b-2"] {
            validate_repo(ok).unwrap();
        }
        for bad in [
            "", "o", "o/", "/n", "o/n/x", "o/..", "o/a..b", "o/.", "o_x/n", "o/n x", "o/n;rm",
            "../n", "o/n\n",
        ] {
            let e = Manifest::default()
                .merge_flags(&UpArgs {
                    repo: Some(bad.into()),
                    ..args()
                })
                .unwrap_err();
            assert!(matches!(e, KeelError::Usage(_)), "{bad:?}");
            assert_eq!(e.exit_code(), 2);
        }
    }

    #[test]
    fn write_is_atomic_and_leaves_no_temp_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(FILE), "repo = \"old/old\"\n").unwrap();
        let m = Manifest {
            repo: Some("o/n".into()),
            ..Manifest::default()
        };
        m.write(d.path()).unwrap();
        assert_eq!(Manifest::load(d.path()).unwrap(), m);
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, [FILE]);
        // A bad repo never reaches the disk.
        let bad = Manifest {
            repo: Some("../x".into()),
            ..Manifest::default()
        };
        assert!(matches!(bad.write(d.path()), Err(KeelError::Usage(_))));
        assert_eq!(Manifest::load(d.path()).unwrap(), m);
    }

    #[test]
    fn template_ref_comes_from_the_hull_pin_for_hull_templates_only() {
        let pins = Pins::from_texts("t", "[pins]\nhull = \"v1\"\n", None).unwrap();
        let mut m = Manifest::parse("template = \"rust\"\n").unwrap();
        assert!(m.default_template_ref(&pins));
        assert_eq!(m.template_ref.as_deref(), Some("v1"));
        assert!(!m.default_template_ref(&pins));
        let mut m = Manifest::parse("template = \"gh:x/y\"\n").unwrap();
        assert!(!m.default_template_ref(&pins));
        assert_eq!(m.template_ref, None);
        m.merge_flags(&UpArgs {
            template_ref: Some("v9".into()),
            ..args()
        })
        .unwrap();
        assert_eq!(m.template_ref.as_deref(), Some("v9"));
        // A repo pins.toml `hull` overrides the embedded pin.
        let pins = Pins::from_texts(
            "t",
            "[pins]\nhull = \"v1\"\n",
            Some("[pins]\nhull = \"v2\"\n"),
        )
        .unwrap();
        let mut m = Manifest::parse("template = \"ts\"\n").unwrap();
        m.default_template_ref(&pins);
        assert_eq!(m.template_ref.as_deref(), Some("v2"));
    }

    #[test]
    fn embedded_pins_pin_hull_to_v0_13_0() {
        let pins = Pins::from_texts("e", crate::pins::EMBEDDED, None).unwrap();
        assert_eq!(pins.version("hull"), Some("v0.13.0"));
    }
}
