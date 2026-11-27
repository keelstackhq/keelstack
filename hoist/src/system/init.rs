//! `hoist system init` (spec §17.3): draft system.toml from what the repo already says.
//! Unknown facts are `"TODO"`; the draft is sorted by id, so it is deterministic.

use super::check::{optional_secrets, path_of};
use super::{
    Cli, Consumer, CredKind, Credential, FILE, Provider, Status, Storage, StorageKind, SystemFile,
    SystemMeta,
};
use crate::error::HoistError;
use crate::repo::workflows::read_workflows;
use crate::report::{Action, Item, Report};
use crate::secrets::access::Access;
use crate::secrets::manifest::Manifest;
use crate::timeutil::{civil_from_days, now_secs};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const TODO: &str = "TODO";

fn provider_of(name: &str) -> &'static str {
    let p = |s: &str| name.starts_with(s);
    if p("CLOUDFLARE_") || p("CF_") || p("R2_") {
        "cloudflare"
    } else if p("FLY_") {
        "fly"
    } else if p("NPM_") {
        "npm"
    } else if p("CARGO_REGISTRY_") || p("CRATES_") {
        "crates"
    } else if p("ANTHROPIC_") {
        "anthropic"
    } else if p("GH_") || p("GITHUB_") || name.ends_with("_APP_KEY") || name == "RELEASE_TAG_KEY" {
        "github"
    } else if p("GOOGLE_") || p("GCP_") || p("GSC_") {
        "gsc"
    } else {
        "other"
    }
}

fn kind_of(name: &str) -> CredKind {
    if name.ends_with("_APP_KEY") {
        CredKind::AppKey
    } else if name == "RELEASE_TAG_KEY" || name.contains("SSH") || name.contains("DEPLOY_KEY") {
        CredKind::SshKey
    } else if name.contains("AGE_KEY") {
        CredKind::AgeKey
    } else if name.contains("SA_KEY") || name.contains("SERVICE_ACCOUNT") {
        CredKind::SaKey
    } else if name.ends_with("_TOKEN")
        || name.ends_with("_API_KEY")
        || name.ends_with("_ACCESS_KEY")
    {
        CredKind::ApiToken
    } else {
        CredKind::Other
    }
}

fn provider_meta(id: &str) -> Provider {
    let (capability, cli, mise) = match id {
        "github" => ("vcs+ci", Some("gh"), "gh"),
        "cloudflare" => ("dns+cdn", Some("wrangler"), "npm:wrangler"),
        "fly" => ("paas.container", Some("flyctl"), "flyctl"),
        "npm" => ("registry.npm", Some("npm"), "node"),
        "crates" => ("registry.crates", Some("cargo"), "rust"),
        "anthropic" => ("llm.api", None, ""),
        "gsc" => ("search-console", Some("gcloud"), "gcloud"),
        _ => (TODO, None, ""),
    };
    Provider {
        id: id.into(),
        capability: capability.into(),
        account: Some(TODO.into()),
        console: None,
        docs: None,
        clis: cli
            .map(|t| Cli {
                tool: t.into(),
                version: None,
                mise: Some(mise.into()),
            })
            .into_iter()
            .collect(),
        alternatives: Vec::new(),
    }
}

fn id_of(name: &str) -> String {
    name.to_lowercase().replace('_', "-")
}

fn blank(id: String, provider: &str, kind: CredKind, role: &str) -> Credential {
    Credential {
        id,
        provider: provider.into(),
        kind,
        role: role.into(),
        purpose: TODO.into(),
        owner: Some(TODO.into()),
        principal: None,
        scopes: Vec::new(),
        resources: Vec::new(),
        created_at: None,
        created_by: None,
        created_via: None,
        expires_at: None,
        max_ttl: None,
        rotate_every: None,
        rotation: None,
        storage: Vec::new(),
        consumers: Vec::new(),
        provider_ref: None,
        status: Status::Active,
        keyless_exempt: None,
        notes: None,
    }
}

fn day(d: i64) -> String {
    let (y, m, dd) = civil_from_days(d);
    format!("{y:04}-{m:02}-{dd:02}")
}

