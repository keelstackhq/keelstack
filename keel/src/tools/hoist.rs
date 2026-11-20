use super::{failure, json, not_json};
use crate::exec::{Cmd, Out, Runner};
use serde_json::Value;
use std::path::Path;

pub enum Res {
    Ok,
    /// Drift (exit 1) or pending human steps (exit 2) as reported by `check`.
    Drift,
    /// The first of `human_steps` (exit 2).
    Human {
        title: String,
        url: String,
        /// `gh variable set …` from the matching item's reason, else `keel up`.
        command: String,
    },
    Failed(String),
}

fn envelope(cmd: &Cmd, out: &Out) -> Result<Value, String> {
    json(out).ok_or_else(|| not_json(cmd))
}

/// For a human step `variable:<NAME>`: the `gh variable set …` text of the item
/// `{kind:"variable", key:<NAME>}`'s `reason`.
fn variable_command(v: &Value, human: &Value) -> Option<String> {
    let name = human["id"].as_str()?.strip_prefix("variable:")?;
    let item = v["items"]
        .as_array()?
        .iter()
        .find(|i| i["kind"] == "variable" && i["key"].as_str() == Some(name))?;
    let reason = item["reason"].as_str()?;
    let at = reason.find("gh variable set")?;
    Some(reason[at..].trim().to_string())
}

pub fn check(r: &dyn Runner, dir: &Path) -> Res {
    let c = Cmd::new("hoist", &["--json", "check"]).dir(dir);
    let o = r.run(&c);
    match o.code {
        0..=2 => match envelope(&c, &o) {
            Ok(_) if o.code == 0 => Res::Ok,
            Ok(_) => Res::Drift,
            Err(e) => Res::Failed(e),
        },
        _ => Res::Failed(failure(&c, &o)),
    }
}

/// `hoist --json apply --yes` (hoist needs `--yes` without a TTY).
pub fn apply(r: &dyn Runner, dir: &Path) -> Res {
    let c = Cmd::new("hoist", &["--json", "apply", "--yes"]).dir(dir);
    let o = r.run(&c);
    match o.code {
        0 => match envelope(&c, &o) {
            Ok(_) => Res::Ok,
            Err(e) => Res::Failed(e),
        },
        2 => match envelope(&c, &o) {
            Ok(v) => {
                let h = &v["human_steps"][0];
                match h["title"].as_str() {
                    Some(t) => Res::Human {
                        title: t.to_string(),
                        url: h["url"].as_str().unwrap_or("").to_string(),
                        command: variable_command(&v, h).unwrap_or_else(|| "keel up".into()),
                    },
                    None => Res::Failed(format!(
                        "`{}` exited 2 without a human step; run it by hand",
                        c.line()
                    )),
                }
            }
            Err(e) => Res::Failed(e),
        },
        _ => Res::Failed(failure(&c, &o)),
    }
}
