//! `hoist release publish`: tag each pending package, publish its GitHub Release, then publish
//! it to npm / crates.io and run its `on = "release"` deploys (spec §9.3–9.5). Exit 3 only
//! before the first write; a failed write or verification is exit 1; human steps alone exit 2.

use crate::auth::Secret;
use crate::auth::github_token;
use crate::error::HoistError;
use crate::http::{Http, Service};
use crate::ops::release::default_branch;
use crate::release::edit::changelog_section;
use crate::release::github_rel::{LocalAsset, local_assets};
use crate::release::gitlog::require_full_history;
use crate::release::launcher::verify_optional_deps;
use crate::release::oidc::require_oidc_env;
use crate::release::planner::order;
use crate::release::tools::{Tool, run};
use crate::release::{Package, PkgKind, ReleaseModel, exe_suffix, read_release_model};
use crate::repo::git::read_repo_local;
use crate::repo::targets::{Target, join_rel, read_targets};
use crate::report::{Action, HumanStep, Item, Report};
use std::path::{Path, PathBuf};

mod execute;
mod github;
mod preflight;
mod registries;
use execute::{dry_run, execute};
use preflight::{preflight, require_actions};

const COMMAND: &str = "release publish";

/// One pending package: its tag, release body, asset files and whether it deploys.
pub(super) struct Job<'a> {
    pkg: &'a Package,
    tag: String,
    body: String,
    assets: Vec<LocalAsset>,
    /// `--dry-run` without `--assets` (release.yml `detect`, before any build): the number of
    /// binaries the release will carry; they are neither read nor checked.
    unbuilt: usize,
    /// A target in the package's directory has `[deploy] on = "release"`.
    deploy: bool,
    /// A launcher's binaries, one per platform package (same order).
    bins: Vec<PathBuf>,
    /// The package's `build` command already ran in this run (once per job).
    built: std::cell::Cell<bool>,
    /// `--only`: registry-only mode (no tag, release or deploy; just the named registries).
    only: Option<Only>,
}

/// The registries `--only` selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Only {
    crates: bool,
    npm: bool,
}

impl Only {
    fn parse(list: &[String]) -> Result<Option<Only>, HoistError> {
        if list.is_empty() {
            return Ok(None);
        }
        let mut only = Only {
            crates: false,
            npm: false,
        };
        for w in list {
            match w.as_str() {
                "crates" => only.crates = true,
                "npm" => only.npm = true,
                other => {
                    return Err(HoistError::Usage(format!(
                        "--only `{other}`: use `crates`, `npm` or `crates,npm`"
                    )));
                }
            }
        }
        Ok(Some(only))
    }

    fn keeps(self, step: Step) -> bool {
        match step {
            Step::Tag | Step::Release | Step::Deploy => false,
            Step::Npm | Step::Platform(_) => self.npm,
            Step::Crates => self.crates,
        }
    }
}

/// What `release publish` was asked for.
pub struct PublishOpts<'a> {
    pub assets: Option<&'a Path>,
    pub dry_run: bool,
    pub only: &'a [String],
    pub npm_dirs: &'a [PathBuf],
    pub npm_tag: Option<&'a str>,
    pub provenance: bool,
}

/// How npm publishes: dist-tag, provenance (and the repository it names).
pub(super) struct NpmFlags {
    tag: Option<String>,
    provenance: bool,
    repository: String,
}

