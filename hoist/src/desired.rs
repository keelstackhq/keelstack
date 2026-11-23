//! The desired GitHub and Cloudflare state, joined from config and repo inference (spec §4, §6).

use crate::config::PublishCfg;
use crate::config::{EnvCfg, Managed, SecretScope, load_root};
use crate::error::HoistError;
use crate::release::{Package, PkgKind, read_release_model};
use crate::repo::Sourced;
use crate::repo::git::{GithubRepo, read_repo};
use crate::repo::targets::{Packages, Target, TargetKind, read_targets};
use crate::repo::workflows::{JobEnv, Workflows, read_workflows};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const RULESET_NAME: &str = "hoist: default branch";
pub const TAG_RULESET_NAME: &str = "hoist: tags";
/// The GitHub Actions app (github-actions[bot]): the only tag-ruleset bypass actor.
pub const ACTIONS_APP_ID: u64 = 15368;
/// GitHub's repository role id of `admin` (`actor_type` `RepositoryRole`): the default-branch
/// ruleset's only bypass actor, in `pull_request` mode (the owner merges a PR past the rules,
/// e.g. a mainfix; no direct push).
pub const ADMIN_ROLE_ID: u64 = 5;
/// The default-branch bypass actor as `<actor_type>:<actor_id>:<bypass_mode>`.
pub const ADMIN_BYPASS: &str = "RepositoryRole:5:pull_request";
/// With `tags_bypass = "deploy_key"`: the deploy keys bypass of the `hoist: tags` ruleset
/// (GitHub's `DeployKey` actor has no id: it stands for every write deploy key of the repo).
pub const DEPLOY_KEY_BYPASS: &str = "DeployKey:0:always";
/// The title of the write deploy key hoist adds for release tags.
pub const TAG_KEY_TITLE: &str = "hoist: release tags";
/// The environment secret holding that key's OpenSSH private key.
pub const TAG_KEY_SECRET: &str = "RELEASE_TAG_KEY";
const ACCOUNT_ENV: &str = "CLOUDFLARE_ACCOUNT_ID";
const DEPLOY_TOKEN: &str = "CLOUDFLARE_API_TOKEN";

#[derive(Debug, Clone, PartialEq)]
pub struct DesiredSettings {
    pub visibility: Option<Sourced<String>>,
    pub allow_auto_merge: Sourced<bool>,
    pub delete_branch_on_merge: Sourced<bool>,
    pub immutable_releases: Option<Sourced<bool>>,
}

/// A required check pinned to its source App (spec §6.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckPin {
    pub context: String,
    pub app: crate::config::CheckApp,
    /// The App's id: from `integration_id`, or from `GET /apps/{slug}` once resolved.
    pub id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesiredRuleset {
    pub name: String,
    pub branch: String,
    pub required_checks: Vec<Sourced<String>>,
    /// Checks pinned to the App that posts them (`[github] required_checks` tables); `id` is
    /// filled by `github::resolve_check_pins` (a slug) or given (`integration_id`).
    pub check_pins: Vec<CheckPin>,
    pub merge_queue: Sourced<bool>,
    /// `[github] admin_bypass`: the repository admin role may merge a PR past the rules.
    pub admin_bypass: bool,
}

/// `[github.tag_ruleset]`: the `hoist: tags` ruleset.
#[derive(Debug, Clone, PartialEq)]
pub struct DesiredTagRuleset {
    pub name: String,
    /// Tag name patterns (`v*`).
    pub patterns: Vec<String>,
    /// `hoist.toml:github.tag_ruleset`
    pub source: String,
    /// `[github] tags_bypass`.
    pub bypass: crate::config::TagsBypass,
    /// The environment the release-tags deploy key's secret goes to (`[release] environment`).
    pub key_env: String,
}

impl DesiredTagRuleset {
    /// `tags_bypass = "deploy_key"`.
    pub fn deploy_key(&self) -> bool {
        self.bypass == crate::config::TagsBypass::DeployKey
    }

