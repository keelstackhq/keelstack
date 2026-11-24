use thiserror::Error;

/// Usage, manifest or environment errors. Every message says how to fix the problem.
#[derive(Debug, Error)]
pub enum KeelError {
    #[error("{0}")]
    Usage(String),
    /// A usage error whose fix is a command line (shown in the JSON `error.fix`).
    #[error("{message}")]
    Fix { message: String, fix: String },
    #[error("{file}: {message}")]
    Manifest { file: String, message: String },
    #[error("{0}")]
    Env(String),
    /// SIGINT/SIGTERM received (the number); exit code 128 + the number, as shells do.
    #[error("interrupted by {}", if *.0 == 2 { "SIGINT" } else { "SIGTERM" })]
    Interrupted(i32),
}

impl KeelError {
    /// How to fix it, when the error knows a specific command.
    pub fn fix(&self) -> Option<&str> {
        match self {
            KeelError::Fix { fix, .. } => Some(fix),
            _ => None,
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            KeelError::Usage(_) | KeelError::Fix { .. } => 2,
            KeelError::Interrupted(sig) => 128 + sig,
            _ => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes() {
        assert_eq!(KeelError::Usage("x".into()).exit_code(), 2);
        assert_eq!(KeelError::Env("x".into()).exit_code(), 3);
        let m = KeelError::Manifest {
            file: "f".into(),
            message: "m".into(),
        };
        assert_eq!(m.exit_code(), 3);
        let f = KeelError::Fix {
            message: "m".into(),
            fix: "f".into(),
        };
        assert_eq!(f.exit_code(), 2);
        assert_eq!(KeelError::Interrupted(2).exit_code(), 130);
        assert_eq!(KeelError::Interrupted(15).exit_code(), 143);
    }
}
