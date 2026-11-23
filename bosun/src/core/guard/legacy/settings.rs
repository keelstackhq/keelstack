//! Settings-lock (spec §4): an agent may not edit the files that switch the guard off, the
//! repo's `.claude/settings*.json` and anything under the user's `~/.claude`, nor write
//! `disableAllHooks`. Best effort and lexical, like the rest of the guard; it fails closed.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::writes::{command_of, target_directory_option};
use super::{
    SEP, bash_writes, edited_path, prose, read_only, shell_words, split_heredocs, word_scripts,
};

const FIX: &str = "the hooks and permissions in .claude/settings*.json and ~/.claude are the \
    owner's: ask the orchestrator to ask the owner to change them in their own PR, and do not set \
    disableAllHooks";

const HOME: &str = r"(?:~[\w.-]*|\$\{?HOME\}?|/root|/home/[^/\s]+|/Users/[^/\s]+)";

/// A path (file tool or redirect target) the lock covers.
static PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^(?:{HOME}/\.claude(?:/|$)|(?:.*/)?\.claude/settings[^/]*\.json$)"
    ))
    .expect("settings path regex")
});

/// The same targets anywhere in a command's text, with globs that could reach them.
static TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i){HOME}/\.claude\b|\.claude/(?:set|s\*|\*|\?|\[)|\.claude/settings[^/\s]*\.json"
    ))
    .expect("settings text regex")
});

/// A `disableAllHooks` key or assignment (`"disableAllHooks": true`, `.disableAllHooks=true`).
static HOOKS_OFF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)disableAllHooks["'\\]{0,3}\s*[:=]"#).expect("disableAllHooks regex")
});

/// Lexical clean-up: `//`, `/./` and `x/../` collapse; the result is only used for matching.
fn normalize(path: &str) -> String {
    let abs = path.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if out.last().is_some_and(|l| *l != "..") => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    format!("{}{}", if abs { "/" } else { "" }, out.join("/"))
}

/// `path` without quotes (`.cla''ude`), backslashes read as `/` (Windows paths), then cleaned lexically; the regexes
/// below ignore case (`.Claude/Settings.json`: macOS and Windows file systems do).
fn plain(path: &str) -> String {
    let bare: String = path
        .chars()
        .filter(|c| !matches!(c, '\'' | '"'))
        .map(|c| if c == '\\' { '/' } else { c })
        .collect();
    normalize(&bare)
}

/// Plans and todos under the user's `~/.claude`: Claude Code's own scratch files, not settings,
/// hooks or agents config. Only paths strictly below `plans/` and `todos/` (never the folder
/// itself: `rm -rf ~/.claude/plans; ln -s .. ~/.claude/plans` must stay locked). Judged after
/// `plain`, so `plans/../settings.json` is not one. Link- and move-creating commands ignore it
/// (`strict`), so no symlink or move can put a settings path under a free one.
static FREE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)^{HOME}/\.claude/(?:plans|todos)/.")).expect("free path regex")
});

fn protected(path: &str) -> bool {
    let path = plain(path);
    PATH.is_match(&path) && !FREE.is_match(&path)
}

/// A word naming a target of a mutating command: a settings file, a glob that could reach one, or
/// the `.claude` directory itself (`rm -rf .claude`, `git checkout -- .claude`).
static TARGET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!("(?i){}|(?:^|/)\\.claude/?$", TEXT.as_str()))
        .expect("settings target regex")
});

/// Interpreters and shells: the code they run is only seen as text, so a target in it counts.
const INTERP: [&str; 12] = [
    "python", "python3", "node", "perl", "ruby", "php", "bash", "sh", "zsh", "dash", "eval", "deno",
];

/// MCP tools that write a repo file, by the name after the server prefix.
const FILE_TOOLS: [&str; 5] = [
    "create_or_update_file",
    "push_files",
    "delete_file",
    "update_file",
    "create_file",
];

fn target(word: &str) -> bool {
    let word = plain(word);
    TARGET.is_match(&word) && !FREE.is_match(&word)
}

/// `target` without the plans/todos exemption, for commands that create links or move files.
fn strict(word: &str) -> bool {
    TARGET.is_match(&plain(word))
}

/// Whether a `cp` option word makes links (`-s`, `-l`, `-sf`, `--symbolic-link`, `--link`).
fn links(arg: &str) -> bool {
    matches!(arg, "--symbolic-link" | "--link")
        || (arg.starts_with('-') && !arg.starts_with("--") && arg.contains(['s', 'l']))
}

/// The value of an option given as `-t X`, `-tX`, `--long X` or `--long=X`.
fn opt<'a>(args: &[&'a str], short: &str, long: &str) -> Option<&'a str> {
    args.iter().enumerate().find_map(|(i, a)| match *a {
        a if a == short || a == long => args.get(i + 1).copied(),
        a => a
            .strip_prefix(format!("{long}=").as_str())
            .or(a.strip_prefix(short)),
    })
}

