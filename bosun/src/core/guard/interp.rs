//! Merge-lock inside interpreter code (spec §4): `python -c`, `node -e`, a heredoc fed to an
//! interpreter, and the script files they or a shell run. The code is not parsed: it is cut
//! into statements and words, and each `gh`/`git` word is judged like a shell command.

use super::{
    BranchAt, MAX_DEPTH, MERGE_FIX, RUNNERS, SEP, Sinks, Target, WRAPPERS, base, check, gh_rule,
    git_rule, heredoc_at, http_rule, literal_path, nested, shell_words, shell_words_seps,
    split_heredocs, strip_comments, unwrap_runners,
};

/// Interpreters and shells whose code or script the scan reads (a version suffix is dropped:
/// `python3.12` is `python`).
const RUNS: &[&str] = &[
    "python", "node", "nodejs", "ruby", "perl", "deno", "bun", "php", "tsx", "ts-node", "sh",
    "bash", "zsh", "dash", "ksh", "rbash", "mksh", "ash", "posh", "yash", "source", ".",
];
const SHELL_RUNS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ksh", "rbash", "mksh", "ash", "posh", "yash", "source", ".",
];

fn runner_name(word: &str) -> &str {
    let b = base(word);
    let t = b.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    if t.is_empty() { b } else { t }
}

/// Whether a word names an interpreter or shell this scan reads (`python3.12`, `tsx`).
pub(super) fn is_runner(word: &str) -> bool {
    RUNS.contains(&runner_name(word)) && word != "."
}

/// The command words of a command: the first word of each segment after assignments,
/// wrappers and runners with their option values (`sudo -u root`, `timeout -s KILL 5`, as
/// `unwrap_runners` reads them), and also, failing closed, after skipping every flag and number.
fn command_words(words: &[String]) -> Vec<usize> {
    command_words_at(words).into_iter().map(|c| c.0).collect()
}

/// [`command_words`], each with the directories its runners move to (`env -C d`; `None`:
/// unknown) and whether it is the word `unwrap_runners` found (not only a flag-skipped guess).
fn command_words_at(words: &[String]) -> Vec<(usize, Option<Vec<String>>, bool)> {
    let mut out: Vec<(usize, Option<Vec<String>>, bool)> = Vec::new();
    let mut s = 0;
    let mut moved_segs = Vec::new();
    for seg in words.split(|w| w == SEP) {
        let all: Vec<&str> = seg.iter().map(String::as_str).collect();
        let mut cmd: &[&str] = &all;
        let mut dirs: Vec<String> = Vec::new();
        loop {
            let wrapper = |w: &str| {
                (w.contains('=') && !w.starts_with('-'))
                    || (WRAPPERS.contains(&w) && !RUNNERS.iter().any(|r| r.0 == base(w)))
            };
            let skip = cmd.iter().take_while(|w| wrapper(w)).count();
            let (rest, d, _) = unwrap_runners(&cmd[skip..]);
            dirs.extend(d.iter().map(|x| (*x).to_string()));
            if rest.len() == cmd.len() {
                break;
            }
            cmd = rest;
        }
        if !dirs.is_empty() {
            moved_segs.push(s..s + seg.len());
        }
        if !cmd.is_empty() {
            out.push((s + all.len() - cmd.len(), Some(dirs), true));
        }
        s += seg.len() + 1;
    }
    for i in flag_skipped(words) {
        if !out.iter().any(|c| c.0 == i) {
            let moved = moved_segs.iter().any(|r| r.contains(&i));
            out.push((i, (!moved).then(Vec::new), false));
        }
    }
    out.sort_unstable_by_key(|c| c.0);
    out
}

fn flag_skipped(words: &[String]) -> Vec<usize> {
    let (mut out, mut start) = (Vec::new(), true);
    for (i, w) in words.iter().enumerate() {
        if w == SEP {
            start = true;
            continue;
        }
        let skip = (w.contains('=') && !w.starts_with('-'))
            || WRAPPERS.contains(&w.as_str())
            || RUNNERS.iter().any(|r| r.0 == base(w))
            || w.starts_with('-')
            || w.bytes().all(|c| c.is_ascii_digit());
        if start && !skip {
            out.push(i);
            start = false;
        }
    }
    out
}

pub(super) fn has_interpreter(command: &str) -> bool {
    let w = shell_words(command);
    command_words(&w).into_iter().any(|i| is_runner(&w[i]))
}

/// Shells and runners whose code the guard cannot read: not bash, or from where it cannot
/// see (`parallel` reads commands from its arguments and stdin).
const UNREAD: &[&str] = &[
    "fish",
    "tcsh",
    "csh",
    "pwsh",
    "powershell",
    "nu",
    "xonsh",
    "elvish",
    "ion",
    "rc",
    "es",
    "osh",
    "ysh",
    "oil",
    "murex",
    "parallel",
    "sem",
];

/// Indirection the guard cannot read through, at one level of shell code: a command name or a
/// `gh`/`git` subcommand built from a variable or substitution, an alias that names `gh`, `git`,
/// a shell or an interpreter, a shell the guard cannot read, and `make -f FILE`.
pub(super) fn indirect_rule(command: &str) -> Option<String> {
    let text = strip_comments(command);
    let code = mask_subs(&strip_comments(&split_heredocs(command).0));
    let w = shell_words(&code);
    let built = |a: &str| a.starts_with('$') || a.contains('`');
    if let Some(why) = code_settings(&w) {
        return Some(why);
    }
    let commands = command_words_at(&w);
    let funcs = functions(&w);
    for &(i, _, exact) in &commands {
        let word = w[i].as_str();
        let args: Vec<&str> = w[i + 1..]
            .iter()
            .take_while(|a| *a != SEP)
            .map(String::as_str)
            .collect();
        // `$HOME/.cargo/bin/cargo` names its program; `$X`, `${C:-cargo}` do not.
        let last = word.rsplit('/').next().unwrap_or(word);
        if exact && built(last) && !positional_ok(&w, i, &funcs, &commands, &text) {
            return Some(format!(
                "`{word}` is refused: a command name built from a variable or substitution can \
                 be `gh` or `git` with any arguments, which the guard cannot read; write the \
                 command name out"
            ));
        }
        let name = base(word);
        if UNREAD.contains(&name) || UNREAD.contains(&runner_name(word)) {
            return Some(format!(
                "`{name}` is refused: the guard cannot read the code it runs; run the commands \
                 themselves in bash"
            ));
        }
        // `xargs` is a runner, so its command is the word after it: look back for it.
        let seg_start = w[..i].iter().rposition(|x| x == SEP).map_or(0, |p| p + 1);
        let xargs = w[seg_start..i].iter().rposition(|x| base(x) == "xargs");
        let xargs_args: Vec<&str> = match xargs {
            Some(x) => w[seg_start + x + 1..]
                .iter()
                .take_while(|a| *a != SEP)
                .map(String::as_str)
                .collect(),
            None => Vec::new(),
        };
        if xargs.is_some() && xargs_runs_unread(&xargs_args) {
            return Some(
                "`xargs` is refused here: it runs a program named by its input (`{}`, a \
                 wrapper such as `env` or `timeout`, a shell), which the guard cannot read; run \
                 the commands themselves"
                    .to_string(),
            );
        }
        let sub = match name {
            "gh" => args.first().copied().filter(|a| built(a)).or_else(|| {
                args.get(1)
                    .copied()
                    .filter(|a| args[0] != "api" && built(a))
            }),
            "git" => git_subcommand(&args).filter(|a| built(a)),
            _ => None,
        };
        if let Some(sub) = sub {
            return Some(format!(
                "`{name} {sub}` is refused: a subcommand built from a variable or substitution \
                 can be one that merges, tags or pushes, which the guard cannot read; write the \
                 subcommand out"
            ));
        }
        // Another makefile than the repo's own: `-f`, `-C`/`--directory`, `--eval`, `MAKEFILES`.
        let other = |a: &&str| {
            [
                "-f",
                "--file",
                "--makefile",
                "-C",
                "--directory",
                "-E",
                "--eval",
            ]
            .iter()
            .any(|f| a.starts_with(f))
                && !a.starts_with("--file-")
        };
        if matches!(name, "make" | "gmake" | "bmake")
            && (args.iter().any(other) || code.contains("MAKEFILES"))
        {
            return Some(format!(
                "`{name}` with `-f`, `-C`, `--eval` or `MAKEFILES` is refused: the guard cannot \
                 read the makefile it is given; run `make` from the repo root with its own \
                 Makefile, or the commands themselves"
            ));
        }
        if name == "alias" {
            for a in &args {
                let Some((_, v)) = a.split_once('=') else {
                    continue;
                };
                let names = |x: &String| {
                    let b = base(x);
                    matches!(b, "gh" | "git" | "hub" | "eval" | "source" | "." | "exec")
                        || is_runner(x)
                        || UNREAD.contains(&b)
                        || WRAPPERS.contains(&b)
                        || RUNNERS.iter().any(|r| r.0 == b)
                };
                if v.contains(['$', '`']) || shell_words(v).iter().any(names) {
                    return Some(format!(
                        "`alias {a}` is refused: an alias that names `gh`, `git`, a shell or an \
                         interpreter hides the command it runs; run the command itself"
                    ));
                }
            }
        }
    }
    None
}