impl Job<'_> {
    /// The steps of this package, in order (spec §9.3).
    fn steps(&self) -> Vec<Step> {
        let all = self.all_steps();
        match self.only {
            Some(o) => all.into_iter().filter(|s| o.keeps(*s)).collect(),
            None => all,
        }
    }

    fn all_steps(&self) -> Vec<Step> {
        if let Some(l) = &self.pkg.launcher {
            // Platform packages first, in target order, then the launcher.
            let mut steps: Vec<Step> = (0..l.platforms.len()).map(Step::Platform).collect();
            steps.push(Step::Npm);
            return steps;
        }
        let mut steps = vec![Step::Tag, Step::Release];
        if self.pkg.publish.npm.is_some() {
            steps.push(Step::Npm);
        }
        if self.pkg.publish.crates.is_some() {
            steps.push(Step::Crates);
        }
        if self.deploy {
            steps.push(Step::Deploy);
        }
        steps
    }

    /// The package a step publishes: a platform package, or the job's own.
    fn step_pkg(&self, step: Step) -> &Package {
        match (step, &self.pkg.launcher) {
            (Step::Platform(i), Some(l)) => &l.platforms[i].pkg,
            _ => self.pkg,
        }
    }

    fn publishes(&self) -> bool {
        self.pkg.publish.npm.is_some() || self.pkg.publish.crates.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Tag,
    Release,
    Npm,
    Crates,
    Deploy,
    /// Copy the binary into a launcher's platform package (by index) and publish it to npm.
    Platform(usize),
}

impl Step {
    fn kind(self) -> &'static str {
        match self {
            Step::Tag => "tag",
            Step::Release => "github_release",
            Step::Npm | Step::Platform(_) => "npm",
            Step::Crates => "crates",
            Step::Deploy => "deploy",
        }
    }

    fn github(self) -> bool {
        matches!(self, Step::Tag | Step::Release)
    }
}

/// The API clients of one run; registry reads are anonymous.
pub(super) struct Clients {
    gh: Http,
    up: Http,
    npm: Http,
    crates: Http,
    flags: NpmFlags,
}

fn git(dir: &Path, args: &[&str], verbose: bool) -> Result<String, HoistError> {
    let out = run(Tool::Git, dir, args, &[])?;
    if verbose {
        eprint!("{out}");
    }
    Ok(out)
}

/// Packages whose manifest version has no tag in `tags`, in dependency order. Launchers have
/// no tag; they ride on their crate's job (see [`launcher_jobs`]).
fn pending<'a>(m: &'a ReleaseModel, tags: &[String]) -> Vec<(&'a Package, String)> {
    order(m)
        .into_iter()
        .filter(|p| p.launcher.is_none())
        .filter_map(|p| {
            let tag = m.tag_for(p, &p.version.value);
            (!tags.contains(&tag)).then_some((p, tag))
        })
        .collect()
}

/// Local tags that mark a finished release: every tag except those on HEAD. A tag on HEAD is
/// this release's own (a re-run), so its package stays pending and every step re-checks.
fn released_tags(dir: &Path, verbose: bool) -> Result<Vec<String>, HoistError> {
    let at_head = git(dir, &["tag", "--points-at", "HEAD"], verbose)?;
    let at_head: Vec<&str> = at_head.lines().map(str::trim).collect();
    Ok(git(dir, &["tag", "--list"], verbose)?
        .lines()
        .map(str::trim)
        .filter(|t| !t.is_empty() && !at_head.contains(t))
        .map(str::to_string)
        .collect())
}

/// `<assets>/<bin>-<target>` (`.exe` for Windows targets) of `owner`. A missing file is exit 3.
fn asset_path(
    owner: &str,
    assets: Option<&Path>,
    bin: &str,
    target: &str,
) -> Result<PathBuf, HoistError> {
    let name = format!("{bin}-{target}{}", exe_suffix(target));
    let Some(dir) = assets else {
        return Err(HoistError::Usage(format!(
            "`{owner}` releases binaries ({name}, …): pass `--assets DIR` with the built files"
        )));
    };
    let path = dir.join(&name);
    if !path.is_file() {
        return Err(HoistError::Usage(format!(
            "asset {name} for `{owner}` is missing in {}; download the build artifacts there \
             first",
            dir.display()
        )));
    }
    Ok(path)
}

/// The asset file per `github_release` target, sorted by file name.
fn asset_files(p: &Package, assets: Option<&Path>) -> Result<Vec<PathBuf>, HoistError> {
    let Some(gr) = &p.publish.github_release else {
        return Ok(Vec::new());
    };
    let bin = gr.bin.as_deref().unwrap_or(&p.name);
    let mut files = gr
        .targets
        .iter()
        .map(|t| asset_path(&p.name, assets, bin, t))
        .collect::<Result<Vec<_>, _>>()?;
    files.sort_by_key(|f| f.file_name().map(|n| n.to_os_string()));
    Ok(files)
}

