//! The `fly` live probe (spec §17.5): read-only `fly tokens list` and `fly secrets list`, run
//! with the token in the child's environment (never argv) and stderr discarded.

use super::{
    Ctx, LiveProbe, LiveResult, Sweep, day_of, day_str, expiry_drift, ref_matches, token_for,
};
use crate::auth::Secret;
use crate::fly::{fly_command, valid_app};
use crate::system::{Credential, Status, StorageKind};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::process::Stdio;

const NO_TOKEN: &str = "no Fly token: set FLY_API_TOKEN (fly tokens create org/readonly)";
const NOT_FOUND: &str = "fly CLI not found (HOIST_FLY or PATH)";

pub struct Probe<'a> {
    ctx: &'a Ctx<'a>,
    secrets: RefCell<HashMap<String, Result<Vec<String>, String>>>,
}

impl<'a> Probe<'a> {
    pub fn new(ctx: &'a Ctx<'a>) -> Probe<'a> {
        Probe {
            ctx,
            secrets: RefCell::new(HashMap::new()),
        }
    }

    /// The JSON `fly <what> list -a <app> --json` prints, or the unknown text.
    fn run(&self, what: &str, app: &str, tok: &Secret) -> Result<Value, String> {
        let args = [what, "list", "-a", app, "--json"];
        let mut cmd = fly_command(&args, self.ctx.dir);
        cmd.env("FLY_API_TOKEN", tok.expose())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let out = cmd.output().map_err(|_| NOT_FOUND.to_string())?;
        let no_json = || format!("`fly {what} list --json` gave no JSON (flyctl too old?)");
        if !out.status.success() {
            return Err(no_json());
        }
        serde_json::from_slice(&out.stdout).map_err(|_| no_json())
    }

    /// The names of the app's secrets, asked once per app.
    fn secret_names(&self, app: &str, tok: &Secret) -> Result<Vec<String>, String> {
        if let Some(c) = self.secrets.borrow().get(app) {
            return c.clone();
        }
        let r = self.run("secrets", app, tok).map(|v| {
            rows(&v)
                .iter()
                .filter_map(|o| field(o, "name"))
                .collect::<Vec<_>>()
        });
        self.secrets.borrow_mut().insert(app.to_string(), r.clone());
        r
    }
}

