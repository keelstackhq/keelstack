use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "keel",
    version,
    about = "Bootstrap a repo with the keelstackhq tools"
)]
pub struct Cli {
    /// The repo checkout.
    #[arg(short = 'C', global = true, default_value = ".")]
    pub dir: PathBuf,
    /// Print exactly one JSON object.
    #[arg(long, global = true)]
    pub json: bool,
    /// Log subprocess calls to stderr.
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run every step, stopping at the first one only a human can do.
    Up(UpArgs),
    /// Check every step and report what `up` would do; writes nothing.
    Plan(UpArgs),
    /// Check preflight and the remote health; writes nothing.
    Doctor,
    /// Move this repo to a keelstack release in one step: tools, splice, bosun pins, hoist, checks.
    Migrate(MigrateArgs),
}

#[derive(Debug, Clone, Default, Args)]
pub struct MigrateArgs {
    /// The release to move to, X.Y.Z (default: keel's own version).
    #[arg(long, value_name = "X.Y.Z")]
    pub to: Option<String>,
    /// Report what would change; write nothing, install nothing, run no git command that writes.
    #[arg(long)]
    pub dry_run: bool,
    /// Push the branch and open ONE pull request with `gh` (keel never merges it).
    #[arg(long)]
    pub pr: bool,
    /// The ref to migrate from (default: `origin/<default branch>` after a fetch that must work).
    #[arg(long, value_name = "REF")]
    pub base: Option<String>,
    /// Also plan a bosun mode move (`bosun migrate --to MODE`); it opens its own PR, so keel only
    /// lists it as a step for after this PR merges.
    #[arg(long, value_name = "MODE", value_parser = ["gatekeeper", "steady", "booster", "autonomous"])]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Default, Args)]
pub struct UpArgs {
    /// OWNER/NAME of the GitHub repo.
    #[arg(long)]
    pub repo: Option<String>,
    /// rust, ts, or a splice source.
    #[arg(long)]
    pub template: Option<String>,
    /// Git ref (tag, branch or sha) of the template; `rust`/`ts` default to the hull pin.
    #[arg(long)]
    pub template_ref: Option<String>,
    /// Splice pieces: ID or ID:NAME, comma separated.
    #[arg(long)]
    pub pieces: Option<String>,
    /// A bosun plan file to import.
    #[arg(long)]
    pub plan: Option<String>,
    /// Seconds a GitHub App gate polls before it returns exit 5.
    #[arg(long, default_value_t = 300)]
    pub wait: u64,
    /// Read the PAT for the token step from stdin.
    #[arg(long)]
    pub token_stdin: bool,
    /// Record a human step as done: `agent-env` (the cloud setup script is attached), `dns` (the
    /// DNS records were added by hand; stores the host) or `backup` (the bucket was created by
    /// hand; stores the fly app). Repeatable.
    #[arg(long = "confirm", value_name = "STEP")]
    pub confirm: Vec<String>,
}

impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Command::Up(_) => "up",
            Command::Plan(_) => "plan",
            Command::Doctor => "doctor",
            Command::Migrate(_) => "migrate",
        }
    }
}
