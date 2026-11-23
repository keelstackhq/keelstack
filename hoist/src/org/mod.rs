//! `hoist org plan|check|apply` (spec §16): `org.toml`, one module per section. A section
//! module exposes `read(cfg, http) -> Result<SectionPlan<W>, HoistError>` (GET only; `W` is the
//! writes `apply` would do) and `apply(http, &[W]) -> Result<Vec<Item>, HoistError>` (one
//! request per write; the items it returns are the failed writes).

use crate::cli::{OrgArea, OrgArgs, OrgCommand};
use crate::error::HoistError;
use crate::http::{Fetched, Http, HttpError, Written};
use crate::ops::check::exit_code;
use crate::report::{Action, HumanStep, Item, Report};
use serde::Deserialize;
use std::io::IsTerminal;
use std::path::Path;

pub mod cloudflare;
pub mod crates;
pub mod github;
pub mod npm;
pub mod search_console;

const FILE: &str = "org.toml";

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct OrgCfg {
    pub github: Option<github::GithubCfg>,
    pub cloudflare: Option<cloudflare::CloudflareCfg>,
    pub npm: Option<npm::NpmCfg>,
    pub crates: Option<crates::CratesCfg>,
    pub search_console: Option<search_console::SearchConsoleCfg>,
}

/// What one section's read found: items, human steps, the writes `apply` would do and a hint.
pub(crate) struct SectionPlan<W> {
    pub items: Vec<Item>,
    pub steps: Vec<HumanStep>,
    pub writes: Vec<W>,
    pub hint: Option<String>,
}

impl<W> Default for SectionPlan<W> {
    fn default() -> Self {
        SectionPlan {
            items: Vec::new(),
            steps: Vec::new(),
            writes: Vec::new(),
            hint: None,
        }
    }
}

pub(crate) fn item(
    area: &str,
    kind: &str,
    key: &str,
    action: Action,
    reason: impl Into<String>,
) -> Item {
    Item {
        area: area.into(),
        kind: kind.into(),
        key: key.into(),
        action,
        reason: reason.into(),
        source: FILE.into(),
        verify: None,
    }
}

/// A human step; its id is `<area>:<kind>:<key>`, its title says how hoist verifies it.
pub(crate) fn step(id: String, title: String, url: &str, status: &str) -> HumanStep {
    HumanStep {
        id,
        title,
        url: url.into(),
        status: status.into(),
    }
}

/// `name` equals `parent` or is a subdomain of it (case-insensitive).
pub(crate) fn under(name: &str, parent: &str) -> bool {
    let (n, p) = (name.to_lowercase(), parent.to_lowercase());
    n == p || n.ends_with(&format!(".{p}"))
}

/// A write that cannot exist: sections without writes (npm, crates).
pub(crate) enum NoWrite {}

/// What a registry trust read found.
pub(crate) enum TrustRead {
    /// The JSON answer.
    Got(serde_json::Value),
    /// 404: the registry has no configuration.
    Absent,
    /// No access (401/403), an unparsable answer or an outage: the reason.
    Unknown(String),
}

/// GET a registry trust endpoint. A 401 or 403 is `unknown` here (spec §16.6), never exit 3.
pub(crate) fn read_trust(http: &Http, path: &str) -> Result<TrustRead, HoistError> {
    Ok(match http.get_json(path) {
        Ok(Fetched::Ok(v)) => TrustRead::Got(v),
        Ok(Fetched::NotFound) => TrustRead::Absent,
        Ok(Fetched::Forbidden(h)) => {
            TrustRead::Unknown(format!("the registry refused the read: {h}"))
        }
        Err(HttpError::Unauthorized { .. }) => {
            TrustRead::Unknown("the registry rejected the token (401)".into())
        }
        Err(e) => TrustRead::Unknown(format!("cannot read the trust configuration: {e}")),
    })
}

/// A refused write is data (its message, a failed item, exit 1); anything else is exit 3.
pub(crate) fn rejected(
    area: &str,
    label: &str,
    r: Result<Written, HttpError>,
) -> Result<Option<Item>, HoistError> {
    match r {
        Ok(_) => Ok(None),
        Err(HttpError::Rejected {
            status, message, ..
        }) => Ok(Some(item(
            area,
            "write",
            label,
            Action::Drift,
            format!("write failed ({status}): {message}"),
        ))),
        Err(e) => Err(e.into()),
    }
}

