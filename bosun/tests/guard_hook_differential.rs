//! Differential test of the no-regression rule at the hook level: every probe from the review
//! rounds (PR #57, #60, #64 and #66 comments), the #66 E/F interpreter-named ELF cases and
//! Write/Edit/bash-write calls on locked files are judged by main's hook (the vendored legacy
//! judge, `hooks::pre_tool_use_legacy::judge`, on the same hook JSON) and by the real
//! `bosun hook pre-tool-use` entry, in a fixture checkout on a feature branch and on `main`.
//! Whatever main's hook denies, the live hook must deny, unless the call is an enumerated
//! intended allow. No input mapping is copied here: both judges read the fixture themselves.

mod common;

use std::path::Path;

use bosun::core::guard::intended;
use bosun::hooks::pre_tool_use::is_deny;
use bosun::hooks::pre_tool_use_legacy;
use serde_json::{Value, json};

const PROBES: &str = include_str!("../src/core/guard/differential_probes.json");
const BAD: &str = "#!/bin/sh\ngit push --force origin main\n";
const ELF: &[u8] =
    b"\x7fELF\x02\x01\x01\0\0 usage: git push --force origin main \0 gh pr merge 1 \0";

/// Reviewers E and F on #66: a host ELF named like an interpreter that carries protected lines,
/// run by inline interpreter code (main read it as lossy text and denied).
const ELF_INTERPRETER_PROBES: &[&str] = &[
    "python3 -c 'import os,sys; os.system(\"sh \"+sys.argv[1])' d/python3",
    "perl -e 'system(\"sh\", $ARGV[0])' d/python3",
    "python3 r.py ./python3",
    "python3 r.py d/python3",
    "perl -e 'system(\"sh\", $ARGV[0])' ./perl",
    "node -e 'require(\"child_process\").execFileSync(\"sh\",[process.argv[1]])' ./node",
    "bash ./sh",
    "./python3",
];

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "git {args:?}");
}

fn write(dir: &Path, rel: &str, bytes: &[u8]) {
    let f = dir.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, bytes).unwrap();
}

