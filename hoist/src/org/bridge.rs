//! `[search_console.worker]` of org.toml (spec §16.8): deploy the Search Console bridge Worker
//! (`bridge_worker.js`, embedded in the binary) through the Cloudflare client and set its two
//! Worker secrets. Reads are GET only. Secret values come from the environment
//! (`HOIST_BRIDGE_TOKEN`, `HOIST_BRIDGE_SA_KEY` or `HOIST_BRIDGE_SA_KEY_FILE`) or a hidden
//! prompt on a terminal; they never reach argv, logs, items or JSON.

use super::{SectionPlan, item, rejected, step};
use crate::auth::Secret;
use crate::error::HoistError;
use crate::github::Fetch;
use crate::http::{Fetched, Http, HttpError, Method, seg};
use crate::report::{Action, Item};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::IsTerminal;

const WORKER_JS: &str = include_str!("bridge_worker.js");
const COMPAT_DATE: &str = "2026-11-01";
const BOUNDARY: &str = "----hoist-bridge-7d3f1c9a";
/// The two Worker secrets: (name, env var, env var holding a file path).
const SECRETS: [(&str, &str, Option<&str>); 2] = [
    ("BRIDGE_TOKEN", "HOIST_BRIDGE_TOKEN", None),
    (
        "SA_KEY",
        "HOIST_BRIDGE_SA_KEY",
        Some("HOIST_BRIDGE_SA_KEY_FILE"),
    ),
];

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerCfg {
    /// The Worker script name.
    pub name: String,
}

