//! The guard's public decisions (the no-regression rule): a call is denied when the new guard
//! denies it OR the legacy judge ([`super::legacy`], main's guard verbatim) denies it. The only
//! exception is a Bash command that is exactly one of the enumerated intended allows
//! ([`super::intended::allow`]); nothing else that the legacy judge denies may pass.

use serde_json::Value;

use super::{BranchAt, ChildTask, Push, Script, intended, legacy};

/// The command of a Bash call, when the call is one.
fn bash_command<'a>(tool: &str, input: &'a Value) -> Option<&'a str> {
    (tool == "Bash").then(|| input.get("command").and_then(Value::as_str).unwrap_or(""))
}

/// Command length up to which a legacy deny also runs the new rules for their reason.
const QUICK_COMMAND: usize = 2000;

/// Legacy deny (unless `command` is an enumerated intended allow), else the new decision.
///
/// The legacy judge runs first, so whatever main denies is denied as fast as main denies it.
fn either(
    new: impl FnOnce() -> Option<String>,
    old: impl FnOnce() -> Option<String>,
    command: Option<&str>,
) -> Option<String> {
    let old = old();
    if old.is_some() && !command.is_some_and(|c| intended::allow(c).is_some()) {
        // A short command (fast for the new rules) gets the new rules' more precise reason.
        if command.is_none_or(|c| c.len() <= QUICK_COMMAND) {
            return new().or(old);
        }
        return old;
    }
    new()
}

/// The legacy judge's view of a script the hook read: a binary is lossy text (as main read it),
/// or unreadable when larger than main's 1 MiB limit.
fn legacy_script(s: Script) -> legacy::Script {
    match s {
        Script::Text(t) | Script::Binary(Some(t)) => legacy::Script::Text(t),
        Script::Binary(None) => legacy::Script::Unreadable("larger than 1 MiB"),
        Script::Unreadable(why) => legacy::Script::Unreadable(why),
    }
}

/// Label-lock and merge-lock (see the new guard's `tool_rules_new`).
pub fn tool_rules(
    tool: &str,
    input: &Value,
    branch_at: BranchAt,
    default_branch: &str,
    own: &str,
) -> Option<String> {
    either(
        || super::tool_rules_new(tool, input, branch_at, default_branch, own),
        || legacy::tool_rules(tool, input, branch_at, default_branch, own),
        bash_command(tool, input),
    )
}

/// The script rule (see `interp::script_rules`).
pub fn script_rules(
    command: &str,
    read: &dyn Fn(&str) -> Option<Script>,
    branch_at: BranchAt,
    default_branch: &str,
) -> Option<String> {
    let old_read = |p: &str| read(p).map(legacy_script);
    either(
        || super::script_rules_new(command, read, branch_at, default_branch),
        || legacy::script_rules(command, &old_read, branch_at, default_branch),
        Some(command),
    )
}

/// Every push either guard reports (push-gates and push config check all of them).
pub fn pushes(
    command: &str,
    read: &dyn Fn(&str) -> Option<Script>,
    default_branch: &str,
) -> Vec<Push> {
    let old_read = |p: &str| read(p).map(legacy_script);
    let mut all = super::pushes_new(command, read, default_branch);
    for p in legacy::pushes(command, &old_read, default_branch) {
        let p = Push {
            dir: p.dir,
            srcs: p.srcs,
        };
        if !all.contains(&p) {
            all.push(p);
        }
    }
    all
}

/// A fetch config value either guard calls risky.
pub fn config_fetch_risk(key: &str, value: &str, default_branch: &str) -> bool {
    super::config_fetch_risk_new(key, value, default_branch)
        || legacy::config_fetch_risk(key, value, default_branch)
}

/// The files either guard reads as written by `command`.
pub fn bash_writes(command: &str) -> Vec<String> {
    let mut all = super::bash_writes_new(command);
    for w in legacy::bash_writes(command) {
        if !all.contains(&w) {
            all.push(w);
        }
    }
    all
}

/// Settings-lock.
pub fn settings_rule(tool: &str, input: &Value, cwd: &str) -> Option<String> {
    either(
        || super::settings_rule_new(tool, input, cwd),
        || legacy::settings_rule(tool, input, cwd),
        bash_command(tool, input),
    )
}

/// Subagent-lock.
pub fn subagent_lock(tool: &str, input: &Value) -> Option<String> {
    either(
        || super::subagent_lock_new(tool, input),
        || legacy::subagent_lock(tool, input),
        bash_command(tool, input),
    )
}

/// Answers-lock.
pub fn answers_lock(tool: &str, input: &Value) -> Option<String> {
    either(
        || super::answers_lock_new(tool, input),
        || legacy::answers_lock(tool, input),
        bash_command(tool, input),
    )
}

/// `.bosun` directory lock.
pub fn bosun_dir_lock(tool: &str, input: &Value) -> Option<String> {
    either(
        || super::bosun_dir_lock_new(tool, input),
        || legacy::bosun_dir_lock(tool, input),
        bash_command(tool, input),
    )
}

/// Prereqs-lock.
pub fn prereqs_lock(tool: &str, input: &Value) -> Option<String> {
    either(
        || super::prereqs_lock_new(tool, input),
        || legacy::prereqs_lock(tool, input),
        bash_command(tool, input),
    )
}

/// Prereqs-lock with the task marker allowed or not.
pub fn prereqs_lock_for(tool: &str, input: &Value, task_ok: bool) -> Option<String> {
    either(
        || super::prereqs_lock_for_new(tool, input, task_ok),
        || legacy::prereqs_lock_for(tool, input, task_ok),
        bash_command(tool, input),
    )
}

/// A registered child's task-issue create: both guards must read it as one.
pub fn child_task_create(tool: &str, input: &Value) -> Result<ChildTask, String> {
    let task = super::child_task_create_new(tool, input)?;
    legacy::child_task_create(tool, input)?;
    Ok(task)
}