/// A checkout on `branch` with the scripts and programs the probes run.
fn fixture(branch: &str, bump: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    git(d, &["init", "-q", "-b", branch]);
    for s in ["evil.sh", "x.sh", "s.sh", "a.sh", "f.sh", "t.sh"] {
        write(d, s, BAD.as_bytes());
    }
    write(d, "ok.sh", b"echo ok\n");
    write(d, "scripts/bump-version.sh", bump.as_bytes());
    write(
        d,
        "decisions/scripts/check-index.sh",
        include_bytes!("../../decisions/scripts/check-index.sh"),
    );
    for b in ["bin", "h", "evil", "xl"] {
        write(d, b, ELF);
    }
    write(d, "r.py", b"import os,sys; os.system('sh '+sys.argv[1])\n");
    let host = ["/usr/bin/true", "/bin/true"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .expect("true");
    let mut elf = std::fs::read(host).unwrap();
    elf.extend_from_slice(b"\ngit push --force origin main\ngh pr merge 1\n");
    for p in ["python3", "d/python3", "perl", "node", "sh"] {
        write(d, p, &elf);
    }
    if let Some(g) = ["/usr/bin/git", "/bin/git"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
    {
        std::fs::copy(g, d.join("g")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(g, d.join("gl")).unwrap();
    }
    dir
}

fn bash(cmd: &str) -> Value {
    json!({"tool_name": "Bash", "tool_input": {"command": cmd}})
}

/// Calls on locked files and tools: settings, `.bosun`, lock files, subagent, label/answers.
fn tool_probes() -> Vec<Value> {
    let mut v = Vec::new();
    for path in [
        ".claude/settings.json",
        ".claude/settings.local.json",
        ".claude/hooks/x.sh",
        ".bosun/approvers.toml",
        ".bosun/answers/1.json",
        ".splice/lock.toml",
        ".github/workflows/ci.yml",
    ] {
        v.push(json!({"tool_name": "Write", "tool_input": {"file_path": path, "content": "x"}}));
        v.push(json!({"tool_name": "Edit", "tool_input":
            {"file_path": path, "old_string": "a", "new_string": "b"}}));
        v.push(json!({"tool_name": "MultiEdit", "tool_input":
            {"file_path": path, "edits": [{"old_string": "a", "new_string": "b"}]}}));
        for cmd in [
            format!("echo x > {path}"),
            format!("tee {path} < /dev/null"),
            format!("sed -i s/a/b/ {path}"),
            format!("cp ok.sh {path}"),
            format!("mv ok.sh {path}"),
            format!("printf x >> {path}"),
        ] {
            v.push(bash(&cmd));
        }
    }
    for cmd in [
        "gh pr merge 1",
        "bosun approve 1",
        "gh issue edit 1 --add-label bosun:state/merged",
        "gh issue create --title t --body '<!-- bosun:task 1 -->'",
    ] {
        let mut sub = bash(cmd);
        sub["agent_id"] = json!("agent_1");
        v.push(sub);
    }
    v
}

/// The live hook's output for `input`, through the real `bosun hook pre-tool-use` entry.
fn live(dir: &Path, input: &Value) -> Option<String> {
    let mut input = input.clone();
    input["cwd"] = json!(dir.to_string_lossy());
    let out = common::bosun()
        .current_dir(dir)
        .args(["hook", "pre-tool-use"])
        .write_stdin(input.to_string())
        .output()
        .expect("bosun hook");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn legacy_denies(dir: &Path, input: &Value) -> bool {
    pre_tool_use_legacy::judge(Some(dir), input).is_some_and(|o| is_deny(&o))
}

fn intended_call(input: &Value) -> bool {
    input["tool_name"] == "Bash"
        && intended::allow(input["tool_input"]["command"].as_str().unwrap_or("")).is_some()
}

#[test]
fn the_live_hook_denies_whatever_mains_hook_denies() {
    let bump = include_str!("../../scripts/bump-version.sh");
    let probes: Vec<String> = serde_json::from_str(PROBES).expect("probes");
    assert!(probes.len() > 1000, "probe set lost");
    let mut inputs: Vec<Value> = probes.iter().map(|p| bash(p)).collect();
    inputs.extend(ELF_INTERPRETER_PROBES.iter().map(|p| bash(p)));
    inputs.extend(tool_probes());
    for branch in ["feature", "main"] {
        let fx = fixture(branch, bump);
        let d = fx.path();
        // E and F's own cases (the first three) are ones main's hook denies.
        for p in &ELF_INTERPRETER_PROBES[..3] {
            assert!(legacy_denies(d, &bash(p)), "main's hook allows {p}");
        }
        let denied: Vec<&Value> = inputs
            .iter()
            .filter(|i| !intended_call(i) && legacy_denies(d, i))
            .collect();
        assert!(denied.len() > 500, "{branch}: only {} denied", denied.len());
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let leaks: Vec<String> = std::thread::scope(|s| {
            let chunk = denied.len().div_ceil(threads);
            let handles: Vec<_> = denied
                .chunks(chunk)
                .map(|c| {
                    s.spawn(move || {
                        c.iter()
                            .filter(|i| !live(d, i).as_deref().is_some_and(is_deny))
                            .map(|i| i.to_string())
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap())
                .collect()
        });
        assert!(
            leaks.is_empty(),
            "{branch}: the live hook allows what main's hook denies: {leaks:#?}"
        );
    }
}

#[test]
fn every_enumerated_allow_is_one_mains_hook_denies_and_the_live_hook_allows() {
    let bump = include_str!("../../scripts/bump-version.sh");
    let fx = fixture("feature", bump);
    let d = fx.path();
    for (name, cmd) in intended::EXAMPLES {
        assert_eq!(intended::allow(cmd), Some(*name), "{cmd}");
        assert!(
            legacy_denies(d, &bash(cmd)),
            "main allows {cmd}; drop `{name}`"
        );
        assert_eq!(live(d, &bash(cmd)), None, "{cmd}");
    }
}

/// Opus hunt on #66: the allow shapes. An edited bump script keeps main's verdict; `+nc` after
/// `-n` runs code; a glob after a `--` that is an option's value.
#[test]
fn the_allow_shapes_do_not_carry_main_denies() {
    let bump = include_str!("../../scripts/bump-version.sh");
    let edited = format!("{bump}\nbash +c -- '(/usr/bin/g?t push -f origin HEAD:main)'\n");
    let fx = fixture("feature", &edited);
    let d = fx.path();
    for cmd in [
        "bash scripts/bump-version.sh 1.2.3",
        "bash -n +nc ./evil.sh",
        "sh -n +cn evil.sh",
        "git blame --since -- *.md",
    ] {
        assert!(
            live(d, &bash(cmd)).as_deref().is_some_and(is_deny),
            "allowed: {cmd}"
        );
    }
}