pub(super) fn validate(w: &WorkerCfg) -> Result<(), String> {
    let ok = !w.name.is_empty()
        && w.name.len() <= 63
        && w.name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !w.name.starts_with('-');
    ok.then_some(()).ok_or_else(|| {
        format!(
            "[search_console.worker] name `{}` must be lower-case letters, digits and `-`",
            w.name
        )
    })
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Write {
    Deploy {
        account: String,
        name: String,
    },
    SetSecret {
        account: String,
        name: String,
        secret: &'static str,
    },
}

impl Write {
    pub(crate) fn label(&self) -> String {
        match self {
            Write::Deploy { name, .. } => format!("create search_console/worker {name} (deploy)"),
            Write::SetSecret { name, secret, .. } => {
                format!("create search_console/worker_secret {name}/{secret}")
            }
        }
    }
}

/// Where a secret's value would come from right now.
fn has_source(secret: &(&str, &str, Option<&str>)) -> bool {
    let set = |v: &str| std::env::var(v).is_ok_and(|x| !x.is_empty());
    set(secret.1) || secret.2.is_some_and(set) || std::io::stdin().is_terminal()
}

/// `true` when the Worker and both secrets are there: the bridge can be read.
pub(crate) fn ready(p: &SectionPlan<Write>) -> bool {
    !p.items.is_empty() && p.items.iter().all(|i| i.action == Action::Ok)
}

pub(crate) fn read(
    w: &WorkerCfg,
    account: &str,
    http: Option<&Http>,
) -> Result<SectionPlan<Write>, HoistError> {
    let mut p = SectionPlan::default();
    let name = w.name.as_str();
    let kinds = |p: &mut SectionPlan<Write>, why: &str| {
        p.items
            .push(item("search_console", "worker", name, Action::Unknown, why));
        for s in SECRETS {
            p.items.push(item(
                "search_console",
                "worker_secret",
                &format!("{name}/{}", s.0),
                Action::Unknown,
                why,
            ));
        }
    };
    let Some(http) = http else {
        kinds(&mut p, "no Cloudflare token (set CLOUDFLARE_API_TOKEN)");
        return Ok(p);
    };
    let base = format!("/accounts/{}/workers/scripts/{}", seg(account), seg(name));
    let exists = match http.get_json(&format!("{base}/settings")) {
        Ok(Fetched::Ok(_)) => Fetch::Got(true),
        Ok(Fetched::NotFound) => Fetch::Got(false),
        Ok(Fetched::Forbidden(h)) => Fetch::Unknown(format!("cannot read Worker `{name}`: {h}")),
        Err(e) => return Err(e.into()),
    };
    let have: Fetch<Vec<String>> = match &exists {
        Fetch::Got(true) => match http.get_json(&format!("{base}/secrets")) {
            Ok(Fetched::Ok(v)) => Fetch::Got(
                v.get("result")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|s| s.get("name").and_then(Value::as_str).map(str::to_string))
                    .collect(),
            ),
            Ok(Fetched::NotFound) => Fetch::Got(Vec::new()),
            Ok(Fetched::Forbidden(h)) => {
                Fetch::Unknown(format!("cannot read the Worker secrets: {h}"))
            }
            Err(e) => return Err(e.into()),
        },
        Fetch::Got(false) => Fetch::Got(Vec::new()),
        Fetch::Unknown(why) => Fetch::Unknown(why.clone()),
    };
    match &exists {
        Fetch::Got(true) => p.items.push(item(
            "search_console",
            "worker",
            name,
            Action::Ok,
            "deployed",
        )),
        Fetch::Got(false) => {
            p.items.push(item(
                "search_console",
                "worker",
                name,
                Action::Create,
                "not deployed",
            ));
            p.writes.push(Write::Deploy {
                account: account.into(),
                name: name.into(),
            });
        }
        Fetch::Unknown(why) => {
            p.items
                .push(item("search_console", "worker", name, Action::Unknown, why))
        }
    }
    for s in SECRETS {
        let key = format!("{name}/{}", s.0);
        match &have {
            Fetch::Unknown(why) => p.items.push(item(
                "search_console",
                "worker_secret",
                &key,
                Action::Unknown,
                why,
            )),
            Fetch::Got(names) if names.iter().any(|n| n == s.0) => p.items.push(item(
                "search_console",
                "worker_secret",
                &key,
                Action::Ok,
                "set (name only)",
            )),
            Fetch::Got(_) if has_source(&s) => {
                p.items.push(item(
                    "search_console",
                    "worker_secret",
                    &key,
                    Action::Create,
                    "not set (value from the environment or a hidden prompt)",
                ));
                p.writes.push(Write::SetSecret {
                    account: account.into(),
                    name: name.into(),
                    secret: s.0,
                });
            }
            Fetch::Got(_) => {
                p.items.push(item(
                    "search_console",
                    "worker_secret",
                    &key,
                    Action::Pending,
                    "not set and no value source",
                ));
                let how = match s.2 {
                    Some(file) => format!("{} or {file} (a service-account key file)", s.1),
                    None => format!(
                        "{} (32+ random characters, e.g. `openssl rand -hex 32`)",
                        s.1
                    ),
                };
                p.steps.push(step(
                    format!("search_console:worker_secret:{key}"),
                    format!(
                        "set Worker secret {} by running `hoist org apply --area search_console` \
                         in a terminal (hidden prompt) or with {how} in the environment; hoist \
                         verifies the name, never the value",
                        s.0
                    ),
                    "https://dash.cloudflare.com",
                    "pending",
                ));
            }
        }
    }
    Ok(p)
}

fn multipart() -> Vec<u8> {
    let meta = json!({"main_module": "worker.js", "compatibility_date": COMPAT_DATE});
    format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"metadata\"\r\n\
         Content-Type: application/json\r\n\r\n{meta}\r\n\
         --{BOUNDARY}\r\nContent-Disposition: form-data; name=\"worker.js\"; filename=\"worker.js\"\r\n\
         Content-Type: application/javascript+module\r\n\r\n{WORKER_JS}\r\n--{BOUNDARY}--\r\n"
    )
    .into_bytes()
}