    /// `conditions.ref_name.include` of the ruleset, sorted.
    pub fn refs(&self) -> Vec<String> {
        let mut refs: Vec<String> = self
            .patterns
            .iter()
            .map(|p| format!("refs/tags/{p}"))
            .collect();
        refs.sort();
        refs.dedup();
        refs
    }
}

/// `[github.environments.<name>]` for one environment.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvSettings {
    /// `None`: the default branch; `Some([])`: no branch restriction.
    pub branches: Option<Vec<String>>,
    /// Deployment tag policies (`tags = ["v*"]`).
    pub tags: Vec<String>,
    pub manage: bool,
    /// `hoist.toml:github.environments.<name>`
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesiredGithub {
    pub settings: DesiredSettings,
    pub ruleset: DesiredRuleset,
    /// `[github.tag_ruleset]`; `None` leaves tags alone.
    pub tag_ruleset: Option<DesiredTagRuleset>,
    pub actions_permissions: Sourced<String>,
    pub environments: Vec<Sourced<String>>,
    /// `environment:` expressions in workflows (`${{ … }}`): reported `unknown`, never created.
    pub unresolved_environments: Vec<Sourced<String>>,
    /// `[github.environments.<name>]` overrides.
    pub env_settings: BTreeMap<String, EnvSettings>,
    pub secrets: Vec<Sourced<String>>,
    /// Repository variables the workflows read (`vars.X`) and `[github.variables]` lists,
    /// upper-cased.
    pub variables: Vec<Sourced<String>>,
    /// `[github.variables]` values by upper-cased name.
    pub variable_values: BTreeMap<String, Sourced<String>>,
    /// Per variable the workflows read: the environments of the jobs that read it and the
    /// jobs that read it without one.
    pub variable_homes: Vec<SecretHomes>,
    /// `[github] optional_variables`, upper-cased: reported apart, never a human step.
    pub optional_variables: Vec<String>,
    /// `[github] optional_secrets`, upper-cased: reported apart, never a human step.
    pub optional_secrets: Vec<String>,
    /// `[github] statuses`: required checks that are commit statuses, not jobs.
    pub statuses: Vec<String>,
    pub apps: Vec<Sourced<String>>,
    pub managed: Managed,
    /// `secret_scope = "environment"`: secrets live in the environments of the jobs that
    /// read them, never in the repository.
    pub env_scope: bool,
    /// Per secret the workflows read: the environments of the jobs that read it, and the jobs
    /// that read it with no `environment:` (spec §8.6).
    pub secret_homes: Vec<SecretHomes>,
    /// `file:job` of each job that passes `secrets: inherit` to another repo's workflow.
    pub opaque_inherits: Vec<String>,
    /// `file:job` of each job with a `secrets` use the scan cannot name.
    pub unresolved_secrets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SecretHomes {
    pub name: String,
    pub envs: Vec<String>,
    /// `file:job` of each job that reads it without an environment.
    pub envless: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesiredPages {
    pub target: String,
    /// The target directory relative to the repo root (`apps/site`).
    pub dir: String,
    pub project: Sourced<String>,
    pub domain: Option<Sourced<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesiredCloudflare {
    pub account_id: Option<Sourced<String>>,
    pub pages: Vec<DesiredPages>,
    pub workers: Vec<Sourced<String>>,
    pub managed: Managed,
    /// `[cloudflare.dns]`, validated (spec §6.3).
    pub dns: Option<crate::dns::DnsCfg>,
    /// `[cloudflare.r2]` (spec §6.2).
    pub r2: Option<crate::r2::DesiredR2>,
}

/// What publishing needs (spec §9.4): the release environment and the workflow job that runs
/// in it, plus the packages with their `[publish]` settings. Present only when a package
/// publishes to npm or crates.io or has a GitHub Release.
#[derive(Debug, Clone, PartialEq)]
pub struct DesiredRelease {
    pub environment: Sourced<String>,
    /// `[release] reviewers`: logins that approve the environment. `None`: the token's user;
    /// `Some([])`: no required reviewer (D-022).
    pub reviewers: Option<Vec<String>>,
    /// `[release] workflow`: the workflow file that publishes, when named.
    pub workflow_name: Option<String>,
    /// The workflow job that declares `environment: <environment>`: in `workflow_name` when
    /// named, else the first one found.
    pub workflow: Option<JobEnv>,
    pub packages: Vec<Package>,
    /// `[registry.npm."<name>"]` and `[registry.crates."<name>"]`: packages published outside
    /// the release model, each by its own workflow.
    pub registry: Vec<RegistryPackage>,
}

/// A registry package published outside the release model (spec §5 `[registry]`).
#[derive(Debug, Clone, PartialEq)]
pub struct RegistryPackage {
    /// Name, kind and `hoist.toml:registry.<kind>.<name>` as its version source.
    pub pkg: Package,
    /// The workflow file its trusted publisher names.
    pub workflow: String,
}

impl DesiredRelease {
    /// The workflow file trusted publishers name for model packages.
    pub fn workflow_file(&self) -> &str {
        self.workflow_name
            .as_deref()
            .or(self.workflow.as_ref().map(|w| w.file.as_str()))
            .unwrap_or("release.yml")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Desired {
    pub repo: GithubRepo,
    pub default_branch: String,
    pub github: DesiredGithub,
    pub cloudflare: DesiredCloudflare,
    pub packages: Packages,
    pub release: Option<DesiredRelease>,
    /// `apply --adopt`: never remove or loosen; a ruleset update that would drop live parts
    /// hoist does not model is drift.
    pub adopt: bool,
}

/// Each referenced secret's job environments and environment-less jobs.
fn secret_homes(w: &Workflows) -> Vec<SecretHomes> {
    let mut out: Vec<SecretHomes> = Vec::new();
    for u in &w.secret_uses {
        let i = match out.iter().position(|h| h.name == u.secret) {
            Some(i) => i,
            None => {
                out.push(SecretHomes {
                    name: u.secret.clone(),
                    ..SecretHomes::default()
                });
                out.len() - 1
            }
        };
        let h = &mut out[i];
        match &u.environment {
            Some(env) if !h.envs.contains(env) => h.envs.push(env.clone()),
            Some(_) => {}
            None => h.envless.push(u.job.clone()),
        }
    }
    out
}

fn env_settings(name: &str, c: &EnvCfg) -> EnvSettings {
    EnvSettings {
        branches: c.branches.clone(),
        tags: c.tags.clone(),
        manage: c.manage,
        source: format!("hoist.toml:github.environments.{name}"),
    }
}

/// The workflow-referenced environments, then the ones only `hoist.toml` lists.
fn environments(w: &Workflows, cfg: &BTreeMap<String, EnvCfg>) -> Vec<Sourced<String>> {
    let mut out = w.environments.clone();
    for name in cfg.keys() {
        if !out.iter().any(|e| &e.value == name) {
            out.push(Sourced {
                value: name.clone(),
                source: format!("hoist.toml:github.environments.{name}"),
            });
        }
    }
    out
}

/// The variables the workflows read, then the ones only `hoist.toml` lists.
/// A name in `[github] optional_variables` is listed too: a job `if:` reads it without `${{ }}`,
/// which the scan does not see.
fn variables(
    w: &Workflows,
    cfg: &BTreeMap<String, String>,
    optional: &[String],
) -> Vec<Sourced<String>> {
    let mut out = w.variables.clone();
    for name in optional {
        let upper = name.to_ascii_uppercase();
        if !out.iter().any(|v| v.value == upper) {
            out.push(Sourced {
                value: upper,
                source: "hoist.toml:github.optional_variables".to_string(),
            });
        }
    }
    for name in cfg.keys() {
        let upper = name.to_ascii_uppercase();
        // A name a workflow reads keeps the workflow line as its source.
        if !out.iter().any(|v| v.value == upper) {
            out.push(Sourced {
                value: upper,
                source: format!("hoist.toml:github.variables.{name}"),
            });
        }
    }
    out
}

/// The apps that must be installed: Renovate when `renovate.json` exists, then `[github] apps`.
fn apps(renovate: bool, configured: &[String]) -> Vec<Sourced<String>> {
    let mut out = Vec::new();
    if renovate {
        out.push(Sourced {
            value: "renovate".to_string(),
            source: "renovate.json:1".to_string(),
        });
    }
    for slug in configured {
        if !out.iter().any(|a| &a.value == slug) {
            out.push(Sourced {
                value: slug.clone(),
                source: "hoist.toml:github.apps".to_string(),
            });
        }
    }
    out
}

/// `[deploy] environment` of a Cloudflare target under `secret_scope = "environment"`: the
/// secrets hoist writes for the deploy (`CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`) belong
/// to that environment, even when no workflow job reads them yet; the jobs that do read them
/// add theirs.
fn deploy_environments(
    g: &mut DesiredGithub,
    root: &Path,
    targets: &[Target],
) -> Result<(), HoistError> {
    for t in targets {
        let Some(env) = t.config.deploy.as_ref().and_then(|d| d.environment.clone()) else {
            continue;
        };
        let rel = rel_dir(root, &t.dir, &t.name);
        if !g.env_scope {
            return Err(HoistError::Config {
                file: format!("{rel}/hoist.toml"),
                message: "`deploy.environment` needs `[github] secret_scope = \"environment\"` in \
                          the root hoist.toml; set it, or remove `deploy.environment`"
                    .to_string(),
            });
        }
        let source = format!("{rel}/hoist.toml:deploy.environment");
        if !g.environments.iter().any(|e| e.value == env) {
            g.environments.push(Sourced {
                value: env.clone(),
                source: source.clone(),
            });
        }
        for name in [DEPLOY_TOKEN, ACCOUNT_ENV] {
            if !g.secrets.iter().any(|s| s.value == name) {
                g.secrets.push(Sourced {
                    value: name.to_string(),
                    source: source.clone(),
                });
            }
            let i = match g.secret_homes.iter().position(|h| h.name == name) {
                Some(i) => i,
                None => {
                    g.secret_homes.push(SecretHomes {
                        name: name.to_string(),
                        ..SecretHomes::default()
                    });
                    g.secret_homes.len() - 1
                }
            };
            if !g.secret_homes[i].envs.contains(&env) {
                g.secret_homes[i].envs.push(env.clone());
            }
        }
    }
    Ok(())
}

/// Each referenced variable's job environments and environment-less jobs.
fn variable_homes(w: &Workflows) -> Vec<SecretHomes> {
    let mut out: Vec<SecretHomes> = Vec::new();
    for u in &w.variable_uses {
        let i = match out.iter().position(|h| h.name == u.name) {
            Some(i) => i,
            None => {
                out.push(SecretHomes {
                    name: u.name.clone(),
                    ..SecretHomes::default()
                });
                out.len() - 1
            }
        };
        let h = &mut out[i];
        match &u.environment {
            Some(env) if !h.envs.contains(env) => h.envs.push(env.clone()),
            Some(_) => {}
            None => h.envless.push(u.job.clone()),
        }
    }
    out
}

/// Top-level `hoist.toml` table, to learn which keys the user set (vs defaults).
fn raw_root(dir: &Path) -> toml::Table {
    std::fs::read_to_string(dir.join("hoist.toml"))
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or_default()
}

fn is_set(raw: &toml::Table, section: &str, key: &str) -> bool {
    raw.get(section).and_then(|s| s.get(key)).is_some()
}

fn pick<T>(raw: &toml::Table, section: &str, key: &str, value: T) -> Sourced<T> {
    let source = if is_set(raw, section, key) {
        format!("hoist.toml:{section}.{key}")
    } else {
        "default".to_string()
    };
    Sourced { value, source }
}

fn rel_dir(root: &Path, dir: &Path, fallback: &str) -> String {
    dir.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| fallback.to_string())
}

/// Ruling R9: the `hoist.toml` key when set, else on iff a workflow has a `merge_group`
/// trigger (source: that workflow), else off (`default`).
fn merge_queue(raw: &toml::Table, configured: bool, workflows: &Workflows) -> Sourced<bool> {
    if is_set(raw, "github", "merge_queue") {
        return pick(raw, "github", "merge_queue", configured);
    }
    Sourced {
        value: workflows.merge_group,
        source: workflows
            .merge_group_source
            .clone()
            .unwrap_or_else(|| "default".to_string()),
    }
}

fn desired_release(
    dir: &Path,
    raw: &toml::Table,
    workflows: &Workflows,
) -> Result<Option<DesiredRelease>, HoistError> {
    let model = read_release_model(dir)?;
    let publishes = |p: &Package| {
        p.publish.npm.is_some() || p.publish.crates.is_some() || p.publish.github_release.is_some()
    };
    let root = load_root(dir)?;
    let has_registry = !root.registry.npm.is_empty() || !root.registry.crates.is_empty();
    if !model.packages.iter().any(publishes) && !has_registry {
        return Ok(None);
    }
    let environment = pick(raw, "release", "environment", model.environment);
    let workflow_name = root.release.workflow.clone();
    let workflow = workflows
        .job_environments
        .iter()
        .find(|j| {
            j.environment == environment.value
                && workflow_name.as_ref().is_none_or(|n| &j.file == n)
        })
        .cloned();
    let reviewers = raw
        .get("release")
        .and_then(|r| r.get("reviewers"))
        .and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        });
    let registry = [
        (PkgKind::Npm, "npm", &root.registry.npm),
        (PkgKind::Crate, "crates", &root.registry.crates),
    ]
    .into_iter()
    .flat_map(|(kind, section, map)| {
        map.iter().map(move |(name, cfg)| RegistryPackage {
            pkg: Package {
                name: name.clone(),
                kind,
                dir: PathBuf::new(),
                version: Sourced {
                    value: String::new(),
                    source: format!("hoist.toml:registry.{section}.\"{name}\""),
                },
                deps: Vec::new(),
                edges: Vec::new(),
                publish: PublishCfg::default(),
                launcher: None,
            },
            workflow: cfg.workflow.clone(),
        })
    })
    .collect();
    Ok(Some(DesiredRelease {
        environment,
        reviewers,
        workflow_name,
        workflow,
        packages: model.packages.into_iter().filter(publishes).collect(),
        registry,
    }))
}

/// The release part of the desired model alone (no GitHub or Cloudflare inference), for
/// `release bootstrap`: `None` when no package publishes.
pub fn read_desired_release(dir: &Path) -> Result<Option<DesiredRelease>, HoistError> {
    desired_release(dir, &raw_root(dir), &read_workflows(dir)?)
}

impl Desired {
    /// Read the release model into `release` (R18: only `plan`, `check` and `apply` do, and
    /// `build_desired` never does). The release environment joins the workflow-inferred
    /// environments once. On an error `self` is unchanged.
    pub fn read_release(&mut self, dir: &Path) -> Result<(), HoistError> {
        let workflows = read_workflows(dir)?;
        let release = desired_release(dir, &raw_root(dir), &workflows)?;
        if let Some(r) = &release
            && !self
                .github
                .environments
                .iter()
                .any(|e| e.value == r.environment.value)
        {
            self.github.environments.push(r.environment.clone());
        }
        self.release = release;
        Ok(())
    }

    /// The deployment branch policies wanted for environment `name`: its `branches` in
    /// hoist.toml, else the default branch. Empty means no branch restriction.
    pub fn env_branches(&self, name: &str) -> Vec<String> {
        match self
            .github
            .env_settings
            .get(name)
            .and_then(|e| e.branches.clone())
        {
            Some(b) => b,
            None => vec![self.default_branch.clone()],
        }
    }

    /// The deployment tag policies wanted for environment `name` (`tags` in hoist.toml).
    pub fn env_tags(&self, name: &str) -> Vec<String> {
        self.github
            .env_settings
            .get(name)
            .map(|e| e.tags.clone())
            .unwrap_or_default()
    }

    /// Whether hoist manages environment `name` (`manage = false` leaves it alone). The
    /// release environment is never excluded here: the release area owns it.
    pub fn env_managed(&self, name: &str) -> bool {
        self.release_environment() == Some(name)
            || self.github.env_settings.get(name).is_none_or(|e| e.manage)
    }

    fn homes_of(&self, name: &str) -> Option<&SecretHomes> {
        match self.github.env_scope {
            true => self.github.secret_homes.iter().find(|h| h.name == name),
            false => None,
        }
    }

    /// Under `secret_scope = "environment"`, the environments that must hold secret `name`
    /// (empty in repository scope).
    pub fn homes(&self, name: &str) -> &[String] {
        self.homes_of(name).map_or(&[], |h| h.envs.as_slice())
    }

    /// The jobs that read `name` without an environment, which an environment secret cannot
    /// reach (empty in repository scope).
    pub fn envless(&self, name: &str) -> &[String] {
        self.homes_of(name).map_or(&[], |h| h.envless.as_slice())
    }

    /// Whether the release environment holds one of the secrets (it is then made with the
    /// github area too).
    pub fn release_holds_secrets(&self) -> bool {
        let release = self.release_environment();
        self.github
            .secret_homes
            .iter()
            .any(|h| self.github.env_scope && h.envs.iter().any(|e| Some(e.as_str()) == release))
    }

    /// The release environment's name, when publishing needs one.
    pub fn release_environment(&self) -> Option<&str> {
        self.release.as_ref().map(|r| r.environment.value.as_str())
    }

    /// The Cloudflare account id, required only when the Cloudflare area is read or explained
    /// and deploy targets exist (`plan --area github` and `auth check` do not need it).
    pub fn require_account_id(&self) -> Result<(), HoistError> {
        let cf = &self.cloudflare;
        if cf.account_id.is_none()
            && !(cf.pages.is_empty() && cf.workers.is_empty() && cf.r2.is_none())
        {
            return Err(HoistError::Config {
                file: "hoist.toml".to_string(),
                message: format!(
                    "Cloudflare resources found but no account id; set `[cloudflare] \
                     account_id` in hoist.toml or export {ACCOUNT_ENV}"
                ),
            });
        }
        Ok(())
    }
}

pub fn build_desired(dir: &Path) -> Result<Desired, HoistError> {
    let root = load_root(dir)?;
    let raw = raw_root(dir);
    let (repo, default_branch) = read_repo(dir)?;
    let workflows = read_workflows(dir)?;
    let (targets, packages, renovate) = read_targets(dir)?;

    let required_checks: Vec<Sourced<String>> = match &root.github.required_checks {
        Some(list) => list
            .iter()
            .map(|c| Sourced {
                value: c.context.clone(),
                source: "hoist.toml:github.required_checks".to_string(),
            })
            .collect(),
        None => workflows
            .jobs
            .iter()
            .filter(|j| j.value == "ci-ok")
            .cloned()
            .collect(),
    };

    let check_pins: Vec<CheckPin> = root
        .github
        .required_checks
        .iter()
        .flatten()
        .filter_map(|c| {
            c.app.clone().map(|app| CheckPin {
                context: c.context.clone(),
                id: match &app {
                    crate::config::CheckApp::Id(id) => Some(*id),
                    crate::config::CheckApp::Slug(_) => None,
                },
                app,
            })
        })
        .collect();

    let mut github = DesiredGithub {
        settings: DesiredSettings {
            visibility: root.github.visibility.clone().map(|value| Sourced {
                value,
                source: "hoist.toml:github.visibility".to_string(),
            }),
            allow_auto_merge: pick(
                &raw,
                "github",
                "allow_auto_merge",
                root.github.allow_auto_merge,
            ),
            delete_branch_on_merge: pick(
                &raw,
                "github",
                "delete_branch_on_merge",
                root.github.delete_branch_on_merge,
            ),
            immutable_releases: root.github.immutable_releases.map(|value| Sourced {
                value,
                source: "hoist.toml:github.immutable_releases".to_string(),
            }),
        },
        ruleset: DesiredRuleset {
            name: RULESET_NAME.to_string(),
            branch: default_branch.clone(),
            required_checks,
            check_pins,
            merge_queue: merge_queue(&raw, root.github.merge_queue, &workflows),
            admin_bypass: root.github.admin_bypass,
        },
        tag_ruleset: root.github.tag_ruleset.as_ref().map(|t| DesiredTagRuleset {
            name: TAG_RULESET_NAME.to_string(),
            patterns: t.patterns.clone(),
            source: "hoist.toml:github.tag_ruleset".to_string(),
            bypass: root.github.tags_bypass,
            key_env: root.release.environment.clone(),
        }),
        actions_permissions: pick(
            &raw,
            "github",
            "actions_permissions",
            root.github.actions_permissions.clone(),
        ),
        environments: environments(&workflows, &root.github.environments),
        env_settings: root
            .github
            .environments
            .iter()
            .map(|(name, c)| (name.clone(), env_settings(name, c)))
            .collect(),
        unresolved_environments: workflows.unresolved_environments.clone(),
        secrets: workflows.secrets.clone(),
        variables: variables(
            &workflows,
            &root.github.variables,
            &root.github.optional_variables,
        ),
        variable_homes: variable_homes(&workflows),
        variable_values: root
            .github
            .variables
            .iter()
            .map(|(k, v)| {
                (
                    k.to_ascii_uppercase(),
                    Sourced {
                        value: v.clone(),
                        source: format!("hoist.toml:github.variables.{k}"),
                    },
                )
            })
            .collect(),
        optional_variables: root
            .github
            .optional_variables
            .iter()
            .map(|s| s.to_ascii_uppercase())
            .collect(),
        optional_secrets: root
            .github
            .optional_secrets
            .iter()
            .map(|s| s.to_ascii_uppercase())
            .collect(),
        statuses: root.github.statuses.clone(),
        apps: apps(renovate, &root.github.apps),
        managed: root.github.managed,
        env_scope: root.github.secret_scope == SecretScope::Environment,
        secret_homes: secret_homes(&workflows),
        opaque_inherits: workflows.opaque_inherits.clone(),
        unresolved_secrets: workflows.unresolved_secrets.clone(),
    };

    let mut pages = Vec::new();
    let mut workers = Vec::new();
    for target in &targets {
        match target.kind.value {
            TargetKind::CloudflarePages => {
                let rel = rel_dir(dir, &target.dir, &target.name);
                let deploy = target.config.deploy.as_ref();
                let project = match deploy.and_then(|d| d.project.clone()) {
                    Some(value) => Sourced {
                        value,
                        source: format!("{rel}/hoist.toml:deploy.project"),
                    },
                    None => Sourced {
                        value: format!("{}-{}", repo.name, target.name),
                        source: "default".to_string(),
                    },
                };
                let domain = deploy.and_then(|d| d.domain.clone()).map(|value| Sourced {
                    value,
                    source: format!("{rel}/hoist.toml:deploy.domain"),
                });
                pages.push(DesiredPages {
                    target: target.name.clone(),
                    dir: rel.clone(),
                    project,
                    domain,
                });
            }
            TargetKind::CloudflareWorkers => workers.push(Sourced {
                value: target.name.clone(),
                source: target.kind.source.clone(),
            }),
        }
    }

    deploy_environments(&mut github, dir, &targets)?;

    let account_id = match &root.cloudflare.account_id {
        Some(value) => Some(Sourced {
            value: value.clone(),
            source: "hoist.toml:cloudflare.account_id".to_string(),
        }),
        None => std::env::var(ACCOUNT_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(|value| Sourced {
                value,
                source: format!("env {ACCOUNT_ENV}"),
            }),
    };
    if let Some(dns) = &root.cloudflare.dns {
        crate::dns::validate(dns).map_err(|message| HoistError::Config {
            file: "hoist.toml".to_string(),
            message,
        })?;
    }
    Ok(Desired {
        repo,
        default_branch,
        github,
        cloudflare: DesiredCloudflare {
            account_id,
            pages,
            workers,
            managed: root.cloudflare.managed,
            dns: root.cloudflare.dns.clone(),
            r2: crate::r2::desired_from_cfg(
                &dir.join("hoist.toml").display().to_string(),
                &root.cloudflare.r2,
            )?,
        },
        packages,
        release: None,
        adopt: false,
    })
}
