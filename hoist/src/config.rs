//! `hoist.toml` loading (spec §5). Unknown keys are an error naming the file and key.

use crate::error::HoistError;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::path::Path;

const FILE_NAME: &str = "hoist.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Managed {
    #[default]
    Hoist,
    External,
}

/// Where hoist keeps the secrets it writes (spec §8.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SecretScope {
    /// Repository secrets (the default).
    #[default]
    Repo,
    /// Environment secrets only: `release` (Cloudflare) and `splice-bot`, never repo-level.
    Environment,
}

/// `[github] tags_bypass`: who may bypass the `hoist: tags` ruleset (spec §5, §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TagsBypass {
    /// The GitHub Actions app (any workflow job with `contents: write`); the default.
    #[default]
    Actions,
    /// Only the repo's write deploy keys: hoist adds one (`hoist: release tags`) and seals its
    /// private key into the release environment's secret `RELEASE_TAG_KEY`.
    DeployKey,
}

impl<'de> Deserialize<'de> for TagsBypass {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match String::deserialize(d)?.as_str() {
            "actions" => Ok(TagsBypass::Actions),
            "deploy_key" => Ok(TagsBypass::DeployKey),
            other => Err(serde::de::Error::custom(format!(
                "`tags_bypass` must be \"actions\" or \"deploy_key\" (got {other:?})"
            ))),
        }
    }
}

/// `[github.environments.<name>]`: how hoist treats one GitHub environment.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EnvCfg {
    /// Deployment branch policies. Unset: the default branch. `[]`: no branch restriction.
    pub branches: Option<Vec<String>>,
    /// Deployment tag policies (name patterns such as `v*`), added to the branch policies.
    pub tags: Vec<String>,
    /// `false`: hoist leaves this environment alone.
    pub manage: bool,
}

impl Default for EnvCfg {
    fn default() -> Self {
        Self {
            branches: None,
            tags: Vec::new(),
            manage: true,
        }
    }
}

/// The App that must post a required check (spec §6.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckApp {
    /// `app = "<slug>"`, resolved with `GET /apps/{slug}`.
    Slug(String),
    /// `integration_id = <id>`.
    Id(u64),
}

/// One `[github] required_checks` entry: a plain `"context"` or `{ context, app | integration_id }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckCfg {
    pub context: String,
    pub app: Option<CheckApp>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCheck {
    context: Option<String>,
    app: Option<String>,
    integration_id: Option<u64>,
}

