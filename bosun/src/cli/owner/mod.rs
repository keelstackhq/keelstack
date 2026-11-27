//! `bosun owner ...`: steps the owner runs on a computer (B2). T1: the GitHub App manifest flow.

use std::path::Path;
use std::time::Duration;

use serde_json::json;

use super::{ManifestArgs, OwnerCmd};
use crate::app::App;
use crate::output::{BosunError, CmdResult, Outcome};
use app_manifest::{Flow, FlowConfig, KeySource};

pub mod app_manifest;

pub fn run(dir: Option<&Path>, cmd: &OwnerCmd) -> CmdResult {
    match cmd {
        OwnerCmd::Manifest(a) => manifest(dir, a),
    }
}

/// Create (or, with `--manual`, take) the gate App and report its public ids. The private key
/// lives in memory only and is wiped when this returns: `owner setup` (T2) uses the library.
fn manifest(dir: Option<&Path>, a: &ManifestArgs) -> CmdResult {
    let (creds, source) = if a.manual {
        let usage = |m: &str| {
            BosunError::usage(
                m.to_string(),
                "pass --app-id, --client-id, --slug and one of --key-file <path> or --key-stdin",
            )
        };
        let (Some(id), Some(client_id), Some(slug)) = (a.app_id, &a.client_id, &a.slug) else {
            return Err(usage("--manual needs --app-id, --client-id and --slug"));
        };
        let key = match (&a.key_file, a.key_stdin) {
            (Some(p), false) => KeySource::File(p.clone()),
            (None, true) => KeySource::Stdin,
            _ => {
                return Err(usage(
                    "--manual needs exactly one of --key-file, --key-stdin",
                ));
            }
        };
        (app_manifest::manual(id, client_id, slug, &key)?, "manual")
    } else {
        let app = App::load(dir)?;
        let cfg = FlowConfig {
            app_name: a
                .name
                .clone()
                .unwrap_or_else(|| format!("{}-bosun-gate", app.ctx.repo.owner)),
            org: a.org.clone(),
            timeout: Duration::from_secs(a.timeout_secs),
        };
        let flow = Flow::bind(cfg)?;
        eprintln!(
            "bosun: open {} in your browser and confirm the App on GitHub (waits {} s)",
            flow.start_url(),
            a.timeout_secs
        );
        (flow.run(&*app.gh)?, "manifest")
    };
    Ok(Outcome::data(json!({
        "app_id": creds.id, "client_id": creds.client_id, "slug": creds.slug, "source": source,
    })))
}