fn config_err(file: &Path, message: impl Into<String>) -> HoistError {
    HoistError::Config {
        file: file.display().to_string(),
        message: message.into(),
    }
}

/// Read and validate `org.toml` (spec §16.2).
pub fn load(path: &Path) -> Result<OrgCfg, HoistError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        config_err(
            path,
            format!("cannot read the org file ({e}); create it or pass --file PATH"),
        )
    })?;
    let cfg: OrgCfg =
        toml::from_str(&text).map_err(|e| config_err(path, e.message().trim_end()))?;
    validate(&cfg).map_err(|m| config_err(path, m))?;
    Ok(cfg)
}

fn validate(cfg: &OrgCfg) -> Result<(), String> {
    if let Some(g) = &cfg.github {
        github::validate(g)?;
    }
    if let Some(c) = &cfg.cloudflare {
        cloudflare::validate(c)?;
    }
    if let Some(n) = &cfg.npm {
        npm::validate(n)?;
    }
    if let Some(s) = &cfg.search_console {
        search_console::validate(s)?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Plan,
    Check,
    Apply { yes: bool },
}

pub fn run(dir: &Path, command: &OrgCommand, verbose: bool) -> Result<Report, HoistError> {
    match command {
        OrgCommand::Plan { args } => drive(dir, args, Mode::Plan, verbose),
        OrgCommand::Check { args } => drive(dir, args, Mode::Check, verbose),
        OrgCommand::Apply { args, yes } => drive(dir, args, Mode::Apply { yes: *yes }, verbose),
    }
}

struct Read {
    gh: Option<SectionPlan<github::EnvWrite>>,
    cf: Option<SectionPlan<cloudflare::Op>>,
    npm: Option<SectionPlan<NoWrite>>,
    crates: Option<SectionPlan<NoWrite>>,
    sc: Option<SectionPlan<search_console::SiteWrite>>,
}

fn drive(dir: &Path, args: &OrgArgs, mode: Mode, verbose: bool) -> Result<Report, HoistError> {
    let path = dir.join(args.file.as_deref().unwrap_or(Path::new(FILE)));
    let cfg = load(&path)?;
    let wants = |a: OrgArea| args.area.is_empty() || args.area.contains(&a);
    let gh = (wants(OrgArea::Github))
        .then_some(cfg.github.as_ref())
        .flatten();
    let cf = (wants(OrgArea::Cloudflare))
        .then_some(cfg.cloudflare.as_ref())
        .flatten();
    let npm = wants(OrgArea::Npm).then_some(cfg.npm.as_ref()).flatten();
    let crates = wants(OrgArea::Crates)
        .then_some(cfg.crates.as_ref())
        .flatten();
    let sc = wants(OrgArea::SearchConsole)
        .then_some(cfg.search_console.as_ref())
        .flatten();
    let gh_http = gh.and_then(|_| github::client(verbose));
    // Search Console writes its TXT record through the Cloudflare client too.
    let cf_http = (cf.is_some() || sc.is_some())
        .then(|| cloudflare::client(verbose))
        .flatten();
    let npm_http = npm.and_then(|_| npm::client(verbose));
    let crates_http = crates.and_then(|_| crates::client(verbose));
    let sc_http = sc.and_then(|c| search_console::client(c, verbose));
    let read = || -> Result<Read, HoistError> {
        Ok(Read {
            gh: gh.map(|c| github::read(c, gh_http.as_ref())).transpose()?,
            cf: cf
                .map(|c| cloudflare::read(c, cf_http.as_ref()))
                .transpose()?,
            npm: npm.map(|c| npm::read(c, npm_http.as_ref())).transpose()?,
            crates: crates
                .map(|c| crates::read(c, crates_http.as_ref()))
                .transpose()?,
            sc: sc
                .map(|c| search_console::read(c, sc_http.as_ref()))
                .transpose()?,
        })
    };
    let mut state = read()?;
    let mut failed = Vec::new();
    let mut sc_pending: Vec<Item> = Vec::new();
    let mut sc_steps: Vec<HumanStep> = Vec::new();
    if let Mode::Apply { yes } = mode {
        if cf.is_some() && cf_http.is_none() {
            // Surfaces the missing-token message (spec §16.5).
            crate::auth::cloudflare_token()?;
        }
        if let (Some(c), None) = (sc, &sc_http)
            && !c.sites.is_empty()
        {
            return Err(HoistError::Usage(
                search_console::unavailable(c).unwrap_or_default(),
            ));
        }
        let cf_writes = state.cf.as_ref().map_or(&[][..], |p| &p.writes[..]);
        let sc_writes = state.sc.as_ref().map_or(&[][..], |p| &p.writes[..]);
        let gh_writes = state.gh.as_ref().map_or(&[][..], |p| &p.writes[..]);
        if sc_writes.iter().any(|w| w.needs_cloudflare()) && cf_http.is_none() {
            crate::auth::cloudflare_token()?;
        }
        let lines: Vec<String> = cf_writes
            .iter()
            .map(|w| w.label())
            .chain(sc_writes.iter().map(|w| w.label()))
            .chain(gh_writes.iter().map(|w| w.label()))
            .collect();
        if !lines.is_empty() {
            if !yes && !confirm(&lines)? {
                return Err(HoistError::Usage("declined; nothing was written".into()));
            }
            // Cloudflare, then Search Console, then GitHub (spec §16.4).
            if let (Some(http), false) = (cf_http.as_ref(), cf_writes.is_empty()) {
                failed.extend(cloudflare::apply(http, cf_writes)?);
            }
            if let (Some(http), Some(c), false) = (sc_http.as_ref(), sc, sc_writes.is_empty()) {
                let out = search_console::apply(http, cf_http.as_ref(), sc_writes, &c.sites)?;
                failed.extend(out.failed);
                sc_pending = out.pending;
                sc_steps = out.steps;
            }
            if let (Some(http), false) = (gh_http.as_ref(), gh_writes.is_empty()) {
                failed.extend(github::apply(http, gh_writes)?);
            }
            state = read()?;
        }
    }
    // A verification Google refused while DNS propagates: pending, not the re-read's `create`.
    if let Some(p) = state.sc.as_mut() {
        for it in sc_pending {
            p.items
                .retain(|i| !(i.area == it.area && i.kind == it.kind && i.key == it.key));
            p.items.push(it);
        }
        p.steps.extend(sc_steps);
    }
    let mut report = Report {
        command: match mode {
            Mode::Plan => "org plan",
            Mode::Check => "org check",
            Mode::Apply { .. } => "org apply",
        }
        .into(),
        items: Vec::new(),
        human_steps: Vec::new(),
        exit_code: 0,
        text: None,
        hint: None,
    };
    let mut hints = Vec::new();
    let mut absorb = |items: Vec<Item>, steps: Vec<HumanStep>, hint: Option<String>| {
        report.items.extend(items);
        report.human_steps.extend(steps);
        hints.extend(hint);
    };
    if let Some(p) = state.cf {
        absorb(p.items, p.steps, p.hint);
    }
    if let Some(p) = state.npm {
        absorb(p.items, p.steps, p.hint);
    }
    if let Some(p) = state.crates {
        absorb(p.items, p.steps, p.hint);
    }
    if let Some(p) = state.sc {
        absorb(p.items, p.steps, p.hint);
    }
    if let Some(p) = state.gh {
        absorb(p.items, p.steps, p.hint);
    }
    report.items.extend(failed);
    report
        .items
        .sort_by(|a, b| (&a.area, &a.kind, &a.key).cmp(&(&b.area, &b.kind, &b.key)));
    report.human_steps.sort_by(|a, b| a.id.cmp(&b.id));
    if mode != Mode::Plan {
        report.exit_code = exit_code(&report.items);
        if report.exit_code == 0 && report.human_steps.iter().any(|s| s.status == "pending") {
            report.exit_code = 2;
        }
    }
    if !hints.is_empty() {
        report.hint = Some(format!("{}\n", hints.join("\n")).into());
    }
    Ok(report)
}

fn confirm(lines: &[String]) -> Result<bool, HoistError> {
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Err(HoistError::Usage(
            "stdin is not a terminal; use --yes in non-interactive runs".into(),
        ));
    }
    crate::ops::apply::ask_lines(
        &format!("hoist org apply will make {} change(s):", lines.len()),
        lines,
        "Apply these changes?",
        stdin.lock(),
        std::io::stderr().lock(),
    )
}
