//! One small adapter per external tool: build argv, parse output. Never reimplement a tool.
use crate::exec::{Cmd, Out};
use serde_json::Value;

pub mod bosun;
pub mod gh;
pub mod git;
pub mod hoist;
pub mod npm;
pub mod splice;

/// The program was not found (or could not start).
pub fn not_found(out: &Out) -> bool {
    out.code == -1 || out.code == 127
}

/// stdout parsed as one JSON value.
pub fn json(out: &Out) -> Option<Value> {
    serde_json::from_str(out.stdout.trim()).ok()
}

fn first_line(s: &str) -> &str {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
}

/// A failure detail that names the command (never its stdin) and says why, from the tool's JSON
/// `error` when it has one, else the first line of stderr or stdout.
pub fn failure(cmd: &Cmd, out: &Out) -> String {
    let why = match json(out).and_then(|v| v.get("error").cloned()) {
        Some(e) if e.is_object() => {
            let m = e["message"].as_str().unwrap_or("");
            let f = e["fix"].as_str().unwrap_or("");
            match (m.is_empty(), f.is_empty()) {
                (_, true) => m.to_string(),
                (true, false) => format!("fix: {f}"),
                (false, false) => format!("{m} (fix: {f})"),
            }
        }
        _ => {
            let s = first_line(&out.stderr);
            if s.is_empty() {
                first_line(&out.stdout)
            } else {
                s
            }
            .to_string()
        }
    };
    if not_found(out) {
        format!("`{}`: program not found; install it and rerun", cmd.line())
    } else if why.is_empty() {
        format!("`{}` exited {}", cmd.line(), out.code)
    } else {
        format!("`{}` exited {}: {why}", cmd.line(), out.code)
    }
}

/// The detail for a tool that exited 0 (or a documented code) without the JSON it promises.
pub fn not_json(cmd: &Cmd) -> String {
    format!(
        "`{}` printed no JSON object; update the tool or run it by hand to see why",
        cmd.line()
    )
}