/// The functions defined in shell words (`f() {`, `function f {`, as `open_bodies` leaves
/// them): name and the word range of the body.
fn functions(w: &[String]) -> Vec<(String, std::ops::Range<usize>)> {
    let mut out = Vec::new();
    for (i, x) in w.iter().enumerate() {
        let name = match x.strip_suffix("()") {
            Some(n) if !n.is_empty() => n.trim_start_matches("function").trim().to_string(),
            _ if i > 0 && w[i - 1] == "function" => x.clone(),
            _ => continue,
        };
        let Some(open) = (i + 1..w.len()).find(|&k| w[k] != SEP) else {
            continue;
        };
        if w[open] != "{" {
            continue;
        }
        let mut depth = 0usize;
        for (k, y) in w.iter().enumerate().skip(open) {
            match y.as_str() {
                "{" => depth += 1,
                "}" => {
                    depth -= 1;
                    if depth == 0 {
                        out.push((name.clone(), open + 1..k));
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Whether a positional parameter as a command name (`"$@"`, `"$2"` in `apply() { shift;
/// "$@" <f; }`) runs only literal, harmless commands: it sits in a function of this text, and
/// at every call of that function the word it stands for (after the body's `shift`s) is a
/// plain command name, not `gh`, `git`, a shell, an interpreter, a wrapper or a variable.
fn positional_ok(
    w: &[String],
    i: usize,
    funcs: &[(String, std::ops::Range<usize>)],
    commands: &[(usize, Option<Vec<String>>, bool)],
    text: &str,
) -> bool {
    let n = match w[i].trim_start_matches('$').trim_matches(['{', '}']) {
        "@" | "*" => 0,
        d if d.len() == 1 && d != "0" && d.bytes().all(|b| b.is_ascii_digit()) => {
            usize::from(d.as_bytes()[0] - b'0') - 1
        }
        _ => return false,
    };
    let Some((name, body)) = funcs.iter().rev().find(|f| f.1.contains(&i)) else {
        return false;
    };
    // A `shift` in a loop runs any number of times: every argument may then be the command.
    let looped = w[body.clone()]
        .iter()
        .any(|x| matches!(x.as_str(), "while" | "until" | "for" | "select"));
    let mut shifts = 0;
    for k in body.start..i {
        if w[k] == "shift" {
            match w.get(k + 1).and_then(|v| v.parse::<usize>().ok()) {
                Some(m) => shifts += m,
                None => shifts += 1,
            }
        }
    }
    let at = n + shifts;
    // Every use of the name is a checked call (or the definition): a call inside `$( )`,
    // a `( )` group, a `trap`, a heredoc or `complete -C` would go unchecked.
    let calls = commands.iter().filter(|c| w[c.0] == *name).count();
    let token = |c: char| c.is_alphanumeric() || "_.-:+@".contains(c);
    let uses = text
        .split(|c: char| !token(c))
        .filter(|t| t == name)
        .count();
    if uses > calls + 1 {
        return false;
    }
    let safe = |a: &str| {
        let b = base(a);
        !(a.contains(['$', '`', '{', '}', '[', '?', '*'])
            || matches!(b, "gh" | "git" | "hub" | "eval" | "source" | "." | "exec")
            || is_runner(a)
            || UNREAD.contains(&b)
            || WRAPPERS.contains(&b)
            || RUNNERS.iter().any(|r| r.0 == b))
    };
    commands.iter().filter(|c| w[c.0] == *name).all(|c| {
        let args: Vec<&str> = w[c.0 + 1..]
            .iter()
            .take_while(|a| *a != SEP)
            .map(String::as_str)
            .collect();
        // `shift N` past the last argument shifts nothing: then any later one may run.
        if looped || args.len() < shifts {
            args.iter().skip(n).all(|a| safe(a))
        } else {
            args.get(at).is_none_or(|a| safe(a))
        }
    })
}

/// The text with each `$( )` and backtick span as `$S`: one word, judged on its own where it
/// runs (`nested`), so `X=$(cd a && pwd)` stays an assignment.
fn mask_subs(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let (mut out, mut i) = (String::new(), 0);
    while i < c.len() {
        if c[i] == '$' && c.get(i + 1) == Some(&'(') {
            let (mut level, mut j) = (1, i + 2);
            while j < c.len() && level > 0 {
                match c[j] {
                    '(' => level += 1,
                    ')' => level -= 1,
                    _ => {}
                }
                j += 1;
            }
            out.push_str("$S");
            i = j;
        } else if c[i] == '`' {
            let end = c[i + 1..]
                .iter()
                .position(|x| *x == '`')
                .map_or(c.len(), |e| i + 2 + e);
            out.push_str("$S");
            i = end;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

/// Variables that make a later command load or run code the guard cannot read (`NODE_OPTIONS`,
/// `BASH_ENV`, an editor or pager git runs), or move what a `$PWD`/`$HOME` path names; a plain
/// pager or editor name passes (`PAGER=cat`, `GIT_EDITOR=true`).
const CODE_VARS: &[&str] = &[
    "NODE_OPTIONS",
    "PERL5OPT",
    "RUBYOPT",
    "BASH_ENV",
    "ENV",
    "PYTHONSTARTUP",
    "PWD",
    "HOME",
    "GIT_EDITOR",
    "GIT_SEQUENCE_EDITOR",
    "GIT_EXTERNAL_DIFF",
    "GIT_PAGER",
    "GIT_SSH_COMMAND",
    "GIT_SSH",
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "PAGER",
    "EDITOR",
    "VISUAL",
    "GIT_CONFIG_PARAMETERS",
    "LD_PRELOAD",
    "PROMPT_COMMAND",
    "GIT_PROXY_COMMAND",
];
const PLAIN_TOOLS: &[&str] = &[
    "cat", "less", "more", "true", ":", "vi", "vim", "nvim", "nano", "emacs",
];

/// Whether a git config key makes git run a command (`core.fsmonitor`, `core.editor`,
/// `diff.external`, `filter.x.clean`, `core.hooksPath`, `credential.helper`, `pager.log`, ..).
fn runs_command_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    const ENDS: &[&str] = &[
        ".fsmonitor",
        "editor",
        ".pager",
        ".sshcommand",
        ".hookspath",
        ".askpass",
        ".external",
        ".textconv",
        ".command",
        ".driver",
        ".clean",
        ".smudge",
        ".process",
        ".program",
        ".helper",
        "packobjectshook",
        ".difffilter",
        ".gitproxy",
        ".proxycommand",
    ];
    ENDS.iter().any(|e| k.ends_with(e)) || k.starts_with("pager.") || k.starts_with("include")
}

/// Settings in shell words that make a command run code the guard cannot read: a `CODE_VARS`
/// assignment (also by `export`, `declare`, `read`, `printf -v`), and git config that runs a
/// command (`git -c core.fsmonitor=..`, `--config-env`, `GIT_CONFIG_KEY_n`, `git config KEY V`).
fn code_settings(w: &[String]) -> Option<String> {
    let plain = |v: &str| {
        let v = v.trim_matches(|c| c == '"' || c == '\'');
        let mut words = v.split(' ').filter(|x| !x.is_empty());
        v.is_empty()
            || !v.contains(['$', '`', ';', '|', '&', '(', '<', '>'])
                && words.next().is_some_and(|t| PLAIN_TOOLS.contains(&base(t)))
                && words.all(|x| x.starts_with('-'))
    };
    for seg in w.split(|x| x == SEP) {
        // Assignments before the command, or a builtin that sets variables.
        let lead = seg
            .iter()
            .take_while(|x| super::assigned(x).is_some())
            .count();
        let setter = seg.iter().any(|x| {
            matches!(
                x.as_str(),
                "read"
                    | "mapfile"
                    | "readarray"
                    | "-v"
                    | "export"
                    | "declare"
                    | "typeset"
                    | "local"
                    | "readonly"
            )
        });
        for (k, x) in seg.iter().enumerate() {
            let (name, value) = match x.split_once('=') {
                Some((n, v)) if super::assigned(x).is_some() => (n.trim_end_matches('+'), Some(v)),
                _ => (x.as_str(), None),
            };
            let named = CODE_VARS.contains(&name) && (k < lead || setter && k > 0);
            if named && !value.is_some_and(plain) {
                return Some(format!(
                    "setting `{name}` is refused: it makes a later command load, run or name \
                     code the guard cannot read; leave it unset, or run the command itself"
                ));
            }
            let key = x
                .strip_prefix("GIT_CONFIG_KEY_")
                .and_then(|r| r.split_once('='))
                .map(|(_, key)| key);
            if key.is_some_and(runs_command_key) {
                return Some(git_cmd_key(key.unwrap_or_default()));
            }
        }
        let Some(g) = seg.iter().position(|x| base(x) == "git") else {
            continue;
        };
        let args = &seg[g + 1..];
        let mut i = 0;
        while let Some(a) = args.get(i) {
            i += 1;
            let kv = match a.as_str() {
                "-c" | "--config-env" => args.get(i).inspect(|_| i += 1).map(String::as_str),
                _ => a
                    .strip_prefix("--config-env=")
                    .or_else(|| a.strip_prefix("-c").filter(|r| r.contains('='))),
            };
            if let Some(kv) = kv {
                let (key, v) = kv.split_once('=').unwrap_or((kv, ""));
                let env = a.starts_with("--config-env");
                if runs_command_key(key) && (env || !plain(v)) {
                    return Some(git_cmd_key(key));
                }
                continue;
            }
            if a.starts_with('-') {
                continue;
            }
            // `git config [opts] KEY VALUE` (also `set`, `--add`): stored for later commands.
            if a == "config" {
                let rest: Vec<&String> = args[i..].iter().filter(|x| !x.starts_with('-')).collect();
                let rest = match rest.first().map(|x| x.as_str()) {
                    Some("set" | "add") => &rest[1..],
                    _ => &rest[..],
                };
                if let [key, v, ..] = rest
                    && runs_command_key(key)
                    && !plain(v)
                {
                    return Some(git_cmd_key(key));
                }
            }
            break;
        }
    }
    None
}

fn git_cmd_key(key: &str) -> String {
    format!(
        "git config `{key}` is refused: it makes git run a command the guard cannot read; run \
         the command itself, and leave git's config to the owner"
    )
}

/// Whether `xargs ARGS` runs a program its input names: the command after xargs's options is
/// its `-I` placeholder, a wrapper or runner (`env`, `nice`, `timeout`, `nohup`), or a shell
/// or interpreter.
fn xargs_runs_unread(args: &[&str]) -> bool {
    let vals = RUNNERS
        .iter()
        .find(|r| r.0 == "xargs")
        .map_or(&[][..], |r| r.1);
    let (mut k, mut repl) = (0, None::<&str>);
    while let Some(a) = args.get(k) {
        k += 1;
        if let Some(r) = a.strip_prefix("-I").filter(|r| !r.is_empty()) {
            repl = Some(r);
        } else if *a == "-I" || a.starts_with("--replace") {
            repl = args.get(k).copied().or(Some("{}"));
            k += usize::from(*a == "-I");
        } else if vals.contains(a) {
            k += 1;
        } else if !a.starts_with('-') {
            let b = base(a);
            return Some(*a) == repl
                || *a == "{}"
                || repl.is_some_and(|r| a.contains(r))
                || WRAPPERS.contains(&b)
                || RUNNERS.iter().any(|r| r.0 == b)
                || is_runner(a)
                || UNREAD.contains(&b);
        }
    }
    false
}

/// The subcommand of `git ARGS`: the first word after git's global options and their values.
fn git_subcommand<'a>(args: &[&'a str]) -> Option<&'a str> {
    const VALUED: &[&str] = &[
        "-C",
        "-c",
        "--git-dir",
        "--work-tree",
        "--namespace",
        "--config-env",
        "--exec-path",
    ];
    let mut k = 0;
    while let Some(a) = args.get(k) {
        if VALUED.contains(a) {
            k += 2;
        } else if a.starts_with('-') {
            k += 1;
        } else {
            return Some(a);
        }
    }
    None
}

/// Whether a file's first bytes are a program this host's kernel runs itself, so no shell
/// reads it as a script: a structurally valid ELF executable for this machine (class, byte
/// order, version, type `EXEC` or `DYN`, machine, header sizes, program headers inside the
/// file), or on macOS a Mach-O executable for this CPU or a fat binary; and, as bash checks
/// before it runs a file as a script, a NUL byte in the first line. Anything else is text:
/// a kernel that refuses a file (`ENOEXEC`) leaves it to `sh`, which runs it as a script.
pub fn host_binary(buf: &[u8]) -> bool {
    let first_line = buf.split(|b| *b == b'\n').next().unwrap_or_default();
    first_line.contains(&0) && (elf_for_host(buf) || macho_for_host(buf))
}

fn elf_for_host(b: &[u8]) -> bool {
    let wide = cfg!(target_pointer_width = "64");
    let (class, ehsize, phentsize) = if wide { (2, 64, 56) } else { (1, 52, 32) };
    let little = cfg!(target_endian = "little");
    let u16_at = |o: usize| {
        let x = [*b.get(o).unwrap_or(&0), *b.get(o + 1).unwrap_or(&0)];
        if little {
            u16::from_le_bytes(x)
        } else {
            u16::from_be_bytes(x)
        }
    };
    let word_at = |o: usize| -> u64 {
        let n = if wide { 8 } else { 4 };
        let mut v = 0u64;
        for k in 0..n {
            let byte = u64::from(*b.get(o + k).unwrap_or(&0));
            v |= if little {
                byte << (8 * k)
            } else {
                byte << (8 * (n - 1 - k))
            };
        }
        v
    };
    let machine: u16 = if cfg!(target_arch = "x86_64") {
        62
    } else if cfg!(target_arch = "x86") {
        3
    } else if cfg!(target_arch = "aarch64") {
        183
    } else if cfg!(target_arch = "arm") {
        40
    } else if cfg!(any(target_arch = "riscv64", target_arch = "riscv32")) {
        243
    } else if cfg!(target_arch = "powerpc64") {
        21
    } else if cfg!(target_arch = "s390x") {
        22
    } else if cfg!(target_arch = "loongarch64") {
        258
    } else {
        return false;
    };

    if b.len() < ehsize || !b.starts_with(b"\x7fELF") {
        return false;
    }
    let (phoff, ph_at) = if wide {
        (word_at(32), 54)
    } else {
        (word_at(28), 42)
    };
    let phnum = u64::from(u16_at(ph_at + 2));
    b[4] == class
        && b[5] == if little { 1 } else { 2 }
        && b[6] == 1
        && matches!(u16_at(16), 2 | 3)
        && u16_at(18) == machine
        && u16_at(20) == 1
        && usize::from(u16_at(ph_at - 2)) == ehsize
        && usize::from(u16_at(ph_at)) == phentsize
        && phnum > 0
        && phnum * phentsize as u64 <= 65536
        && phoff >= ehsize as u64
        && phoff + phnum * phentsize as u64 <= b.len() as u64
        && program_headers_load(b, phoff as usize, phnum as usize, wide, &u16_at, &word_at)
}

/// The program header checks the kernel makes before it loads an ELF (else `ENOEXEC`, and
/// `sh` runs the file): at least one `PT_LOAD`, and a `PT_INTERP` of 2 to 4096 bytes inside
/// the file that ends in a NUL.
fn program_headers_load(
    b: &[u8],
    phoff: usize,
    phnum: usize,
    wide: bool,
    u16_at: &dyn Fn(usize) -> u16,
    word_at: &dyn Fn(usize) -> u64,
) -> bool {
    let size = if wide { 56 } else { 32 };
    let (off_at, filesz_at) = if wide { (8, 32) } else { (4, 16) };
    let u32_at = |o: usize| u32::from(u16_at(o)) | (u32::from(u16_at(o + 2)) << 16);
    let u32_at = |o: usize| {
        if cfg!(target_endian = "little") {
            u32_at(o)
        } else {
            (u32::from(u16_at(o)) << 16) | u32::from(u16_at(o + 2))
        }
    };
    let mut loads = 0;
    for k in 0..phnum {
        let h = phoff + k * size;
        match u32_at(h) {
            1 => loads += 1,
            3 => {
                let (off, len) = (word_at(h + off_at), word_at(h + filesz_at));
                let end = off.saturating_add(len);
                if !(2..=4096).contains(&len) || end > b.len() as u64 || b[end as usize - 1] != 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    loads > 0
}

fn macho_for_host(b: &[u8]) -> bool {
    if !cfg!(target_os = "macos") || b.len() < 32 {
        return false;
    }
    let le = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let be = |o: usize| u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let cpu: u32 = if cfg!(target_arch = "x86_64") {
        0x0100_0007
    } else if cfg!(target_arch = "aarch64") {
        0x0100_000c
    } else {
        return false;
    };

    // 64-bit Mach-O, executable (`MH_EXECUTE`), for this CPU.
    (le(0) == 0xfeed_facf && le(4) == cpu && le(12) == 2)
        // A fat binary: a small number of slices, one for this CPU.
        || (be(0) == 0xcafe_babe
            && (1..=8).contains(&be(4))
            && (0..be(4) as usize).any(|k| 8 + 20 * k + 4 <= b.len() && be(8 + 20 * k) == cpu))
}

/// The fix of a refusal: the same as the shell merge-lock, plus the owner.
fn refuse(what: &str, why: &str) -> String {
    format!(
        "{what} is refused ({why}): {MERGE_FIX}; anything else that merges, tags or releases is the owner's to run"
    )
}

/// Cut code into statements at newlines and `;` outside brackets (a call spread over lines
/// stays one statement), and at `&` and `|` anywhere (a shell chain inside a string).
fn statements(code: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0usize);
    for c in code.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            '&' | '|' => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            '\n' | ';' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

/// The words of a statement: quotes, commas and brackets split; `{x}` and `${x}` become `$x`,
/// so a computed push target stays unreadable (refused); object keys (`args:`) are dropped.
fn words(statement: &str) -> Vec<String> {
    let s = statement
        .replace("${", "$")
        .replace('{', "$")
        .replace('}', "");
    s.split(|c: char| !(c.is_alphanumeric() || "-_./:+@*=~$^".contains(c)))
        .filter(|w| !w.is_empty() && !w.ends_with(':'))
        .map(str::to_string)
        .collect()
}

/// Merge-lock over code: each `gh` or `git` word with the rest of its statement, and the
/// GitHub API rules of `curl` on each statement.
pub(super) fn code_rule(code: &str, target: &Target) -> Option<&'static str> {
    for st in statements(code) {
        if let Some(why) = http_rule(&st) {
            return Some(why);
        }
        let w = words(&st);
        for (i, word) in w.iter().enumerate() {
            let rest: Vec<&str> = w[i + 1..].iter().map(String::as_str).collect();
            let why = match base(word) {
                "gh" => gh_rule(&rest, &st),
                "git" => git_rule(&rest, target),
                _ => None,
            };
            if why.is_some() {
                return why;
            }
        }
    }
    None
}

/// The inline-code rule of one command level: when an interpreter runs, its code (`-c`, `-e`,
/// `eval`, a heredoc) is somewhere in the command text, so the whole text is scanned.
pub(super) fn inline_rule(command: &str, target: &Target) -> Option<String> {
    if !has_interpreter(command) {
        return None;
    }
    code_rule(command, target).map(|why| refuse("interpreter code", why))
}

/// A script path in a command: the path, whether a shell runs it, and whether it is the
/// script itself (the first operand or the command word) rather than a later argument.
struct ScriptPath {
    path: String,
    shell: bool,
    first: bool,
    /// Code the guard cannot see (a variable path, `bash -c "$(..)"`, `bash <<< "$X"`): why,
    /// and whether only the typed command is refused for it (a variable path in a script is a
    /// known miss).
    unseen: Option<(&'static str, bool)>,
    /// The command word itself (`./tool`): the kernel runs it, so a binary is a program.
    direct: bool,
    /// Run through a program (`env`, `timeout`, `nice`, ..) that hands a file the kernel
    /// refuses to `sh` (glibc `execvp`), not by bash itself.
    wrapped: bool,
    /// The directory it runs in, relative to the hook's (`None`: a `cd` the guard cannot tell).
    cwd: Cwd,
    /// A relative path after a `cd` the guard cannot tell: which file runs is unknown.
    lost: bool,
    /// A command in the same call that is not known to be read-only runs before it (or next to
    /// it, in a pipeline or the background): it may write the file after the hook reads it.
    tainted: bool,
}

impl ScriptPath {
    fn file(path: String, shell: bool, first: bool) -> Self {
        ScriptPath {
            path,
            shell,
            first,
            unseen: None,
            direct: false,
            wrapped: false,
            cwd: Some(String::new()),
            lost: false,
            tainted: false,
        }
    }
}

/// A command level's directory relative to the hook's (`""`), or `None` after a `cd` the guard
/// cannot follow.
type Cwd = Option<String>;

/// `path` as seen from the hook's directory when run in `cwd`.
fn join(cwd: &str, path: &str) -> String {
    if cwd.is_empty() || path.starts_with(['/', '~']) {
        return path.to_string();
    }
    format!(
        "{}/{}",
        cwd.trim_end_matches('/'),
        path.trim_start_matches("./")
    )
}

/// The directory a `cd target` from `cwd` moves to (logical, as bash's `cd` without `-P`).
fn move_to(cwd: &str, target: &str) -> String {
    let l = super::lexical(&join(cwd, target));
    if l == "." { String::new() } else { l }
}

/// The directory part of a path (`""` for a bare name).
fn dirname(path: &str) -> String {
    match path.rsplit_once('/') {
        Some(("", _)) => "/".into(),
        Some((d, _)) => d.into(),
        None => String::new(),
    }
}

/// The directory a script moves to with `cd "$(dirname "$0")"`, `cd "$(dirname
/// "${BASH_SOURCE[0]}")/.."` and the like, given the script's own directory.
fn own_dir_cd(word: &str, script_dir: Option<&str>) -> Option<String> {
    let dir = script_dir?;
    let rest = [
        "$(dirname $0)",
        "$(dirname ${0})",
        "$(dirname ${BASH_SOURCE[0]})",
        "$(dirname ${BASH_SOURCE})",
        "$(dirname $BASH_SOURCE)",
    ]
    .iter()
    .find_map(|p| word.strip_prefix(p))?;
    if !(rest.is_empty() || rest.starts_with('/')) {
        return None;
    }
    let rest = literal_path(&format!(".{rest}"))?;
    Some(move_to(dir, &rest))
}

/// The separator a run of `SEP`s stands for, newlines dropped (`&&`, `||`, `;`, `|`, `&`, ``).
fn sep_kind(run: &str) -> String {
    run.chars().filter(|c| *c != '\n').collect()
}

/// Whether the next command surely runs in this shell after the one before this separator.
fn sequential(run: &str) -> bool {
    matches!(sep_kind(run).as_str(), "" | ";" | "&&")
}

/// The commands of one level split at separators: the word ranges and the separator after each
/// (`""` after the last).
fn split_segments(w: &[String], seps: &[char]) -> (Vec<std::ops::Range<usize>>, Vec<String>) {
    let (mut segs, mut runs, mut run, mut k, mut i) = (Vec::new(), Vec::new(), String::new(), 0, 0);
    while i < w.len() {
        if w[i] == SEP {
            run.push(seps.get(k).copied().unwrap_or(';'));
            k += 1;
            i += 1;
            continue;
        }
        match runs.last_mut() {
            Some(r) => *r = std::mem::take(&mut run),
            None => run.clear(),
        }
        let start = i;
        while i < w.len() && w[i] != SEP {
            i += 1;
        }
        segs.push(start..i);
        runs.push(String::new());
    }
    (segs, runs)
}

/// Private stand-ins for blanks and separators inside `$( )` and backticks, so a substitution
/// stays one word (`root=$(cd .. && pwd)`); [`unprotect`] puts them back.
const PROTECT: [(char, char); 5] = [
    (' ', '\u{e000}'),
    ('\t', '\u{e001}'),
    (';', '\u{e002}'),
    ('|', '\u{e003}'),
    ('&', '\u{e004}'),
];

fn protect_subs(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let (mut out, mut i, mut level, mut tick) = (String::new(), 0, 0usize, false);
    let mut quote = false;
    while i < c.len() {
        let ch = c[i];
        match ch {
            '\'' if level == 0 && !tick => quote = !quote,
            '$' if !quote && c.get(i + 1) == Some(&'(') => {
                out.push_str("$(");
                level += 1;
                i += 2;
                continue;
            }
            '(' if level > 0 => level += 1,
            ')' if level > 0 => level -= 1,
            '`' if !quote => tick = !tick,
            _ => {}
        }
        let inside = (level > 0 || tick) && ch != '\n';
        out.push(match PROTECT.iter().find(|p| p.0 == ch) {
            Some(p) if inside => p.1,
            _ => ch,
        });
        i += 1;
    }
    out
}

fn unprotect(word: &str) -> String {
    word.chars()
        .map(|c| PROTECT.iter().find(|p| p.1 == c).map_or(c, |p| p.0))
        .collect()
}

/// `2>&1`, `>&2`, `<&0` and `&>f` are redirections: `shell_words` splits them at the `&`,
/// which is put back here.
fn rejoin_redirects(w: Vec<String>, seps: Vec<char>) -> (Vec<String>, Vec<char>) {
    let (mut out, mut out_seps, mut k) = (Vec::<String>::new(), Vec::new(), 0);
    let mut i = 0;
    while i < w.len() {
        if w[i] == SEP {
            let ch = seps.get(k).copied().unwrap_or(';');
            k += 1;
            let prev = out.last().filter(|p| *p != SEP);
            let next = w.get(i + 1).filter(|n| *n != SEP);
            if ch == '&'
                && prev.is_some_and(|p| p.ends_with(['>', '<']))
                && let Some(n) = next
            {
                let p = out.pop().unwrap_or_default();
                out.push(format!("{p}&{n}"));
                i += 2;
                continue;
            }
            if ch == '&'
                && let Some(n) = next.filter(|n| n.starts_with('>'))
            {
                out.push(format!("&{n}"));
                i += 2;
                continue;
            }
            out.push(SEP.to_string());
            out_seps.push(ch);
        } else {
            out.push(w[i].clone());
        }
        i += 1;
    }
    (out, out_seps)
}

/// Shell keywords that open or close a compound command; they write nothing.
const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "do", "done", "while", "until", "!", "{", "}", "esac",
    "in", "time", "case",
];

/// Whether a simple command is known to write no file (so it cannot change a script read
/// before the call runs): read-only tools, `cd`, `mkdir`, `touch` (no content), `cargo`
/// (it runs the repo's own build code, which the guard does not read anyway), with no output
/// redirect into a file and no substitution.
fn writes_nothing(seg: &[String]) -> bool {
    let mut i = 0;
    while let Some(x) = seg.get(i) {
        if KEYWORDS.contains(&x.as_str()) {
            i += 1;
            continue;
        }
        if let Some(n) = super::assigned(x) {
            if super::risky_var(n) {
                return false;
            }
            i += 1;
            continue;
        }
        break;
    }
    let words = &seg[i..];
    // `for x in WORDS` and `select` only assign.
    let words = match words.first().map(String::as_str) {
        Some("for" | "select") => &words[..0],
        _ => words,
    };
    let code = |x: &String| ["$(", "`", "<(", ">("].iter().any(|t| x.contains(t));
    if words.iter().any(code) || writes_a_file(words) {
        return false;
    }
    let Some(cmd) = words.first() else {
        return true;
    };
    let args: Vec<&str> = words[1..].iter().map(String::as_str).collect();
    match cmd.as_str() {
        "cd" | "pushd" | "popd" | "echo" | "true" | "false" | ":" | "test" | "[" | "[[" | "]]"
        | "pwd" | "ls" | "cat" | "head" | "tail" | "wc" | "grep" | "egrep" | "fgrep" | "jq"
        | "diff" | "which" | "sleep" | "date" | "basename" | "dirname" | "realpath"
        | "readlink" | "uname" | "id" | "whoami" | "mkdir" | "touch" | "shift" | "exit"
        | "return" | "break" | "continue" | "set" | "cargo" | "bosun" | "type" => true,
        "printf" => !args.iter().any(|a| a.starts_with("-v")),
        "command" => matches!(args.first(), Some(&("-v" | "-V"))),
        "export" | "readonly" | "local" | "declare" | "typeset" => args
            .iter()
            .all(|a| a.starts_with('-') || !super::risky_var(super::assigned(a).unwrap_or(a))),
        "git" => {
            let sub = args.iter().position(|a| !a.starts_with('-'));
            let sub = sub.map(|k| args[k]);
            matches!(
                sub,
                Some(
                    "status"
                        | "log"
                        | "diff"
                        | "show"
                        | "fetch"
                        | "rev-parse"
                        | "ls-files"
                        | "describe"
                        | "blame"
                        | "shortlog"
                        | "cat-file"
                        | "rev-list"
                        | "merge-base"
                )
            ) && !args
                .iter()
                .any(|a| a.starts_with("--output") || a.starts_with("--ext-diff") || *a == "-C")
        }
        "gh" => matches!(
            args.as_slice(),
            [
                "issue" | "pr" | "run" | "release" | "repo",
                "list" | "view" | "status" | "checks" | "diff",
                ..
            ]
        ),
        _ => false,
    }
}

/// Whether the words redirect output into a file (`> f`, `>>f`, `&> f`, `1>|f`); `>&2`,
/// `2>&1` and `/dev/null` write none.
fn writes_a_file(words: &[String]) -> bool {
    words.iter().enumerate().any(|(i, w)| {
        let Some(at) = w.find('>') else { return false };
        let after = w[at + 1..].trim_start_matches(['>', '|']);
        if let Some(fd) = after.strip_prefix('&') {
            return !(fd.is_empty() || fd == "-" || fd.bytes().all(|b| b.is_ascii_digit()));
        }
        let target = if after.is_empty() {
            words.get(i + 1).map_or("", String::as_str)
        } else {
            after
        };
        !matches!(target, "/dev/null" | "/dev/stdout" | "/dev/stderr")
    })
}

/// Whether every command of a text writes nothing (see [`writes_nothing`]).
fn text_writes_nothing(text: &str) -> bool {
    let w = shell_words(text);
    w.split(|x| x == SEP).all(writes_nothing)
}

/// The literal paths an interpreter or shell is given in a command (also a `./x` or `/x`
/// command word), at every nesting level (`bash -c '..'`, `$( )`), placed in the directory
/// each runs in. `script_dir`: the directory of the script this code is (`$0`), if any;
/// `tainted`: an outer level already writes before this code runs.
fn script_paths(
    command: &str,
    depth: usize,
    cwd: Cwd,
    script_dir: Option<&str>,
    tainted: bool,
) -> Vec<ScriptPath> {
    // A heredoc body no shell or interpreter reads is data (a commit message naming x.sh);
    // the substitutions of an unquoted body run.
    let (code, subs) = split_heredocs(&super::open_bodies(command));
    let code = strip_comments(&code);
    let (raw, seps) = shell_words_seps(&protect_subs(&code));
    let (raw, seps) = rejoin_redirects(raw, seps);
    let raw = raw.into_iter().map(|x| unprotect(&x)).collect();
    let w = leading_redirects_last(raw);
    let (segs, runs) = split_segments(&w, &seps);
    let dirty: Vec<bool> = segs
        .iter()
        .map(|r| !writes_nothing(&w[r.clone()]))
        .collect();
    // In a loop, a `trap` or a function body, a command may run again or later, after any
    // other: order says nothing then.
    let unordered = w.iter().any(|x| {
        matches!(
            x.as_str(),
            "while" | "until" | "for" | "select" | "trap" | "function"
        ) || x.ends_with("()")
    });
    // Whether a command that may write runs before segment `j` or next to it.
    let tainted_at = |j: usize| {
        let after = |i: usize| {
            let between = &runs[j..i];
            between
                .iter()
                .all(|r| matches!(sep_kind(r).as_str(), "|" | "|&"))
                || between.iter().any(|r| sep_kind(r) == "&")
        };
        let subst = w[segs[j].clone()].iter().any(|x| {
            ["$(", "`", "<(", ">("].iter().any(|t| x.contains(t))
                && !nested(x).iter().all(|n| text_writes_nothing(n))
        });
        let risky = w[segs[j].clone()]
            .iter()
            .map_while(|x| super::assigned(x))
            .any(|n| matches!(n, "PATH" | "BASH_ENV" | "ENV") || n.starts_with("LD_"));
        tainted
            || subst
            || risky
            || (0..j).any(|i| dirty[i])
            || (j + 1..segs.len()).any(|i| dirty[i] && (unordered || after(i)))
    };
    // The directory before each segment: a `cd`/`pushd` to a literal (or the script's own)
    // directory that surely runs in this shell moves it; any other `cd`, `pushd` or `popd`
    // (a variable, `-`, home, in a group, a loop, a condition, a pipeline) loses it.
    let mut at: Vec<Cwd> = Vec::new();
    let (mut cur, mut chained, mut open) = (cwd.clone(), false, 0i32);
    let mut moves = false;
    for (j, r) in segs.iter().enumerate() {
        at.push(cur.clone());
        let seg = &w[r.clone()];
        let prev = if j == 0 { "" } else { runs[j - 1].as_str() };
        let cds = |x: &String| {
            matches!(
                x.trim_start_matches(['(', '{']),
                "cd" | "pushd" | "popd" | "chdir"
            )
        };
        if seg.iter().any(cds) {
            moves = true;
            let own = seg.get(1).and_then(|t| own_dir_cd(t, script_dir));
            let args: Vec<&str> = seg[1..].iter().map(String::as_str).collect();
            let target = own
                .clone()
                .or_else(|| super::cd_target(&args).filter(|t| !t.starts_with('+')));
            let grouped = open > 0
                || seg
                    .iter()
                    .skip(usize::from(own.is_some()) * 2)
                    .any(|x| x.contains(['(', ')']));
            // Followed only through `&&`: after `;` a failed `cd` leaves the shell where it was.
            let clean = matches!(seg[0].as_str(), "cd" | "pushd")
                && sequential(prev)
                && matches!(sep_kind(&runs[j]).as_str(), "&&" | "")
                && !grouped;
            cur = match (cur, target, clean) {
                (_, Some(t), true) if own.is_some() => Some(t),
                (Some(c), Some(t), true) => Some(move_to(&c, &t)),
                _ => None,
            };
            chained = true;
        }
        for x in seg {
            open += x.matches('(').count() as i32 - x.matches(')').count() as i32;
        }
        if chained && !matches!(sep_kind(&runs[j]).as_str(), "&&" | "") {
            cur = None;
        }
    }
    let seg_of = |i: usize| segs.iter().position(|r| r.contains(&i)).unwrap_or(0);
    let place = |mut p: ScriptPath, cwd: &Cwd, j: usize| {
        if p.unseen.is_none() {
            match cwd {
                Some(c) => p.path = join(c, &p.path),
                None => p.lost = !p.path.starts_with(['/', '~']),
            }
        }
        p.cwd = cwd.clone();
        p.tainted = tainted_at(j);
        p
    };
    let mut out = Vec::new();
    for (i, dirs, _) in command_words_at(&w) {
        let j = seg_of(i);
        let here = match &dirs {
            Some(d) => d.iter().fold(at[j].clone(), |c, d| {
                let t = literal_path(d).filter(|_| d != "/")?;
                Some(move_to(&c?, &t))
            }),
            None => None,
        };
        moves |= dirs.as_ref().is_none_or(|d| !d.is_empty());
        let (word, name) = (&w[i], runner_name(&w[i]));
        if word.contains('/') && !RUNS.contains(&name) && pathish(word) {
            // Bash runs the command itself after keywords and its own builtins; any other word
            // before it is a program that runs it with `execvp`.
            let own = |x: &String| {
                super::assigned(x).is_some()
                    || matches!(
                        x.as_str(),
                        "command"
                            | "exec"
                            | "builtin"
                            | "time"
                            | "!"
                            | "{"
                            | "if"
                            | "then"
                            | "else"
                            | "elif"
                            | "do"
                            | "while"
                            | "until"
                    )
            };
            let wrapped = !w[segs[j].start..i].iter().all(own);
            let p = match script_word(word) {
                Some(path) => ScriptPath {
                    direct: true,
                    wrapped,
                    ..ScriptPath::file(path, false, true)
                },
                None => ScriptPath {
                    unseen: Some((BUILT_PATH, true)),
                    ..ScriptPath::file(word.clone(), false, true)
                },
            };
            out.push(place(p, &here, j));
        }
        let args = w[i + 1..].iter().take_while(|a| *a != SEP);
        if SHELL_RUNS.contains(&name) {
            // A shell runs only its first operand; later ones are the script's arguments.
            let all: Vec<String> = args.cloned().collect();
            let run = shell_run(name, &all).filter(|p| p.unseen.is_some() || pathish(&p.path));
            out.extend(run.map(|p| place(p, &here, j)));
        } else if RUNS.contains(&name) {
            let all: Vec<String> = args.cloned().collect();
            for p in interpreter_run(name, &all) {
                out.push(place(p, &here, j));
            }
        } else if word == "eval" && args.clone().any(|a| built(a)) {
            out.push(ScriptPath {
                unseen: Some((BUILT_CODE, false)),
                ..ScriptPath::file("eval".into(), true, true)
            });
        }
    }
    if depth < MAX_DEPTH {
        let inner_cwd = if moves { None } else { cwd };
        let mut runs = super::substitutions(&code, false);
        runs.extend(super::word_scripts(&code));
        for inner in nested(&code) {
            // Taint follows the segment the inner code sits in.
            let j = segs
                .iter()
                .position(|r| w[r.clone()].iter().any(|x| x.contains(inner.as_str())));
            let t = match j {
                Some(j) => tainted_at(j),
                None => tainted || dirty.iter().any(|d| *d),
            };
            let found = script_paths(&inner, depth + 1, inner_cwd.clone(), script_dir, t);
            // A group the shell does not run (inside quotes: awk or regex text) may name a
            // file: read when it is there, never refused for being missing or unplaceable.
            let runs = runs.contains(&inner);
            out.extend(found.into_iter().map(|mut p| {
                p.first &= runs;
                p
            }));
        }
        for inner in subs {
            let t = tainted || dirty.iter().any(|d| *d);
            out.extend(script_paths(
                &inner,
                depth + 1,
                inner_cwd.clone(),
                script_dir,
                t,
            ));
        }
    }
    out
}

const BUILT_PATH: &str = "its path is built when the command runs (a variable, a glob, a \
    substitution), which the guard cannot read; run it by a literal path";

/// The path a script word names: a literal path, `~/x`, `$HOME/x` or `${HOME}/x` (as `~/x`,
/// which the hook expands), or `$PWD/x` (relative to where it runs).
fn script_word(word: &str) -> Option<String> {
    if let Some(p) = literal_path(word) {
        return Some(p);
    }
    let w = word.trim_matches(|c| c == '"' || c == '\'');
    for (pre, to) in [
        ("~/", "~/"),
        ("$HOME/", "~/"),
        ("${HOME}/", "~/"),
        ("$PWD/", ""),
        ("${PWD}/", ""),
    ] {
        if let Some(rest) = w.strip_prefix(pre) {
            return literal_path(rest)
                .filter(|r| !r.starts_with('/'))
                .map(|r| format!("{to}{r}"));
        }
    }
    None
}

/// How an interpreter reads one option word: `Inline` (`-c CODE`, `-m MOD`: no script file),
/// `Load` (`-r FILE`: a file it runs first), `Value` (takes the next word unless attached),
/// or `Flag`.
#[derive(Clone, Copy, PartialEq)]
enum Opt {
    Inline,
    Load,
    Value,
    Flag,
    /// A value only when attached (`-Ilib`); alone, a flag.
    Attached,
}

/// One short option letter of an interpreter.
fn short_opt(name: &str, c: char) -> Opt {
    match (name, c) {
        ("python", 'c' | 'm') => Opt::Inline,
        ("python", 'W' | 'X') => Opt::Value,
        ("node" | "nodejs" | "deno" | "bun" | "tsx" | "ts-node", 'e' | 'p') => Opt::Inline,
        ("node" | "nodejs" | "deno" | "bun" | "tsx" | "ts-node", 'r') => Opt::Load,
        ("node" | "nodejs", 'C') => Opt::Value,
        ("perl", 'e' | 'E') => Opt::Inline,
        // Perl's option values are attached (`-Ilib`, `-x` alone is a flag).
        ("perl", 'I' | 'M' | 'm' | 'x' | 'l' | '0' | 'i' | 'C' | 'd' | 'D' | 'F') => Opt::Attached,
        ("ruby", 'e') => Opt::Inline,
        ("ruby", 'r') => Opt::Load,
        ("ruby", 'x' | 'K' | 'T' | '0' | 'i' | 'F') => Opt::Attached,
        ("ruby", 'I' | 'C' | 'E') => Opt::Value,
        ("php", 'r' | 'R' | 'B' | 'E') => Opt::Inline,
        ("php", 'f') => Opt::Load,
        ("php", 'd' | 'c' | 'z' | 't') => Opt::Value,
        _ => Opt::Flag,
    }
}

/// One long option of an interpreter (the name before any `=`).
fn long_opt(name: &str, o: &str) -> Opt {
    match o {
        "--eval" | "--print" | "--command" => Opt::Inline,
        "--require" | "--import" | "--loader" | "--experimental-loader" => Opt::Load,
        "--input-type" | "--conditions" | "--env-file" | "--title" => Opt::Value,
        "--check-hash-based-pycs" if name == "python" => Opt::Value,
        _ => Opt::Flag,
    }
}

/// The script files an interpreter runs: its first operand after options (none after inline
/// code or a module: `-c`, `-e`, `-m`, `--eval`), the files `-r`/`--require`/`--import`/
/// `--loader` load, a `< f` file for `-` or no operand; later operands (data) are listed as
/// not first. A built first operand is unseen.
fn interpreter_run(name: &str, args: &[String]) -> Vec<ScriptPath> {
    let mut out = Vec::new();
    let load = |v: &str, out: &mut Vec<ScriptPath>| {
        if v.contains('/') || script_name(v) {
            out.extend(script_word(v).map(|p| ScriptPath::file(p, false, true)));
        }
    };
    let (mut k, mut stdin, mut dash, mut operand) = (0, None::<String>, false, None::<usize>);
    // `deno run x.ts`, `bun run x.ts`: the subcommand is not the script.
    if matches!(name, "deno" | "bun") && args.first().is_some_and(|a| a == "run") {
        k = 1;
    }
    while let Some(a) = args.get(k) {
        k += 1;
        let r = a.trim_start_matches(|c: char| c.is_ascii_digit());
        if r.starts_with(['<', '>']) || r.starts_with("&>") {
            let op = r.chars().take_while(|c| "<>&|".contains(*c)).count();
            let target = if r.len() > op {
                r[op..].to_string()
            } else {
                k += 1;
                args.get(k - 1).cloned().unwrap_or_default()
            };
            if &r[..op] == "<" && a.len() == r.len() {
                stdin = Some(target);
            }
            continue;
        }
        if operand.is_some() {
            continue;
        }
        if a == "-" {
            dash = true;
            operand = Some(k - 1);
            continue;
        }
        if a == "--" {
            operand = args.get(k).map(|_| k);
            k += 1;
            continue;
        }
        if let Some(long) = a.strip_prefix("--").map(|_| a.as_str()) {
            let (o, v) = long
                .split_once('=')
                .map_or((long, None), |(o, v)| (o, Some(v)));
            match long_opt(name, o) {
                Opt::Inline => return out,
                Opt::Load => {
                    let v = v.map(str::to_string).or_else(|| {
                        k += 1;
                        args.get(k - 1).cloned()
                    });
                    if let Some(v) = v {
                        load(&v, &mut out);
                    }
                }
                Opt::Value if v.is_none() => k += 1,
                _ => {}
            }
            continue;
        }
        if let Some(cluster) = a.strip_prefix('-').filter(|c| !c.is_empty()) {
            for (at, c) in cluster.char_indices() {
                let rest = cluster[at + c.len_utf8()..].trim_start_matches('=');
                match short_opt(name, c) {
                    Opt::Inline => return out,
                    Opt::Flag => continue,
                    Opt::Attached => break,
                    opt => {
                        let v = if rest.is_empty() {
                            k += 1;
                            args.get(k - 1).cloned()
                        } else {
                            Some(rest.to_string())
                        };
                        if let (Opt::Load, Some(v)) = (opt, v) {
                            load(&v, &mut out);
                        }
                        break;
                    }
                }
            }
            continue;
        }
        operand = Some(k - 1);
    }
    let Some(at) = operand.filter(|_| !dash) else {
        // `-` or no operand: the script is stdin; a `< f` file is read as it.
        if let Some(f) = stdin {
            out.push(match script_word(&f) {
                Some(p) => ScriptPath::file(p, false, true),
                None => ScriptPath {
                    unseen: Some((BUILT_PATH, true)),
                    ..ScriptPath::file(f, false, true)
                },
            });
        }
        return out;
    };
    let a = &args[at];
    let first = match script_word(a) {
        // `bun run build` runs a package script, not a file.
        Some(path) if name == "bun" && at == 1 && !script_name(&path) => return out,
        Some(path) => ScriptPath::file(path, false, true),
        None => ScriptPath {
            unseen: Some((BUILT_PATH, true)),
            ..ScriptPath::file(a.clone(), false, true)
        },
    };
    out.push(first);
    for later in args[at + 1..]
        .iter()
        .filter(|a| !a.starts_with(['-', '<', '>']))
        .take(3)
    {
        out.extend(script_word(later).map(|p| ScriptPath::file(p, false, false)));
    }
    out
}

const BUILT_CODE: &str = "its code is built when the command runs (`$(..)`, a backtick or a \
    variable), which the guard cannot read";

/// Whether a code word is built at run time: a substitution, or one variable (`"$X"`).
fn built(word: &str) -> bool {
    let w = word.trim_matches(|c| c == '"' || c == '\'');
    let var = w.strip_prefix('$').is_some_and(|v| {
        let v = v.trim_start_matches('{').trim_end_matches('}');
        !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    });
    !word.starts_with('\'') && (w.contains("$(") || w.contains('`') || var)
}

/// Each segment with its leading redirections (`< f bash`, `<f sh -s`) moved after the
/// command, where `shell_run` reads them.
fn leading_redirects_last(words: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for (n, seg) in words.split(|w| w == SEP).enumerate() {
        if n > 0 {
            out.push(SEP.to_string());
        }
        let (mut moved, mut k) = (Vec::new(), 0);
        while let Some(w) = seg.get(k) {
            let r = w.trim_start_matches(|c: char| c.is_ascii_digit());
            if !(r.starts_with(['<', '>']) || r.starts_with("&>")) {
                break;
            }
            moved.push(w.clone());
            let op = r.chars().take_while(|c| "<>&|".contains(*c)).count();
            if r.len() == op {
                moved.extend(seg.get(k + 1).cloned());
                k += 1;
            }
            k += 1;
        }
        out.extend(seg[k.min(seg.len())..].iter().cloned());
        out.extend(moved);
    }
    out
}

/// What a shell runs: the file of its first operand after options (`-o`/`-O`/`+o`/`+O` and
/// `--rcfile`/`--init-file` take a value, `--` ends them; `source` and `.` take no options);
/// with `-c`, its code string (judged as a nested command, refused when built); with no
/// operand or `-s`, its stdin: a `< f` file is read as the script, a here-string with `$` or a
/// substitution is refused. A pipe into a shell is refused elsewhere.
fn shell_run(name: &str, args: &[String]) -> Option<ScriptPath> {
    // Exactly `sh -n FILE` only parses; any other option may turn running back on (`+n`).
    if let [n, f] = args
        && n == "-n"
        && !f.starts_with(['-', '+', '<'])
    {
        return None;
    }
    let (mut opts, mut i) = (!matches!(name, "source" | "."), 0);
    let (mut file, mut code, mut expect_code, mut stdin_mode) = (None, None, false, false);
    let (mut stdin, mut here) = (None::<&str>, None::<&str>);
    while i < args.len() {
        let a = args[i].as_str();
        i += 1;
        let r = a.trim_start_matches(|c: char| c.is_ascii_digit());
        let fd0 = a.len() == r.len() || a.starts_with('0');
        if r.starts_with(['<', '>']) || r.starts_with("&>") {
            let op = r.chars().take_while(|c| "<>&|".contains(*c)).count();
            let target = if r.len() > op {
                &r[op..]
            } else {
                i += 1;
                args.get(i - 1).map_or("", String::as_str)
            };
            match &r[..op] {
                "<<<" if fd0 => here = Some(target),
                "<" if fd0 => stdin = Some(target),
                _ => {}
            }
            continue;
        }
        if file.is_some() || code.is_some() || stdin_mode {
            continue;
        }
        if expect_code {
            code = Some(a);
            continue;
        }
        if opts {
            if a == "--" {
                opts = false;
                continue;
            }
            if matches!(a, "--rcfile" | "--init-file") {
                i += 1;
                continue;
            }
            if a.starts_with("--") {
                continue;
            }
            if a.len() > 1 && a.starts_with(['-', '+']) {
                expect_code |= a[1..].contains('c');
                stdin_mode |= a[1..].contains('s');
                if a[1..].contains(['o', 'O']) {
                    i += 1;
                }
                continue;
            }
        }
        file = Some(a);
    }
    let unseen = |why, top_only| ScriptPath {
        unseen: Some((why, top_only)),
        ..ScriptPath::file(name.to_string(), true, true)
    };
    if expect_code {
        return code.filter(|c| built(c)).map(|_| unseen(BUILT_CODE, false));
    }
    if let Some(f) = file.filter(|_| !stdin_mode) {
        return match literal_path(f) {
            Some(path) => Some(ScriptPath::file(path, true, true)),
            None => Some(unseen(
                "its script path is built when the command runs (a variable, a glob), which \
                 the guard cannot read",
                true,
            )),
        };
    }
    if let Some(h) = here {
        return (built(h) || h.contains('$')).then(|| unseen(BUILT_CODE, false));
    }
    let f = stdin?;
    Some(match literal_path(f) {
        Some(path) => ScriptPath::file(path, true, true),
        None => unseen(
            "it reads its script from a file the guard cannot name",
            false,
        ),
    })
}

/// What the hook found at a script path.
pub enum Script {
    /// The text (lossy UTF-8) of a regular file of at most 1 MiB.
    Text(String),
    /// A file the hook will not read in full: why (too large, not a regular file, /proc).
    Unreadable(&'static str),
    /// An ELF or Mach-O program: run directly like one on PATH, never read; refused when a
    /// shell or interpreter is told to run it.
    Binary,
}

const STDIN: &[&str] = &["/dev/stdin", "/dev/fd/0", "/proc/self/fd/0"];

/// Whether a heredoc feeds the stdin of the command that names `path`: one on that command
/// itself, or on an earlier stage of the same pipeline (`cat <<'EOF' | python3 /dev/stdin`).
/// Pipelines end at `;`, `&&`, `||`, a newline and a lone `&`; the `&` of `>&`, `<&` and `&>`
/// is part of a redirection.
fn heredoc_feeds(command: &str, path: &str) -> bool {
    let c: Vec<char> = command.chars().collect();
    let (mut pipelines, mut cur) = (Vec::new(), String::new());
    let mut i = 0;
    while i < c.len() {
        let (ch, next, prev) = (c[i], c.get(i + 1).copied(), i.checked_sub(1).map(|p| c[p]));
        let redir_amp = ch == '&' && (matches!(prev, Some('>' | '<')) || next == Some('>'));
        let ends = match ch {
            ';' | '\n' => 1,
            '&' if next == Some('&') => 2,
            '|' if next == Some('|') => 2,
            '&' if !redir_amp => 1,
            _ => 0,
        };
        if ends > 0 {
            pipelines.push(std::mem::take(&mut cur));
            i += ends;
            continue;
        }
        cur.push(ch);
        i += 1;
    }
    pipelines.push(cur);
    pipelines.iter().any(|p| {
        let stages: Vec<&str> = p.split('|').collect();
        stages
            .iter()
            .position(|s| s.contains(path))
            .is_some_and(|at| {
                let feeds = |(i, s): (usize, &&str)| {
                    heredoc_at(s).is_some() && !input_redirect(s) && (i == at || heredoc_only(s))
                };
                stages[..=at].iter().enumerate().any(feeds)
            })
    })
}

/// Whether a stage has an input redirection besides its heredoc (`< f`, `0< f`, `<<< s`),
/// which may replace the heredoc on stdin.
fn input_redirect(stage: &str) -> bool {
    shell_words(stage).iter().any(|w| {
        let w = w.trim_start_matches(|c: char| c.is_ascii_digit());
        w.starts_with("<<<") || (w.starts_with('<') && !w.starts_with("<<"))
    })
}

/// Whether a stage with a heredoc reads only it: no operand after the command word besides
/// options and redirections (`cat <<'EOF' evil.py` prints evil.py too).
fn heredoc_only(stage: &str) -> bool {
    let (mut skip, mut operands) = (false, 0);
    for w in shell_words(stage).iter().skip(1) {
        let glued = w.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
        let op = glued.trim_start_matches(['<', '>', '-']);
        if std::mem::take(&mut skip) {
            continue;
        }
        if glued.starts_with(['<', '>']) {
            skip = op.is_empty() && !glued.starts_with(">&");
        } else if !w.starts_with('-') {
            operands += 1;
        }
    }
    operands == 0
}

/// Whether a word can be a path that runs: not a redirection (`>/dev/null`, `2>/tmp/x`) and
/// not the edge of a group (`(./x`, `./x)`), whose inner command `nested` reads on its own.
fn pathish(word: &str) -> bool {
    let r = word.trim_start_matches(|c: char| c.is_ascii_digit());
    !(r.starts_with(['<', '>', '&']) || word.starts_with('(') || word.ends_with(')'))
}

/// Whether a path names a script file by its extension (`x.py`, `gen.mjs`), not a module
/// (`python3 -m pytest`) or inline code.
fn script_name(path: &str) -> bool {
    const EXT: &[&str] = &[
        "py", "js", "mjs", "cjs", "ts", "mts", "cts", "tsx", "jsx", "rb", "pl", "pm", "php", "sh",
        "bash", "zsh", "ksh",
    ];
    path.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && !stem.ends_with('/') && EXT.contains(&ext))
        && !path.contains([' ', '(', '\'', '"', ';'])
}

/// A `#!` line that runs a shell, also with arguments (`#!/bin/bash -e`, `#!/usr/bin/env sh`).
fn shell_shebang(first: &str) -> bool {
    first.strip_prefix("#!").is_some_and(|l| {
        l.split_whitespace()
            .any(|w| SHELL_RUNS.contains(&runner_name(w)))
    })
}

/// The script rule: each script file an interpreter or shell runs, read through `read`
/// (`None`: no such file, skipped), is scanned like inline code; a shell script is also judged
/// by the whole shell merge-lock. A script the hook cannot read in full is refused (a later
/// argument, which may be data, is skipped).
pub fn script_rules(
    command: &str,
    read: &dyn Fn(&str) -> Option<Script>,
    branch_at: BranchAt,
    default_branch: &str,
) -> Option<String> {
    script_walk(command, read, branch_at, default_branch, None)
}

/// [`script_rules`], reporting each allowed push to `sinks`.
pub(super) fn script_walk(
    command: &str,
    read: &dyn Fn(&str) -> Option<Script>,
    branch_at: BranchAt,
    default_branch: &str,
    sinks: Option<&Sinks>,
) -> Option<String> {
    let mut seen = Vec::new();
    let at = Some(String::new());
    walk(
        command,
        read,
        branch_at,
        default_branch,
        sinks,
        &mut seen,
        (at, None),
    )
}

/// How deep scripts run scripts before the guard stops reading (and refuses).
const SCRIPT_DEPTH: usize = 4;

/// One level of [`script_walk`]: the scripts `command` runs; a shell script's own scripts are
/// read the same way (`seen` holds the chain, so a script that runs itself is read once).
fn walk(
    command: &str,
    read: &dyn Fn(&str) -> Option<Script>,
    branch_at: BranchAt,
    default_branch: &str,
    sinks: Option<&Sinks>,
    seen: &mut Vec<String>,
    (cwd, script_dir): (Cwd, Option<&str>),
) -> Option<String> {
    let target = Target {
        dir: Some(&[]),
        alone: false,
        branch_at,
        default_branch,
        sink: sinks.map(|s| s.code),
    };
    // A non-interactive shell expands aliases only after `shopt -s expand_aliases`; an alias
    // hides the command it runs.
    if command.contains("expand_aliases") {
        return Some(
            "`shopt -s expand_aliases` is refused: an alias hides the command it runs; run \
             the command itself"
                .into(),
        );
    }
    let writes: Vec<String> = super::bash_writes(command)
        .iter()
        .map(|w| w.trim_start_matches("./").to_string())
        .collect();
    for ScriptPath {
        path,
        shell,
        first,
        unseen,
        direct,
        wrapped,
        cwd: run_in,
        lost,
        tainted,
    } in script_paths(command, 0, cwd, script_dir, false)
    {
        if let Some((why, top_only)) = unseen {
            if !top_only || seen.is_empty() {
                return Some(format!(
                    "`{path}` is refused: {why}; write the script to a file, read it, then run \
                     the file"
                ));
            }
            continue;
        }
        // stdin by name, fed by a heredoc: the body is in the command text, which
        // `inline_rule` scans. A heredoc in another command does not count.
        if STDIN.contains(&path.as_str()) && heredoc_feeds(command, &path) {
            continue;
        }
        if lost && !first {
            continue;
        }
        if lost {
            return Some(format!(
                "the script `{path}` is refused: a `cd` before it moves to a directory the guard \
                 cannot tell, so it cannot read the file that runs; run the script by its \
                 absolute path, or `cd` to a literal directory first"
            ));
        }
        // The hook reads the file before the command runs: one this command writes is read
        // stale or not at all.
        let bare = path.trim_start_matches("./");
        if first
            && writes
                .iter()
                .any(|w| w == bare || path.ends_with(&format!("/{w}")))
        {
            return Some(format!(
                "the script `{path}` is refused: this command writes it, so the guard cannot read \
                 what will run; write the file first, then run the script in a separate call"
            ));
        }
        if seen.contains(&path) {
            continue;
        }
        let found = read(&path);
        if first && tainted && matches!(found, Some(Script::Text(_) | Script::Binary)) {
            return Some(format!(
                "the script `{path}` is refused: a command before it in this call (or next to \
                 it in a pipeline or the background) is not known to be read-only and may \
                 change the file after the guard reads it; run the script in a separate call"
            ));
        }
        let code = match found {
            // Missing when the hook runs: the command makes it (`curl -o`, `tar x`, `python`),
            // so it cannot be read. Inside a script a path may be made by an earlier step
            // (skipped).
            None if first && seen.is_empty() => {
                return Some(format!(
                    "the script `{path}` is refused: it does not exist when the guard runs, so \
                     whatever this command makes there cannot be read; create it first, read \
                     it, then run it in a separate call"
                ));
            }
            None => continue,
            Some(Script::Binary) if direct && !wrapped => continue,
            Some(Script::Binary) if direct => {
                return Some(format!(
                    "the program `{path}` is refused: run through another program (`env`, \
                     `timeout`, `nice`, ..), a file the kernel refuses is handed to `sh`, and the \
                     guard does not read programs; run it directly (`./x`), or ask the owner"
                ));
            }
            // A shell or interpreter told to run a binary may run text inside it (`perl -x`
            // skips to a `#!` line): not read, so refused.
            Some(Script::Binary) => {
                return Some(format!(
                    "the script `{path}` is refused: a shell or interpreter is told to run a \
                     binary, which the guard cannot read as a script; run the program directly"
                ));
            }
            Some(Script::Text(code)) => code,
            Some(Script::Unreadable(_)) if !first => continue,
            Some(Script::Unreadable(why)) => {
                return Some(format!(
                    "the script `{path}` is refused: the guard cannot scan it ({why}); run a \
                     script file of at most 1 MiB, or ask the owner to run it"
                ));
            }
        };
        if let Some(why) = super::identity_tamper(&code, "") {
            return Some(format!("in the script `{path}`: {why}"));
        }
        let line = code.lines().next().unwrap_or("");
        let shell =
            shell || path.ends_with(".sh") || path.ends_with(".bash") || shell_shebang(line);
        if shell && let Some(why) = check(&code, branch_at, default_branch, MAX_DEPTH / 2, sinks) {
            return Some(format!("in the script `{path}`: {why}"));
        }
        if let Some(why) = code_rule(&code, &target) {
            return Some(refuse(&format!("the script `{path}`"), why));
        }
        if shell {
            if seen.len() >= SCRIPT_DEPTH {
                return Some(format!(
                    "the script `{path}` is refused: scripts run scripts more than \
                     {SCRIPT_DEPTH} deep, which the guard does not read; run an inner script \
                     directly"
                ));
            }
            seen.push(path.clone());
            let own = dirname(&path);
            let at = (run_in, Some(own.as_str()));
            let why = walk(&code, read, branch_at, default_branch, sinks, seen, at);
            seen.pop();
            if let Some(why) = why {
                return Some(format!("in the script `{path}`: {why}"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::tool_rules;
    use super::*;
    use serde_json::json;

    fn bash(cmd: &str) -> Option<String> {
        tool_rules(
            "Bash",
            &json!({ "command": cmd }),
            &|_| Some("feature".into()),
            "main",
            "",
        )
    }

    fn script(cmd: &str, body: &str) -> Option<String> {
        let read = |p: &str| {
            if p.ends_with("big.py") || p == "/dev/stdin" {
                return Some(Script::Unreadable("larger than 1 MiB"));
            }
            let known = ["x.py", "x.sh", "x.mjs", "x.php", "x.ts", "x.rb"];
            (known.iter().any(|k| p.ends_with(k)) || p == "./run")
                .then(|| Script::Text(body.to_string()))
        };
        script_rules(cmd, &read, &|_| Some("feature".into()), "main")
    }

    #[test]
    fn every_runner_and_nested_scripts_are_read() {
        let merge = "system('gh pr merge 3')\n";
        for cmd in [
            "php x.php",
            "nodejs x.mjs",
            "ts-node x.ts",
            "tsx x.ts",
            "dash x.sh",
            "ksh x.sh",
            "source x.sh",
            ". x.sh",
            "bash -c 'python3 x.py'",
            "echo $(node x.mjs)",
        ] {
            assert!(script(cmd, merge).is_some(), "allowed: {cmd}");
        }
        let shebang = "#!/bin/bash -e\ncd sub && git push origin main\n";
        assert!(script("./run", shebang).is_some());
    }

    #[test]
    fn a_script_the_hook_cannot_read_in_full_is_refused() {
        let why = script("python3 big.py", "").expect("refused");
        assert!(
            why.contains("cannot scan") && why.contains("owner"),
            "{why}"
        );
        assert_eq!(
            script("python3 x.py big.py", "print(1)"),
            None,
            "an argument is data"
        );
        assert!(
            script("python3 /dev/stdin < run.py; cat <<EOF\nx\nEOF", "").is_some(),
            "a heredoc in another segment does not feed stdin"
        );
        assert_eq!(
            script("python3 /dev/stdin <<'EOF'\nprint(1)\nEOF", ""),
            None
        );
        for cmd in [
            "python3 /dev/stdin 2>&1 <<'EOF'\nprint(1)\nEOF",
            "python3 /dev/stdin &>out.log <<'EOF'\nprint(1)\nEOF",
            "cat <<'EOF' | python3 /dev/stdin\nprint(1)\nEOF",
        ] {
            assert_eq!(script(cmd, ""), None, "{cmd}");
        }
        for cmd in [
            "python3 /dev/stdin & cat <<EOF\nx\nEOF",
            "python3 /dev/stdin | cat <<EOF\nx\nEOF",
            "python3 /dev/stdin < run.py && cat <<EOF\nx\nEOF",
            "cat <<'EOF' evil.py | python3 /dev/stdin\nprint(1)\nEOF",
            "cat <<'EOF' < evil.py | python3 /dev/stdin\nprint(1)\nEOF",
            "cat <<'EOF' 0< evil.py | python3 /dev/stdin\nprint(1)\nEOF",
            "python3 /dev/stdin <<'EOF' < evil.py\nprint(1)\nEOF",
            "cat '<<' evil.py | python3 /dev/stdin\nprint(1)",
            "cat \"<<\" evil.py | python3 /dev/stdin\nprint(1)",
            "cat -n <<'EOF' a/evil.py 2>&1 | python3 /dev/stdin\nprint(1)\nEOF",
        ] {
            assert!(script(cmd, "").is_some(), "{cmd}");
        }
    }

    #[test]
    fn inline_interpreter_code_that_merges_tags_or_force_pushes_is_refused() {
        for cmd in [
            r#"python -c 'import subprocess; subprocess.run(["gh", "pr", "merge", "3"])'"#,
            r#"python3 -c "import os; os.system('gh pr merge 3 --squash')""#,
            r#"python3.12 -c 'import os; os.system("git push --force origin x")'"#,
            r#"node -e 'require("child_process").execSync("git tag v1.0.0")'"#,
            r#"node --eval 'cp.execSync("git push origin --tags")'"#,
            r#"ruby -e 'system("gh", "release", "create", "v1")'"#,
            r#"perl -e 'system("git push -f origin HEAD")'"#,
            r#"deno eval 'new Deno.Command("gh", {args: ["pr", "merge"]}).outputSync()'"#,
            r#"bun -e 'Bun.spawnSync(["git", "push", "origin", "main"])'"#,
            r#"node -e 'fetch("https://api.github.com/graphql", {method: "POST", body: "mutation { mergePullRequest(input: {}) { clientMutationId } }"})'"#,
            r#"python3 -c 'requests.put(f"https://api.github.com/repos/o/r/pulls/{n}/merge")'"#,
            r#"node -e 'q("enablePullRequestAutoMerge")'"#,
            "python3 - <<'EOF'\nimport subprocess\nsubprocess.run([\n  \"gh\",\n  \"pr\", \"merge\"])\nEOF",
            r#"python -c 'os.system(f"git push origin {branch}")'"#,
            "python3.12 - <<'EOF'\nimport os\nos.system('gh pr merge 3')\nEOF",
            "tsx <<'EOF'\nexecSync('gh pr merge 3')\nEOF",
            "cat <<'EOF' | python3.11\nos.system('gh pr merge 3')\nEOF",
            "env python3.12 - <<'EOF'\nos.system('git push --force')\nEOF",
            "timeout 5 node -e 'execSync(\"git tag v1\")'",
            "sudo -u root python3 -c 'os.system(\"gh pr merge 3\")'",
            "env -u FOO python3 -c 'os.system(\"gh pr merge 3\")'",
            "timeout -s KILL 5 python3 -c 'os.system(\"gh pr merge 3\")'",
            "exec -a x python3 -c 'os.system(\"gh pr merge 3\")'",
            "time -f %e python3 -c 'os.system(\"gh pr merge 3\")'",
            "stdbuf -o L python3 -c 'os.system(\"gh pr merge 3\")'",
        ] {
            let why = bash(cmd).unwrap_or_else(|| panic!("allowed: {cmd}"));
            assert!(
                why.contains("interpreter code") && why.contains("owner"),
                "{cmd}: {why}"
            );
        }
    }

    #[test]
    fn harmless_interpreter_code_passes() {
        for cmd in [
            "python -c 'print(1)'",
            "python3 -c 'import json; print(json.dumps({\"merge\": 1}))'",
            "node -e 'console.log(process.argv)'",
            "node script.mjs",
            r#"python3 -c 'subprocess.run(["git", "push", "-u", "origin", "claude/x:claude/x"])'"#,
            r#"node -e 'execSync("git status && git log --oneline -3")'"#,
            "ruby -e 'puts 1'",
            "deno eval 'console.log(1)'",
            "git push -u origin HEAD:claude/x && sh -c 'echo $HOME'",
            "git push -u origin claude/x:claude/x && python3 -c 'print(1.5)'",
            "git commit -F - <<'EOF'\nfix: python -c and gh pr merge refusals\nEOF",
        ] {
            assert_eq!(bash(cmd), None, "{cmd}");
        }
    }

    #[test]
    fn script_files_that_merge_are_refused() {
        let merge = "import subprocess\nsubprocess.run(['gh', 'pr', 'merge', '--auto'])\n";
        for cmd in [
            "python x.py",
            "python3 -u x.py arg",
            "cd a && python3 ./x.py",
            "sudo -u root python3 x.py",
        ] {
            let why = script(cmd, merge).unwrap_or_else(|| panic!("allowed: {cmd}"));
            assert!(why.contains("x.py") && why.contains("owner"), "{why}");
        }
        assert!(script("node x.mjs", "execSync('git push --force')").is_some());
        assert!(script("bash x.sh", "set -e\ngit tag v2\n").is_some());
        assert!(script("sh x.sh", "cd sub && git push origin main\n").is_some());
        assert!(script("./run", "#!/bin/sh\ngh pr merge 4\n").is_some());
        assert!(
            script(
                "FOO=1 ./run",
                "#!/usr/bin/env python3\nos.system('gh pr merge')\n"
            )
            .is_some()
        );
    }

    #[test]
    fn script_files_without_merge_words_pass() {
        assert_eq!(script("node x.mjs", "console.log('merge sort')\n"), None);
        assert_eq!(script("python x.py", "print(1)\n"), None);
        assert_eq!(
            script(
                "bash x.sh",
                "cargo test\ngit push -u origin claude/x:claude/x\n"
            ),
            None
        );
        assert!(
            script("python other.py", "gh pr merge").is_some_and(|w| w.contains("does not exist")),
            "a missing script file is refused"
        );
        assert_eq!(script("cat x.py", "gh pr merge"), None, "not run");
        for cmd in [
            "grep -n python x.py",
            "rg -n sh x.sh",
            "git diff . x.sh",
            "echo bash x.sh",
        ] {
            assert_eq!(script(cmd, "gh pr merge"), None, "not a runner: {cmd}");
        }
    }
}
