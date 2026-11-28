//! `hoist system check --live` (spec §17.5): one probe per provider reads the provider's view of
//! a credential with GET requests only (or a read-only `fly` subcommand). Token values come from
//! env and are never printed. A missing token, a 401/403 or an API error is `unknown` with a
//! fix, never a failure of the whole check.

mod cloudflare;
mod crates;
mod fly;
mod github;
mod google;
mod npm;

use super::{Credential, Expiry, SystemFile, parse_expiry};
use crate::auth::{Secret, from_env};
use crate::http::HttpError;
use crate::timeutil::{civil_from_days, parse_date};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveResult {
    /// The provider agrees with system.toml; the detail says what was read.
    Ok(String),
    /// The provider disagrees (expired, revoked, other scopes); the detail says how.
    Drift(String),
    /// Not readable (no probe, no token, API error); never a failure on its own.
    Unknown(String),
}

/// A finding that is not one credential's: a secret the provider holds that no `[[credential]]`
/// declares (`kind = "undeclared"`), or a scope the probe could not list (`kind = "live"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sweep {
    pub kind: &'static str,
    pub key: String,
    pub source: String,
    pub result: LiveResult,
}

/// What every probe may read: the repo dir (for `fly`), the system file, today (`HOIST_NOW`).
pub struct Ctx<'a> {
    pub dir: &'a Path,
    pub sys: &'a SystemFile,
    pub today: i64,
    pub verbose: bool,
}

pub trait LiveProbe {
    /// The `[[provider]] id` this probe answers for.
    fn provider(&self) -> &str;
    /// Whether it answers for provider `id` (Google answers for `google`, `gcp` and `gsc`).
    fn answers(&self, id: &str) -> bool {
        self.provider() == id
    }
    fn probe(&self, cred: &Credential) -> LiveResult;
    /// Findings across credentials (undeclared secrets); called once after every probe.
    fn sweep(&self) -> Vec<Sweep> {
        Vec::new()
    }
}

/// The registered probes, built once per check (each reads its tokens from env here).
pub fn probes<'a>(ctx: &'a Ctx<'a>) -> Vec<Box<dyn LiveProbe + 'a>> {
    vec![
        Box::new(cloudflare::Probe::new(ctx)),
        Box::new(github::Probe::new(ctx)),
        Box::new(fly::Probe::new(ctx)),
        Box::new(npm::Probe::new(ctx)),
        Box::new(crates::Probe::new(ctx)),
        Box::new(google::Probe::new(ctx)),
    ]
}

/// The probe for `cred`'s provider, or `unknown`.
pub fn probe(probes: &[Box<dyn LiveProbe + '_>], cred: &Credential) -> LiveResult {
    probes
        .iter()
        .find(|p| p.answers(&cred.provider))
        .map_or_else(
            || {
                LiveResult::Unknown(format!(
                    "no live probe for provider `{}` yet",
                    cred.provider
                ))
            },
            |p| p.probe(cred),
        )
}

// ---- helpers shared by the probes ----

/// The credential's own value, when one of its `storage[].key` names is set in env (in a job
/// that reads it), else the first of `fallback` that is set. Only names the credential's
/// provider owns ([`owned_names`]) are ever read; any other name is skipped (see
/// [`no_token`]). Never printed.
pub(crate) fn token_for(cred: &Credential, fallback: &[&str]) -> Option<Secret> {
    env_names(cred, fallback).into_iter().find_map(from_env)
}

/// `gcp` and `gsc` credentials are Google's.
fn provider_of(cred: &Credential) -> &str {
    match cred.provider.as_str() {
        "gcp" | "gsc" => "google",
        p => p,
    }
}

/// The token env names a provider owns: the only names hoist reads a credential's value from.
pub(crate) fn owned_names(provider: &str) -> &'static [&'static str] {
    match provider {
        "github" => &["GITHUB_TOKEN", "GH_TOKEN", "GH_PAT"],
        "cloudflare" => &["CLOUDFLARE_API_TOKEN"],
        "fly" => &["FLY_API_TOKEN", "FLY_ACCESS_TOKEN"],
        "npm" => &["NPM_TOKEN"],
        "crates" => &["CARGO_REGISTRY_TOKEN"],
        "google" | "gcp" | "gsc" => &["GOOGLE_OAUTH_ACCESS_TOKEN"],
        _ => &[],
    }
}

