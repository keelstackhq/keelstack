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
    let mut elf = host_elf();
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
    // A NUL byte does not make a script a binary: only a valid ELF or Mach-O header does.
    std::fs::write(
        dir.join("n.sh"),
        "#!/bin/bash\ngit push --force origin main\n\0\n",
    )
    .unwrap();
    std::fs::write(dir.join("n"), "git push --force origin main\n\0\n").unwrap();
    for command in ["bash n.sh", "./n.sh", "./n", "bash tool"] {
        assert!(hook(dir, command).contains("\"deny\""), "{command}");
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

/// A minimal ELF header with one program header for this host (64-bit little-endian x86_64
/// or aarch64, the hosts CI runs on).
fn host_elf() -> Vec<u8> {
    let machine: u16 = if cfg!(target_arch = "aarch64") {
        183
    } else {
        62
    };
    let mut b = vec![0u8; 64 + 56];
    b[..8].copy_from_slice(b"\x7fELF\x02\x01\x01\0");
    b[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    b[18..20].copy_from_slice(&machine.to_le_bytes());
    b[20..24].copy_from_slice(&1u32.to_le_bytes());
    b[32..40].copy_from_slice(&64u64.to_le_bytes()); // e_phoff
    b[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
    b[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
    b[56..58].copy_from_slice(&1u16.to_le_bytes()); // e_phnum
    b[64..68].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    b
}

#[test]
fn text_behind_program_magic_is_read_as_a_script() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join(".bosun")).unwrap();
    std::fs::create_dir_all(dir.join("D")).unwrap();
    std::fs::write(dir.join(".bosun/version"), "0.9.0\n").unwrap();
    // The kernel refuses these (ENOEXEC), so bash or glibc's execvp hands them to sh.
    std::fs::write(dir.join("mpoly"), b"\xfe\xed\xfa\xce\ngh pr merge 1\n").unwrap();
    std::fs::write(dir.join("fat"), b"\xca\xfe\xba\xbe\ngh pr merge 1\n").unwrap();
    std::fs::write(dir.join("poly"), b"\x7fELF\ngh pr merge 1\n").unwrap();
    std::fs::write(dir.join("D/poly"), b"\x7fELF\ngh pr merge 1\n").unwrap();
    // A valid header whose only program header is an empty PT_INTERP: the kernel refuses it.
    let mut elfx = host_elf();
    elfx[64..68].copy_from_slice(&3u32.to_le_bytes());
    elfx.extend(b"\ngh pr merge 1\n");
    std::fs::write(dir.join("elfx"), elfx).unwrap();
    for command in [
        "./mpoly",
        "cd . && ./mpoly",
        "timeout 5 ./mpoly",
        "./fat",
        "timeout 5 ./fat",
        "env ./poly",
        "timeout 5 ./poly",
        "nice ./poly",
        "find . -name poly -exec {} \\;",
        "cd D && ./poly",
        "./poly",
        "env ./elfx",
        "timeout 60 ./elfx",
        "nohup ./elfx",
    ] {
        assert!(hook(dir, command).contains("\"deny\""), "{command}");
    }
    let mut real = host_elf();
    real.extend(b"gh pr merge 1\n");
    std::fs::write(dir.join("prog"), real).unwrap();
    for command in ["./prog", "timeout 5 ./prog", "cd . && ./prog"] {
        assert_eq!(hook(dir, command), "", "{command}");
    }
}