/// The draft for `dir`. `repo` is `owner/name` when keel.toml names it.
pub fn draft(dir: &Path) -> Result<SystemFile, HoistError> {
    let keel: Option<toml::Value> = std::fs::read_to_string(dir.join("keel.toml"))
        .ok()
        .and_then(|t| toml::from_str(&t).ok());
    let repo = keel
        .as_ref()
        .and_then(|k| k.get("repo"))
        .and_then(|r| r.as_str())
        .map_or_else(|| TODO.to_string(), str::to_string);
    let mut creds: BTreeMap<String, Credential> = BTreeMap::new();
    let by_name = |name: &str, creds: &mut BTreeMap<String, Credential>| -> String {
        let id = id_of(name);
        creds
            .entry(id.clone())
            .or_insert_with(|| blank(id.clone(), provider_of(name), kind_of(name), TODO));
        id
    };
    let gh_storage = |env: Option<&str>, key: &str| Storage {
        kind: if env.is_some() {
            StorageKind::GhEnvSecret
        } else {
            StorageKind::GhSecret
        },
        repo: Some(repo.clone()),
        env: env.map(str::to_string),
        path: None,
        app: None,
        key: key.into(),
    };
    let push_storage = |c: &mut Credential, s: Storage| {
        if !c.storage.contains(&s) {
            c.storage.push(s);
        }
    };

    let wf = read_workflows(dir)?;
    for u in wf.secret_uses.iter().filter(|u| u.secret != "GITHUB_TOKEN") {
        let id = by_name(&u.secret, &mut creds);
        let c = creds.get_mut(&id).expect("just inserted");
        push_storage(c, gh_storage(u.environment.as_deref(), &u.secret));
        let (workflow, job) = u.job.split_once(':').unwrap_or((&u.job, ""));
        let consumer = Consumer {
            workflow: Some(workflow.into()),
            job: Some(job.into()),
            environment: u.environment.clone(),
            runtime: None,
        };
        if !c.consumers.contains(&consumer) {
            c.consumers.push(consumer);
        }
    }
    for s in wf.secrets.iter().filter(|s| s.value != "GITHUB_TOKEN") {
        let id = by_name(&s.value, &mut creds);
        let c = creds.get_mut(&id).expect("just inserted");
        if c.storage.is_empty() {
            push_storage(c, gh_storage(None, &s.value));
        }
    }

    let hoist: Option<toml::Value> = std::fs::read_to_string(dir.join("hoist.toml"))
        .ok()
        .and_then(|t| toml::from_str(&t).ok());
    for name in optional_secrets(dir)? {
        let id = by_name(&name, &mut creds);
        let c = creds.get_mut(&id).expect("just inserted");
        if c.storage.is_empty() {
            push_storage(c, gh_storage(None, &name));
        }
        c.notes = Some("optional ([github] optional_secrets in hoist.toml)".into());
    }
    let gh = hoist.as_ref().and_then(|h| h.get("github"));
    if gh
        .and_then(|g| g.get("tags_bypass"))
        .and_then(|v| v.as_str())
        == Some("deploy_key")
    {
        let env = gh
            .and_then(|g| g.get("tags_key_environment"))
            .or_else(|| hoist.as_ref()?.get("release")?.get("environment"))
            .and_then(|v| v.as_str())
            .unwrap_or("release")
            .to_string();
        let id = by_name("RELEASE_TAG_KEY", &mut creds);
        let c = creds.get_mut(&id).expect("just inserted");
        c.kind = CredKind::SshKey;
        c.role = "push-tags".into();
        c.principal = Some("deploy key `hoist: release tags`".into());
        c.created_via = Some("hoist apply".into());
        push_storage(c, gh_storage(Some(&env), "RELEASE_TAG_KEY"));
    }

    if let Ok(text) = std::fs::read_to_string(dir.join("secrets.toml")) {
        let m = Manifest::parse(&text, "secrets.toml")?;
        for spec in m.env.values() {
            for (var, r) in &spec.vars {
                let id = by_name(var, &mut creds);
                let c = creds.get_mut(&id).expect("just inserted");
                push_storage(
                    c,
                    Storage {
                        kind: StorageKind::Sops,
                        repo: m.sources.get(&r.source).map(|s| s.repo.clone()),
                        env: None,
                        path: Some(r.path.clone()),
                        app: None,
                        key: r.key.clone(),
                    },
                );
            }
        }
    }

    if let Ok(text) = std::fs::read_to_string(dir.join("access.toml")) {
        let a = Access::parse(&text, "access.toml", now_secs())?;
        for r in &a.recipients {
            let mut c = blank(
                format!("age-{}", id_of(&r.name)),
                "other",
                CredKind::AgeKey,
                "decrypt",
            );
            c.principal = Some(format!("age recipient {}", r.name));
            c.expires_at = Some(day(r.days_left(0)));
            c.created_via = Some(format!("hoist secrets grant (recipient {})", r.name));
            match (&r.repo, &r.env) {
                (Some(repo), env) => c.storage.push(Storage {
                    kind: if env.is_some() {
                        StorageKind::GhEnvSecret
                    } else {
                        StorageKind::GhSecret
                    },
                    repo: Some(repo.clone()),
                    env: env.clone(),
                    path: None,
                    app: None,
                    key: "SOPS_AGE_KEY".into(),
                }),
                (None, _) => c.consumers.push(Consumer {
                    workflow: None,
                    job: None,
                    environment: None,
                    runtime: Some(format!("human:{}", r.name)),
                }),
            }
            creds.insert(c.id.clone(), c);
        }
    }

    let mut providers: BTreeSet<String> = BTreeSet::new();
    if let Ok(org) = crate::org::load(&dir.join("org.toml")) {
        for (on, id) in [
            (org.github.is_some(), "github"),
            (org.cloudflare.is_some(), "cloudflare"),
            (org.npm.is_some(), "npm"),
            (org.crates.is_some(), "crates"),
            (org.search_console.is_some(), "gsc"),
            (org.fly.is_some(), "fly"),
        ] {
            if on {
                providers.insert(id.into());
            }
        }
        for app in org.fly.iter().flat_map(|f| &f.apps) {
            for s in &app.secrets {
                let mut c = blank(
                    format!("fly-{}-{}", id_of(&app.name), id_of(s)),
                    "fly",
                    kind_of(s),
                    TODO,
                );
                c.storage.push(Storage {
                    kind: StorageKind::FlySecret,
                    repo: None,
                    env: None,
                    path: None,
                    app: Some(app.name.clone()),
                    key: s.clone(),
                });
                c.consumers.push(Consumer {
                    workflow: None,
                    job: None,
                    environment: None,
                    runtime: Some(format!("fly:{}", app.name)),
                });
                creds.insert(c.id.clone(), c);
            }
        }
    }

    if let Some(t) = keel.as_ref().and_then(|k| k.get("token")) {
        let name = t
            .get("secret")
            .and_then(|v| v.as_str())
            .unwrap_or("SPLICE_BOT_TOKEN");
        let env = t
            .get("environment")
            .and_then(|v| v.as_str())
            .unwrap_or("splice-bot");
        let id = by_name(name, &mut creds);
        let c = creds.get_mut(&id).expect("just inserted");
        c.provider = "github".into();
        c.role = "pr-write".into();
        c.storage.retain(|s| s.kind != StorageKind::GhSecret);
        push_storage(c, gh_storage(Some(env), name));
    }

    providers.extend(creds.values().map(|c| c.provider.clone()));
    Ok(SystemFile {
        version: 1,
        system: SystemMeta {
            name: repo,
            owner: TODO.into(),
            description: Some(TODO.into()),
        },
        providers: providers.iter().map(|p| provider_meta(p)).collect(),
        credentials: creds.into_values().collect(),
        trusts: Vec::new(),
    })
}