/// The env names that may hold `cred`'s value, in order: its storage keys, then `fallback`,
/// each kept only when `cred`'s provider owns it, so a credential cannot make hoist read an
/// arbitrary env var or send, say, `GH_PAT` to Cloudflare. `GITHUB_TOKEN` (the job's own
/// token) is never a credential's value.
pub(crate) fn env_names<'a>(cred: &'a Credential, fallback: &[&'a str]) -> Vec<&'a str> {
    let owned = owned_names(provider_of(cred));
    cred.storage
        .iter()
        .map(|s| s.key.as_str())
        .filter(|k| *k != "GITHUB_TOKEN")
        .chain(fallback.iter().copied())
        .filter(|k| owned.contains(k))
        .collect()
}

/// The storage keys hoist will not read because the provider does not own them.
pub(crate) fn foreign_keys(cred: &Credential) -> Vec<&str> {
    let owned = owned_names(provider_of(cred));
    cred.storage
        .iter()
        .map(|s| s.key.as_str())
        .filter(|k| *k != "GITHUB_TOKEN" && !owned.contains(k))
        .collect()
}

/// `unknown` for a token that is not in env: `why`, plus, when a storage key names an env var
/// the provider does not own, which names hoist reads instead.
pub(crate) fn no_token(cred: &Credential, why: &str) -> LiveResult {
    let foreign = foreign_keys(cred);
    let owned: Vec<&str> = owned_names(provider_of(cred))
        .iter()
        .copied()
        .filter(|n| *n != "GITHUB_TOKEN")
        .collect();
    if foreign.is_empty() {
        return LiveResult::Unknown(why.to_string());
    }
    let fix = if owned.is_empty() {
        format!("hoist reads no env var for provider `{}`", cred.provider)
    } else {
        format!("export the value as {} to check it", owned.join(" or "))
    };
    LiveResult::Unknown(format!(
        "{why}; hoist never reads {} (not a {} token env name): {fix}",
        foreign.join(", "),
        provider_of(cred)
    ))
}

/// The value named by one of the credential's storage keys only (no fallback).
pub(crate) fn own_token(cred: &Credential) -> Option<Secret> {
    token_for(cred, &[])
}

/// `unknown` for a failed read: 401 says the token is rejected; the fix names what to set.
pub(crate) fn unknown_err(err: &HttpError, fix: &str) -> LiveResult {
    match err {
        HttpError::Unauthorized { .. } => {
            LiveResult::Unknown(format!("token rejected (401): {fix}"))
        }
        other => LiveResult::Unknown(format!("{other}: {fix}")),
    }
}

/// The words of `provider_ref` (split on whitespace and `,`), to match an id or a name. Empty
/// when unset.
pub(crate) fn ref_words(cred: &Credential) -> Vec<&str> {
    cred.provider_ref
        .as_deref()
        .unwrap_or("")
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .collect()
}

/// Whether `provider_ref` names `id` or `name` (a whole word, or the whole ref).
pub(crate) fn ref_matches(cred: &Credential, id: &str, name: &str) -> bool {
    let whole = cred.provider_ref.as_deref().map(str::trim);
    let words = ref_words(cred);
    (!id.is_empty() && (words.contains(&id) || whole == Some(id)))
        || (!name.is_empty() && (words.contains(&name) || whole == Some(name)))
}

/// The entry `provider_ref` names: an id match first; else a name match, which must be unique
/// (`Err` carries the `unknown` text when several entries share the name).
pub(crate) fn find_ref<'t, T>(
    cred: &Credential,
    list: &'t [T],
    id_name: impl Fn(&T) -> (String, String),
) -> Result<Option<&'t T>, String> {
    let ref_text = cred.provider_ref.as_deref().unwrap_or("").trim();
    if let Some(t) = list.iter().find(|t| {
        let (id, _) = id_name(t);
        ref_matches(cred, &id, "")
    }) {
        return Ok(Some(t));
    }
    let named: Vec<&T> = list
        .iter()
        .filter(|t| {
            let (_, name) = id_name(t);
            ref_matches(cred, "", &name)
        })
        .collect();
    match named.len() {
        0 => Ok(None),
        1 => Ok(Some(named[0])),
        n => Err(format!(
            "provider_ref {ref_text} matches {n} tokens; set it to the token id"
        )),
    }
}