/// The jobs of the launchers that ride on crate `krate`'s release: each must already carry
/// the crate's version (as must its platform packages), else exit 3 before any write.
fn launcher_jobs<'a>(
    dir: &Path,
    m: &'a ReleaseModel,
    krate: &Package,
    assets: Assets,
) -> Result<Vec<Job<'a>>, HoistError> {
    let mut jobs = Vec::new();
    for pkg in &m.packages {
        let Some(l) = pkg.launcher.as_ref().filter(|l| l.crate_name == krate.name) else {
            continue;
        };
        let want = &krate.version.value;
        let behind = std::iter::once(pkg)
            .chain(l.platforms.iter().map(|p| &p.pkg))
            .find(|p| &p.version.value != want);
        if let Some(b) = behind {
            return Err(HoistError::Usage(format!(
                "`{}` is at {} ({}), but `{}` releases {want}; an npm launcher and its platform \
                 packages share the crate's version: run `hoist release prepare` and merge its PR",
                b.name, b.version.value, b.version.source, krate.name
            )));
        }
        let file = join_rel(&pkg.dir.to_string_lossy(), "package.json");
        let text = std::fs::read_to_string(dir.join(&file))
            .map_err(|e| HoistError::Usage(format!("cannot read {file}: {e}")))?;
        verify_optional_deps(&file, &text, pkg, want)?;
        let bins = match assets {
            Assets::Unbuilt => Vec::new(),
            Assets::Dir(assets) => l
                .platforms
                .iter()
                .map(|p| asset_path(&pkg.name, assets, &l.bin, &p.target))
                .collect::<Result<Vec<_>, _>>()?,
        };
        jobs.push(Job {
            pkg,
            tag: format!("{}@{}", pkg.name, pkg.version.value),
            body: String::new(),
            assets: Vec::new(),
            unbuilt: 0,
            deploy: false,
            bins,
            built: std::cell::Cell::new(false),
            only: None,
        });
    }
    Ok(jobs)
}

fn release_body(dir: &Path, p: &Package, tag: &str) -> String {
    std::fs::read_to_string(dir.join(&p.dir).join("CHANGELOG.md"))
        .ok()
        .and_then(|text| changelog_section(&text, &p.version.value))
        .unwrap_or_else(|| format!("Release {tag}."))
}

pub(super) fn item(job: &Job, step: Step, action: Action, reason: String) -> Item {
    let mut it = item_of(job, step.kind(), action, reason);
    if job.only.is_some() || matches!(step, Step::Platform(_)) {
        let p = job.step_pkg(step);
        it.key = format!("{}@{}", p.name, p.version.value);
        it.source = p.version.source.clone();
    }
    it
}

fn item_of(job: &Job, kind: &str, action: Action, reason: String) -> Item {
    Item {
        area: "release".to_string(),
        kind: kind.to_string(),
        key: job.tag.clone(),
        action,
        reason,
        source: job.pkg.version.source.clone(),
        verify: None,
    }
}

pub(super) fn report(items: Vec<Item>, human_steps: Vec<HumanStep>, exit_code: i32) -> Report {
    Report {
        command: COMMAND.to_string(),
        items,
        human_steps,
        exit_code,
        text: None,
        hint: None,
    }
}

/// Whether a target with `[deploy] on = "release"` lives in the package's directory.
fn deploys_on_release(dir: &Path, targets: &[Target], p: &Package) -> bool {
    let pkg_dir = dir.join(&p.dir);
    targets
        .iter()
        .any(|t| t.config.deploy.as_ref().is_some_and(|d| d.on == "release") && t.dir == pkg_dir)
}

/// Where the binaries come from: `--assets DIR` (`None`: not given, an error once one is
/// needed), or not built yet (`--dry-run` without `--assets`).
#[derive(Clone, Copy)]
enum Assets<'a> {
    Dir(Option<&'a Path>),
    Unbuilt,
}