fn q(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn list(v: &[String]) -> String {
    format!(
        "[{}]",
        v.iter().map(|s| q(s)).collect::<Vec<_>>().join(", ")
    )
}

/// An inline table of the `Some` fields, in the given order.
fn inline(fields: &[(&str, Option<&str>)]) -> String {
    let parts: Vec<String> = fields
        .iter()
        .filter_map(|(k, v)| v.map(|v| format!("{k} = {}", q(v))))
        .collect();
    format!("{{ {} }}", parts.join(", "))
}

/// The draft as TOML text: one inline table per storage and consumer, so a credential reads
/// as one block.
pub fn render(sys: &SystemFile) -> String {
    let mut o = String::from(
        "# System description (hoist spec §17, D-031): names and metadata only, never values.\n\
         # Drafted by `hoist system init`; replace every TODO, then run `hoist system check`.\n",
    );
    o += &format!(
        "version = {}\n\n[system]\nname = {}\nowner = {}\n",
        sys.version,
        q(&sys.system.name),
        q(&sys.system.owner)
    );
    if let Some(d) = &sys.system.description {
        o += &format!("description = {}\n", q(d));
    }
    for p in &sys.providers {
        o += &format!(
            "\n[[provider]]\nid = {}\ncapability = {}\n",
            q(&p.id),
            q(&p.capability)
        );
        for (k, v) in [
            ("account", &p.account),
            ("console", &p.console),
            ("docs", &p.docs),
        ] {
            if let Some(v) = v {
                o += &format!("{k} = {}\n", q(v));
            }
        }
        if !p.clis.is_empty() {
            let c: Vec<String> = p
                .clis
                .iter()
                .map(|c| {
                    inline(&[
                        ("tool", Some(&c.tool)),
                        ("version", c.version.as_deref()),
                        ("mise", c.mise.as_deref()),
                    ])
                })
                .collect();
            o += &format!("clis = [{}]\n", c.join(", "));
        }
        o += &format!("alternatives = {}\n", list(&p.alternatives));
    }
    for c in &sys.credentials {
        o += &format!(
            "\n[[credential]]\nid = {}\nprovider = {}\nkind = {}\nrole = {}\npurpose = {}\n",
            q(&c.id),
            q(&c.provider),
            q(c.kind.as_str()),
            q(&c.role),
            q(&c.purpose)
        );
        let opt: [(&str, &Option<String>); 11] = [
            ("owner", &c.owner),
            ("principal", &c.principal),
            ("created_at", &c.created_at),
            ("created_by", &c.created_by),
            ("created_via", &c.created_via),
            ("expires_at", &c.expires_at),
            ("max_ttl", &c.max_ttl),
            ("rotate_every", &c.rotate_every),
            ("provider_ref", &c.provider_ref),
            ("keyless_exempt", &c.keyless_exempt),
            ("notes", &c.notes),
        ];
        for (k, v) in &opt[..2] {
            if let Some(v) = v {
                o += &format!("{k} = {}\n", q(v));
            }
        }
        if !c.scopes.is_empty() {
            o += &format!("scopes = {}\n", list(&c.scopes));
        }
        if !c.resources.is_empty() {
            o += &format!("resources = {}\n", list(&c.resources));
        }
        for (k, v) in &opt[2..] {
            if let Some(v) = v {
                o += &format!("{k} = {}\n", q(v));
            }
        }
        if let Some(r) = &c.rotation {
            let steps = if r.steps.is_empty() {
                String::new()
            } else {
                format!(", steps = {}", list(&r.steps))
            };
            let cmd = r
                .command
                .as_deref()
                .map(|c| format!(", command = {}", q(c)))
                .unwrap_or_default();
            o += &format!(
                "rotation = {{ strategy = {}{cmd}{steps} }}\n",
                q(r.strategy.as_str())
            );
        }
        if c.status != Status::Active {
            o += &format!(
                "status = {}\n",
                q(&format!("{:?}", c.status).to_lowercase())
            );
        }
        let storage: Vec<String> = c
            .storage
            .iter()
            .map(|s| {
                let kind = serde_json::to_value(s.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
                inline(&[
                    ("kind", Some(&kind)),
                    ("repo", s.repo.as_deref()),
                    ("env", s.env.as_deref()),
                    ("path", s.path.as_deref()),
                    ("app", s.app.as_deref()),
                    ("key", Some(&s.key)),
                ])
            })
            .collect();
        o += &format!("storage = [{}]\n", block(&storage));
        let consumers: Vec<String> = c
            .consumers
            .iter()
            .map(|u| {
                inline(&[
                    ("workflow", u.workflow.as_deref()),
                    ("job", u.job.as_deref()),
                    ("environment", u.environment.as_deref()),
                    ("runtime", u.runtime.as_deref()),
                ])
            })
            .collect();
        o += &format!("consumers = [{}]\n", block(&consumers));
    }
    for t in &sys.trusts {
        o += &format!(
            "\n[[trust]]\nprovider = {}\nkind = {}\nsubject = {}\n",
            q(&t.provider),
            q(t.kind.as_str()),
            q(&t.subject)
        );
        if let Some(r) = &t.replaces {
            o += &format!("replaces = {}\n", q(r));
        }
    }
    o
}

/// One entry per line once there are two or more.
fn block(v: &[String]) -> String {
    match v.len() {
        0 => String::new(),
        1 => v[0].clone(),
        _ => format!("\n  {},\n", v.join(",\n  ")),
    }
}

pub fn system_init(dir: &Path, print: bool) -> Result<Report, HoistError> {
    let sys = draft(dir)?;
    let text = render(&sys);
    // The draft must parse: a renderer bug is caught here, not in the user's next check.
    SystemFile::parse(&text, "system.toml (draft)")?;
    let path = path_of(dir, None);
    let item = |action, reason: &str| Item {
        area: "system".into(),
        kind: "file".into(),
        key: FILE.into(),
        action,
        reason: reason.into(),
        source: FILE.into(),
        verify: None,
    };
    let (it, out, hint) = if print {
        (
            item(Action::Ok, "draft printed, nothing written"),
            Some(text),
            None,
        )
    } else if !write_new(&path, &text)? {
        (
            item(Action::Ok, "exists, left unchanged"),
            None,
            Some("system.toml exists; `hoist system init --print` shows a fresh draft\n".into()),
        )
    } else {
        (
            item(
                Action::Create,
                &format!(
                    "{} credentials, {} providers drafted; replace every TODO",
                    sys.credentials.len(),
                    sys.providers.len()
                ),
            ),
            None,
            Some("next: fill in the TODOs, then `hoist system check`\n".into()),
        )
    };
    Ok(Report {
        command: "system init".into(),
        items: vec![it],
        human_steps: Vec::new(),
        exit_code: 0,
        text: out,
        hint: hint.map(String::into_boxed_str),
    })
}

/// Create `path` with `text`; `false` when it already exists (never overwritten, no race).
fn write_new(path: &Path, text: &str) -> Result<bool, HoistError> {
    super::create_atomic(path, text).map_err(|e| HoistError::Config {
        file: FILE.into(),
        message: format!("cannot write: {e}"),
    })
}