/// Whether a short-option cluster ending in `flag` (`-sSLo F`, `-qO F`) is followed by a target.
fn cluster_value(args: &[&str], flag: char) -> bool {
    args.iter().enumerate().any(|(i, a)| {
        a.starts_with('-')
            && !a.starts_with("--")
            && a.ends_with(flag)
            && args.get(i + 1).is_some_and(|n| target(n))
    })
}

/// The index of the git subcommand: the first word that is not an option or its value.
fn git_sub(args: &[&str]) -> Option<usize> {
    let valued = ["-C", "-c", "--work-tree", "--git-dir", "--namespace"];
    (0..args.len())
        .find(|&i| !args[i].starts_with('-') && !(i > 0 && valued.contains(&args[i - 1])))
}

/// The directories git moves to before its subcommand: every `-C D` (cumulative) and
/// `--work-tree D` or `--work-tree=D`.
fn git_dirs<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let end = git_sub(args).unwrap_or(args.len());
    (0..end)
        .filter_map(|i| match args[i] {
            "-C" | "--work-tree" => args.get(i + 1).copied(),
            a => a.strip_prefix("--work-tree="),
        })
        .collect()
}

/// Whether a word is a redirection (`>f`, `2>f`, `&>f`), which ends a command's operands.
fn is_redir(word: &str) -> bool {
    word.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&')
        .starts_with('>')
}

/// Where `cp` or `install` puts its files: the `-t` value (`-t D`, `-tD`, `-rt D`,
/// `--target-directory[=]D`), else the last operand.
fn destination<'a>(args: &[&'a str]) -> Option<&'a str> {
    let args: Vec<&str> = args.iter().copied().take_while(|a| !is_redir(a)).collect();
    let dir = args.iter().enumerate().find_map(|(i, a)| {
        let next = args.get(i + 1).copied().unwrap_or("");
        if let Some(v) = target_directory_option(a) {
            return Some(v.unwrap_or(next));
        }
        let short = a.starts_with('-') && !a.starts_with("--");
        let at = a.find('t').filter(|_| short);
        at.map(|at| Some(&a[at + 1..]).filter(|r| !r.is_empty()).unwrap_or(next))
    });
    dir.or_else(|| args.iter().rev().find(|a| !a.starts_with('-')).copied())
}

/// Whether `dir` is `.claude` or below it.
fn inside(dir: &str) -> bool {
    static IN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)(?:^|/)\.claude(?:/|$)").expect("inside regex"));
    IN.is_match(&plain(dir))
}

/// The directory after moving to `to`, when it is (still) in `.claude`: `from` is where the
/// command runs now, if that is in `.claude`. An unknown word (`cd -`, `cd $X`) stays inside.
fn enter(from: Option<&str>, to: &str) -> Option<String> {
    let free = to.starts_with(['/', '~']);
    let at = match from {
        Some(d) if !free => format!("{d}/{to}"),
        _ => to.into(),
    };
    inside(&at).then_some(at)
}

/// A word as a path from `dir`, for a command that runs there (`rm settings.json`).
fn at(dir: &str, word: &str) -> String {
    let free = word.starts_with(['-', '/', '~', '$', '>']) || word.contains('>');
    if free {
        word.into()
    } else {
        format!("{dir}/{word}")
    }
}

/// The settings-lock for one tool call; `None` allows it.
pub fn settings_rule(tool: &str, input: &Value, cwd: &str) -> Option<String> {
    if tool == "Bash" {
        let command = input.get("command").and_then(Value::as_str).unwrap_or("");
        return bash(command, cwd);
    }
    let name = tool.rsplit("__").next().unwrap_or(tool);
    let hit = if FILE_TOOLS.contains(&name) {
        let files = input.get("files").and_then(Value::as_array);
        let paths = files.into_iter().flatten().chain([input]);
        paths.filter_map(|f| f.get("path")?.as_str()).any(protected)
    } else {
        let path = edited_path(tool, input)?;
        let path = enter(Some(cwd).filter(|c| inside(c)), &path).unwrap_or(path);
        // A `disableAllHooks` key only counts in a JSON file: a spec or test may name it.
        let sets = path.ends_with(".json") && strings(input).iter().any(|t| HOOKS_OFF.is_match(t));
        protected(&path) || sets
    };
    hit.then(|| format!("`{tool}` edits the guard's own settings: {FIX}"))
}

