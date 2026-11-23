//! `hoist changeset ...` commands. None sends an HTTP request.

mod add;
mod init;
mod status;

use crate::cli::ChangesetCommand;
use crate::error::HoistError;
use crate::report::Report;
use std::path::Path;

pub fn run(dir: &Path, command: &ChangesetCommand) -> Result<Report, HoistError> {
    match command {
        ChangesetCommand::Add {
            packages,
            message,
            empty,
            id,
        } => add::add(dir, packages, message.as_deref(), *empty, id.as_deref()),
        ChangesetCommand::Init { from_commits } => init::init(dir, *from_commits),
        ChangesetCommand::Status { since } => status::status(dir, since.as_deref()),
    }
}

/// A report with one `create` item for a file this command wrote.
fn created(kind: &str, key: &str, rel: &str, reason: String) -> crate::Item {
    crate::Item {
        area: "changeset".into(),
        kind: kind.into(),
        key: key.into(),
        action: crate::Action::Create,
        reason,
        source: rel.into(),
        verify: None,
    }
}

fn report(command: &str, items: Vec<crate::Item>) -> Report {
    Report {
        command: command.into(),
        exit_code: 0,
        items,
        human_steps: vec![],
        text: None,
        hint: None,
    }
}

/// Exit 3 when `<dir>/.changeset` is a symlink: writing through it would leave the repo.
/// Returns whether the directory is absent (so a failing command can remove what it created).
fn check_changeset_dir(dir: &Path) -> Result<bool, HoistError> {
    match std::fs::symlink_metadata(dir.join(".changeset")) {
        Ok(m) if m.file_type().is_symlink() => Err(HoistError::Config {
            file: ".changeset".into(),
            message: "is a symlink; .changeset must be a regular directory".into(),
        }),
        Ok(_) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(HoistError::Io(format!("cannot read .changeset: {e}"))),
    }
}
