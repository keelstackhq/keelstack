//! Running `fly` (the Fly.io CLI) for hoist: shared by the DNS address lookup and the R2
//! credential sink.

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
/// project's `fly.toml` and not the one of the process's working directory.
pub fn fly_command(args: &[&str], dir: &Path) -> Command {
    let mut c = Command::new(fly_program());
    c.args(args).current_dir(dir);
    for name in SCRUBBED {
        c.env_remove(name);
    }
    c
}

/// Tokens of other tools that `fly` must never inherit. `FLY_API_TOKEN` is not here: callers
/// that pass a credential set it explicitly, the rest use the operator's own.
pub const SCRUBBED: [&str; 7] = [
    "CLOUDFLARE_API_TOKEN",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "FLY_ACCESS_TOKEN",
    "NPM_TOKEN",
    "CARGO_REGISTRY_TOKEN",
    "GOOGLE_OAUTH_ACCESS_TOKEN",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn fly_command_strips_other_tools_tokens() {
        let c = fly_command(&["version"], Path::new("."));
        let envs: Vec<_> = c.get_envs().collect();
        for name in SCRUBBED {
            assert!(
                envs.iter()
                    .any(|(k, v)| *k == OsStr::new(name) && v.is_none()),
                "{name} not removed"
            );
        }
        assert!(envs.iter().all(|(k, _)| *k != OsStr::new("FLY_API_TOKEN")));
    }
}
