//! A fake npm registry and a repo with tool pins, shared by the pin tests.

use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub fn b64(bytes: &[u8]) -> String {
    bosun::github::b64::encode(bytes)
}

pub fn sha(bytes: &[u8]) -> String {
    b64(ring::digest::digest(&ring::digest::SHA512, bytes).as_ref())
}

/// The three npm packages of a tool.
pub fn pkgs(tool: &str) -> [String; 3] {
    [
        tool.into(),
        format!("{tool}-linux-x64"),
        format!("{tool}-linux-arm64"),
    ]
}

/// A fake npm registry; `log` holds every requested path.
pub struct Registry {
    pub base: String,
    pub log: Arc<Mutex<Vec<String>>>,
}

impl Registry {
    /// `latest`: tool -> version. `tamper`: serve other bytes than the integrity covers for this package.
    /// `foreign`: this package's tarball URL points at another host.
    pub fn start(latest: &[(&str, &str)], tamper: Option<&str>, foreign: Option<&str>) -> Registry {
        Registry::start_with(latest, &[], &[], &[], tamper, foreign)
    }

    /// As `start`, plus `older`: tool -> another published version (served by version only, never
    /// as `latest`), and `missing`: `package@version` entries left out of the registry (a 404); `deprecated`: versions (or `package@version`) npm marks deprecated (a yanked release).
    pub fn start_with(
        latest: &[(&str, &str)],
        older: &[(&str, &str)],
        deprecated: &[&str],
        missing: &[&str],
        tamper: Option<&str>,
        foreign: Option<&str>,
    ) -> Registry {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!(
            "http://127.0.0.1:{}",
            server.server_addr().to_ip().unwrap().port()
        );
        let mut routes: HashMap<String, Vec<u8>> = HashMap::new();
        let all: Vec<(&str, &str, bool)> = latest
            .iter()
            .map(|(t, v)| (*t, *v, true))
            .chain(older.iter().map(|(t, v)| (*t, *v, false)))
            .collect();
        for (tool, ver, is_latest) in all {
            for p in pkgs(tool) {
                if missing.contains(&format!("{p}@{ver}").as_str()) {
                    continue;
                }
                let tar = format!("{p}@{ver}").into_bytes();
                let host = if foreign == Some(p.as_str()) {
                    "http://evil.example"
                } else {
                    base.as_str()
                };
                let tarball = format!("{host}/@keelstackhq/{p}/-/{p}-{ver}.tgz");
                let mut meta = json!({"version": ver, "dist": {"integrity": format!("sha512-{}", sha(&tar)), "tarball": tarball}});
                if deprecated.contains(&ver) || deprecated.contains(&format!("{p}@{ver}").as_str())
                {
                    meta["deprecated"] = json!("yanked: broken release");
                }
                let served = if tamper == Some(p.as_str()) {
                    b"other bytes".to_vec()
                } else {
                    tar
                };
                if is_latest {
                    routes.insert(
                        format!("/@keelstackhq/{p}/latest"),
                        meta.to_string().into_bytes(),
                    );
                }
                routes.insert(
                    format!("/@keelstackhq/{p}/{ver}"),
                    meta.to_string().into_bytes(),
                );
                routes.insert(format!("/@keelstackhq/{p}/-/{p}-{ver}.tgz"), served);
            }
        }
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                let url = req.url().to_string();
                seen.lock().unwrap().push(url.clone());
                let _ = match routes.get(&url) {
                    Some(b) => req.respond(tiny_http::Response::from_data(b.clone())),
                    None => {
                        req.respond(tiny_http::Response::from_string("no").with_status_code(404))
                    }
                };
            }
        });
        Registry { base, log }
    }

    pub fn tarballs(&self) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|u| u.contains("/-/"))
            .count()
    }
}

pub const PINS: [(&str, &str); 4] = [
    ("splice", "0.4.0"),
    ("hoist", "0.4.0"),
    ("berth", "0.1.0"),
    ("bosun", "0.9.0"),
];

pub fn lines(tool: &str, ver: &str) -> String {
    let u = tool.to_uppercase();
    format!(
        "{u}_VERSION={ver}\n{u}_SHA512=old\n{u}_LINUX_X64_SHA512=old\n{u}_LINUX_ARM64_SHA512=old\n"
    )
}

/// A repo with the setup script, .bosun/version and the channel template.
pub fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let mut script = String::from("NODE_MAJOR=24\n");
    let mut tpl = script.clone();
    for (t, v) in PINS {
        script += &lines(t, v);
        if t != "bosun" {
            tpl += &lines(t, v);
        }
    }
    tpl += "BOSUN_VERSION={% if bosun_version %}x{% endif %}\n";
    for (rel, text) in [
        ("docs/setup-script.sh", script.as_str()),
        (
            "channel/pieces/core/files/docs/setup-script.sh.jinja",
            tpl.as_str(),
        ),
        (".bosun/version", "0.9.0\n"),
    ] {
        let p = d.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}