/// The value of one secret: env, a key file, else a hidden prompt. Validated, never echoed.
fn value(secret: &(&str, &str, Option<&str>)) -> Result<Secret, HoistError> {
    let env = |v: &str| std::env::var(v).ok().filter(|x| !x.is_empty());
    let raw = if let Some(v) = env(secret.1) {
        v
    } else if let Some(path) = secret.2.and_then(env) {
        std::fs::read_to_string(&path).map_err(|e| {
            HoistError::Usage(format!(
                "cannot read the file named by {} ({e})",
                secret.2.unwrap_or_default()
            ))
        })?
    } else if std::io::stdin().is_terminal() {
        rpassword::prompt_password(format!("value for Worker secret {}: ", secret.0))
            .map_err(|e| HoistError::Usage(format!("cannot read the prompt: {e}")))?
    } else {
        return Err(HoistError::Usage(format!(
            "no value for Worker secret {}: set {} or run in a terminal",
            secret.0, secret.1
        )));
    };
    let raw = raw.trim().to_string();
    match secret.0 {
        "BRIDGE_TOKEN" if raw.len() < 32 => Err(HoistError::Usage(
            "BRIDGE_TOKEN must be at least 32 characters (e.g. `openssl rand -hex 32`)".into(),
        )),
        "SA_KEY" => {
            let ok = serde_json::from_str::<Value>(&raw)
                .is_ok_and(|v| v.get("client_email").is_some() && v.get("private_key").is_some());
            if ok {
                Ok(Secret::from(raw))
            } else {
                Err(HoistError::Usage(
                    "SA_KEY must be the service-account JSON key (client_email and private_key)"
                        .into(),
                ))
            }
        }
        _ => Ok(Secret::from(raw)),
    }
}

/// Deploy, then set the secrets. Values are read before the first request, so a bad value
/// stops the run before anything is written.
pub(crate) fn resolve(writes: &[Write]) -> Result<Values, HoistError> {
    let mut values = Vec::new();
    for w in writes {
        if let Write::SetSecret { secret, .. } = w
            && let Some(s) = SECRETS.iter().find(|s| s.0 == *secret)
        {
            values.push((*secret, value(s)?));
        }
    }
    Ok(values)
}

/// Resolved secret values, by Worker secret name.
pub(crate) type Values = Vec<(&'static str, Secret)>;

pub(crate) fn apply(
    http: &Http,
    writes: &[Write],
    values: &Values,
) -> Result<Vec<Item>, HoistError> {
    let mut failed = Vec::new();
    let mut deployed_ok = true;
    for w in writes {
        match w {
            Write::Deploy { account, name } => {
                let base = format!("/accounts/{}/workers/scripts/{}", seg(account), seg(name));
                let r = http.send_bytes(
                    Method::Put,
                    &base,
                    &format!("multipart/form-data; boundary={BOUNDARY}"),
                    &multipart(),
                );
                let bad = rejected("search_console", &w.label(), r)?;
                deployed_ok = bad.is_none();
                failed.extend(bad);
                if deployed_ok {
                    let r = http.send_json(
                        Method::Post,
                        &format!("{base}/subdomain"),
                        Some(&json!({"enabled": true, "previews_enabled": false})),
                    );
                    failed.extend(rejected(
                        "search_console",
                        &format!("enable workers.dev for {name}"),
                        r,
                    )?);
                }
            }
            Write::SetSecret {
                account,
                name,
                secret,
            } if deployed_ok => {
                let text = values.iter().find(|v| v.0 == *secret).map(|v| &v.1);
                let body = json!({"name": secret, "text": text.map(Secret::expose), "type": "secret_text"});
                let r: Result<_, HttpError> = http.send_json(
                    Method::Put,
                    &format!(
                        "/accounts/{}/workers/scripts/{}/secrets",
                        seg(account),
                        seg(name)
                    ),
                    Some(&body),
                );
                failed.extend(rejected("search_console", &w.label(), r)?);
            }
            Write::SetSecret { .. } => {}
        }
    }
    Ok(failed)
}
