//! The steps of `keel up` (spec §5), in order.
use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::report::HumanStep;
use crate::step::{Check, Step};

pub mod agent_env;
pub mod apps;
pub mod backup;
pub mod deploy;
pub mod dns;
pub mod doctor;
pub mod governance;
pub mod merge;
pub mod preflight;
pub mod render;
pub mod settings;
pub mod token;
pub mod verify;

/// All steps, in order.
pub fn all_steps() -> Vec<Box<dyn Step>> {
    vec![
        Box::new(preflight::Preflight),
        Box::new(apps::Apps),
        Box::new(render::Render),
        Box::new(governance::Governance),
        Box::new(settings::Settings),
        Box::new(token::Token),
        Box::new(agent_env::AgentEnv),
        Box::new(deploy::Deploy),
        Box::new(dns::Dns),
        Box::new(backup::Backup),
        Box::new(doctor::Doctor),
        Box::new(merge::Merge),
        Box::new(verify::Verify),
    ]
}

/// A hoist `--area cloudflare` outcome for the `[SECTION]` a step owns as a step result. `Done` carries `done_msg`.
pub(crate) fn hoist_check(
    res: crate::tools::hoist::Res,
    id: &str,
    only: &str,
    done_msg: &str,
) -> Check {
    use crate::tools::hoist::Res;
    match res {
        Res::Ok => Check::Done(done_msg.into()),
        Res::Drift => Check::Todo(format!("hoist reports drift in [{only}]")),
        Res::Human {
            title,
            url,
            command,
        } => Check::Human(HumanStep {
            id: id.into(),
            title,
            url,
            command,
        }),
        Res::Failed(e) => Check::Failed(e),
        Res::Upgrade => Check::Human(HumanStep {
            id: id.into(),
            title: format!("Upgrade hoist to a release with [{only}]"),
            url: String::new(),
            command: "npm install -g @keelstackhq/hoist@latest".into(),
        }),
    }
}

/// The human step for a Cloudflare step without `CLOUDFLARE_API_TOKEN`.
pub(crate) fn cloudflare_token_step(id: &str, what: &str) -> Check {
    Check::Human(HumanStep {
        id: id.into(),
        title: format!("Set CLOUDFLARE_API_TOKEN or {what}"),
        url: "https://dash.cloudflare.com/profile/api-tokens".into(),
        command: format!(
            "export CLOUDFLARE_API_TOKEN=... then keel up (or, once done by hand: keel up --confirm {id})"
        ),
    })
}

pub(crate) fn repo(cx: &Ctx) -> Result<String, KeelError> {
    cx.manifest.repo.clone().ok_or_else(|| {
        KeelError::Env("no repo: pass --repo OWNER/NAME or set `repo` in keel.toml".into())
    })
}

/// Every line of `hoist.toml` that keel's section generators write for this manifest: the
/// `[cloudflare] account_id` line, the dns section and the r2 section. Blank lines are not
/// listed (see [`only_keel_lines`]).
pub(crate) fn hoist_lines(m: &crate::manifest::Manifest) -> Vec<String> {
    use crate::manifest::{Dns, Provider};
    let mut text = String::new();
    if let Some(b) = &m.backup
        && let Some(id) = &b.account_id
    {
        text.push_str(&format!(
            "[cloudflare]\naccount_id = {}\n",
            crate::tools::hoist::toml_str(id)
        ));
    }
    if let Some(f) = m.fly() {
        if f.dns == Dns::Cloudflare {
            text.push_str(&dns::section_text(f));
        }
        if let Some(b) = &m.backup
            && b.provider == Provider::R2
        {
            text.push_str(&backup::section_text(
                &crate::manifest::Backup::bucket(&f.app),
                b.retention_days,
            ));
        }
    }
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect()
}

/// Whether `git diff --unified=0` output of one file only adds lines keel writes (see
/// [`hoist_lines`]) and removes none. Blank added lines are the separators keel adds.
pub(crate) fn only_keel_lines(diff: &str, keel: &[String]) -> bool {
    diff.lines()
        .filter(|l| {
            !(l.starts_with("+++") || l.starts_with("---") || l.starts_with("@@"))
                && (l.starts_with('+') || l.starts_with('-'))
        })
        .all(|l| {
            let (sign, rest) = l.split_at(1);
            sign == "+" && (rest.trim().is_empty() || keel.iter().any(|k| k == rest.trim_end()))
        })
}

/// The branch steps 3-9 work on (§5.1), without switching: the current branch unless it is the
/// default branch or HEAD is detached; then the manifest's `branch`.
pub(crate) fn work_branch(cx: &Ctx) -> Result<String, String> {
    let repo = repo(cx).map_err(|e| e.to_string())?;
    let default = crate::tools::gh::default_branch(cx.runner, &repo)?;
    let cur = crate::tools::git::current_branch(cx.runner, &cx.dir)?;
    Ok(crate::tools::git::resolve_branch(
        &cur,
        &default,
        cx.manifest.branch_name(),
    ))
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;
    use crate::ctx::Opts;
    use crate::exec::{FakeRunner, Out};

    #[test]
    fn work_branch_follows_5_1() {
        for (head, want) in [
            ("main\n", "keel/bootstrap"),
            ("HEAD\n", "keel/bootstrap"),
            ("claude/work\n", "claude/work"),
        ] {
            let r = FakeRunner::new()
                .on(
                    &["gh", "repo", "view"],
                    Out::ok(r#"{"defaultBranchRef":{"name":"main"}}"#),
                )
                .on(&["git", "rev-parse", "--abbrev-ref", "HEAD"], Out::ok(head));
            let cx = ctx(
                &r,
                ".".into(),
                manifest("rust"),
                PINS,
                Opts::default(),
                &no_sleep,
                &no_stdin,
            );
            assert_eq!(work_branch(&cx).unwrap(), want, "{head}");
        }
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use crate::ctx::{Ctx, Opts};
    use crate::exec::FakeRunner;
    use crate::manifest::Manifest;
    use crate::pins::Pins;
    use std::path::PathBuf;
    use std::time::Duration;
    use zeroize::Zeroizing;

    pub const PINS: &str = "[pins]\nsplice = \"0.6.0\"\nbosun = \"0.21.0\"\nhoist = \"0.5.1\"\nberth = \"0.2.1\"\n\n[refs.hull]\ntag = \"v0.13.0\"\nsha = \"3f4a65fda568cdd20c76168ed156d25a28ba5630\"\n";

    pub fn manifest(template: &str) -> Manifest {
        Manifest {
            repo: Some("owner/name".into()),
            template: Some(template.into()),
            template_ref: Some("v0.13.0".into()),
            pieces: vec!["rust-cli:my-cli".into()],
            ..Manifest::default()
        }
    }

    pub fn ctx<'a>(
        r: &'a FakeRunner,
        dir: PathBuf,
        manifest: Manifest,
        pins_text: &str,
        opts: Opts,
        sleep: &'a dyn Fn(Duration),
        stdin: &'a dyn Fn() -> std::io::Result<Zeroizing<String>>,
    ) -> Ctx<'a> {
        Ctx {
            runner: r,
            dir,
            manifest,
            pins: Pins::from_texts("t", pins_text, None).unwrap(),
            opts,
            verify: None,
            sleep,
            stdin,
        }
    }

    pub fn no_sleep(_: Duration) {}
    pub fn no_stdin() -> std::io::Result<Zeroizing<String>> {
        Err(std::io::Error::other("no stdin"))
    }
}