impl<'de> Deserialize<'de> for CheckCfg {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = CheckCfg;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a check name or a table { context, app | integration_id }")
            }
            fn visit_str<E: Error>(self, v: &str) -> Result<CheckCfg, E> {
                Ok(CheckCfg {
                    context: v.to_string(),
                    app: None,
                })
            }
            fn visit_map<A: MapAccess<'de>>(self, m: A) -> Result<CheckCfg, A::Error> {
                let raw = RawCheck::deserialize(serde::de::value::MapAccessDeserializer::new(m))?;
                let Some(context) = raw.context.filter(|c| !c.is_empty()) else {
                    return Err(A::Error::custom(
                        "a required_checks table needs a non-empty `context`",
                    ));
                };
                let app = match (raw.app, raw.integration_id) {
                    (Some(slug), None) => {
                        let ok = !slug.is_empty()
                            && slug
                                .chars()
                                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                        if !ok {
                            return Err(A::Error::custom(format!(
                                "required check `{context}`: `app` must be a slug ([a-z0-9-]+), \
                                 got {slug:?}; use `integration_id` for a numeric id"
                            )));
                        }
                        Some(CheckApp::Slug(slug))
                    }
                    (None, Some(0)) => {
                        return Err(A::Error::custom(format!(
                            "required check `{context}`: `integration_id` must be positive"
                        )));
                    }
                    (None, Some(id)) => Some(CheckApp::Id(id)),
                    (None, None) => None,
                    (Some(_), Some(_)) => {
                        return Err(A::Error::custom(format!(
                            "required check `{context}`: give `app` or `integration_id`, not both"
                        )));
                    }
                };
                Ok(CheckCfg { context, app })
            }
        }
        d.deserialize_any(V)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GithubCfg {
    pub visibility: Option<String>,
    pub required_checks: Option<Vec<CheckCfg>>,
    /// Required checks posted as commit statuses (not jobs), beside the built-in bosun set.
    pub statuses: Vec<String>,
    /// Secrets the workflows name that may stay unset: listed apart, never a human step.
    pub optional_secrets: Vec<String>,
    /// Repository variables hoist sets: `NAME = "value"` (names are upper-cased).
    pub variables: BTreeMap<String, String>,
    /// Variables the workflows read that may stay unset: listed apart, never a human step.
    pub optional_variables: Vec<String>,
    pub merge_queue: bool,
    pub actions_permissions: String,
    pub allow_auto_merge: bool,
    pub delete_branch_on_merge: bool,
    /// Opt-in: `Some(true)` wants GitHub's immutable releases on; unset leaves it alone.
    pub immutable_releases: Option<bool>,
    pub managed: Managed,
    pub secret_scope: SecretScope,
    /// Per-environment overrides; a name listed only here is created too.
    pub environments: BTreeMap<String, EnvCfg>,
    /// GitHub App slugs that must be installed on the repo, beside the inferred ones.
    pub apps: Vec<String>,
    /// `[github.tag_ruleset]`: the `hoist: tags` ruleset protecting release tags.
    pub tag_ruleset: Option<TagRulesetCfg>,
    /// Who bypasses the `hoist: tags` ruleset: `actions` (default) or `deploy_key`.
    pub tags_bypass: TagsBypass,
    /// With `deploy_key`: the environment that holds `RELEASE_TAG_KEY`. Unset: the release
    /// environment (`[release] environment`). A separate one keeps the tag key out of the job
    /// that publishes to registries.
    pub tags_key_environment: Option<String>,
    /// Opt-in: the repository admin role may merge a PR past the default-branch ruleset.
    pub admin_bypass: bool,
}

/// `[github.tag_ruleset]`: tags only the bypass actor may create, move or delete.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct TagRulesetCfg {
    /// Tag name patterns (`v*`); the ruleset includes `refs/tags/<pattern>`.
    pub patterns: Vec<String>,
    /// Who may bypass: only `github-actions` (the GitHub Actions app) is supported.
    pub bypass: Vec<String>,
}

