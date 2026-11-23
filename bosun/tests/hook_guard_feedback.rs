//! Consumer feedback on the Bash guard through the built `bosun hook pre-tool-use`: binaries
//! are programs (never read), text scripts are read at every level, and this repo's own
//! bump script passes.
mod common;

use common::bosun;
use serde_json::{Value, json};

fn hook(dir: &std::path::Path, command: &str) -> String {
    let input = json!({"session_id":"s","tool_name":"Bash","tool_input":{"command":command},
        "cwd":dir.to_str().unwrap()});
    let mut cmd = bosun();
    cmd.current_dir(dir).args(["hook", "pre-tool-use"]);
    let out = cmd.write_stdin(input.to_string()).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{command}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn reason(out: &str) -> String {
    let v: Value = serde_json::from_str(out).expect("one JSON object");
    v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn a_large_binary_runs_like_a_program_and_a_large_script_is_refused_everywhere() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join(".bosun")).unwrap();
    std::fs::write(dir.join(".bosun/version"), "0.9.0\n").unwrap();
    let mut elf = b"\x7fELF\x02\x01\x01\0".to_vec();
    elf.resize((1 << 20) + 10, 0);
    std::fs::write(dir.join("tool"), elf).unwrap();
    std::fs::write(dir.join("big.sh"), vec![b'#'; (1 << 20) + 10]).unwrap();
    std::fs::write(
        dir.join("ci.sh"),
        "#!/bin/bash\n./tool --test\nbash big.sh\n",
    )
    .unwrap();
    std::fs::write(dir.join("ci-bin.sh"), "#!/bin/bash\n./tool --test\n").unwrap();
    for command in ["./tool --version", "bash ci-bin.sh"] {
        assert_eq!(hook(dir, command), "", "{command}");
    }
    for command in ["bash big.sh", "./big.sh", "bash ci.sh"] {
        let why = reason(&hook(dir, command));
        assert!(
            why.contains("big.sh") && why.contains("cannot scan"),
            "{command}: {why}"
        );
    }
}

#[test]
fn a_script_written_and_run_by_one_command_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join(".bosun")).unwrap();
    std::fs::write(dir.join(".bosun/version"), "0.9.0\n").unwrap();
    let why = reason(&hook(dir, "printf 'echo hi\\n' > s.sh && bash s.sh"));
    assert!(why.contains("s.sh") && why.contains("writes it"), "{why}");
    std::fs::write(dir.join("s.sh"), "echo hi\n").unwrap();
    assert_eq!(hook(dir, "bash s.sh"), "");
}

#[test]
fn this_repos_bump_script_passes() {
    // A copy, so the hook's own state (repeat-lock) stays out of the checkout.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join(".bosun")).unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join(".bosun/version"), "0.9.0\n").unwrap();
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/bump-version.sh");
    std::fs::copy(script, dir.join("scripts/bump-version.sh")).unwrap();
    let command = "BUMP_DATE=2026-11-23 bash scripts/bump-version.sh 0.22.0";
    let out = hook(dir, command);
    assert_eq!(out, "", "{out}");
}
