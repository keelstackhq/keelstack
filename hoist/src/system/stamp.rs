//! `hoist system stamp <id>` (spec §17.1): record when, how and by whom a credential was
//! created, and when it expires, in `system.toml`. Edits only that credential's keys in place
//! (comments, order and every other credential stay byte-identical), never reads a value, makes
//! no HTTP request and never commits.

use super::check::path_of;
use super::{SystemFile, parse_expiry, source_of};
use crate::error::HoistError;
use crate::report::{Action, Item, Report};
use crate::timeutil::parse_date;
use std::path::Path;
use toml_edit::{DocumentMut, Item as TomlItem, Value};

/// The flags of `hoist system stamp`.
pub struct StampArgs<'a> {
    pub id: &'a str,
    pub created: &'a str,
    pub expires: Option<&'a str>,
    pub by: Option<&'a str>,
    pub via: Option<&'a str>,
    pub provider_ref: Option<&'a str>,
}

fn config(file: &str, message: impl Into<String>) -> HoistError {
    HoistError::Config {
        file: file.to_string(),
        message: message.into(),
    }
}

/// Set string keys inside the `[[credential]]` with this `id`, keeping comments and layout.
/// Existing keys keep their spacing and trailing comment; missing ones are added at the end of
/// the same table. The result is parsed and validated as a `SystemFile` before it is returned.
pub(crate) fn set_keys(
    text: &str,
    file: &str,
    id: &str,
    pairs: &[(&str, &str)],
) -> Result<String, HoistError> {
    let mut doc: DocumentMut = text.parse().map_err(|e| config(file, format!("{e}")))?;
    let table = doc
        .get_mut("credential")
        .and_then(TomlItem::as_array_of_tables_mut)
        .and_then(|tables| {
            tables
                .iter_mut()
                .find(|t| t.get("id").and_then(TomlItem::as_str) == Some(id))
        })
        .ok_or_else(|| config(file, format!("no [[credential]] with id `{id}`")))?;
    for (key, val) in pairs {
        if let Some(old) = table.get_mut(key).and_then(TomlItem::as_value_mut) {
            if old.as_str() == Some(val) {
                continue;
            }
            let mut new = Value::from(*val);
            *new.decor_mut() = old.decor().clone();
            *old = new;
        } else {
            table.insert(key, toml_edit::value(*val));
        }
    }
    let out = doc.to_string();
    SystemFile::parse(&out, file)?;
    Ok(out)
}

/// Write `text` to `path` atomically: a temp file in the same directory, synced, then renamed
/// over the target (the mode of the old file is kept). A failure leaves `path` untouched.
pub(crate) fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use ring::rand::{SecureRandom, SystemRandom};
    use std::io::Write;
    let mut random = [0u8; 8];
    SystemRandom::new()
        .fill(&mut random)
        .map_err(|_| std::io::Error::other("no randomness"))?;
    let hex: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!("{name}.tmp-{hex}"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    // Keep the file's mode on unix; other targets take the default.
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mode = std::fs::metadata(path).map_or(0o644, |m| m.permissions().mode() & 0o7777);
        options.mode(mode);
    }
    let mut file = options.open(&tmp)?;
    let written = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all());
    match written.and_then(|()| std::fs::rename(&tmp, path)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub fn system_stamp(
    dir: &Path,
    file: Option<&Path>,
    args: &StampArgs,
) -> Result<Report, HoistError> {
    if parse_date(args.created).is_none() {
        return Err(HoistError::Usage(format!(
            "--created `{}` is not a YYYY-MM-DD day",
            args.created
        )));
    }
    if let Some(e) = args.expires
        && parse_expiry(e).is_none()
    {
        return Err(HoistError::Usage(format!(
            "--expires `{e}` is not a YYYY-MM-DD day or `never`"
        )));
    }
    let path = path_of(dir, file);
    let shown = path.display().to_string();
    let sys = SystemFile::load(&path)?;
    if !sys.credentials.iter().any(|c| c.id == args.id) {
        return Err(HoistError::Usage(format!(
            "{shown}: no [[credential]] with id `{}`",
            args.id
        )));
    }
    let text =
        std::fs::read_to_string(&path).map_err(|e| config(&shown, format!("cannot read ({e})")))?;
    let mut pairs = vec![("created_at", args.created)];
    pairs.extend(args.expires.map(|v| ("expires_at", v)));
    pairs.extend(args.by.map(|v| ("created_by", v)));
    pairs.extend(args.via.map(|v| ("created_via", v)));
    pairs.extend(args.provider_ref.map(|v| ("provider_ref", v)));
    let new = set_keys(&text, &shown, args.id, &pairs)?;
    let changed = new != text;
    if changed {
        write_atomic(&path, &new)
            .map_err(|e| config(&shown, format!("cannot write ({e}); nothing was changed")))?;
    }
    let keys: Vec<&str> = pairs.iter().map(|(k, _)| *k).collect();
    Ok(Report {
        command: "system stamp".into(),
        items: vec![Item {
            area: "system".into(),
            kind: "stamp".into(),
            key: args.id.to_string(),
            action: if changed { Action::Update } else { Action::Ok },
            reason: if changed {
                format!("set {}", keys.join(", "))
            } else {
                "no change".into()
            },
            source: source_of(&text, &shown, args.id),
            verify: None,
        }],
        human_steps: Vec::new(),
        exit_code: 0,
        text: None,
        hint: None,
    })
}
