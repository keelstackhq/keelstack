//! The `fly` live probe (spec §17.5): read-only `fly tokens list` and `fly secrets list`, run
//! with the token in the child's environment (never argv) and stderr discarded.

use super::{
    Ctx, LiveProbe, LiveResult, Sweep, day_of, day_str, expiry_drift, find_ref, no_token,
    own_token, token_for,
};
use crate::auth::{Secret, from_env};
use crate::fly::{fly_command, valid_app};
use crate::system::{Credential, Status, StorageKind};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

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
        let out = list_command(what, app, self.ctx.dir, tok)
            .output()
            .map_err(|_| NOT_FOUND.to_string())?;
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
            return no_token(cred, NO_TOKEN);
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
        let t = match find_ref(cred, &rows, |o| {
            (
                field(o, "id").unwrap_or_default(),
                field(o, "name").unwrap_or_default(),
            )
        }) {
            Ok(t) => t.copied(),
            Err(why) => return LiveResult::Unknown(why),
        };
        let Some(t) = t else {
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
                // Only Fly's own token: env FLY_API_TOKEN, or the value of a credential whose
                // provider is fly. Another provider's value is never sent to Fly.
                if apps[i].1.is_none() && c.provider == "fly" {
                    apps[i].1 = own_token(c);
                }
                apps[i].2.push(&s.key);
            }
        }
        for (app, tok, declared) in apps {
            let unknown = |why: String| Sweep {
                kind: "live",
                key: format!("fly:{app}"),
                source: String::new(),
                result: LiveResult::Unknown(why),
            };
            let Some(tok) = from_env("FLY_API_TOKEN").or(tok) else {
                out.push(unknown(NO_TOKEN.into()));
                continue;
            };
            let names = match self.secret_names(&app, &tok) {
                Ok(n) => n,
                Err(why) => {
                    out.push(unknown(format!(
                        "{why}: set FLY_API_TOKEN (fly tokens create org/readonly)"
                    )));
                    continue;
                }
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

/// `fly <what> list -a <app> --json` with only the intended token (`FLY_API_TOKEN`) in its env.
fn list_command(what: &str, app: &str, dir: &Path, tok: &Secret) -> Command {
    let args = [what, "list", "-a", app, "--json"];
    let mut cmd = fly_command(&args, dir);
    cmd.env("FLY_API_TOKEN", tok.expose())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn list_command_env_holds_only_the_intended_fly_token() {
        let tok = Secret::from("canary");
        let cmd = list_command("tokens", "app", Path::new("."), &tok);
        let envs: Vec<_> = cmd.get_envs().collect();
        let get = |n: &str| {
            envs.iter()
                .find(|(k, _)| *k == OsStr::new(n))
                .map(|(_, v)| *v)
        };
        assert_eq!(get("FLY_API_TOKEN"), Some(Some(OsStr::new("canary"))));
        for n in [
            "FLY_ACCESS_TOKEN",
            "NPM_TOKEN",
            "CARGO_REGISTRY_TOKEN",
            "GOOGLE_OAUTH_ACCESS_TOKEN",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "CLOUDFLARE_API_TOKEN",
        ] {
            // The env is cleared (crate::fly tests), so none may be set again here.
            assert!(!matches!(get(n), Some(Some(_))), "{n} must not reach fly");
        }
    }
}
