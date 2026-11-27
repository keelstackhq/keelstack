//! `splice set [--channel NAME] K=V...` (spec §8.3a): change locked template variables and
//! re-render the owned files they touch at the locked template commit.

use indexmap::IndexMap;
use semver::Version;

use crate::apply::{ApplyCtx, LockOut, apply};
use crate::cli::{GlobalArgs, SetArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::filter::Filter;
use crate::fsio::sha256_hex;
use crate::lock::{ChannelLock, LOCK_PATH, Lock, LockFile};
use crate::manifest::{Template, When};
use crate::output::{Action, Class, Report};
use crate::planner::{Disk, PlanInput, plan};
use crate::render::{RenderInput, RenderedTree, render_tree};
use crate::source::TemplateSource;
use crate::sources::Sources;
use crate::vars::{VarSource, resolve_vars, validate_assignments};

type Result<T> = std::result::Result<T, SpliceError>;

fn parse_assignments(items: &[String]) -> Result<IndexMap<String, String>> {
    let mut out = IndexMap::new();
    for item in items {
        match item.split_once('=') {
            Some((k, v)) if !k.is_empty() => {
                out.insert(k.to_string(), v.to_string());
            }
            _ => {
                return Err(SpliceError::Usage(format!("set expects K=V, got `{item}`")));
            }
        }
    }
    Ok(out)
}

/// The owned files of `tree` whose bytes differ from `other` (or are missing there).
fn touched(tree: &RenderedTree, other: &RenderedTree) -> RenderedTree {
    tree.iter()
        .filter(|(p, f)| {
            f.class == Class::Owned && other.get(*p).is_none_or(|o| o.bytes != f.bytes)
        })
        .map(|(p, f)| (p.clone(), f.clone()))
        .collect()
}

pub fn run(g: &GlobalArgs, a: &SetArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    let assignments = parse_assignments(&a.vars)?;
    if !a.allow_dirty {
        super::update::require_clean(repo, "set")?;
    }
    let sources = Sources::load(repo)?;
    let (lock, channel): (&Lock, Option<&ChannelLock>) = match &a.channel {
        Some(name) => {
            let cl = sources.channel(name)?;
            (&cl.lock, Some(cl))
        }
        None => match &sources.main {
            Some(l) => (l, None),
            None if sources.channels.is_empty() => return Err(super::no_lock(repo)),
            None => {
                return Err(SpliceError::Usage(format!(
                    "no {LOCK_PATH}; use --channel NAME"
                )));
            }
        },
    };

    // The locked commit's template: vars are validated against its definitions.
    let src = TemplateSource::parse_from(&lock.template.source, repo)?;
    let resolved =
        Fetcher::from_env().checkout(&src, &lock.template.commit, lock.template.version.clone())?;
    let template = Template::load(&resolved.root)?;
    let running = Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version");
    template.check_min_splice(&running)?;
    let given = validate_assignments(&template.manifest.vars, &assignments)?;
    let mut known = lock.vars.clone();
    known.extend(given.clone());
    let none = IndexMap::new();
    let vars = resolve_vars(
        &template.manifest.vars,
        &known,
        &VarSource {
            data: &none,
            interactive: false,
        },
    )?;

    let mut report = Report::new("set");
    if vars == lock.vars {
        report.msg("nothing to do: the variables already have these values");
        return Ok(report);
    }

    // Render at the locked commit with the old and the new vars; keep what the var changes.
    let render = |template_vars: &IndexMap<String, String>| -> Result<RenderedTree> {
        let tree = render_tree(
            &template,
            &RenderInput {
                template_vars: template_vars.clone(),
                instances: lock.instances(),
                version: lock.template.version.clone(),
            },
        )?;
        match channel {
            Some(cl) => Ok(Filter::from_meta(&cl.meta, &cl.path())?.apply_tree(tree)),
            None => Ok(tree),
        }
    };
    let (old, new) = (render(&lock.vars)?, render(&vars)?);
    // Only owned paths whose render the var changes (differs, appears or disappears) are planned.
    let (mut base, mut theirs) = (touched(&old, &new), touched(&new, &old));
    let paths: Vec<String> = base.keys().chain(theirs.keys()).cloned().collect();
    for p in paths {
        if let Some(f) = old.get(&p).filter(|f| f.class == Class::Owned) {
            base.insert(p.clone(), f.clone());
        }
        if let Some(f) = new.get(&p).filter(|f| f.class == Class::Owned) {
            theirs.insert(p, f.clone());
        }
    }

    let disk = Disk::snapshot(repo, base.keys().chain(theirs.keys()).map(String::as_str))?;
    let plan = plan(&PlanInput {
        base: &base,
        theirs: &theirs,
        disk: &disk,
        lock,
        removes: Vec::new(),
        force: a.force,
    });
    let owner = match &a.channel {
        Some(n) => format!("channel {n}"),
        None => "the template".to_string(),
    };
    if !plan.drift.is_empty() {
        report.ok = false;
        for path in &plan.drift {
            report.msg(format!(
                "`{path}` is owned by {owner} but was edited locally"
            ));
        }
        report.msg("nothing was written; revert the files above or pass --force to replace them");
        return Err(SpliceError::Drift(report));
    }

    // The new lock: vars and the hashes of what was written; everything else as locked.
    let mut lock_new = lock.clone();
    lock_new.vars = vars;
    for c in &plan.changes {
        match (c.action, &c.bytes) {
            (Action::Delete, _) => {
                lock_new.files.remove(&c.path);
            }
            (Action::Skip, _) | (_, None) => {}
            (_, Some(bytes)) => {
                lock_new.files.insert(
                    c.path.clone(),
                    LockFile {
                        class: Class::Owned,
                        sha256: sha256_hex(bytes),
                        kept: Vec::new(),
                    },
                );
            }
        }
    }
    let lock_out = match channel {
        Some(cl) => LockOut::Channel(ChannelLock {
            meta: cl.meta.clone(),
            lock: lock_new,
            sections: cl.sections.clone(),
        }),
        None => LockOut::Main(lock_new),
    };
    let version = lock.template.version.clone();
    let mut done = apply(ApplyCtx {
        repo,
        plan: &plan,
        steps: Vec::new(),
        from: version.clone(),
        to: version,
        lock: lock_out,
        post: Vec::new(),
        when: When::Update,
        no_post: true,
        command: "set",
        sections: &[],
    })?;
    for (k, v) in &given {
        done.msg(format!("set {k}={v}"));
    }
    Ok(done)
}