/// Whether one command word, with its arguments, mutates a target (never a read of one).
fn mutates(cmd: &str, args: &[&str]) -> bool {
    let any = |a: &[&str]| a.iter().any(|w| !w.starts_with('>') && target(w));
    match cmd {
        "rm" | "rmdir" | "unlink" | "shred" | "truncate" | "touch" | "chmod" | "chown" => any(args),
        // Links and moves are never exempt under plans/todos (a link there could point at the
        // settings, or the folder itself be replaced by one).
        "ln" | "mv" => args.iter().any(|w| !w.starts_with('>') && strict(w)),
        "install" | "rsync" => destination(args).is_some_and(strict),
        "cp" if args.iter().any(|a| links(a)) => destination(args).is_some_and(strict),
        // `bash_writes` skips `~` and `$HOME` targets: judge them here (a path word only).
        // The destination is the `-t` value (also in a cluster: `-rt DIR`) or the last operand,
        // and may be the `.claude` directory itself.
        "cp" => destination(args).is_some_and(target),
        "tee" | "sponge" => args.iter().any(|a| protected(a)),
        // `dd of=FILE` (also after `if=`/`bs=` words).
        "dd" => args
            .iter()
            .any(|a| a.strip_prefix("of=").is_some_and(protected)),
        // The output file (`-o F`, `-oF`, `--output F`) or directory (`--output-dir D`).
        "curl" => {
            cluster_value(args, 'o')
                || opt(args, "-o", "--output").is_some_and(|o| protected(o) || target(o))
                || args.iter().enumerate().any(|(i, a)| {
                    let dir = a.strip_prefix("--output-dir=").or_else(|| {
                        (*a == "--output-dir")
                            .then(|| args.get(i + 1).copied())
                            .flatten()
                    });
                    dir.is_some_and(target)
                })
        }
        // `-O F`, `--output-document[=]F`, and the directory for downloads (`-P D`).
        "wget" => {
            cluster_value(args, 'O')
                || opt(args, "-O", "--output-document").is_some_and(|o| protected(o) || target(o))
                || opt(args, "-P", "--directory-prefix").is_some_and(target)
        }
        // gawk's in-place extension rewrites its file operands.
        "awk" | "gawk"
            if args.iter().enumerate().any(|(i, a)| {
                a.strip_prefix("-i").is_some_and(|r| {
                    r == "inplace" || (r.is_empty() && args.get(i + 1) == Some(&"inplace"))
                }) || *a == "--include=inplace"
            }) =>
        {
            any(args)
        }
        "sed"
            if args.iter().any(|a| {
                a.starts_with("--in-place")
                    || (a.starts_with('-') && !a.starts_with("--") && a.contains('i'))
            }) =>
        {
            args.iter().any(|a| protected(a))
        }
        "find" => {
            let acts = ["-delete", "-exec", "-execdir", "-ok", "-fprint", "-fprintf"];
            let named = args
                .iter()
                .any(|a| a.starts_with("settings") && a.ends_with(".json"));
            args.iter().any(|a| acts.contains(a)) && (any(args) || named)
        }
        // They write the files a patch or an archive names: the operands name one, or `-d D`
        // (`--directory=D` for git) moves there. A patch body naming a target is `bash`'s text check.
        "patch" => any(args) || opt(args, "-d", "--directory").is_some_and(target),
        "unzip" => any(args) || opt(args, "-d", "-d").is_some_and(target),
        "tar" => {
            let x = |a: &&str| {
                matches!(*a, "--extract" | "--get")
                    || (a.starts_with('-') && !a.starts_with("--") && a.contains('x'))
            };
            let bundled = args
                .first()
                .is_some_and(|a| !a.starts_with('-') && a.contains('x'));
            (args.iter().any(x) || bundled)
                && (any(args) || opt(args, "-C", "--directory").is_some_and(target))
        }
        "git" => {
            let sub = git_sub(args);
            let ops = ["checkout", "restore", "stash", "reset", "clean", "rm", "mv"];
            let patches = |i: usize| {
                matches!(args[i], "apply" | "am")
                    && (any(&args[i + 1..])
                        || opt(&args[i + 1..], "--directory", "--directory").is_some_and(target))
            };
            sub.is_some_and(|i| (ops.contains(&args[i]) && any(&args[i + 1..])) || patches(i))
        }
        _ => false,
    }
}

/// Whether a redirection in the segment (`>f`, `>> f`, `2>f`) writes a target; unlike
/// `bash_writes` it also sees `~` and `$HOME` paths.
fn redirects(seg: &[String], dir: Option<&str>) -> bool {
    seg.iter().enumerate().any(|(i, w)| {
        let op = w.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
        // `>|` forces a write past noclobber.
        let rest = op
            .strip_prefix('>')
            .map(|r| r.trim_start_matches(['>', '|']));
        // `2>&1` duplicates a descriptor: no file.
        let to = rest.map(|r| match r.strip_prefix('&').unwrap_or(r) {
            "" => seg.get(i + 1).map_or("", |n| n),
            fd if r.starts_with('&') && fd.chars().all(|c| c.is_ascii_digit() || c == '-') => "",
            _ => r,
        });
        to.is_some_and(|t| !t.is_empty() && target(&dir.map_or(t.into(), |d| at(d, t))))
    })
}

