//! `hoist release check`: read-only (GET only). Per registry package, is it published, which
//! version is the newest, does that match the workspace version, and is the trusted publisher
//! set up. Trusted publishers cannot be read without a login (npm) or at all (crates.io), so
//! they are reported `unknown` with where to look; `hoist release bootstrap` registers them.

use super::bootstrap::all_packages;
use crate::auth::Secret;
use crate::desired::read_desired_release;
use crate::diff_release::first_publish_step;
use crate::error::HoistError;
use crate::github::Fetch;
use crate::http::{Http, Service};
use crate::release::registry::{latest_version, version_exists};
use crate::release::{Package, PkgKind};
use crate::report::{Action, HumanStep, Item, Report};
use std::path::Path;

const COMMAND: &str = "release check";

fn item(kind: &str, p: &Package, action: Action, reason: String) -> Item {
    Item {
        area: "release".to_string(),
        kind: kind.to_string(),
        key: format!("{}@{}", p.name, p.version.value),
        action,
        reason,
        source: p.version.source.clone(),
        verify: None,
    }
}

fn registry(p: &Package) -> &'static str {
    match p.kind {
        PkgKind::Npm => "npm",
        PkgKind::Crate => "crates.io",
    }
}

fn trust_where(p: &Package) -> String {
    match p.kind {
        PkgKind::Npm => format!("unreadable without a login: `npm trust list {}`", p.name),
        PkgKind::Crate => format!(
            "crates.io has no API for it: https://crates.io/crates/{}/settings/new-trusted-publisher",
            p.name
        ),
    }
}

pub fn release_check(dir: &Path, verbose: bool) -> Result<Report, HoistError> {
    let Some(release) = read_desired_release(dir)? else {
        return Ok(Report {
            command: COMMAND.to_string(),
            items: vec![],
            human_steps: vec![],
            exit_code: 0,
            text: Some("no package publishes to a registry\n".to_string()),
            hint: None,
        });
    };
    let anonymous = || Secret::from("");
    let npm = Http::new(Service::Npm, anonymous()).verbose(verbose);
    let crates = Http::new(Service::Crates, anonymous()).verbose(verbose);
    let mut items = Vec::new();
    let mut humans: Vec<HumanStep> = Vec::new();
    for (p, _) in all_packages(&release) {
        let http = match p.kind {
            PkgKind::Npm => &npm,
            PkgKind::Crate => &crates,
        };
        let want = &p.version.value;
        let reg = registry(p);
        let latest = latest_version(http, p)?;
        let has = version_exists(http, p, want)?;
        match (latest, has) {
            (Fetch::Got(None), _) => {
                items.push(item(
                    "registry",
                    p,
                    Action::Pending,
                    format!("not on {reg} yet: run `hoist release bootstrap` (first publish)"),
                ));
                humans.push(first_publish_step(&p.name, p.kind));
                continue;
            }
            (Fetch::Got(Some(latest)), Fetch::Got(true)) => items.push(item(
                "registry",
                p,
                Action::Ok,
                format!("on {reg} at {want} (newest {latest})"),
            )),
            (Fetch::Got(Some(latest)), Fetch::Got(false)) => items.push(item(
                "registry",
                p,
                Action::Update,
                format!("newest on {reg} is {latest}, the workspace is at {want}: the release workflow publishes it"),
            )),
            (Fetch::Unknown(why), _) | (_, Fetch::Unknown(why)) => {
                items.push(item(
                    "registry",
                    p,
                    Action::Unknown,
                    format!("cannot read {reg}: {why}"),
                ));
                continue;
            }
        }
        items.push(item(
            "trusted_publisher",
            p,
            Action::Unknown,
            trust_where(p),
        ));
    }
    let exit_code = crate::ops::check::exit_code(&items);
    Ok(Report {
        command: COMMAND.to_string(),
        items,
        human_steps: humans,
        exit_code,
        text: None,
        hint: None,
    })
}