impl LiveProbe for Probe<'_> {
    fn provider(&self) -> &str {
        "fly"
    }

    fn probe(&self, cred: &Credential) -> LiveResult {
        let Some(tok) = token_for(cred, &["FLY_API_TOKEN"]) else {
            return LiveResult::Unknown(NO_TOKEN.into());
        };
        let Some(app) = app_of(cred) else {
            return LiveResult::Unknown("name the app in resources as app:<name>".into());
        };
        if !valid_app(&app) {
            return LiveResult::Unknown(format!("`{app}` is not a Fly app name"));
        }
        if cred
            .provider_ref
            .as_deref()
            .is_none_or(|r| r.trim().is_empty())
        {
            return LiveResult::Unknown("set provider_ref to the Fly token id".into());
        }
        let list = match self.run("tokens", &app, &tok) {
            Ok(v) => v,
            Err(m) => return LiveResult::Unknown(m),
        };
        let rows = rows(&list);
        let Some(t) = rows.iter().find(|o| {
            ref_matches(
                cred,
                &field(o, "id").unwrap_or_default(),
                &field(o, "name").unwrap_or_default(),
            )
        }) else {
            return LiveResult::Drift(format!("token not found in fly tokens list for {app}"));
        };
        let name = field(t, "name").unwrap_or_else(|| "?".into());
        let revoked = field(t, "revokedat").filter(|r| !zero(r));
        if let Some(at) = revoked {
            return LiveResult::Drift(format!(
                "token {name} was revoked on {}",
                day_of(&at).map_or(at, day_str)
            ));
        }
        let expires = field(t, "expiresat")
            .filter(|e| !zero(e))
            .and_then(|e| day_of(&e));
        if let Some(d) = expiry_drift(cred, expires, self.ctx.today) {
            return LiveResult::Drift(d);
        }
        LiveResult::Ok(format!(
            "token {name} expires {}",
            expires.map_or_else(|| "never".to_string(), day_str)
        ))
    }

    fn sweep(&self) -> Vec<Sweep> {
        let mut out = Vec::new();
        // app -> (token, declared secret names)
        let mut apps: Vec<(String, Option<Secret>, Vec<&str>)> = Vec::new();
        for c in &self.ctx.sys.credentials {
            if c.status != Status::Active {
                continue;
            }
            for s in c
                .storage
                .iter()
                .filter(|s| s.kind == StorageKind::FlySecret)
            {
                let Some(app) = s.app.clone().filter(|a| valid_app(a)) else {
                    continue;
                };
                let i = apps
                    .iter()
                    .position(|(a, _, _)| *a == app)
                    .unwrap_or_else(|| {
                        apps.push((app, None, Vec::new()));
                        apps.len() - 1
                    });
                if apps[i].1.is_none() {
                    apps[i].1 = token_for(c, &["FLY_API_TOKEN"]);
                }
                apps[i].2.push(&s.key);
            }
        }
        for (app, tok, declared) in apps {
            let Some(tok) = tok else { continue };
            let Ok(names) = self.secret_names(&app, &tok) else {
                continue;
            };
            for c in &self.ctx.sys.credentials {
                if c.status != Status::Active {
                    continue;
                }
                for s in c.storage.iter().filter(|s| {
                    s.kind == StorageKind::FlySecret && s.app.as_deref() == Some(app.as_str())
                }) {
                    let result = if names.contains(&s.key) {
                        LiveResult::Ok(format!("fly:{app}/{} is set", s.key))
                    } else {
                        LiveResult::Drift(format!("missing fly:{app}/{}", s.key))
                    };
                    out.push(Sweep {
                        kind: "live_storage",
                        key: c.id.clone(),
                        source: format!("fly:{app}"),
                        result,
                    });
                }
            }
            for n in names.iter().filter(|n| !declared.contains(&n.as_str())) {
                out.push(Sweep {
                    kind: "undeclared",
                    key: n.clone(),
                    source: format!("fly:{app}"),
                    result: LiveResult::Drift(format!(
                        "fly:{app}/{n} is set but no credential declares it"
                    )),
                });
            }
        }
        out
    }
}

/// The Fly app a credential lives in: its `fly-secret` storage, `resources` (`app:<n>` or
/// `fly:<n>`) or a `fly:<app>` runtime consumer.
fn app_of(cred: &Credential) -> Option<String> {
    cred.storage
        .iter()
        .filter(|s| s.kind == StorageKind::FlySecret)
        .find_map(|s| s.app.clone())
        .or_else(|| {
            cred.resources.iter().find_map(|r| {
                r.strip_prefix("app:")
                    .or_else(|| r.strip_prefix("fly:"))
                    .map(str::to_string)
            })
        })
        .or_else(|| {
            cred.consumers.iter().find_map(|c| {
                c.runtime
                    .as_deref()
                    .and_then(|r| r.strip_prefix("fly:"))
                    .map(str::to_string)
            })
        })
        .filter(|a| !a.is_empty())
}

/// The objects of a list: an array, or an object holding one under `tokens` or `data`.
fn rows(v: &Value) -> Vec<&Value> {
    let arr = v
        .as_array()
        .or_else(|| v["tokens"].as_array())
        .or_else(|| v["data"].as_array());
    arr.map(|a| a.iter().filter(|o| o.is_object()).collect())
        .unwrap_or_default()
}

/// A field by normalised name (lower case, no `_` or space): `expires_at`, `expiresAt` and
/// `Expires At` are all `expiresat`. Strings and numbers; null and empty are `None`.
fn field(o: &Value, name: &str) -> Option<String> {
    o.as_object()?.iter().find_map(|(k, v)| {
        let norm: String = k
            .chars()
            .filter(|c| !matches!(c, '_' | ' '))
            .flat_map(char::to_lowercase)
            .collect();
        if norm != name {
            return None;
        }
        match v {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    })
}

/// Go's zero time, which flyctl prints for "not set".
fn zero(s: &str) -> bool {
    s.starts_with("0001-")
}