/// Whether the commands in `text` mutate a target. `cwd` is set once a `cd`, `pushd`, `git -C`,
/// `env -C` or `sudo -D` goes into `.claude`: relative words then count as paths in it (sticky
/// to the end, which only makes it stricter). Subshell groups are scanned in line; `sh -c`,
/// `eval` and `env -S` strings are scanned again with the current `cwd`.
fn mutated(text: &str, mut cwd: Option<String>, depth: usize) -> bool {
    // `>|` is a plain redirect that overrides noclobber; `|` would otherwise split the command.
    let text = &text.replace(">|", ">");
    let words = shell_words(&split_heredocs(text).0);
    for seg in words.split(|w| w == SEP) {
        let (cmd, args, dirs) = command_of(seg);
        let git = if cmd == "git" {
            git_dirs(&args)
        } else {
            vec![]
        };
        let mut here = cwd.clone();
        for d in dirs.iter().map(String::as_str).chain(git) {
            here = enter(here.as_deref(), d).or(here);
        }
        // Git's options and subcommand are words, not paths.
        let lead = if cmd == "git" {
            git_sub(&args).map_or(args.len(), |i| i + 1)
        } else {
            0
        };
        let paths: Vec<String> = (args.iter().enumerate())
            .map(|(i, a)| match &here {
                Some(d) if i >= lead => at(d, a),
                _ => a.to_string(),
            })
            .collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let applies = matches!(cmd, "patch" | "unzip")
            || (cmd == "git" && git_sub(&args).is_some_and(|i| matches!(args[i], "apply" | "am")));
        // Run from `.claude`, the patch or archive lands there.
        if mutates(cmd, &refs) || redirects(seg, here.as_deref()) || (applies && here.is_some()) {
            return true;
        }
        if matches!(cmd, "cd" | "pushd") {
            let to = args.iter().find(|a| !a.starts_with('-'));
            cwd = to.and_then(|d| enter(cwd.as_deref(), d));
        }
    }
    depth < 4 && (word_scripts(text).iter()).any(|s| mutated(s, cwd.clone(), depth + 1))
}

/// `python -m json.tool [opts] [FILE]`: reads one file (a second operand would be the output).
fn json_read(cmd: &str, args: &[&str]) -> bool {
    let ops = (args.iter().skip(2)).filter(|a| !a.starts_with('-') && a.parse::<u8>().is_err());
    matches!(cmd, "python" | "python3")
        && args.starts_with(&["-m", "json.tool"])
        && ops.take_while(|a| !is_redir(a)).count() <= 1
}

fn bash(command: &str, cwd: &str) -> Option<String> {
    let text = prose::strip_prose(command);
    let dequoted: String = text
        .chars()
        .filter(|c| !matches!(c, '\'' | '"' | '\\'))
        .collect();
    let words = shell_words(&split_heredocs(&text).0);
    let interp = (words.split(|w| w == SEP)).any(|seg| {
        let (cmd, args, _) = command_of(seg);
        INTERP.contains(&cmd) && !json_read(cmd, &args)
    });
    // `read_only` ignores redirections: a `>` in code that runs counts as a write.
    let reads = read_only(&text, 0) && !text.contains('>');
    let sets = (HOOKS_OFF.is_match(&text) || HOOKS_OFF.is_match(&dequoted))
        && dequoted.contains(".json")
        && !reads;
    let code = interp && TEXT.is_match(&dequoted) && !reads;
    // A patch fed on stdin (`patch -p1 <<EOF`, `git apply <<EOF`) names its files in its body.
    let patcher = (words.split(|w| w == SEP)).any(|seg| {
        let (cmd, args, _) = command_of(seg);
        cmd == "patch"
            || (cmd == "git" && git_sub(&args).is_some_and(|i| matches!(args[i], "apply" | "am")))
    });
    let code = code || (patcher && TEXT.is_match(&dequoted));
    let writes = bash_writes(command).iter().any(|p| protected(p));
    let start = Some(cwd.to_string()).filter(|c| inside(c));
    (mutated(&text, start, 0) || code || sets || writes)
        .then(|| format!("the command writes the guard's own settings: {FIX}"))
}

/// Every string value in `v`, at any depth.
fn strings(v: &Value) -> Vec<&str> {
    match v {
        Value::String(s) => vec![s.as_str()],
        Value::Object(m) => m.values().flat_map(strings).collect(),
        Value::Array(a) => a.iter().flat_map(strings).collect(),
        _ => Vec::new(),
    }
}