/// A package built from a pre-staged npm directory (`--npm-dir`): name and version from its
/// package.json, and its `optionalDependencies` (name, version) for [`staged_packages`].
fn staged_package(dir: &Path) -> Result<(Package, Vec<(String, String)>), HoistError> {
    let dir = &std::fs::canonicalize(dir)
        .map_err(|e| HoistError::Usage(format!("--npm-dir: cannot read {}: {e}", dir.display())))?;
    let file = dir.join("package.json");
    let text = std::fs::read_to_string(&file).map_err(|e| {
        HoistError::Usage(format!("--npm-dir: cannot read {}: {e}", file.display()))
    })?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        HoistError::Usage(format!("--npm-dir: {} is not JSON: {e}", file.display()))
    })?;
    let field = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .filter(|x| !x.is_empty())
            .map(str::to_string)
            .ok_or_else(|| HoistError::Usage(format!("--npm-dir: {} has no `{k}`", file.display())))
    };
    let (name, version) = (field("name")?, field("version")?);
    let optional: Vec<(String, String)> = v
        .get("optionalDependencies")
        .and_then(|o| o.as_object())
        .map(|o| {
            o.iter()
                .map(|(k, x)| (k.clone(), x.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default();
    let pkg = Package {
        name,
        kind: PkgKind::Npm,
        dir: dir.to_path_buf(),
        version: crate::repo::Sourced {
            value: version,
            source: file.display().to_string(),
        },
        deps: Vec::new(),
        edges: Vec::new(),
        publish: crate::config::PublishCfg {
            npm: Some(crate::config::NpmPublish::default()),
            ..Default::default()
        },
        launcher: None,
    };
    Ok((pkg, optional))
}

/// The `--npm-dir` packages in publish order, whatever order the flags came in: a package's
/// `optionalDependencies` that are other staged packages are its `deps` (a launcher waits for
/// a platform package that is not on npm yet) and are published before it. A staged
/// dependency pinned to another version than the one staged is a usage error.
fn staged_packages(dirs: &[PathBuf], base: &Path) -> Result<Vec<Package>, HoistError> {
    let mut all = Vec::new();
    for d in dirs {
        all.push(staged_package(&base.join(d))?);
    }
    let versions: Vec<(String, String)> = all
        .iter()
        .map(|(p, _)| (p.name.clone(), p.version.value.clone()))
        .collect();
    for (p, optional) in &mut all {
        for (name, want) in optional.iter() {
            let Some((_, have)) = versions.iter().find(|(n, _)| n == name) else {
                continue;
            };
            if want != have {
                return Err(HoistError::Usage(format!(
                    "--npm-dir: {} requires {name}@{want} but the staged {name} is {have}",
                    p.name
                )));
            }
            p.deps.push(name.clone());
        }
        p.deps.sort();
    }
    let mut left: Vec<Package> = all.into_iter().map(|(p, _)| p).collect();
    let mut out: Vec<Package> = Vec::new();
    while !left.is_empty() {
        let at = left
            .iter()
            .position(|p| p.deps.iter().all(|d| out.iter().any(|o| &o.name == d)))
            .ok_or_else(|| {
                HoistError::Usage(
                    "--npm-dir: the staged packages depend on each other in a cycle".to_string(),
                )
            })?;
        out.push(left.remove(at));
    }
    Ok(out)
}

/// Tag and release every package whose manifest version is not yet released; with `--only`,
/// publish every package to the named registries instead (idempotent, no tags).
pub fn release_publish(dir: &Path, opts: PublishOpts, verbose: bool) -> Result<Report, HoistError> {
    let PublishOpts {
        assets,
        dry_run: dry_run_only,
        ..
    } = opts;
    let only = Only::parse(opts.only)?;
    if !opts.npm_dirs.is_empty() && !only.is_some_and(|o| o.npm) {
        return Err(HoistError::Usage(
            "--npm-dir needs `--only npm`".to_string(),
        ));
    }
    if only.is_none() && (opts.npm_tag.is_some() || opts.provenance) {
        return Err(HoistError::Usage(
            "--npm-tag and --provenance apply to registry publishing: add `--only npm` (or \
             `--only crates,npm`)"
                .to_string(),
        ));
    }
    if !dry_run_only {
        require_actions()?;
    }
    let (repo, local) = read_repo_local(dir)?;
    let model = read_release_model(dir)?;
    let head = git(dir, &["rev-parse", "HEAD"], verbose)?
        .trim()
        .to_string();
    let token = github_token()?;
    let gh = Http::new(Service::Github, token.clone()).verbose(verbose);
    if !dry_run_only {
        let default = default_branch(&gh, &repo, local)?;
        // Registry-only runs publish the checked-out release tag, not GITHUB_SHA.
        preflight(dir, &default, &model, &head, only.is_none())?;
    }
    let todo: Vec<(&Package, String)> = if only.is_some() {
        order(&model)
            .into_iter()
            .filter(|p| p.launcher.is_none())
            .map(|p| (p, model.tag_for(p, &p.version.value)))
            .collect()
    } else {
        require_full_history(dir)?;
        let todo = pending(&model, &released_tags(dir, verbose)?);
        if todo.is_empty() {
            let it = Item {
                area: "release".to_string(),
                kind: "publish".to_string(),
                key: "pending".to_string(),
                action: Action::Ok,
                reason: "every package version is already tagged".to_string(),
                source: "default".to_string(),
                verify: None,
            };
            return Ok(report(vec![it], vec![], 0));
        }
        todo
    };
    let (targets, _, _) = read_targets(dir)?;
    // A registry-only run that skips npm needs no binaries; one that publishes npm needs them
    // (`--assets`) unless it is a dry run.
    let assets = match (assets, dry_run_only, only) {
        (_, _, Some(o)) if !o.npm => Assets::Unbuilt,
        (None, true, _) => Assets::Unbuilt,
        (a, _, _) => Assets::Dir(a),
    };
    let staged = staged_packages(opts.npm_dirs, dir)?;
    let mut jobs = Vec::new();
    for (pkg, tag) in todo {
        let (files, unbuilt) = match (assets, only) {
            (_, Some(_)) => (Vec::new(), 0),
            (Assets::Unbuilt, None) => (
                Vec::new(),
                pkg.publish
                    .github_release
                    .as_ref()
                    .map_or(0, |g| g.targets.len()),
            ),
            (Assets::Dir(a), None) => (asset_files(pkg, a)?, 0),
        };
        jobs.push(Job {
            assets: local_assets(&files)?,
            unbuilt,
            body: if only.is_some() {
                String::new()
            } else {
                release_body(dir, pkg, &tag)
            },
            deploy: deploys_on_release(dir, &targets, pkg),
            pkg,
            tag,
            bins: Vec::new(),
            built: std::cell::Cell::new(false),
            only: None,
        });
        jobs.extend(launcher_jobs(dir, &model, pkg, assets)?);
    }
    for pkg in &staged {
        jobs.push(Job {
            pkg,
            tag: format!("{}@{}", pkg.name, pkg.version.value),
            body: String::new(),
            assets: Vec::new(),
            unbuilt: 0,
            deploy: false,
            bins: Vec::new(),
            built: std::cell::Cell::new(false),
            only: None,
        });
    }
    for j in &mut jobs {
        j.only = only;
    }
    jobs.retain(|j| !j.steps().is_empty());
    if !dry_run_only && jobs.iter().any(Job::publishes) {
        require_oidc_env()?;
    }
    let anonymous = || Secret::from("");
    let c = Clients {
        gh,
        up: Http::new(Service::GithubUploads, token).verbose(verbose),
        npm: Http::new(Service::Npm, anonymous()).verbose(verbose),
        crates: Http::new(Service::Crates, anonymous()).verbose(verbose),
        flags: NpmFlags {
            tag: opts.npm_tag.map(str::to_string),
            provenance: opts.provenance,
            repository: repo.to_string(),
        },
    };
    if dry_run_only {
        return dry_run(&c, &repo, &jobs, &head);
    }
    Ok(execute(&c, dir, &repo, &jobs, &head))
}
