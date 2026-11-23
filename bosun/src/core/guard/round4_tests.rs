//! Review round four of the Bash guard: regressions against main (every `find -exec` group is
//! judged, a bare pathspec glob can expand to a git option, a shell binary run directly, the
//! script's own directory after `cd "$(dirname "$0")/.."`) and a shell reading a dup'd fd.

use super::{Script, script_rules, tool_rules};
use serde_json::json;

/// The guard as the hook runs it: tool rules, then the scripts the command runs, read from
/// `files` (path as the hook asks for it, text); `bin` is a binary.
fn hook(cmd: &str, files: &[(&str, &str)]) -> Option<String> {
    tool_rules(
        "Bash",
        &json!({ "command": cmd }),
        &|_| Some("feature".into()),
        "main",
        "",
    )
    .or_else(|| {
        let read = |p: &str| {
            let p = p.trim_start_matches("./");
            if p == "bin" || p == "xl" || p == "target/debug/bosun" {
                return Some(Script::Binary);
            }
            files
                .iter()
                .find(|f| f.0 == p)
                .map(|f| Script::Text(f.1.to_string()))
        };
        script_rules(cmd, &read, &|_| Some("feature".into()), "main")
    })
}

const OK: &str = "echo hi\n";
const BAD: &str = "gh pr merge 1\n";

#[test]
fn every_find_exec_group_is_judged_and_no_shell_runs_a_found_file() {
    let files = [("y1.sh", BAD), ("x.sh", OK)];
    for cmd in [
        "find . -name y1.sh -exec sh -n {} \\; -exec ./y1.sh \\;",
        "find . -name y1.sh -exec sh -n {} \\; -exec env {} \\;",
        "find . -name y1.sh -exec sh -n {} + -exec env {} +",
        "find . -name y1.sh -exec sh -n {} \\; -exec bash {} \\;",
        "find . -name y1.sh -exec wc -l {} \\; -exec bash {} \\;",
        "find . -name y1.sh -exec wc -l {} + -ok gh pr merge 1 \\;",
        "find . -name y1.sh -execdir bash {} \\;",
        "find . -name y1.sh -ok bash {} \\;",
        "find . -name y1.sh -okdir bash {} \\;",
        "find . -name y1.sh -exec python3 {} \\;",
        "find . -name '*.sh' -exec bash -n {} \\;",
        "find . -name '*.sh' -exec sh -n {} \\;",
        "fd -e sh -x wc -l -X bash",
        "fd -e sh -X bash",
        "fd -e sh --exec-batch sh -n",
    ] {
        assert!(hook(cmd, &files).is_some(), "allowed: {cmd}");
    }
    for cmd in [
        "find . -name '*.rs' -exec wc -l {} \\;",
        "find . -name '*.rs' -exec grep -l x {} + -exec wc -l {} \\;",
        "find . -type f -name '*.tmp' -delete",
        "fd -e rs -x wc -l",
    ] {
        assert_eq!(hook(cmd, &files), None, "{cmd}");
    }
}

#[test]
fn a_bare_pathspec_glob_can_expand_to_a_git_option() {
    for cmd in [
        "git grep -e x *",
        "git log *",
        "git log */*",
        "git diff *",
        "git show *",
        "git add *",
        "git status *",
        "git ls-files *",
        "git blame *",
        "git restore *",
        "git rm *",
        "git add *.rs",
        "git ls-files *.rs | wc -l",
        "git diff -*",
        "git add $d/*",
        "git add ?/*",
        // A glob whose last part can be `git` (`*`) is refused in a segment naming git, as
        // on main (flipped from the allow list below).
        "git diff -- *",
        "git log --oneline -- src/*",
        "git status bosun/src/*",
        "git add ./*",
    ] {
        assert!(hook(cmd, &[]).is_some(), "allowed: {cmd}");
    }
    for cmd in [
        "git add src/*.rs",
        "git diff -- *.md",
        "git grep -e x -- '*.rs' *.md",
        "git ls-files docs/**/*.md",
    ] {
        assert_eq!(hook(cmd, &[]), None, "{cmd}");
    }
}

#[test]
fn a_shell_or_interpreter_binary_run_directly_is_refused() {
    for cmd in [
        "./xl -c 'gh pr merge 1'",
        "./bin -c 'gh pr merge 1'",
        "./bin -e 'system(\"gh pr merge 1\")'",
        "./bin --eval 'x'",
        "./bin -E 'x'",
        "./bin --command=x",
        "./bin -lc x",
        "./bash x.sh",
        "./python3 x.py",
        "tools/node x.js",
    ] {
        let files = [("bash", ""), ("python3", ""), ("tools/node", "")];
        let read_bin = |p: &str| {
            let p = p.trim_start_matches("./");
            (p == "bin" || p == "xl" || files.iter().any(|f| f.0 == p)).then_some(Script::Binary)
        };
        let why = tool_rules(
            "Bash",
            &json!({ "command": cmd }),
            &|_| Some("feature".into()),
            "main",
            "",
        )
        .or_else(|| script_rules(cmd, &read_bin, &|_| Some("feature".into()), "main"));
        assert!(why.is_some(), "allowed: {cmd}");
    }
    for cmd in [
        "./bin --version",
        "./bin -v",
        "cargo build && ./target/debug/bosun gate",
        "./target/debug/bosun hook pre-tool-use --cmd x",
    ] {
        assert_eq!(hook(cmd, &[]), None, "{cmd}");
    }
}

