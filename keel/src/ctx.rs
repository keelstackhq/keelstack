use crate::exec::Runner;
use crate::manifest::Manifest;
use crate::pins::Pins;
use std::path::PathBuf;
use std::time::Duration;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Default)]
pub struct Opts {
    pub wait_secs: u64,
    pub poll_secs: u64,
    pub token_stdin: bool,
    /// Seconds before a long tool call (hoist, fly-setup.sh, smoke.sh) is killed; 0 = the default.
    pub tool_timeout_secs: u64,
    pub dry_run: bool,
    /// `CLOUDFLARE_API_TOKEN` is set. Only its presence is ever read.
    pub cloudflare_token: bool,
    /// `CLOUDFLARE_ACCOUNT_ID` (an id, not a secret), when set and non-empty.
    pub cloudflare_account_id: Option<String>,
}

pub const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 600;

impl Opts {
    /// The timeout for hoist, `scripts/fly-setup.sh` and `scripts/smoke.sh`.
    pub fn tool_timeout(&self) -> Duration {
        Duration::from_secs(match self.tool_timeout_secs {
            0 => DEFAULT_TOOL_TIMEOUT_SECS,
            n => n,
        })
    }
}

pub struct Ctx<'a> {
    pub runner: &'a dyn Runner,
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub pins: Pins,
    pub opts: Opts,
    /// Set by the verify step.
    pub verify: Option<crate::report::Verify>,
    pub sleep: &'a dyn Fn(Duration),
    pub stdin: &'a dyn Fn() -> std::io::Result<Zeroizing<String>>,
}