/// The day of an RFC 3339 timestamp or `YYYY-MM-DD…` (the first 10 chars).
pub(crate) fn day_of(ts: &str) -> Option<i64> {
    parse_date(ts.get(..10)?)
}

pub(crate) fn day_str(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Drift text when the provider's expiry (`None` = never expires) differs from `expires_at`,
/// or the credential has expired at the provider; `None` when they agree or nothing is
/// declared.
pub(crate) fn expiry_drift(cred: &Credential, live: Option<i64>, today: i64) -> Option<String> {
    if let Some(at) = live
        && at < today
    {
        return Some(format!("expired at the provider on {}", day_str(at)));
    }
    let declared = cred.expires_at.as_deref().and_then(parse_expiry)?;
    match (declared, live) {
        (Expiry::On(d), Some(l)) if d != l => Some(format!(
            "provider expiry {} differs from expires_at {}",
            day_str(l),
            day_str(d)
        )),
        (Expiry::On(d), None) => Some(format!(
            "provider says it never expires, expires_at is {}",
            day_str(d)
        )),
        (Expiry::Never, Some(l)) => Some(format!(
            "provider expiry {} but expires_at is never",
            day_str(l)
        )),
        _ => None,
    }
}

/// `last used <day>` / `never used` for an ok detail.
pub(crate) fn last_used(ts: Option<&str>) -> String {
    ts.and_then(day_of).map_or_else(
        || "never used".to_string(),
        |d| format!("last used {}", day_str(d)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::{CredKind, Status};

    fn cred(r: Option<&str>, expires: Option<&str>) -> Credential {
        Credential {
            id: "c".into(),
            provider: "cloudflare".into(),
            kind: CredKind::ApiToken,
            role: "deploy".into(),
            purpose: String::new(),
            owner: None,
            principal: None,
            scopes: vec![],
            resources: vec![],
            created_at: None,
            created_by: None,
            created_via: None,
            expires_at: expires.map(str::to_string),
            max_ttl: None,
            rotate_every: None,
            rotation: None,
            storage: vec![],
            consumers: vec![],
            provider_ref: r.map(str::to_string),
            status: Status::Active,
            keyless_exempt: None,
            notes: None,
        }
    }

    fn with_keys(provider: &str, keys: &[&str]) -> Credential {
        let mut c = cred(None, None);
        c.provider = provider.into();
        c.storage = keys
            .iter()
            .map(|k| crate::system::Storage {
                kind: crate::system::StorageKind::GhSecret,
                repo: None,
                env: None,
                path: None,
                app: None,
                key: k.to_string(),
            })
            .collect();
        c
    }

    #[test]
    fn token_names_are_limited_to_the_providers_own() {
        let c = with_keys(
            "cloudflare",
            &["GH_PAT", "NPM_TOKEN", "CF_DEPLOY", "GITHUB_TOKEN"],
        );
        assert_eq!(
            env_names(&c, &["CLOUDFLARE_API_TOKEN"]),
            ["CLOUDFLARE_API_TOKEN"]
        );
        assert_eq!(foreign_keys(&c), ["GH_PAT", "NPM_TOKEN", "CF_DEPLOY"]);
        let c = with_keys("github", &["GH_PAT", "CLOUDFLARE_API_TOKEN"]);
        assert_eq!(env_names(&c, &[]), ["GH_PAT"]);
        let c = with_keys("fly", &["GH_TOKEN", "FLY_API_TOKEN"]);
        assert_eq!(
            env_names(&c, &["FLY_API_TOKEN"]),
            ["FLY_API_TOKEN", "FLY_API_TOKEN"]
        );
        let c = with_keys("gsc", &["FLY_API_TOKEN"]);
        assert_eq!(
            env_names(&c, &["GOOGLE_OAUTH_ACCESS_TOKEN"]),
            ["GOOGLE_OAUTH_ACCESS_TOKEN"]
        );
    }

    #[test]
    fn a_storage_key_hoist_does_not_own_is_unknown_with_a_fix() {
        let c = with_keys("npm", &["MY_NPM_SECRET"]);
        let LiveResult::Unknown(why) = no_token(&c, "no npm token") else {
            panic!("not unknown");
        };
        assert!(why.starts_with("no npm token"), "{why}");
        assert!(why.contains("MY_NPM_SECRET"), "{why}");
        assert!(why.contains("NPM_TOKEN"), "{why}");
        let c = with_keys("npm", &["NPM_TOKEN"]);
        assert_eq!(
            no_token(&c, "no npm token"),
            LiveResult::Unknown("no npm token".into())
        );
        assert!(foreign_keys(&with_keys("github", &["GITHUB_TOKEN"])).is_empty());
    }

    #[test]
    fn provider_ref_matches_a_word_or_the_whole_ref() {
        let c = cred(Some("token id abc123"), None);
        assert!(ref_matches(&c, "abc123", "x"));
        assert!(!ref_matches(&c, "abc", "y"));
        let c = cred(Some("hoist:acme/app:deploy"), None);
        assert!(ref_matches(&c, "zz", "hoist:acme/app:deploy"));
        assert!(!ref_matches(&cred(None, None), "", ""));
    }

    #[test]
    fn find_ref_prefers_an_id_and_refuses_an_ambiguous_name() {
        let t = |id: &str, name: &str| (id.to_string(), name.to_string());
        let id_name = |x: &(String, String)| x.clone();
        let list = vec![t("a", "dup"), t("b", "dup"), t("dup", "other")];
        // an id match wins over name matches
        let c = cred(Some("dup"), None);
        assert_eq!(find_ref(&c, &list, id_name).unwrap(), Some(&list[2]));
        let list = vec![t("a", "dup"), t("b", "dup")];
        let err = find_ref(&c, &list, id_name).unwrap_err();
        assert!(err.contains("provider_ref dup matches 2 tokens"), "{err}");
        assert!(err.contains("set it to the token id"), "{err}");
        let list = vec![t("a", "dup"), t("b", "x")];
        assert_eq!(find_ref(&c, &list, id_name).unwrap(), Some(&list[0]));
        assert_eq!(
            find_ref(&cred(Some("zzz"), None), &list, id_name).unwrap(),
            None
        );
    }

    #[test]
    fn expiry_drift_compares_days_and_never() {
        let d = parse_date("2027-01-20").unwrap();
        let today = parse_date("2026-11-27").unwrap();
        assert_eq!(
            expiry_drift(&cred(None, Some("2027-01-20")), Some(d), today),
            None
        );
        assert!(expiry_drift(&cred(None, Some("2027-01-21")), Some(d), today).is_some());
        assert!(expiry_drift(&cred(None, Some("2027-01-20")), None, today).is_some());
        assert!(expiry_drift(&cred(None, Some("never")), Some(d), today).is_some());
        assert_eq!(expiry_drift(&cred(None, Some("never")), None, today), None);
        let past = parse_date("2026-11-01").unwrap();
        assert!(
            expiry_drift(&cred(None, None), Some(past), today)
                .unwrap()
                .contains("expired")
        );
    }
}
