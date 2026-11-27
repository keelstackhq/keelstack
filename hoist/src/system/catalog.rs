//! The org catalog of alternatives (spec §17.6): data only, `catalog.toml`.
//! `hoist system explain` lists a provider's `alternatives` from it when it is found.

use crate::error::HoistError;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const FILE: &str = "catalog.toml";
pub const ENV: &str = "HOIST_CATALOG";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: i64,
    #[serde(default, rename = "capability")]
    pub capabilities: Vec<Capability>,
    #[serde(default, rename = "alternative")]
    pub alternatives: Vec<Alternative>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alternative {
    pub id: String,
    pub capability: String,
    pub name: String,
    #[serde(default)]
    pub docs: Option<String>,
    #[serde(default)]
    pub keyless: Vec<String>,
    pub migration: String,
}

fn err(file: &str, message: String) -> HoistError {
    HoistError::Config {
        file: file.into(),
        message,
    }
}

impl Catalog {
    pub fn parse(text: &str, file: &str) -> Result<Catalog, HoistError> {
        let c: Catalog = toml::from_str(text).map_err(|e| err(file, e.to_string()))?;
        if c.version != 1 {
            return Err(err(file, format!("version must be 1, found {}", c.version)));
        }
        let mut caps = BTreeSet::new();
        for cap in &c.capabilities {
            if !caps.insert(cap.id.as_str()) {
                return Err(err(file, format!("duplicate capability id `{}`", cap.id)));
            }
        }
        let mut ids = BTreeSet::new();
        for a in &c.alternatives {
            if !ids.insert(a.id.as_str()) {
                return Err(err(file, format!("duplicate alternative id `{}`", a.id)));
            }
            if !caps.contains(a.capability.as_str()) {
                return Err(err(
                    file,
                    format!(
                        "alternative `{}` names capability `{}` that no [[capability]] declares",
                        a.id, a.capability
                    ),
                ));
            }
        }
        Ok(c)
    }

    pub fn load(path: &Path) -> Result<Catalog, HoistError> {
        let file = path.display().to_string();
        let text = std::fs::read_to_string(path)
            .map_err(|e| err(&file, format!("cannot read the catalog ({e})")))?;
        Catalog::parse(&text, &file)
    }

    pub fn alternative(&self, id: &str) -> Option<&Alternative> {
        self.alternatives.iter().find(|a| a.id == id)
    }
}

/// The catalog to use: `--catalog`, else `HOIST_CATALOG`, else `catalog.toml` next to the
/// system file when it exists. A path given by flag or env must exist (exit 3); none found is
/// `None` (explain output unchanged).
pub fn find(
    dir: &Path,
    system_file: &Path,
    flag: Option<&Path>,
) -> Result<Option<Catalog>, HoistError> {
    let rel = |p: &Path| -> PathBuf {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            dir.join(p)
        }
    };
    if let Some(p) = flag {
        return Catalog::load(&rel(p)).map(Some);
    }
    if let Some(p) = std::env::var_os(ENV).filter(|v| !v.is_empty()) {
        return Catalog::load(&rel(Path::new(&p))).map(Some);
    }
    let next = system_file.parent().unwrap_or(dir).join(FILE);
    if next.is_file() {
        return Catalog::load(&next).map(Some);
    }
    Ok(None)
}
