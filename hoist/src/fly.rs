//! Running `fly` (the Fly.io CLI) for hoist: shared by the DNS address lookup and the R2
//! credential sink.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

/// A Fly app name: letters, digits, `.`, `_` and `-`, starting with a letter or digit (so it
/// can never be read as a flag).
pub fn valid_app(app: &str) -> bool {
    app.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && app
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn fly_program() -> String {
    std::env::var("HOIST_FLY")
        .ok()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "fly".to_string())
}

/// A `fly` that never sees hoist's tokens and runs in the repo dir (`-C`), so it reads that
/// project's `fly.toml` and not the one of the process's working directory. Its env is cleared
/// and only [`PASSED`] (plus `FLY_API_TOKEN`) come through; a caller that runs `fly` with a
/// credential's value sets `FLY_API_TOKEN` itself, which replaces the operator's.
pub fn fly_command(args: &[&str], dir: &Path) -> Command {
    command(&fly_program(), args, dir, std::env::vars_os())
}

fn command(
    program: &str,
    args: &[&str],
    dir: &Path,
    parent: impl IntoIterator<Item = (OsString, OsString)>,
) -> Command {
    let mut c = Command::new(program);
    c.args(args).current_dir(dir).env_clear();
    for (k, v) in parent {
        if k == "FLY_API_TOKEN" || k.to_str().is_some_and(|k| PASSED.contains(&k)) {
            c.env(k, v);
        }
    }
    c
}

/// The env `fly` inherits: where to find programs and its config, temp dirs, proxies and CA
/// bundles. No token but `FLY_API_TOKEN` (not even `FLY_ACCESS_TOKEN`) ever reaches it.
pub const PASSED: [&str; 22] = [
    "PATH",
    "HOME",
    "TMPDIR",
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "NO_PROXY",
    "no_proxy",
    "ALL_PROXY",
    "all_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    // Windows: needed to start a process, find the profile and temp dir.
    "SYSTEMROOT",
    "SystemRoot",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    "PATHEXT",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn child_env(parent: &[(&str, &str)]) -> Vec<String> {
        let parent: Vec<(OsString, OsString)> =
            parent.iter().map(|(k, v)| (k.into(), v.into())).collect();
        let out = command("/usr/bin/env", &[], Path::new("."), parent)
            .output()
            .expect("run /usr/bin/env");
        let mut lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect();
        lines.sort();
        lines
    }

    #[test]
    fn fly_sees_only_the_allowlist_and_its_own_token() {
        let env = child_env(&[
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/home/op"),
            ("HTTPS_PROXY", "http://proxy:3128"),
            ("no_proxy", "localhost"),
            ("SSL_CERT_FILE", "/etc/ca.pem"),
            ("FLY_API_TOKEN", "fly-op"),
            ("FLY_ACCESS_TOKEN", "fly-other"),
            ("GH_PAT", "gh"),
            ("CLOUDFLARE_API_TOKEN", "cf"),
            ("AWS_SECRET_ACCESS_KEY", "aws"),
            ("MY_DEPLOY_SECRET", "x"),
        ]);
        assert_eq!(
            env,
            [
                "FLY_API_TOKEN=fly-op",
                "HOME=/home/op",
                "HTTPS_PROXY=http://proxy:3128",
                "PATH=/usr/bin:/bin",
                "SSL_CERT_FILE=/etc/ca.pem",
                "no_proxy=localhost",
            ]
        );
    }

    #[test]
    fn a_callers_token_replaces_the_operators() {
        let parent: Vec<(OsString, OsString)> = vec![("FLY_API_TOKEN".into(), "fly-op".into())];
        let out = command("/usr/bin/env", &[], Path::new("."), parent)
            .env("FLY_API_TOKEN", "fly-cred")
            .output()
            .expect("run /usr/bin/env");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "FLY_API_TOKEN=fly-cred\n"
        );
    }
}