impl Default for GithubCfg {
    fn default() -> Self {
        Self {
            visibility: None,
            required_checks: None,
            statuses: Vec::new(),
            optional_secrets: Vec::new(),
            variables: BTreeMap::new(),
            optional_variables: Vec::new(),
            merge_queue: false,
            actions_permissions: "read".to_string(),
            allow_auto_merge: true,
            delete_branch_on_merge: true,
            immutable_releases: None,
            managed: Managed::Hoist,
            secret_scope: SecretScope::Repo,
            environments: BTreeMap::new(),
            apps: Vec::new(),
            tag_ruleset: None,
            tags_bypass: TagsBypass::Actions,
            tags_key_environment: None,
            admin_bypass: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct CloudflareCfg {
    pub account_id: Option<String>,
    pub managed: Managed,
    /// `[cloudflare.dns]` (spec §6.3).
    pub dns: Option<crate::dns::DnsCfg>,
    /// `[cloudflare.r2]`: the backup bucket (spec §6.2).
    pub r2: Option<crate::r2::R2Cfg>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Versioning {
    #[default]
    PerPackage,
    Fixed,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ReleaseCfg {
    pub versioning: Versioning,
    pub tag_format: Option<String>,
    pub environment: String,
    /// GitHub logins that approve the release environment. Unset: the token's user; `[]`: no
    /// required reviewer at all.
    pub reviewers: Option<Vec<String>>,
    /// The workflow file (`release.yml`) that publishes in the release environment; unset: the
    /// first workflow job found that uses it.
    pub workflow: Option<String>,
}

impl Default for ReleaseCfg {
    fn default() -> Self {
        Self {
            versioning: Versioning::PerPackage,
            tag_format: None,
            environment: "release".to_string(),
            reviewers: None,
            workflow: None,
        }
    }
}

/// `[workspace]` (root only): workspace members beside `pnpm-workspace.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct WorkspaceCfg {
    /// Directories or `dir/*` globs read like pnpm workspace packages.
    pub members: Vec<String>,
}

/// `[registry.npm."<name>"]` / `[registry.crates."<name>"]` (root only): a package published
/// outside the release model, by its own workflow.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryPkgCfg {
    /// The workflow file (`cargo-deny-npm.yml`) its trusted publisher names.
    pub workflow: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct RegistryCfg {
    pub npm: BTreeMap<String, RegistryPkgCfg>,
    pub crates: BTreeMap<String, RegistryPkgCfg>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct RootConfig {
    pub github: GithubCfg,
    pub cloudflare: CloudflareCfg,
    pub release: ReleaseCfg,
    pub workspace: WorkspaceCfg,
    pub registry: RegistryCfg,
    /// Only for a single-package repo, whose root is the package (spec §5).
    pub publish: Option<PublishCfg>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DeployCfg {
    pub provider: Option<String>,
    pub project: Option<String>,
    pub dir: Option<String>,
    pub domain: Option<String>,
    /// The GitHub environment holding this target's deploy secrets under
    /// `secret_scope = "environment"`.
    pub environment: Option<String>,
    pub on: String,
}

impl Default for DeployCfg {
    fn default() -> Self {
        Self {
            provider: None,
            project: None,
            dir: None,
            domain: None,
            environment: None,
            on: "push".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NpmAccess {
    #[default]
    Public,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct NpmPublish {
    pub access: NpmAccess,
    pub launcher: bool,
    /// Shell command run in the package directory before it is packed and published.
    pub build: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct CratesPublish {}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct GithubRelease {
    pub targets: Vec<String>,
    pub bin: Option<String>,
}

/// `[publish]` (spec §5): how a package is released.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(try_from = "RawPublish")]
pub struct PublishCfg {
    pub npm: Option<NpmPublish>,
    pub crates: Option<CratesPublish>,
    /// `crates = false`: a crate that is not published to crates.io (it ships binaries only).
    pub no_crates: bool,
    pub github_release: Option<GithubRelease>,
}

/// `crates = {}` publishes to crates.io, `crates = false` opts out.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum CratesField {
    Flag(bool),
    Table(CratesPublish),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct RawPublish {
    npm: Option<NpmPublish>,
    crates: Option<CratesField>,
    github_release: Option<GithubRelease>,
}

impl TryFrom<RawPublish> for PublishCfg {
    type Error = String;

    fn try_from(raw: RawPublish) -> Result<Self, String> {
        let (crates, no_crates) = match raw.crates {
            None => (None, false),
            Some(CratesField::Table(c)) => (Some(c), false),
            Some(CratesField::Flag(false)) => (None, true),
            Some(CratesField::Flag(true)) => {
                return Err(
                    "`crates = true` is not valid: write `crates = {}` to publish to \
                            crates.io or `crates = false` to opt out"
                        .to_string(),
                );
            }
        };
        Ok(Self {
            npm: raw.npm,
            crates,
            no_crates,
            github_release: raw.github_release,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct TargetConfig {
    pub deploy: Option<DeployCfg>,
    pub publish: Option<PublishCfg>,
}

/// Load `<dir>/hoist.toml` as the root config. A missing file gives the defaults.
pub fn load_root(dir: &Path) -> Result<RootConfig, HoistError> {
    let cfg: RootConfig = load(dir)?;
    validate_variables(dir, &cfg.github)?;
    validate_tag_ruleset(dir, &cfg.github, &cfg.release.environment)?;
    validate_checks(dir, &cfg.github)?;
    Ok(cfg)
}

/// A pinned check's context is listed once (a second entry would pin it twice).
fn validate_checks(dir: &Path, g: &GithubCfg) -> Result<(), HoistError> {
    let list = g.required_checks.as_deref().unwrap_or_default();
    for (i, c) in list.iter().enumerate() {
        let twice = list
            .iter()
            .enumerate()
            .any(|(j, o)| j != i && o.context == c.context && (c.app.is_some() || o.app.is_some()));
        if twice {
            return Err(HoistError::Config {
                file: dir.join(FILE_NAME).display().to_string(),
                message: format!(
                    "required check `{}` is listed twice; list each context once",
                    c.context
                ),
            });
        }
    }
    Ok(())
}

/// `[github.tag_ruleset]`: at least one pattern; the bypass is `github-actions`, or, with
/// `tags_bypass = "deploy_key"`, the release-tags deploy key (then `bypass` stays unset).
fn validate_tag_ruleset(dir: &Path, g: &GithubCfg, release_env: &str) -> Result<(), HoistError> {
    let bad = |message: String| HoistError::Config {
        file: dir.join(FILE_NAME).display().to_string(),
        message,
    };
    if let Some(e) = &g.tags_key_environment
        && e != release_env
        && !g.environments.contains_key(e)
    {
        return Err(bad(format!(
            "`github.tags_key_environment = {e:?}` names an environment that is not declared: \
             add `[github.environments.{e}]` (or use `[release] environment`)"
        )));
    }
    let Some(t) = &g.tag_ruleset else {
        if g.tags_bypass == TagsBypass::DeployKey {
            return Err(bad(
                "`github.tags_bypass = \"deploy_key\"` needs a `[github.tag_ruleset]` with \
                 `patterns`; add it or remove `tags_bypass`"
                    .to_string(),
            ));
        }
        return Ok(());
    };
    if t.patterns.is_empty() || t.patterns.iter().any(|p| p.trim().is_empty()) {
        return Err(bad(
            "`github.tag_ruleset.patterns` needs at least one tag pattern such as `v*`; fix it"
                .to_string(),
        ));
    }
    if g.tags_bypass == TagsBypass::DeployKey {
        if !t.bypass.is_empty() {
            return Err(bad(format!(
                "`github.tag_ruleset.bypass` must be unset with `github.tags_bypass = \
                 \"deploy_key\"` (got {:?}): the release-tags deploy key is the bypass; remove \
                 `bypass`",
                t.bypass
            )));
        }
        return Ok(());
    }
    if t.bypass != ["github-actions"] {
        return Err(bad(format!(
            "`github.tag_ruleset.bypass` must be exactly [\"github-actions\"] (got {:?}); set it \
             in `[github.tag_ruleset]`",
            t.bypass
        )));
    }
    Ok(())
}

/// Variable names GitHub accepts: letters, digits and `_`, not starting with a digit, no
/// `GITHUB_` prefix; two keys differing only in case are the same variable.
fn validate_variables(dir: &Path, g: &GithubCfg) -> Result<(), HoistError> {
    let bad = |key: &str, why: &str| HoistError::Config {
        file: dir.join(FILE_NAME).display().to_string(),
        message: format!("invalid variable name `{key}`: {why}; fix it in `[github]`"),
    };
    let check = |key: &str| -> Result<(), HoistError> {
        let mut chars = key.chars();
        let first_ok = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if !first_ok || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(bad(
                key,
                "use letters, digits and `_`, not starting with a digit",
            ));
        }
        if key.to_ascii_uppercase().starts_with("GITHUB_") {
            return Err(bad(key, "names starting with `GITHUB_` are reserved"));
        }
        Ok(())
    };
    let mut seen: Vec<String> = Vec::new();
    for key in g.variables.keys() {
        check(key)?;
        let upper = key.to_ascii_uppercase();
        if seen.contains(&upper) {
            return Err(bad(key, "the same name as another key, ignoring case"));
        }
        seen.push(upper);
    }
    for key in &g.optional_variables {
        check(key)?;
    }
    Ok(())
}

/// Load `<dir>/hoist.toml` as a target config. A missing file gives the defaults.
pub fn load_target(dir: &Path) -> Result<TargetConfig, HoistError> {
    load(dir)
}

fn load<T: DeserializeOwned + Default>(dir: &Path) -> Result<T, HoistError> {
    let path = dir.join(FILE_NAME);
    let file = path.display().to_string();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => {
            return Err(HoistError::Config {
                file,
                message: format!("cannot read file: {e}; check its permissions"),
            });
        }
    };
    let config_err = |message: String| HoistError::Config {
        file: file.clone(),
        message,
    };
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| config_err(e.to_string().trim_end().to_string()))?;
    match T::deserialize(table.clone()) {
        Ok(cfg) => Ok(cfg),
        Err(whole) => Err(config_err(describe(&table, &whole, |t| {
            T::deserialize(t).err()
        }))),
    }
}

/// Name the offending key as a dotted path (`github.foo`) by retrying one top-level entry at a time.
fn describe<F>(table: &toml::Table, whole: &toml::de::Error, retry: F) -> String
where
    F: Fn(toml::Table) -> Option<toml::de::Error>,
{
    for (key, value) in table {
        let mut single = toml::Table::new();
        single.insert(key.clone(), value.clone());
        let Some(err) = retry(single) else { continue };
        return match unknown_field(err.message()) {
            Some(field) if field == *key => format!("unknown key `{key}`; remove it"),
            Some(field) => {
                // Several tables may hold a key of this name: the culprit is the one whose
                // removal clears the error.
                let mut paths = Vec::new();
                find_paths(value, field, &mut paths);
                let culprit = paths.iter().find(|p| {
                    let mut v = value.clone();
                    remove_at(&mut v, p, field);
                    let mut t = toml::Table::new();
                    t.insert(key.clone(), v);
                    retry(t).is_none_or(|e| unknown_field(e.message()) != Some(field))
                });
                let path = culprit.or(paths.first()).cloned().unwrap_or_default();
                let path: String = path.iter().map(|p| format!("{p}.")).collect();
                format!("unknown key `{key}.{path}{field}`; remove it")
            }
            None => format!("invalid `{key}`: {}", err.message()),
        };
    }
    whole.message().to_string()
}

/// Every table path below `value` (as segments) that holds `field`; arrays count as `name[i]`.
fn find_paths(value: &toml::Value, field: &str, out: &mut Vec<Vec<String>>) {
    fn walk(v: &toml::Value, field: &str, trail: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        match v {
            toml::Value::Table(t) => {
                if t.contains_key(field) {
                    out.push(trail.clone());
                }
                for (k, c) in t {
                    trail.push(k.clone());
                    walk(c, field, trail, out);
                    trail.pop();
                }
            }
            toml::Value::Array(a) => {
                for (i, c) in a.iter().enumerate() {
                    if let Some(last) = trail.last_mut() {
                        let base = last.clone();
                        *last = format!("{base}[{i}]");
                        walk(c, field, trail, out);
                        trail.pop();
                        trail.push(base);
                    }
                }
            }
            _ => {}
        }
    }
    walk(value, field, &mut Vec::new(), out);
}

fn remove_at(value: &mut toml::Value, path: &[String], field: &str) {
    let mut cur = value;
    for seg in path {
        let (name, idx) = match seg.split_once('[') {
            Some((n, i)) => (n, i.trim_end_matches(']').parse::<usize>().ok()),
            None => (seg.as_str(), None),
        };
        let Some(next) = cur.get_mut(name) else {
            return;
        };
        cur = match idx {
            Some(i) => match next.get_mut(i) {
                Some(n) => n,
                None => return,
            },
            None => next,
        };
    }
    if let toml::Value::Table(t) = cur {
        t.remove(field);
    }
}

fn unknown_field(message: &str) -> Option<&str> {
    let rest = message.strip_prefix("unknown field `")?;
    rest.split('`').next()
}

pub const PINS_FILE: &str = ".hoist/release.toml";

/// One package's entry in `.hoist/release.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Pin {
    pub version: Option<String>,
    pub skip: bool,
}

/// `.hoist/release.toml`: per-package version pins and skips.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct ReleasePins {
    pub packages: std::collections::BTreeMap<String, Pin>,
}

/// Parse the text of `.hoist/release.toml`. Unknown keys are an error.
pub fn parse_pins(text: &str) -> Result<ReleasePins, HoistError> {
    toml::from_str(text).map_err(|e| HoistError::Config {
        file: PINS_FILE.to_string(),
        message: e.message().trim_end().to_string(),
    })
}

/// Read `<dir>/.hoist/release.toml`. A missing file gives no pins.
pub fn read_pins(dir: &Path) -> Result<ReleasePins, HoistError> {
    match std::fs::read_to_string(dir.join(PINS_FILE)) {
        Ok(text) => parse_pins(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ReleasePins::default()),
        Err(e) => Err(HoistError::Config {
            file: PINS_FILE.to_string(),
            message: format!("cannot read file: {e}; check its permissions"),
        }),
    }
}