#[test]
fn the_scripts_own_directory_is_followed_through_dirname_with_a_suffix() {
    for (cd, inner) in [
        ("cd \"$(dirname \"$0\")/..\"", "s/g.sh"),
        ("cd \"$(dirname \"$0\")\"/..", "s/g.sh"),
        ("cd \"$(dirname \"${BASH_SOURCE[0]}\")/..\"", "s/g.sh"),
        ("cd \"$(dirname \"$0\")\"", "g.sh"),
        ("cd $(dirname $0)/..", "s/g.sh"),
    ] {
        let top = format!("#!/usr/bin/env bash\nset -euo pipefail\n{cd}\nbash {inner}\n");
        let ok = [("d/s/c.sh", top.as_str()), ("d/s/g.sh", OK)];
        assert_eq!(hook("bash d/s/c.sh", &ok), None, "{cd}");
        let bad = [("d/s/c.sh", top.as_str()), ("d/s/g.sh", BAD)];
        let why = hook("bash d/s/c.sh", &bad).unwrap_or_default();
        assert!(why.contains("d/s/g.sh"), "{cd}: {why}");
    }
}

#[test]
fn a_shell_reading_a_duplicated_fd_is_refused() {
    let files = [("g.txt", BAD)];
    for code in [
        "exec 3<g.txt; bash <&3\n",
        "exec 3<g.txt\nsh <&3\n",
        "exec 3< g.txt; bash 0<&3\n",
        "bash <&3\n",
        "python3 0<&4\n",
    ] {
        let script = [("t.sh", code), files[0]];
        assert!(hook("bash t.sh", &script).is_some(), "allowed: {code}");
        assert!(hook(code.trim_end(), &files).is_some(), "allowed: {code}");
    }
    let script = [("t.sh", "exec 3<g.txt\nwc -l <&3\necho hi >&2\n")];
    assert_eq!(hook("bash t.sh", &script), None);
}

#[test]
fn this_repos_index_check_runs_and_a_bad_snapshot_script_inside_it_is_refused() {
    // It uses `grep`, not a `case` glob (refused, as on main).
    let top = include_str!("../../../../decisions/scripts/check-index.sh");
    let ok = [
        ("decisions/scripts/check-index.sh", top),
        ("decisions/scripts/gen-snapshot.sh", OK),
    ];
    assert_eq!(hook("bash decisions/scripts/check-index.sh", &ok), None);
    let bad = [
        ("decisions/scripts/check-index.sh", top),
        ("decisions/scripts/gen-snapshot.sh", BAD),
    ];
    let why = hook("bash decisions/scripts/check-index.sh", &bad).unwrap_or_default();
    assert!(why.contains("gen-snapshot.sh"), "{why}");
}

#[test]
fn a_quoted_paren_in_a_substitution_does_not_hide_a_later_command() {
    for code in [
        "x=$(tr -d '()' < f)\ngh pr merge 1\n",
        "x=$(grep -oE '[^)]+' f); gh pr merge 1\n",
        "x=$(echo \"it's\"); gh pr merge 1\n",
        "a=(x\n y)\ngh pr merge 1\n",
        "a=(x \"$(gh pr merge 1)\")\n",
        "done < <(grep x f); gh pr merge 1\n",
    ] {
        let files = [("t.sh", code)];
        assert!(hook("bash t.sh", &files).is_some(), "allowed: {code}");
    }
}

#[test]
fn an_array_assignment_holds_no_command_word() {
    let code = "a=(--storage \"$s\" --port x\n\t--summary \"$w/s.md\")\na+=(--pack on)\necho \"${a[@]}\"\n";
    assert_eq!(hook("bash t.sh", &[("t.sh", code)]), None);
}

#[test]
fn what_the_guard_cannot_place_stays_refused() {
    // The repo root is a git query the pure core does not run: run the script by its path.
    assert!(
        hook(
            "bash \"$(git rev-parse --show-toplevel)/scripts/x.sh\"",
            &[("scripts/x.sh", OK)]
        )
        .is_some()
    );
    // A `cd` to an environment variable (`keel/src/proxy_ca.sh`) leaves the next relative
    // script unplaced.
    let code = "cd -- \"$KEEL_REPO_DIR\" 2>/dev/null || true\nbash ./scripts/p.sh\n";
    assert!(hook("bash t.sh", &[("t.sh", code), ("scripts/p.sh", OK)]).is_some());
    // A program named by a variable set from the environment (`hoard/tests/e2e/turbo.sh`).
    let code = "h=\"${HOARD_BIN:-}\"\n\"$h\" serve\n";
    assert!(hook("bash t.sh", &[("t.sh", code)]).is_some());
}
