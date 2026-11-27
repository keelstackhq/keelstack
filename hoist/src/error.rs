use thiserror::Error;

/// Usage, config or auth errors. Every message says how to fix the problem.
#[derive(Debug, Error)]
pub enum HoistError {
    #[error("{0}")]
    Usage(String),
    #[error("{file}: {message}")]
    Config { file: String, message: String },
    /// A failed API call; the message names the service and path, never headers or bodies.
    #[error("{0}")]
    Network(String),
    /// A local file operation that failed (e.g. copying a release binary); during a publish
    /// it becomes a failed step, not exit 3.
    #[error("{0}")]
    Io(String),
    /// A ruleset write the GitHub plan refuses (Free, private repo): `apply` reports it as
    /// drift and goes on with the other writes.
    #[error("{0}")]
    PlanLimited(String),
    /// The repo has no publishable package, so there is nothing to release or to write
    /// (`init`). Stable JSON `error.kind` `nothing-to-publish`.
    #[error("{0}")]
    NothingToPublish(String),
}

impl HoistError {
    pub fn exit_code(&self) -> i32 {
        3
    }

    /// A stable machine-readable kind for the `--json` error object, when one is promised.
    pub fn kind(&self) -> Option<&'static str> {
        match self {
            HoistError::NothingToPublish(_) => Some("nothing-to-publish"),
            _ => None,
        }
    }
}
