// PreToolUse guard: denies releases, force/tag pushes, pushes to the default branch, gh admin verbs
// (merge without --auto, release, secret, repo edit), secret reads and edits of the guard rail itself
// (.claude/hooks, .claude/settings*.json, release workflows, .splice/lock.toml, .bosun/); asks before
// editing files the template owns. Every deny names the allowed alternative. A guard rail, not a
// sandbox: the real fix is a server-side branch ruleset and environment-gated release.
// Judges the commands that run: the executable and its arguments, `$(...)`, backticks and `<(...)`
// outside single quotes (also in heredocs with an unquoted delimiter), and shell heredocs. Quoted
// heredoc bodies and quoted prose are data. `eval`, an interpreter fed from a pipe, `$VAR` or
// `$(...)` in command position and git `-c alias.x=` deny when the text mentions push, publish or
// release; interpreter code (`-c`/`-e`, heredoc) is data only when every statement is inert
// (`inert()`: print/echo of literals or names; Python: read or json/yaml-safe-load a literal path);
// any other code is scanned (fail closed). Interpreter code from stdin is scanned as text; a shell
// fed from stdin, a pipe or `<(...)` is refused (below).
// Comments, `case` patterns, quoted heredoc words (`E"OF"`) and `$'...'` escapes are read
// like the shell; joined string literals are checked for secret file names.
// A shell script the command runs (any shell by basename: `bash f`, `< f rbash`, `source f`, `./f`
// with a shell, `env -S` or busybox shell, or no `#!`; a bare name in a dir the command adds to
// PATH; through wrappers: sudo, timeout, flock, nice, su -c, ...; runners: git rebase --exec,
// git bisect run, git submodule foreach, npm exec, at, expect -c, nix-shell --run; trap and
// coproc bodies) is read and judged
// like typed commands, nested scripts, function bodies and startup files (`BASH_ENV`, `ENV`,
// `--rcfile`) too: they share the chain (a file written earlier is
// refused) and `$1`/`$@` stand for the literal arguments. Fail closed: a script the guard cannot
// read in full (> 512 KiB, NUL bytes, unreadable, > 32 scripts, a path
// from a variable or `{}`, a shell fed by a pipe, stdin or an expanding here-string, `xargs bash`)
// is refused; so are `alias x=...` and a PATH dir that is not literal ($HOME, $PWD, $(pwd) known).
// `gh api` with an explicit GET and graphql queries are reads; a POST whose endpoint (the first
// positional) dispatches a literal workflow file that is not a release workflow (release, publish,
// deploy; GUARD_RELEASE_WORKFLOWS overrides) or re-runs a run/job matches `gh workflow run` /
// `gh run rerun`; a POST to exactly `repos/<o>/<r>/pulls` opens a PR like `gh pr create`. These
// routes and `gh pr|issue create` apply only when every word is literal (no `$VAR`, `"$@"`,
// `${...}`, `$(...)` or backtick, a -f / --body value too) with no GH_HOST / token env prefix;
// a body comes from a non-secret repo file (`--body-file pr.md`, `-F body=@pr.md`). A word the
// shell changes at run time is refused in any risky command. Other `-F x=@file` and `--input`
// read a file into the request: refused (use `-f`).
// `cd` into a guarded dir is allowed when the rest of the chain only reads (docs/agents/guard.md
// lists the safe patterns). A deny names its rule and the match.
// A script missing at hook time, or run after an earlier command that may write files the guard
// cannot name (`opaqueWriter`: downloads, archives, decoders, builds, interpreters), is refused, as
// is a `#!` naming an interpreter the guard does not know. `gh api` flags are parsed like pflag
// (`-iXPUT`); an unknown flag is refused.
// Known gaps: scripts in other languages, binaries, a `$VAR`/`"$@"` in command position (only
// text-checked), programs that run their own config (npm scripts, make), a re-run by run/job id of a
// release run (the id does not name its workflow), a `cd` to a variable inside a script, editing
// guarded files from an interpreter, a dir reached through a variable or substitution in a flag
// value (`tar -C "$D"`; `cd "$D"` itself is refused), a patch piped in (refused) or made by a
// producer other than `git diff|show|format-patch -- <paths>` (refused), and a default branch
// that origin/HEAD does not name. Node built-ins only.
// No regression (`decide`): a call is also denied when the legacy judge (guard-legacy.mjs, main's
// guard verbatim) denies it, unless the Bash command is exactly an enumerated literal allow
// (`intendedAllow`: `gh api -X POST repos/<o>/<r>/pulls ...`); so the gh api dispatch and re-run
// routes above are refused (main refuses them). docs/agents/guard.md lists the allows.
// An internal error denies when the call mentions push, publish, release, gh, secret or sops (retry),
// else allows and prints one stderr line. Settings deny rules only cover force pushes and publish
// commands: the guard is the one layer for default-branch pushes and secrets.
import {
  closeSync,
  existsSync,
  openSync,
  lstatSync,
  readFileSync,
  readSync,
  realpathSync,
  statSync,
} from "node:fs";
import nodeFs from "node:fs"; // `globSync` (Node 22+), looked up at run time
import { homedir } from "node:os";
import { basename, dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** The legacy judge (main's guard, verbatim): loaded here so a missing or broken copy fails
 * closed (a risky call is denied) instead of the hook crashing open. */
const LEGACY = await import("./guard-legacy.mjs").catch(() => null);

const SECRET_SAFE = new Set([".env.example", ".env.sample", ".env.template", ".dev.vars.example"]);
const MANAGERS = new Set(["npm", "pnpm", "yarn", "bun"]);
/** Any shell, by basename (`rbash`, `mksh`, `bash5.2`, `busybox` is unwrapped first): its code is
 * read like typed commands. */
const SHELL_RE =
  /^(?:r?bash|sh|zsh|dash|ksh|ksh93|mksh|lksh|pdksh|oksh|ash|yash|posh|hush|fish|csh|tcsh|elvish|nu|xonsh|osh|ysh|rc|es)(?:[-.]?\d[\w.-]*)?$/;
const SHELLS = { has: (p) => SHELL_RE.test(String(p)) };
const INTERPRETERS = /^(python[\d.]*|node|nodejs|bun|deno|ruby|perl|php)$/;
const LOCK_RE = /(^|\/)\.splice\/lock\.toml$/i;
const EDIT_TOOLS = new Set(["Edit", "Write", "MultiEdit", "NotebookEdit"]);
const FILE_TOOLS = new Set(["Read", "Grep", "Glob", ...EDIT_TOOLS]);
const TAG_READ_RE =
  /^(-l|-n\d*|-v|--(list|verify|contains|no-contains|points-at|merged|no-merged|sort|format|column)(=.*)?)$/;
const RISKY_RE = /\b(push|publish|unpublish|release|dist-tag|gh|secrets?|sops)\b/i;
const EXEC_RE =
  /\b(git|gh|npm|pnpm|yarn|bun|cargo|make|hoist|sops)\b[^\n]*\b(push|publish|unpublish|release|dist-tag|secrets?|merge|decrypt)\b/i;
const INLINE_RE = /^-[a-zA-Z]*[ceEp]$|^--eval$/;
// The only inert statements (on code whose strings are replaced by S): print/echo string literals
// or plain names, and (Python) read or json/yaml-safe-load a literal path with open()'s default
// read mode. Everything else is code and gets the full scan.
const ARG = String.raw`(?:S|[A-Za-z_]\w*|S\s+in\s+[A-Za-z_]\w*)`;
const PRINT_RE = new RegExp(
  String.raw`^(?:print|console\.log|puts|echo|say)\s*(?:\(\s*(?:${ARG}(?:\s*,\s*${ARG})*)?\s*\)|${ARG}(?:\s*,\s*${ARG})*)?$`,
);
const PY_INERT_RES = [
  /^[A-Za-z_]\w*\s*=\s*open\s*\(\s*S\s*\)\s*\.\s*read\s*\(\s*\)(?:\s*\.\s*(?:strip|splitlines)\s*\(\s*\)|\s*\.\s*split\s*\(\s*S?\s*\))?$/,
  /^[A-Za-z_]\w*\s*=\s*(?:json\s*\.\s*load|yaml\s*\.\s*safe_load)\s*\(\s*open\s*\(\s*S\s*\)\s*\)$/,
  /^import\s+(?:json|yaml)(?:\s*,\s*(?:json|yaml))?$/,
];

/** True when interpreter code is only inert statements (PRINT_RE, PY_INERT_RES): literals printed,
 * a literal path read or safe-loaded. Anything else is code and is scanned (fail closed). */
export function inert(body, prog) {
  const lang = /^(ruby|perl|php)/.test(prog) ? "rpp" : /^python/.test(prog) ? "py" : "js";
  let code = "";
  for (let i = 0; i < body.length; i++) {
    const c = body[i];
    // A comment ends at the newline, whatever quotes it holds.
    // Perl/Ruby/PHP: `#` can delimit a quote (`q#..#`, `%q#..#`), so it is code, never a comment.
    if (lang === "rpp" && c === "#") return false;
    if ((lang === "py" && c === "#") || (lang === "js" && c === "/" && body[i + 1] === "/")) {
      const nl = body.indexOf("\n", i);
      if (nl < 0) break;
      i = nl - 1;
      continue;
    }
    if (c === "`" && lang === "rpp") return false; // backticks run a program
    if (c !== "'" && c !== '"' && c !== "`") {
      code += c;
      continue;
    }
    let j = i + 1;
    while (j < body.length && body[j] !== c) j += body[j] === "\\" ? 2 : 1;
    if (j >= body.length) return false; // unterminated
    if (/[$#@]\{|\{\$/.test(body.slice(i + 1, j))) return false; // interpolation
    code += " S ";
    i = j;
  }
  if (/[^\x20-\x7e\t\n\r]/.test(code)) return false; // full-width names are names (NFKC)
  const stmts = code
    .split(/[;\n]/)
    .map((x) => x.trim())
    .filter(Boolean);
  // A printed name must be assigned earlier by an inert statement (`process`, `ENV`, `$_` are not).
  const assigned = new Set();
  for (const x of stmts) {
    if (lang === "py" && PY_INERT_RES.some((re) => re.test(x))) {
      const name = /^([A-Za-z_]\w*)\s*=/.exec(x)?.[1];
      if (name) assigned.add(name);
    } else if (PRINT_RE.test(x)) {
      const names = x.replace(/^\w+(\.\w+)?/, "").match(/[A-Za-z_]\w*/g) ?? [];
      if (names.some((n) => n !== "S" && n !== "in" && !assigned.has(n))) return false;
    } else return false;
  }
  return true;
}

const UNICODE_NAMES = { "FULL STOP": ".", SOLIDUS: "/", "HYPHEN-MINUS": "-", "LOW LINE": "_" };

/** Escapes in string text decoded (`".\x65nv"` is `.env`), for secret scans. */
function unescape(body) {
  return body
    .replace(/\\\n/g, "")
    .replace(/\\N\{([A-Z -]+)\}/g, (m, n) => {
      const letter = /^LATIN (SMALL|CAPITAL) LETTER ([A-Z])$/.exec(n);
      if (letter) return letter[1] === "SMALL" ? letter[2].toLowerCase() : letter[2];
      return UNICODE_NAMES[n] ?? m;
    })
    .replace(
      /\\x([0-9a-fA-F]{2})|\\u([0-9a-fA-F]{4})|\\U([0-9a-fA-F]{8})|\\([0-7]{1,3})/g,
      (_, x, u, U, o) => {
        const cp = Number.parseInt(x ?? u ?? U ?? o, o ? 8 : 16);
        return cp <= 0x10ffff ? String.fromCodePoint(cp) : "";
      },
    );
}

/** Adjacent or joined string literals merged (`".en" "v"`, `".en" + "v"`) for secret scans. */
function joinStrings(body) {
  let prev;
  let out = body;
  for (let n = 0; n < 20 && out !== prev; n++) {
    prev = out;
    out = out.replace(
      /(['"])([^'"\n]*)\1\s*[+.]?\s*(['"])([^'"\n]*)\3/g,
      (_, q, a, _q, b) => q + a + b + q,
    );
  }
  return out;
}

/** The whole environment passed as a value: `console.log(process)`, `puts ENV`, `p ENV.to_h`. */
const WHOLE_ENV_RE =
  /\bprocess\s*[,)\]}]|\bprocess\s*$|\bENV\b(?!\s*\[|\s*\.\s*fetch)|%ENV\b|\bgetenv\s*\(\s*\)|\$_(SERVER|ENV)\b(?!\s*\[)|\b(Bun|Deno)\s*(?:\.\s*env|\[\s*["']env["']\s*\])(?![\w$])(?!\s*\[|\s*\.\s*(get\b|[A-Za-z_]\w*\b(?!\s*\()))|\bos\.environb\b(?![.[\w])/m;
const SECRET_VAR_RE = /\$\{?[A-Za-z0-9_]*(KEY|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIAL)/i;
const SECRET_DIR_RE = /(^|\/)\.(aws|ssh|age)(\/|$)|(^|\/)\.config\/gh(\/|$)|^\/proc\/.+\/environ$/;
const PROTECTED_RES = [
  /(^|\/)\.claude\/hooks(\/|$)/i,
  /(^|\/)\.claude\/settings[^/]*\.json$/i,
  /(^|\/)\.github\/workflows\/release[^/]*\.ya?ml$/i,
  /(^|\/)\.bosun(\/|$)/i,
  /(^|\/)\.claude$/i,
  /(^|\/)pins\.toml$/i, // tool versions that release workflows install with release credentials
  /(^|\/)\.git\/(hooks(\/|$)|config$)/i, // git runs hooks and config programs
  /(^|\/)scripts\/release-pre\.sh$/i, // runs in the publish job, with publish rights
];
const FAIL_CLOSED_RE = /\.claude\/(hooks|settings)|workflows\/release|\.bosun\/|\.splice\/lock/i;
const isProtected = (p) => PROTECTED_RES.some((re) => re.test(p));
const stripTok = (x) =>
  x
    .replace(/^\d*(<>|[>&]*)/, "")
    .replace(/^(of=)?(\.\/)?/, "")
    .replace(/(.)\/+$/, "$1");

/** Program name without its directory: `/usr/bin/npm` → `npm`. */
const base = (prog) => String(prog).slice(String(prog).lastIndexOf("/") + 1);

const REASON = {
  publish: "Releases run in CI (release.yml), as AGENTS.md says; do not publish from here.",
  force:
    "Push without --force or +refspec; rebase onto the remote branch, or ask the human to rewrite history.",
  tag: "a tag push publishes: push branches only and ask the human to create and push release tags.",
  deleteDefault:
    "never delete the default branch on the remote: delete feature branches only, or ask the human.",
  pushDefault:
    "Never push to the default branch: push a feature branch (git push -u origin <feature-branch>), open a PR (gh pr create) and enable gh pr merge --auto.",
  rewriteDefault:
    "Never delete or rewrite the default branch: work on a feature branch (git switch -c <name>) and sync with git pull --ff-only.",
  secret:
    "Secret files and values are off limits: read .env.example for the variable names, run what needs values as `hoist secrets exec -- <your command>` (never env, printenv or a shell), or ask the human.",
  ghApi:
    "gh api writes are blocked except opening a PR: `gh api -X POST repos/<owner>/<repo>/pulls -f title=... -f head=<branch> -f base=<default> -F body=@<file>` (every word literal; or `gh pr create --title ... --body-file <file>`); use plain GET reads; start a workflow with `gh workflow run <file> --ref <branch>` and re-run one with `gh run rerun <run-id>` (release, publish and deploy workflows excepted); for anything else (merge, review, tags, releases) ask the human (a PR merges by its owner).",
  ghField:
    "gh api / gh workflow run -F/--field @file and --input read a local file into the request: pass literal values with -f/--raw-field (`-f ref=main -f inputs[x]=1`); only a PR body may come from a repo file (`gh api -X POST repos/<o>/<r>/pulls ... -F body=@pr.md`).",
  ghLiteral:
    'gh opens a PR or issue or starts a workflow from here only when the guard can read every word: write each word literally (no $VAR, "$@", ${...}, $(...) or backticks, in a title or body too), with no GH_*, GITHUB_* or *TOKEN* variable set anywhere in the command, no cd/pushd before it, no --hostname or -R/--repo, and put the body in a regular file in the repo (not a dotfile, symlink or credential file), from the repo root: `gh pr create --title <title> --body-file pr.md` or `gh api -X POST repos/<o>/<r>/pulls -f title=<title> -f head=<branch> -f base=<default> -F body=@pr.md`.',
  unscanned:
    "The guard must read a script before a shell runs it, and cannot read this one (too large, binary bytes, missing, unreadable, nested too deep, fed on stdin or a pipe, or named by a variable, glob or {}): run it as `bash <path>` with a literal path to a readable text script, or run the command directly.",
  ghFlag:
    "gh api with a flag the guard does not know cannot be read: use only -X/--method, -f/--raw-field, -F/--field, -H, -q/--jq, -t, -p, --input, --paginate, -i/--include, --silent, --verbose, --slurp, --hostname, --cache, each as its own word (`gh api -X GET repos/<o>/<r>/pulls -i`).",
  pushVar:
    "The guard cannot read a remote or branch from a variable or $(...): write the branch name literally (git push -u origin my-feature), or from a feature branch run `git push -u origin HEAD`.",
  scriptLater:
    "The guard reads a script before the command runs, and this one is missing now or may be written earlier in this same command (a download, archive, decoder, patch, build or interpreter): run that command first, then run the script in a separate Bash call so the guard reads what is there.",
  rcfile:
    "Your shell startup file holds code the guard cannot read, and the Bash tool already starts from your shell's environment: drop `source ~/.bashrc` (or ~/.profile, ~/.zshrc) and run the command; to set up one tool, export its variables or run it by its path (`.venv/bin/<tool>`, `node_modules/.bin/<tool>`).",
  dynamic:
    'The code here comes from a variable or a command\'s output (`eval "$(...)"`, `bash -c "$X"`, `$(cat f) ...`), which the guard cannot read: spell out the command, or run a script file with `bash <path>`.',
  gitProgram:
    "This makes git (or a pager or editor) run a program the guard cannot see (core.sshCommand, core.pager, core.editor, core.fsmonitor, diff.external, *.textconv, filter.*, credential.*, include.*, GIT_SSH_COMMAND, GIT_CONFIG_*): run git without it (`git -c core.pager=cat` and `GIT_PAGER=cat` pass), or ask the human.",
  alias:
    "Aliases hide the command they run from the guard: run the command directly (git push -u origin <feature-branch>).",
  ghMerge:
    "gh pr merge only with --auto (merges once required checks pass); never --admin or an immediate merge: ask the human.",
  ghAdmin:
    "Releases, repo settings, branch protection, rulesets and secrets belong to the human: ask the human to run it; gh pr create, view, checks and merge --auto stay open.",
  evasion:
    "Hidden or indirect push, publish or release is blocked: run the command directly and write the word plainly, with no $(...), backticks, ${...}, $'...', {a,b} or line continuation in it (git push -u origin <feature-branch>), so the guard can see it.",
  lock: "only splice writes the lock (.splice/lock.toml): run `splice update` or `splice add` instead.",
  protected:
    "Guard rails (.claude/hooks, .claude/settings*.json, release workflows, .bosun/, pins.toml, .git/hooks, .git/config, git hooksPath and aliases) belong to the human: ask the human; put local rules in docs/agents/project.md. Use paths from the repo root (no cd, -C, --chdir or --work-tree into guarded dirs) and apply patches from a file in the repo.",
  ghReview:
    "gh pr review --approve is an owner step: comment with `gh pr review --comment -b <text>` and ask the human (or the review process) to approve.",
  legacy:
    "main's guard denies this call and it is not on guard.mjs's enumerated allow list (docs/agents/guard.md, No regression): write it another way that both allow, or ask the human.",
  failClosed:
    "The guard hit an internal error on a push, publish, release or secret call: retry the call; if it keeps failing, ask the human.",
};

/** The `ask` text for an owned file: name the place local changes go. */
const ownedReason = (rel) =>
  rel === ".github/workflows/ci.yml"
    ? "owned by the template: `splice update` overwrites it; put repo checks in scripts/ci-extra.sh (runs last in CI)"
    : "owned by the template: `splice update` overwrites it; put local changes in docs/agents/project.md or a seed file";

/** Tool argument that names a file: `file_path ?? notebook_path ?? path`. */
export function targetPath(toolInput) {
  return toolInput?.file_path ?? toolInput?.notebook_path ?? toolInput?.path;
}

/** Repo-relative paths with `class = "owned"` in the `[files]` table of `.splice/lock.toml`. */
export function ownedPaths(lockText) {
  const owned = new Set();
  let inFiles = false;
  for (const line of String(lockText ?? "").split("\n")) {
    const header = /^\s*\[([^\]]*)\]\s*$/.exec(line);
    if (header) {
      inFiles = header[1].trim() === "files";
      continue;
    }
    if (!inFiles) continue;
    const m = /^\s*"([^"]+)"\s*=\s*\{[^}]*\bclass\s*=\s*"owned"/.exec(line);
    if (m) owned.add(m[1]);
  }
  return owned;
}

const ANSI = {
  n: "\n",
  t: "\t",
  r: "\r",
  a: "\x07",
  b: "\b",
  e: "\x1b",
  E: "\x1b",
  f: "\f",
  v: "\v",
};

/** One ANSI-C escape (the text after the backslash): [decoded text, characters used]. */
function ansiC(s) {
  let m = /^x([0-9a-fA-F]{1,2})/.exec(s);
  if (m) return [String.fromCharCode(Number.parseInt(m[1], 16)), m[0].length];
  m = /^u([0-9a-fA-F]{1,4})|^U([0-9a-fA-F]{1,8})/.exec(s);
  if (m) {
    const cp = Number.parseInt(m[1] ?? m[2], 16);
    return [cp <= 0x10ffff ? String.fromCodePoint(cp) : "", m[0].length];
  }
  m = /^[0-7]{1,3}/.exec(s);
  if (m) return [String.fromCharCode(Number.parseInt(m[0], 8) & 255), m[0].length];
  // `\c'` leaves the quote's end alone: bash keeps `\c` and closes the quote
  if (s[0] === "c" && s[1] === "\\" && s.length > 2) return [String.fromCharCode(0x1c), 3]; // `\c\\`
  if (s[0] === "c" && s.length > 1 && s[1] !== "'")
    return [String.fromCharCode(s.charCodeAt(1) & 31), 2];
  if (s[0] === "c") return ["\\c", 1];
  return [ANSI[s[0]] ?? s[0], 1];
}

/** End index of the substitution opened at `i` (`$(`, `<(`, `>(` or a backtick), or the text's end.
 * A backtick body ends at the next unescaped backtick (bash's rule); a `(` body is read by the same
 * lexer as split() (`lex`), so escapes, quotes, line continuations, comments and heredocs are
 * read one way for both. */
function subEnd(cmd, i) {
  if (cmd[i] === "`") {
    for (let j = i + 1; j < cmd.length; j++) {
      if (cmd[j] === "\\") j++;
      else if (cmd[j] === "`") return j;
    }
    return cmd.length;
  }
  return lex(cmd, i + 1, true).end;
}

/** Index of the `}` closing the `{` at `i` (`${...}`), following quotes, escapes, nested `${`
 * and substitutions; the text's end when none does. */
function braceEnd(cmd, i) {
  let depth = 0;
  for (let j = i; j < cmd.length; j++) {
    const c = cmd[j];
    if (c === "\\") j++;
    else if (c === "'") {
      j = cmd.indexOf("'", j + 1);
      if (j < 0) return cmd.length;
    } else if (c === '"') {
      for (j++; j < cmd.length && cmd[j] !== '"'; j++) if (cmd[j] === "\\") j++;
    } else if (c === "`" || (c === "$" && cmd[j + 1] === "("))
      j = subEnd(cmd, c === "`" ? j : j + 1);
    else if (c === "{") depth++;
    else if (c === "}" && --depth === 0) return j;
  }
  return cmd.length;
}

/** Bodies of `$(...)`, `<(...)`, `>(...)` and backticks in heredoc text (quotes there are literal). */
function docSubs(body) {
  const out = [];
  for (let i = 0; i < body.length; i++) {
    if (body[i] === "\\") i++;
    else if (body[i] === "`" || (body[i] === "$" && body[i + 1] === "(")) {
      const tick = body[i] === "`";
      const e = subEnd(body, tick ? i : i + 1);
      const sub = body.slice(i + (tick ? 1 : 2), e);
      out.push(tick ? sub.replace(/\\([\\`$])/g, "$1") : sub);
      i = e;
    }
  }
  return out;
}

/** Split a command line into simple commands (token lists), ignoring quotes' content for splitting.
 * `segments.subs` holds the command substitutions the shell runs: `$(...)`, backticks and `<(...)`
 * outside single quotes, and those in heredocs whose delimiter is unquoted. */
/** A quoted or escaped `<`/`>` is data, not a redirect: kept as a sentinel until after splitRedirect. */
const MARKS = { ">": "\uE001", "<": "\uE002" };
const mark = (c) => MARKS[c] ?? c;
const unmark = (x) => x.replaceAll("\uE001", ">").replaceAll("\uE002", "<");

function split(cmd) {
  return lex(cmd, 0, false).segments;
}

/** The one shell lexer. At the top (`nested` false) it splits `cmd` into simple commands. Nested,
 * it reads the body of a `$(`/`<(`/`>(` from `from` (just after the `(`) and stops at the `)` that
 * closes it: `end` is that index, or the text's end when none does (`$((...))` is arithmetic: no
 * comments, no heredocs). Inside `case ... esac` a pattern's `)` does not close it. */
function lex(cmd, from, nested, arithOk = true) {
  const segments = [];
  const subs = [];
  let segSubs = []; // this segment's substitutions, read right before it
  let raws = []; // each word as typed (quotes, escapes and substitutions kept)
  let tokens = [];
  let cur = "";
  let has = false;
  let quote = "";
  let pending = [];
  let docs = [];
  let expand = false; // a heredoc with an unquoted delimiter holds `$` or a backtick
  const comments = [];
  // `$((` is arithmetic only when the inner `(` closes right before the outer `)` (`$((1+2))`);
  // else bash reads it as `$( (cmd) ...)`, and so does the guard (lexed again, comments and all).
  const arith = nested && arithOk && cmd[from] === "(";
  let depth = 1; // nested: the open `(`s, the substitution's own included
  let cases = 0; // nested: `case ... esac` depth
  let sep = "\n"; // nested: the separator before this word (`;`/newline: `esac` can close a case)
  let tokStart = from;
  // Nested, `case` counts wherever it stands (`time -p case`, `coproc case`, `function f case`):
  // counting too many only makes the substitution run on, which the guard reads (fail closed).
  // `esac` counts only after `;`/`;;` or a newline, never in a pattern (`x|esac)`).
  const endToken = (at) => {
    if (has) {
      tokens.push(cur);
      raws.push(at === undefined ? cur : cmd.slice(tokStart, at));
      if (nested) {
        const raw = cmd.slice(tokStart, at).replaceAll("\\\n", "");
        if (raw === "case") cases++;
        else if (raw === "esac" && sep) cases = Math.max(0, cases - 1);
        sep = "";
      }
    }
    cur = "";
    has = false;
  };
  const endSegment = (at) => {
    endToken(at);
    sep = cmd[at] === ";" || cmd[at] === "\n" ? "\n" : "";
    if (tokens.length) {
      if (docs.length) tokens.docs = docs;
      if (expand) tokens.docExpand = true;
      tokens.raws = raws;
      tokens.subs = segSubs;
      segSubs = [];
      segments.push(tokens);
    }
    raws = [];
    tokens = [];
    docs = [];
    expand = false;
  };
  for (let i = from; i < cmd.length; i++) {
    const c = cmd[i];
    if (!has) tokStart = i;
    const open =
      quote !== "'" &&
      quote !== "$'" &&
      (c === "`" ||
        (c === "$" && cmd[i + 1] === "(") ||
        (!quote && /[<>]/.test(c) && cmd[i + 1] === "("));
    if (quote === "$'") {
      // ANSI-C quoting: decode escapes the way bash does (`$'\x2df'` is `-f`)
      if (c === "\\" && i + 1 < cmd.length) {
        const [ch, len] = ansiC(cmd.slice(i + 1));
        cur += ch;
        i += len;
      } else if (c === "'") quote = "";
      else cur += mark(c);
    } else if (!quote && c === "$" && cmd[i + 1] === "'") {
      quote = "$'";
      has = true;
      i++;
    } else if (c === "\\" && cmd[i + 1] === "\n" && quote !== "'") {
      i++; // a line continuation is removed before words are read (`a\<newline>#` is `a#`)
    } else if (!quote && c === "#" && !has && !arith) {
      // A comment runs nothing: skip to the end of the line.
      const nl = cmd.indexOf("\n", i);
      comments.push([i, nl < 0 ? cmd.length : nl]);
      if (nl < 0) break;
      i = nl - 1;
    } else if (nested && !quote && c === "$" && cmd[i + 1] === "{") {
      // `${x/)/}`, `${x:-)}`: a parameter expansion runs to its own `}`
      const e = braceEnd(cmd, i + 1);
      if (e >= cmd.length) return { segments, end: cmd.length };
      cur += cmd.slice(i, e + 1);
      has = true;
      i = e;
    } else if (nested && !quote && (c === "(" || c === ")")) {
      endToken(i);
      sep = "";
      if (cases) continue; // a case pattern's parens
      if (c === "(") depth++;
      else if (--depth === 1 && arith) {
        if (cmd[i + 1] === ")") return { segments, end: i + 1 };
        return lex(cmd, from, true, false); // `$((cmd) ...)`: a subshell, not arithmetic
      } else if (depth === 0) return { segments, end: i };
    } else if (open) {
      const at = c === "`" ? i : i + 1;
      const e = subEnd(cmd, at);
      const body = cmd.slice(at + 1, e);
      // in backticks `\``, `\$` and `\\` lose the backslash (nested backticks run too)
      const sub = c === "`" ? body.replace(/\\([\\`$])/g, "$1") : body;
      subs.push(sub);
      segSubs.push(sub);
      if (e >= cmd.length) subs.unclosed = true; // runs to the end: the guard cannot read it
      cur += cmd.slice(i, e + 1);
      has = true;
      i = e;
    } else if (quote) {
      if (c === quote) quote = "";
      else if (c === "\\" && quote === '"' && i + 1 < cmd.length) cur += mark(cmd[++i]);
      else cur += mark(c);
    } else if (c === "'" || c === '"') {
      quote = c;
      has = true;
    } else if (c === "\\" && i + 1 < cmd.length) {
      cur += mark(cmd[++i]);
      has = true;
    } else if (c === "<" && cmd.startsWith("<<<", i)) {
      endToken(); // a here-string: its word is data fed to the command
      cur = "<<<";
      has = true;
      i += 2;
    } else if (c === "<" && cmd[i + 1] === "<" && cmd[i + 2] !== "<" && !arith) {
      const m = /^<<(-?)[ \t]*((?:'[^']*'|"[^"]*"|\\.|[^\s;&|<>()'"\\])+)/.exec(cmd.slice(i));
      // `((y=1<<2))` shifts; a numeric delimiter is read as commands (fail closed either way)
      const sofar = nested ? cmd.slice(from, i) : `${tokens.join(" ")} ${cur}`;
      const shift = sofar.split("((").length > sofar.split("))").length;
      // nested: a heredoc the guard cannot read runs to the end (unclosed: fail closed)
      if (nested && !shift && (!m || /^\d+$/.test(m[2]))) return { segments, end: cmd.length };
      if (m && !/^\d+$/.test(m[2]) && !shift) {
        endToken();
        // The delimiter is the whole word minus its quotes (`"E"OF`, `E"OF"`, `\EOF` are EOF);
        // any quote or backslash in it makes the body data.
        const word = m[2].replace(/\$?'([^']*)'|\$?"([^"]*)"|\\(.)/g, "$1$2$3"); // `$'EOF'` too
        pending.push({ strip: m[1] === "-", word, data: /['"\\]/.test(m[2]) });
        i += m[0].length - 1;
      } else {
        cur += c;
        has = true;
      }
    } else if (c === "\n" && pending.length) {
      // Heredoc bodies are data, not commands; keep them only for a shell that reads them.
      let at = i + 1;
      for (const { strip, word, data } of pending) {
        let body = "";
        let closed = false;
        while (at < cmd.length) {
          let end = cmd.indexOf("\n", at);
          if (end < 0) end = cmd.length;
          const line = cmd.slice(at, end);
          at = end + 1;
          if ((strip ? line.replace(/^\t+/, "") : line) === word) {
            closed = true;
            break;
          }
          body += `${line}\n`;
        }
        if (nested && !closed) return { segments, end: cmd.length }; // unclosed: fail closed
        docs.push(body);
        if (!data) {
          const found = docSubs(body); // an unquoted delimiter runs $(...) and backticks
          subs.push(...found);
          segSubs.push(...found);
        }
        if (!data && /[$`]/.test(body)) expand = true;
      }
      pending = [];
      i = at - 1;
      endSegment(i);
    } else if (c === "|" && cur.endsWith(">")) {
      cur += c; // `>|` forces a redirect
    } else if (c === ";" || c === "|" || c === "\n") {
      endSegment(i);
    } else if (c === "&" && !cur.endsWith(">") && !cur.endsWith("<")) {
      endSegment(i);
    } else if (c === " " || c === "\t") {
      endToken(i);
    } else {
      cur += c;
      has = true;
    }
  }
  endSegment(cmd.length);
  segments.subs = subs;
  segments.orphanSubs = segSubs; // substitutions in no command's words
  segments.comments = comments;
  return { segments, end: cmd.length };
}

/** Words before a command that keep command position (`then case ...`, `time -p case ...`). */
const CASE_PREFIX = new Set("if while until then do else elif ! { ( time coproc".split(" "));
/** Index of a `case` keyword after keyword prefixes, or -1. */
function caseAt(tokens) {
  let k = 0;
  while (
    k < tokens.length &&
    (CASE_PREFIX.has(tokens[k]) || (tokens[k] === "-p" && tokens[k - 1] === "time"))
  )
    k++;
  return base(String(tokens[k] ?? "").replace(/^[({]+/, "")) === "case" ? k : -1;
}

/** Marks a token list as run through a wrapper (not enumerable: lists still compare as words). */
const wrap = (t, on) =>
  Object.defineProperty(t, "wrapped", { value: !!on, writable: true, configurable: true });

/** Prefixes stripped (`stripCore`). The result's `wrapped` is true when a program (not the shell
 * itself) runs the command with execvp, which hands a file the kernel refuses to /bin/sh: `env`,
 * `timeout`, `nice`, `nohup`, `sudo`, `xargs`, ... Through `xargs` the program must stay literal:
 * none left after its wrappers (`xargs env`), `{}`, the replace string or a variable is refused. */
function stripPrefixes(tokens, inCase = false) {
  const st = { wrapped: false, xargs: null };
  const t = stripCore(tokens, inCase, st);
  if (st.xargs) {
    const p = st.n === 0 ? undefined : t[0];
    const { rep } = st.xargs;
    if (
      p === undefined ||
      /[$`{}]/.test(p) ||
      (rep !== undefined && p.includes(rep)) ||
      SHELLS.has(base(p))
    )
      return ["@deny", "unscanned", tokens.join(" ")];
  }
  return wrap(t, st.wrapped);
}

function stripCore(tokens, inCase, st) {
  // A function definition runs its body when called: `f() { cmd; }`, `function f { cmd; }`.
  if (tokens[0] === "function") tokens = tokens.slice(2);
  else if (/^[^\s()$`'"]+\(\)[{(]?$/.test(tokens[0] ?? "")) tokens = tokens.slice(1);
  if (tokens[0] === "()") tokens = tokens.slice(1);
  // Inside `case`, a lone `pat)` on its own line is a pattern, not a command.
  if (inCase && tokens.length === 1 && /^[^()]*\)$/.test(tokens[0])) return [];
  // `then case w in pat) cmd`, `{ case ...`: a keyword prefix before `case` is dropped first.
  const k0 = caseAt(tokens);
  if (k0 > 0) tokens = [tokens[k0].replace(/^[({]+/, ""), ...tokens.slice(k0 + 1)];
  // `case w in pat) cmd` and a later `pat) cmd` branch run `cmd`.
  const afterPattern = (x) => x.slice(x.indexOf(")") + 1); // `pat)cmd` runs cmd
  if (base(tokens[0] ?? "") === "case") {
    const k = tokens.findIndex((x, n) => n > 2 && x.includes(")"));
    tokens = k > 0 ? [afterPattern(tokens[k]), ...tokens.slice(k + 1)].filter(Boolean) : [];
  } else if (/^[^()]*\)./.test(tokens[0] ?? ""))
    tokens = [afterPattern(tokens[0]), ...tokens.slice(1)];
  else if (tokens.length > 1 && /^[^()]*\)$/.test(tokens[0])) tokens = tokens.slice(1);
  let t = tokens
    .map((x, i) => (i === 0 ? x.replace(/^[({]+/, "") : x).replace(/[)}]+$/, ""))
    .filter((x) => x !== "");
  const skipOptions = (valueFlags = []) => {
    while (t[0]?.startsWith("-")) t = t.slice(valueFlags.includes(t[0]) ? 2 : 1);
  };
  const skipEnvOptions = () => {
    while (t[0]?.startsWith("-")) {
      const o = t[0];
      if (o === "-S" || o === "--split-string") {
        t = [...split(t[1] ?? "").flat(), ...t.slice(2)];
        return;
      }
      const inline = /^(?:-S|--split-string=)(.+)$/.exec(o);
      if (inline) {
        t = [...split(inline[1]).flat(), ...t.slice(1)];
        return;
      }
      t = t.slice(["-u", "--unset", "-C", "--chdir"].includes(o) ? 2 : 1);
    }
  };
  // A redirect before the command name (`< f bash`, `2>/dev/null cmd`) moves after it.
  const tail = [];
  let shellIfEmpty = false; // `sudo -s`, `unshare`, `chroot d` with no command start a shell
  const done = () => {
    st.n = t.length; // words of the command itself (redirects in `tail` are not)
    return !t.length && shellIfEmpty ? ["sh", ...tail] : tail.length ? [...t, ...tail] : t;
  };
  // `-c <code>` of a wrapper that runs a shell (`su -c`, `script -c`, `flock -c`).
  const shellC = (from) => {
    for (let n = from; n < t.length; n++) {
      const x = t[n];
      const eq = /^--command=(.*)$/s.exec(x);
      if (eq) return ["sh", "-c", eq[1]];
      if (x === "--command" || /^-[a-zA-Z]*c$/.test(x)) return ["sh", "-c", t[n + 1] ?? ""];
    }
    return null;
  };
  for (;;) {
    const raw = t[0];
    if (raw === undefined) return done();
    const a = base(raw);
    const b = t[1];
    if (/^(\d*|&)?(<<<|<>|>>|>\||>&|<&|&>>|&>|<(?!\()|>(?!\())/.test(raw)) {
      const bare = /^(\d*|&)?(<<<|<>|>>|>\||>&|<&|&>>|&>|<|>)$/.test(raw);
      tail.push(...t.slice(0, bare ? 2 : 1));
      t = t.slice(bare ? 2 : 1);
    } else if (/^[({]+$/.test(raw)) t = t.slice(1);
    else if (/^[A-Za-z_][A-Za-z0-9_]*=/.test(raw)) t = t.slice(1);
    else if (
      [
        "if",
        "while",
        "until",
        "then",
        "do",
        "else",
        "elif",
        "!",
        "command",
        "builtin",
        "time",
        "nohup",
        "setsid",
      ].includes(a)
    ) {
      if (a === "nohup" || a === "setsid") st.wrapped = true;
      t = t.slice(1);
      skipOptions();
    } else if (a === "exec") {
      t = t.slice(1);
      skipOptions(["-a"]);
    } else if (a === "coproc") {
      // `coproc NAME { cmd; }` or `coproc cmd`
      t = t.slice(1);
      if (/^[A-Za-z_]\w*$/.test(t[0] ?? "") && /^[({]/.test(t[1] ?? "")) t = t.slice(1);
      if (/^[({]+./.test(t[0] ?? "")) t = [t[0].replace(/^[({]+/, ""), ...t.slice(1)];
    } else if (a === "env") {
      st.wrapped = true;
      t = t.slice(1);
      skipEnvOptions();
    } else if (a === "npx") {
      st.wrapped = true;
      t = t.slice(1);
      skipOptions();
    } else if (a === "sudo") {
      st.wrapped = true;
      t = t.slice(1);
      skipOptions(["-u", "-g", "-h", "-p", "-C", "-D", "-R", "-T", "-U", "--chdir", "--user"]);
      shellIfEmpty = true;
    } else if (a === "nice") {
      st.wrapped = true;
      t = t.slice(1);
      skipOptions(["-n", "--adjustment"]);
    } else if (a === "ionice") {
      st.wrapped = true;
      t = t.slice(1);
      skipOptions(["-c", "-n", "-p", "-P", "-u", "--class", "--classdata", "--pid"]);
    } else if (a === "stdbuf") {
      st.wrapped = true;
      t = t.slice(1);
      skipOptions(["-i", "-o", "-e", "--input", "--output", "--error"]);
    } else if (a === "doas") {
      st.wrapped = true;
      t = t.slice(1);
      skipOptions(["-u", "-C"]);
    } else if (a === "busybox" || a === "toybox") {
      st.wrapped = true;
      t = t.slice(1);
    } else if (a === "watch") {
      st.wrapped = true;
      t = t.slice(1);
      skipOptions(["-n", "--interval"]);
      if (t.length === 1 && /\s/.test(t[0])) t = split(t[0]).flat();
    } else if (a === "xargs") {
      // the replace string (`-I R`, `-IR`, `-i[R]`, `--replace[=R]`) stands for stdin text
      st.wrapped = true;
      t = t.slice(1);
      let rep;
      const valued =
        "--arg-file --delimiter --max-args --max-lines --max-procs --max-chars --eof --process-slot-var".split(
          " ",
        );
      // getopt: a short group is bools, then one option that takes the rest or the next word
      // (`-tIX X` is `-t -I X`); `-e`, `-i`, `-l` take only an attached value.
      while (t[0]?.startsWith("-") && t[0] !== "-") {
        const o = t[0];
        t = t.slice(1);
        if (o === "--") break;
        if (o.startsWith("--")) {
          const eq = /^(--[\w-]+)=(.*)$/s.exec(o);
          const name = eq ? eq[1] : o;
          if (name === "--replace") rep = eq ? eq[2] : "{}";
          else if (!eq && valued.includes(name)) t = t.slice(1);
          continue;
        }
        for (let k = 1; k < o.length; k++) {
          const c = o[k];
          const rest = o.slice(k + 1);
          if ("adEILnPs".includes(c)) {
            const v = rest || (t[0] ?? "");
            if (!rest) t = t.slice(1);
            if (c === "I") rep = v;
            break;
          }
          if ("eil".includes(c)) {
            if (c === "i") rep = rest || "{}";
            break;
          }
        }
      }
      if (!t.length) return done(); // xargs alone runs echo
      st.xargs = { rep };
    } else if (a === "timeout") {
      st.wrapped = true;
      t = t.slice(1);
      skipOptions(["-s", "-k", "--signal", "--kill-after"]);
      t = t.slice(1);
    } else if (a === "flock") {
      st.wrapped = true;
      // `flock [opts] file cmd...` or `flock [opts] file -c cmd`
      const c = shellC(1);
      if (c) return [...c, ...tail];
      t = t.slice(1);
      skipOptions(["-w", "--wait", "--timeout", "-E", "--conflict-exit-code"]);
      t = t.slice(1);
    } else if (a === "taskset") {
      st.wrapped = true;
      t = t.slice(1);
      if (t.some((x) => /^-[a-zA-Z]*p[a-zA-Z]*$|^--pid$/.test(x))) return done(); // changes a pid
      skipOptions();
      t = t.slice(1); // the mask or cpu list
    } else if (a === "chrt") {
      st.wrapped = true;
      t = t.slice(1);
      if (t.some((x) => /^-[a-zA-Z]*p[a-zA-Z]*$|^--pid$/.test(x))) return done();
      skipOptions(["-T", "-P", "-D", "--sched-runtime", "--sched-period", "--sched-deadline"]);
      if (/^\d+$/.test(t[0] ?? "")) t = t.slice(1);
    } else if (a === "unshare" || a === "chroot") {
      st.wrapped = true;
      t = t.slice(1);
      skipOptions(
        "-S -G -R -w --setuid --setgid --root --wd --propagation --map-user --map-group --map-users --map-groups --userspec --groups".split(
          " ",
        ),
      );
      if (a === "chroot") t = t.slice(1);
      shellIfEmpty = true;
    } else if (a === "script" || a === "su" || a === "runuser") {
      // only `-c` runs a command the guard can read; without it they start an interactive shell
      // without `-c` they start a shell that reads stdin
      const c = shellC(1);
      return c ? [...c, ...tail] : ["sh", ...tail];
    } else if (a === "pnpm" && (b === "exec" || b === "dlx")) {
      st.wrapped = true;
      t = t.slice(2);
      skipOptions();
    } else if (a === "npm" && (b === "exec" || b === "x")) {
      st.wrapped = true;
      // `npm exec [opts] [--] cmd`, `npm exec -c 'cmd'` (a shell runs it)
      t = t.slice(2);
      const dd = t.indexOf("--");
      for (let n = 0; n < (dd < 0 ? t.length : dd); n++) {
        const eq = /^--call=(.*)$/s.exec(t[n]);
        if (eq) return ["sh", "-c", eq[1], ...tail];
        if (t[n] === "-c" || t[n] === "--call") return ["sh", "-c", t[n + 1] ?? "", ...tail];
      }
      if (dd >= 0) t = t.slice(dd + 1);
      else {
        // without `--`, an option the guard does not know may or may not take a value
        const flags =
          /^(-y|--yes|--no|-q|--quiet|-s|--silent|--ws|--workspaces|--include-workspace-root|--no-[\w-]+|--[\w-]+=.*)$/s;
        const valued = ["-p", "--package", "-w", "--workspace", "--prefix"];
        while (t[0]?.startsWith("-")) {
          if (valued.includes(t[0])) t = t.slice(2);
          else if (flags.test(t[0])) t = t.slice(1);
          else return ["@deny", "unscanned", `npm exec ${t.join(" ")}`];
        }
      }
    } else if (a === "nix-shell") {
      // `nix-shell [opts] --run 'cmd'` runs cmd in a shell; without it, an interactive shell
      for (let n = 1; n < t.length; n++) {
        const eq = /^--(?:run|command)=(.*)$/s.exec(t[n]);
        if (eq) return ["sh", "-c", eq[1], ...tail];
        if (t[n] === "--run" || t[n] === "--command") return ["sh", "-c", t[n + 1] ?? "", ...tail];
      }
      return ["sh", ...tail];
    } else if (a === "nix" && (b === "develop" || b === "shell")) {
      st.wrapped = true;
      // `nix develop [opts] -c cmd args`: argv after `-c`/`--command`; without it, a shell
      const k = t.findIndex((x) => x === "-c" || x === "--command");
      if (k < 0) return ["sh", ...tail];
      t = t.slice(k + 1);
    } else if (a === "at" || a === "batch") {
      // at/batch run their stdin (a heredoc, here-string, pipe) or `-f file` with sh later;
      // options are getopt bundles: `-f file`, `-ffile`, `-mf file`, `-q queue`, `-t time`
      for (let n = 1; n < t.length && /^-./.test(t[n]); n++) {
        const o = t[n];
        for (let k = 1; k < o.length; k++) {
          if (o[k] === "f") return ["sh", o.slice(k + 1) || (t[n + 1] ?? ""), ...tail];
          if ("lrdc".includes(o[k])) return done(); // list, remove, show jobs
          if ("qt".includes(o[k])) {
            if (k === o.length - 1) n++;
            break;
          }
        }
      }
      const input = t.flatMap((x, n) =>
        /^\d*<(?!\()/.test(x) ? (/^\d*<<?<?$/.test(x) ? [x, t[n + 1] ?? ""] : [x]) : [],
      );
      return ["sh", ...input, ...tail];
    } else return done();
  }
}

/** Simple commands in `cmd` as token lists, prefixes stripped, `bash -c '<s>'` expanded recursively.
 * Pseudo-commands `@evade` (text with a risky word is refused) and `@evade-exec` (same, for code
 * bodies) mark constructs the guard cannot read. */
const fed = (x) => /^(\/dev\/(stdin|fd\/\d+)|\/proc\/[^/]+\/fd\/\d+|-|<\(.*)$/.test(x);

/** Commands another shell runs (`sh -c`, a heredoc, a runner's text): /bin/sh may be dash, which
 * runs a file the kernel refuses as a script, so they count as run through a wrapper. */
const viaSh = (list) => list.map((x) => wrap(x, true));

/** A word as typed that the shell changes when it runs: a substitution (`$(`, backtick, `${`),
 * ANSI-C or locale quoting (`$'`, `$"`), a brace expansion (`{a,b}`, `{a..b}`) or a line
 * continuation. Quoted text is data (`'{a,b}'`, `'$(x)'`; braces in double quotes too). */
function changesAtRunTime(raw) {
  if (raw.includes("\\\n")) return true;
  let quote = "";
  let plain = ""; // the unquoted text, quoted parts blanked
  for (let i = 0; i < raw.length; i++) {
    const c = raw[i];
    if (quote === "'") {
      if (c === "'") quote = "";
      plain += " ";
      continue;
    }
    if (c === "\\") {
      i++;
      plain += "  ";
      continue;
    }
    if (c === "$" && /[('{"`]/.test(raw[i + 1] ?? "")) return true;
    if (c === "`") return true;
    if (c === '"') quote = quote ? "" : '"';
    else if (!quote && c === "'") quote = "'";
    plain += quote || c === '"' ? " " : c;
  }
  return /\{[^{}\s]*(,|\.\.)[^{}\s]*\}/.test(plain);
}

/** A word with its substitution bodies cut (`$(cat <<EOF ... EOF)` is `$()`): the command's
 * own words, not the text it is given. */
function bare(word) {
  let out = "";
  for (let i = 0; i < word.length; i++) {
    const c = word[i];
    if (c === "`" || (c === "$" && word[i + 1] === "(")) {
      i = subEnd(word, c === "`" ? i : i + 1);
      out += "$()";
    } else out += c;
  }
  return out;
}

export function commands(cmd) {
  const text = String(cmd ?? "");
  const out = [];
  const DUMP_RE = /process\.env(?![.[\w])|os\.environ(?![.[\w])|\bENVIRON\b/;
  const secretTokens = (body) => {
    if (DUMP_RE.test(body)) out.push(["printenv"]);
    // String pieces in order, joined two and three at a time: a path built from parts.
    const parts = [...body.matchAll(/(['"`])([^'"`\n]*)\1/g)].map((m) => m[2]);
    const built = parts.flatMap((p, i) => [
      p + (parts[i + 1] ?? ""),
      p + (parts[i + 1] ?? "") + (parts[i + 2] ?? ""),
    ]);
    for (const text of [body, joinStrings(body), built.join(" "), unescape(joinStrings(body))])
      for (const m of text.matchAll(/[^\s"'`()<>=,;:{}[\]]+/g)) {
        if (secretLike(m[0])) out.push(["cat", m[0]]);
      }
  };
  const segs = split(text);
  if (segs.subs.unclosed) out.push(["@evade", text]);
  for (const sub of segs.orphanSubs) for (const inner of commands(sub)) out.push(inner);
  let caseDepth = 0;
  for (const seg of segs) {
    // A substitution runs right before its command, after the earlier `cd` and `PATH=`.
    for (const sub of seg.subs ?? []) for (const inner of commands(sub)) out.push(inner);
    if (caseAt(seg) >= 0) caseDepth++;
    const stripped = stripPrefixes(seg, caseDepth > 0);
    const marked = stripped.flatMap(splitRedirect);
    const t = marked.map(unmark);
    wrap(t, stripped.wrapped);
    // The words with their quoted `<`/`>` still marked: writesTarget reads a redirect only there.
    Object.defineProperty(t, "marked", { value: marked, configurable: true });
    // `GH_HOST=x gh ...`, `env GH_TOKEN=x gh ...`: gh talks to another host or as another user.
    const prefix = seg.slice(0, Math.max(0, seg.length - stripped.length));
    const ghEnv = prefix.some((x) => GH_ENV_RE.test(unmark(x)));
    Object.defineProperty(t, "ghEnv", { value: ghEnv, configurable: true });
    if (seg.some((x) => /^esac\b/.test(x)) && caseDepth > 0) caseDepth--;
    // An assignment that makes git or a pager run a program (`GIT_EXTERNAL_DIFF=x git diff`).
    if (seg.some((x) => RUNS_ENV_RE.test(x))) out.push(["@runs-env", seg.join(" ")]);
    // `GIT_SSH_COMMAND='bash f' git fetch`, `GIT_CONFIG_KEY_0=alias.x`: a program the guard cannot see
    const envHit = seg.find((x) => envRunsProgram(unmark(x)));
    if (envHit) out.push(["@deny", "gitProgram", envHit]);
    // `PATH=dir:$PATH cmd`, `export PATH=...`: a bare name may now run a script in dir.
    for (const x of seg) {
      const m = /^PATH\+?=(.*)$/s.exec(unmark(x));
      if (m) out.push(["@path", m[1]]);
      // A shell runs this file first: BASH_ENV (any later bash), ENV (sh started here or after export).
      const env = /^(BASH_ENV|ENV)=(.+)$/s.exec(unmark(x));
      const shellHere = SHELLS.has(base(stripPrefixes(seg)[0] ?? "")) || seg[0] === "export";
      if (env && (env[1] === "BASH_ENV" || shellHere)) out.push(["source", env[2]]);
    }
    if (!t.length) {
      // A bare `env` (options and assignments only) prints the environment.
      if (base(seg[0].replace(/^[({]+/, "")) === "env") out.push(["printenv"]);
      continue;
    }
    if (seg.docs) t.docs = seg.docs;
    // `ls -d`, `grep -o`, `sort -t`: these flags take no directory in read-only tools.
    if (!NO_DIR_FLAGS.has(base(t[0]))) for (const d of chdirs(seg)) out.push(["@chdir", d]);
    out.push(t);
    const prog = base(t[0]);
    const segText = seg.join(" ");
    // A risky command (push, publish, merge, release, secrets) with a word that changes at run
    // time (`publish$()`, `--force${x}`, `HEAD:{main,x}`) is not what the guard read.
    if (seg.raws?.some(changesAtRunTime) && RISKY_RE.test(seg.map(bare).join(" ")))
      out.push(["@deny", "evasion", segText]);
    // `xargs git ...` takes its last arguments from stdin; `find -exec cmd ... ;` runs cmd.
    const viaXargs = base(seg[0].replace(/^[({]+/, "")) === "xargs";
    if (viaXargs) out.push(["@evade", text]); // args come from the pipe
    if (viaXargs && (SHELLS.has(prog) || prog === "source" || prog === "."))
      out.push(["@deny", "unscanned", segText]); // the script path comes from the pipe
    if (prog === "alias" && t.slice(1).some((x) => x.includes("=")))
      out.push(["@deny", "alias", segText]);
    if (SHELLS.has(prog))
      for (let n = 1; n < t.length; n++) {
        const rc = /^--?(?:rcfile|init-file)(?:=(.*))?$/s.exec(t[n]); // bash: `-rcfile` too
        if (rc) out.push(["source", rc[1] ?? t[n + 1] ?? ""]);
      }
    if (SHELLS.has(prog) && !viaXargs) {
      // The code a shell runs must be text the guard reads: a `-c` string, a script file, a
      // `<file`, a heredoc or here-string with nothing expanded. A pipe or a variable is refused.
      const src = shellSource(t);
      const herestring = t.find((x) => x.startsWith("<<<"));
      const hsBody =
        herestring && (herestring.length > 3 ? herestring : t[t.indexOf(herestring) + 1]);
      if (
        (src.kind === "stdin" && !src.stdinFile) ||
        (src.kind === "file" && fed(src.file)) ||
        (src.kind === "herestring" && /[$`]/.test(hsBody ?? "")) ||
        (src.kind === "doc" && seg.docExpand)
      )
        out.push(["@deny", "unscanned", segText]);
    }
    if ((prog === "source" || prog === ".") && (t.length === 1 || fed(t[1])))
      out.push(["@deny", "unscanned", segText]);
    // `source /dev/stdin`, `bash <(...)`, `xargs sh -c`: the code comes from elsewhere.
    if (
      ((prog === "source" || prog === ".") && (t.length === 1 || fed(t[1]))) ||
      ((SHELLS.has(prog) || INTERPRETERS.test(prog)) &&
        t.slice(1).some(fed) &&
        !seg.docs?.length) ||
      (base(seg[0].replace(/^[({]+/, "")) === "xargs" &&
        (SHELLS.has(prog) || INTERPRETERS.test(prog)))
    )
      out.push(["@evade", text]);
    // `sort --compress-program=P` runs P: only a known compressor passes.
    if (prog === "sort")
      for (let e = 1; e < t.length; e++) {
        const m = /^--com[a-z-]*(?:=(.*))?$/s.exec(t[e]);
        if (!m) continue;
        const p = m[1] ?? t[e + 1] ?? "";
        if (!/^(gzip|pigz|xz|zstd|lz4|bzip2|lzop)$/.test(p))
          out.push(["@deny", "unscanned", segText]);
      }
    if (prog === "find")
      for (let e = 0; e < t.length; e++) {
        if (!/^-(exec|execdir|ok|okdir)$/.test(t[e])) continue;
        const end = t.findIndex((x, n) => n > e && (x === ";" || x === "+"));
        const inner = stripPrefixes(t.slice(e + 1, end < 0 ? undefined : end));
        wrap(inner, true); // find runs it with execvp
        // `-exec {} ;`, `-exec env {} ;`, `-exec "$P" ;`: the program is not a literal
        if (!inner.length || /[$`{}]/.test(inner[0])) out.push(["@deny", "unscanned", segText]);
        if (inner.length) out.push(inner);
        // -execdir runs in each match's dir: a relative script there is unknown here
        const ip = base(inner[0] ?? "");
        if (
          /^-(execdir|okdir)$/.test(t[e]) &&
          (SHELLS.has(ip) || ip === "source" || String(inner[0]).includes("/"))
        )
          out.push(["@deny", "unscanned", segText]);
        const ci = inner.findIndex((x, n) => n > 0 && SHELL_C_RE.test(x));
        if (SHELLS.has(base(inner[0] ?? "")) && ci > 0 && inner[ci + 1] !== undefined)
          for (const x of viaSh(commands(inner[ci + 1]))) out.push(x);
        if (end > e) e = end;
      }
    // `/proc/self/exe`, `/dev/fd/3` as the program: the guard cannot know what runs
    if (/^\/(proc|dev)\//.test(t[0])) out.push(["@deny", "unscanned", segText]);
    // The program comes from a command's output (`$(cat f) ...`, `` `x` ``): code the guard cannot read.
    if (/^`|^\$\(/.test(t[0])) out.push(["@deny", "dynamic", segText]);
    if (/^[$`]|\$\(/.test(t[0])) {
      out.push(["@evade", segText]);
      // `$SHELL -c '...'`: the program may be a shell, so its code is read like one
      const ci = t.findIndex((x, n) => n > 0 && SHELL_C_RE.test(x));
      if (ci > 0 && t[ci + 1] !== undefined) for (const x of commands(t[ci + 1])) out.push(x);
    }
    // A wrapper that changes dir (`env -C d`, `sudo -D d`, `unshare -w d`) before a script run:
    // the script path resolves where the guard cannot follow.
    const pre = seg.slice(0, Math.max(0, seg.indexOf(t[0])));
    if (
      pre.some((x) => /^(-C|--chdir|-D|-w|--wd)(=|$)|^-C./.test(x)) &&
      (SHELLS.has(prog) || prog === "source" || prog === "." || t[0].includes("/"))
    )
      out.push(["@deny", "unscanned", segText]);
    // An unknown program given a command line as text runs it (`parallel 'git push ...'`).
    if (
      !ARGS_ARE_DATA.has(prog) &&
      !SHELLS.has(prog) &&
      !INTERPRETERS.test(prog) &&
      !t[0].startsWith("@")
    )
      if (EXEC_RE.test(t.slice(1).join(" "))) out.push(["@evade-exec", t.slice(1).join(" ")]);
    // `git $SUB ...` hides the subcommand; `git -C "$D" push` hides where it runs. A variable after
    // the subcommand is an argument.
    if (prog === "git") {
      const { sub, args } = gitSub(t);
      const hidden = (x) => /^[$`]|\$\(/.test(x ?? "");
      if (hidden(sub)) out.push(["@evade", text]);
      // `git -c alias.x='!cmd' x`: the alias runs cmd like a shell
      const al = gitSub(t)
        .cfg.map((c) => /^alias\.([^=]+)=!(.*)$/is.exec(c))
        .find((m) => m && m[1].toLowerCase() === String(sub).toLowerCase());
      if (al) for (const x of commands(`${al[2]} ${gitSub(t).args.join(" ")}`)) out.push(x);
      else if (t.slice(1, t.length - args.length - 1).some(hidden)) out.push(["@evade", segText]);
      // Runners: `rebase -x/--exec cmd` and `submodule foreach cmd` run cmd with sh; `bisect run
      // cmd args` runs argv. Their command text is read like typed commands.
      const run = [];
      if (sub === "rebase")
        for (let n = 0; n < args.length && args[n] !== "--"; n++) {
          const a = args[n];
          const long = /^--ex(?:e(?:c)?)?(?:=(.*))?$/s.exec(a); // git takes a unique prefix
          if (long) run.push(long[1] ?? args[++n] ?? "");
          else if (/^-[^-]/.test(a))
            // a bundle (`-ix cmd`, `-xcmd`); `-s`, `-X`, `-C` take the rest as their value
            for (let k = 1; k < a.length; k++) {
              if ("sXC".includes(a[k])) {
                if (k === a.length - 1) n++;
                break;
              }
              if (a[k] === "x") {
                run.push(k < a.length - 1 ? a.slice(k + 1) : (args[++n] ?? ""));
                break;
              }
            }
        }
      // `bisect run` passes its argv quoted: keep each word one word
      const q = (x) => `'${x.replaceAll("'", "'\\''")}'`;
      if (sub === "bisect" && args[0] === "run") run.push(args.slice(1).map(q).join(" "));
      if (sub === "submodule" && args.includes("foreach"))
        run.push(
          args
            .slice(args.indexOf("foreach") + 1)
            .filter(
              (x, n, all) => !all.slice(0, n + 1).every((y) => /^--(recursive|quiet)$/.test(y)),
            )
            .join(" "),
        );
      for (const body of run) for (const x of viaSh(commands(body))) out.push(x);
    }
    // `expect -c 'spawn git push ...'`: Tcl `spawn`/`exec`/`system` run programs; read them as
    // commands (other Tcl is not parsed; a risky word in it is refused as `@evade-exec`).
    if (prog === "expect")
      for (let n = 1; n < t.length; n++)
        if (t[n] === "-c" && t[n + 1] !== undefined) {
          const body = t[n + 1];
          out.push(["@evade-exec", body]);
          const sh = body
            .replace(/[{}[\]"]/g, "\n")
            .replace(/(^|[\n;])\s*(spawn|exec|system)\s+/g, "$1\n");
          for (const x of viaSh(commands(sh))) out.push(x);
        }
    if (prog === "trap") {
      // `trap '<code>' SIG`: the code runs later, read like typed commands
      const body = t.slice(1).find((x) => !/^-[-pl]?$/.test(x));
      if (body !== undefined) {
        out.push(["@evade", body]);
        for (const inner of commands(body)) out.push(inner);
      }
    }
    if (prog === "eval") {
      const body = t.slice(1).join(" ");
      out.push(["@evade", body]);
      // code from a value or output; the output of a known env-setup tool is that tool's own work
      const tool = /^"?\$\(\s*([\w./-]+)[^$`()]*\)?"?$/.exec(body.trim())?.[1]; // `)` may be cut
      // (the tool itself is read as a substitution of this command)
      if (!ENV_PRINTERS.has(base(tool ?? ""))) {
        if (/[$`]/.test(body)) out.push(["@deny", "dynamic", segText]);
        for (const inner of commands(body)) out.push(inner);
      }
    }
    const noArgs = t.slice(1).every((x) => x === "-" || x === "-s") && !seg.docs?.length;
    // stripPrefixes drops a token's trailing `)`: give it back before the inert test.
    // perl/ruby -n/-p loop over stdin with implicit variables: never inert.
    const loops = /^(perl|ruby)/.test(prog) && t.some((x) => /^-[a-zA-Z]*[np][a-zA-Z]*$/.test(x));
    const runs = (body) => loops || ![body, `${body})`, `${body}))`].some((b) => inert(b, prog));
    // `bash <<< 'cmd'`: the here-string is evaluated.
    const hs = t.findIndex((x) => x.startsWith("<<<"));
    if (hs >= 0 && (SHELLS.has(prog) || INTERPRETERS.test(prog))) {
      const body = t[hs].length > 3 ? t[hs].slice(3) : (t[hs + 1] ?? "");
      if (SHELLS.has(prog)) for (const inner of viaSh(commands(body))) out.push(inner);
      if (SHELLS.has(prog) || runs(body)) out.push(["@evade", body], ["@evade-exec", body]);
    }
    if (INTERPRETERS.test(prog)) {
      const i =
        prog === "deno" && t[1] === "eval"
          ? 1
          : t.findIndex((x, n) => n > 0 && (INLINE_RE.test(x) || (prog === "php" && x === "-r")));
      if (i > 0 && t[i + 1] !== undefined) {
        if (runs(t[i + 1])) out.push(["@evade", loops ? text : t[i + 1]]); // -n/-p: stdin is code
        if (runs(t[i + 1]) && WHOLE_ENV_RE.test(t[i + 1])) out.push(["printenv"]);
        secretTokens(t[i + 1]);
      } else if (noArgs) out.push(["@evade", text]); // code fed on stdin
      // Heredoc code is not scanned like a shell, only for reads of secret files and exec text.
      for (const body of seg.docs ?? []) {
        if (runs(body)) out.push(["@evade-exec", body]);
        if (runs(body) && WHOLE_ENV_RE.test(body)) out.push(["printenv"]);
        secretTokens(body);
      }
    }
    if (SHELLS.has(prog)) {
      for (const body of seg.docs ?? []) {
        for (const inner of viaSh(commands(body))) out.push(inner);
      }
      const i = t.findIndex((x, n) => n > 0 && SHELL_C_RE.test(x));
      if (i > 0 && t[i + 1] !== undefined) {
        // `bash -c "$X"`, `bash -c "$(cat f)"`: the code is a value the guard cannot read
        if (/^\s*[$`]/.test(t[i + 1])) out.push(["@deny", "dynamic", segText]);
        for (const inner of viaSh(commands(t[i + 1]))) out.push(inner);
      } else if (noArgs) out.push(["@evade", text]); // script fed on stdin
    }
  }
  return out;
}

function isSecretName(name) {
  if (!name || SECRET_SAFE.has(name)) return false;
  if (name === ".env" || name.startsWith(".env.") || name === ".envrc") return true;
  if (name === ".dev.vars" || name.startsWith(".dev.vars.")) return true;
  if (/^[\w-]+\.env$/.test(name) && !/^(process|Deno|Bun)\.env$/.test(name)) return true;
  if (name.endsWith(".pem") || name === "keys.txt" || /^id_[a-z0-9]+$/.test(name)) return true;
  if (/[*?[{]/.test(name)) {
    const prefix = name.slice(0, name.search(/[*?[{]/));
    if (prefix.length >= 2 && (".env".startsWith(prefix) || ".dev.vars".startsWith(prefix)))
      return true; // `.e*`, `.en?` can expand to .env
    return (
      prefix.length > 0 &&
      !SECRET_SAFE.has(prefix) &&
      (prefix.startsWith(".env") || prefix.startsWith(".dev.vars"))
    );
  }
  return false;
}

function secretLike(value) {
  if (typeof value !== "string" || !value) return false;
  const clean = value.replace(/^[<>]+/, "");
  const variants = [clean, clean.slice(clean.lastIndexOf("=") + 1), clean.split(/[<>]/).pop()];
  variants.push(...variants.map((v) => posix.normalize(v))); // `/proc/self/./environ`
  for (const v of new Set(variants)) {
    if (isSecretName(v.slice(v.lastIndexOf("/") + 1)) || SECRET_DIR_RE.test(v.replace(/\/+$/, "")))
      return true;
  }
  return false;
}

function gitSub(t) {
  let i = 1;
  const cfg = [];
  while (i < t.length && t[i].startsWith("-")) {
    const takesValue = ["-C", "-c", "--git-dir", "--work-tree", "--namespace"].includes(t[i]);
    if (t[i] === "-c" && t[i + 1] !== undefined) cfg.push(t[i + 1]);
    i += takesValue ? 2 : 1;
  }
  return { sub: t[i], args: t.slice(i + 1), cfg };
}

/** Directories that hold refs: the repo's .git (or a worktree's gitdir and its common dir). */
function gitDirs(projectDir) {
  const dirs = [];
  try {
    const dot = resolve(projectDir, ".git");
    if (statSync(dot).isDirectory()) dirs.push(dot);
    else {
      const gd = /^gitdir:\s*(.+)$/m.exec(readFileSync(dot, "utf8"))?.[1];
      if (gd) {
        const dir = resolve(projectDir, gd.trim());
        dirs.push(dir);
        const cd = resolve(dir, readFileSync(resolve(dir, "commondir"), "utf8").trim());
        dirs.push(cd);
      }
    }
  } catch {
    // not a git checkout, or unreadable: no known tags
  }
  return dirs;
}

/** True when `name` is an existing tag (loose ref or packed-refs); a bare push dst can be one. */
function isTagName(name, projectDir) {
  if (!name || name.includes("..") || name.startsWith("/")) return false;
  const short = name.replace(/^refs\/tags\//, "");
  for (const dir of gitDirs(projectDir)) {
    if (existsSync(resolve(dir, "refs/tags", short))) return true;
    try {
      const packed = readFileSync(resolve(dir, "packed-refs"), "utf8");
      if (packed.split("\n").some((l) => l.endsWith(` refs/tags/${short}`))) return true;
    } catch {
      // no packed-refs
    }
  }
  return false;
}

/** `main`, `master` and the branch origin/HEAD points at. */
function isDefault(name, projectDir) {
  const n = String(name)
    .replace(/^\+/, "")
    .replace(/^refs\/heads\//, "");
  if (n === "main" || n === "master") return true;
  for (const dir of gitDirs(projectDir)) {
    try {
      const head = readFileSync(resolve(dir, "refs/remotes/origin/HEAD"), "utf8");
      if (/^ref:\s*refs\/remotes\/[^/\s]+\/(.+)$/m.exec(head)?.[1].trim() === n) return true;
    } catch {
      // no origin/HEAD
    }
  }
  return false;
}

/** True when the wildcard ref `dst` matches a default branch (main, master, origin/HEAD's). */
function wildcardHitsDefault(dst, projectDir) {
  const names = ["main", "master"];
  for (const dir of gitDirs(projectDir)) {
    try {
      const head = readFileSync(resolve(dir, "refs/remotes/origin/HEAD"), "utf8");
      const m = /^ref:\s*refs\/remotes\/[^/\s]+\/(.+)$/m.exec(head);
      if (m) names.push(m[1].trim());
    } catch {
      // no origin/HEAD
    }
  }
  const re = new RegExp(`^${dst.replace(/[.+?^${}()|[\]\\]/g, "\\$&").replaceAll("*", ".*")}$`);
  return names.some((n) => re.test(n) || re.test(`refs/heads/${n}`));
}

/** Branch HEAD points at, or undefined (detached, not a checkout). */
function currentBranch(projectDir) {
  const dir = gitDirs(projectDir)[0];
  try {
    return /^ref:\s*refs\/heads\/(.+)$/m
      .exec(readFileSync(resolve(dir, "HEAD"), "utf8"))?.[1]
      .trim();
  } catch {
    return undefined;
  }
}

function pushRule(args, projectDir) {
  let tag = false;
  let del = false;
  const refs = [];
  for (const a of args) {
    if (a === "--prune" || a === "--all") return "pushDefault";
    if (a === "--mirror" || a === "--force" || a.startsWith("--force-") || a.startsWith("--force="))
      return "force";
    if (/^-[A-Za-z]*f[A-Za-z]*$/.test(a)) return "force";
    if (a.startsWith("+")) return "force";
    if (a === "--tags" || a === "--follow-tags") tag = true;
    if (a === "tag" && refs.length === 1) tag = true; // `git push <remote> tag <name>`
    if (a === "--delete" || /^-[A-Za-z]*d[A-Za-z]*$/.test(a)) del = true;
    // A remote or refspec the guard cannot read (`"$B"`, `"$(git branch --show-current)"`) may
    // name the default branch or a tag.
    if (!a.startsWith("-") && /[$`]/.test(a)) return "pushVar";
    if (!a.startsWith("-")) {
      refs.push(a);
      const dst = a.slice(a.lastIndexOf(":") + 1);
      if (a.startsWith(":") && isDefault(dst, projectDir)) return "deleteDefault";
      if (dst.startsWith("refs/tags/") || isTagName(dst, projectDir)) tag = true;
      // A tag src with an unqualified dst (`tmp:name`) still pushes a tag.
      const src = a.includes(":") ? a.slice(0, a.indexOf(":")).replace(/^\+/, "") : "";
      if (src && (src.startsWith("refs/tags/") || isTagName(src, projectDir))) tag = true;
    }
  }
  if (del && refs.some((r) => isDefault(r, projectDir))) return "deleteDefault";
  // A version-shaped dst (`v1.2.3`, `cli-v0.4.0`) is a release tag even when it is not local yet.
  const TAG_LIKE = /^(?:refs\/tags\/)?(?:[\w.-]*-)?v?\d+\.\d+\.\d+(?:[-+][\w.]+)?$/;
  if (refs.slice(1).some((r) => TAG_LIKE.test(r.slice(r.lastIndexOf(":") + 1)))) tag = true;
  if (tag) return "tag";
  // refs[0] is the remote; every later positional is a refspec whose dst must not be the default.
  for (const r of refs.slice(1)) {
    const spec = r.replace(/^\+/, "");
    if (spec.startsWith(":")) continue;
    let dst = spec.slice(spec.lastIndexOf(":") + 1);
    // A wildcard refspec (`refs/heads/*:refs/heads/*`) reaches every branch it matches.
    if (dst.includes("*") && wildcardHitsDefault(dst, projectDir)) return "pushDefault";
    if (dst === "HEAD" || dst === "@") dst = currentBranch(projectDir) ?? dst;
    if (isDefault(dst, projectDir)) return "pushDefault";
  }
  // No refspec: push.default pushes the current branch.
  if (refs.length <= 1 && !del) {
    const cur = currentBranch(projectDir);
    if (cur && isDefault(cur, projectDir)) return "pushDefault";
  }
  return null;
}

/** True when the command `t` writes, moves or deletes its operand at index `i`. */
function writesTarget(t, i) {
  // A quoted `<>` or `>` is data (`grep '<>' f`): the marked words keep it apart; a list without
  // them (a copy) reads every `>` as a redirect (fail closed).
  const m = t.marked?.length === t.length ? t.marked : t;
  const tok = m[i];
  if (/^\d*>[>|]?/.test(tok) || /^&>>?/.test(tok) || /^\d*<>/.test(tok)) return true;
  if (/^\d*>[>|]?$|^&>>?$|^\d*<>$/.test(m[i - 1] ?? "")) return true;
  const prog = base(t[0]);
  // `find <root> -delete` or `-exec rm {} ;` writes under each root it starts from.
  if (prog === "find") return findRoots(t).includes(i) && findWrites(t);
  if (
    ["tee", "mv", "rm", "truncate", "touch", "chmod", "chown", "shred", "unlink", "rmdir"].includes(
      prog,
    )
  )
    return true;
  if (prog === "dd") return tok.startsWith("of=");
  if (["ex", "vi", "vim", "nvim", "ed", "patch"].includes(prog)) return true;
  if (["awk", "gawk"].includes(prog) && t.includes("inplace")) return true;
  if (["cp", "install", "ln", "rsync"].includes(prog)) {
    let last = t.length - 1;
    while (last > 0 && t[last].startsWith("-")) last--;
    return i === last;
  }
  if (prog === "git") return t.some((x) => ["rm", "mv", "checkout", "restore"].includes(x));
  return (
    (prog === "sed" || prog === "perl") &&
    t.some((x) => /^-[A-Za-z]*i/.test(x) || /^--in-place/.test(x))
  );
}

/** Indices of `find`'s start paths: the operands after its global options (-H -L -P -D -O) and
 * before the first expression word (`-name`, `(`, `!`). */
function findRoots(t) {
  const out = [];
  let i = 1;
  while (i < t.length && /^-([HLP]|D|O\d*)$/.test(t[i])) i += t[i] === "-D" ? 2 : 1;
  for (; i < t.length && !/^(-.|[(!])/.test(t[i]); i++) out.push(i);
  return out;
}

/** `find` that deletes, or hands what it finds (`{}`) to a program that is not a read-only tool
 * (wrappers stripped: `-exec timeout 5 cat {} ;` reads). The program itself is judged apart. */
function findWrites(t) {
  return t.some((x, e) => {
    if (x === "-delete") return true;
    if (!/^-(exec|execdir|ok|okdir)$/.test(x)) return false;
    const end = t.findIndex((y, n) => n > e && (y === ";" || y === "+"));
    const inner = stripPrefixes(t.slice(e + 1, end < 0 ? undefined : end));
    // `{}` is lexed as `{`: either form is the found path.
    return (
      inner.some((y) => y === "{" || y.includes("{}")) && !READ_ONLY_PROGS.has(base(inner[0] ?? ""))
    );
  });
}

/** `gh api` that writes: a non-GET method, or with no method a body/field flag (these imply POST);
 * graphql writes on a mutation or a query the guard cannot read (a file, stdin or a variable). */
function ghApiWrites(args) {
  const { method, endpoint, fields, input } = ghApiParse(args);
  if (endpoint === "graphql")
    return (
      input !== undefined || fields.some((f) => /\bmutation\b/i.test(f) || /^query=[@$]/.test(f))
    );
  if (method !== undefined) return String(method).toUpperCase() !== "GET";
  return fields.length > 0 || input !== undefined;
}

/** `gh api` flags (pflag): short bools, short flags that take a value, long bools, long values. */
const GH_API_SHORT_BOOL = new Set(["i", "h"]);
const GH_API_SHORT_VALUE = new Set(["X", "f", "F", "H", "q", "t", "p"]);
const GH_API_LONG_BOOL = new Set(
  "--include --paginate --silent --verbose --slurp --help".split(" "),
);
const GH_API_LONG_VALUE = new Set(
  "--method --raw-field --field --header --jq --template --input --hostname --cache --preview".split(
    " ",
  ),
);
const GH_API_SHORT_LONG = { X: "--method", f: "--raw-field", F: "--field", H: "--header" };

/** `gh api` arguments read like gh (pflag): a short group is bools, then at most one value flag
 * that takes the rest of the word or the next argument (`-iXPUT`, `-iF k=v`); the endpoint is the
 * first positional. `fileField` is the first `-F`/`--field` value read from a file (`@path`,
 * all in `fileFields`), `input` a
 * `--input` body, `unknown` a flag gh api does not have (or a group the guard cannot read). */
function ghApiParse(args) {
  let method;
  let endpoint;
  let fileField;
  let input;
  let unknown;
  let override = false; // a method-override header: the server may run another method
  const fields = [];
  const fileFields = []; // every `-F k=@path`
  const take = (flag, value) => {
    if (flag === "--method") method = value;
    if (flag === "--field" || flag === "--raw-field") fields.push(value ?? "");
    if (flag === "--field" && /^[^=]*=@/.test(value ?? "")) {
      fileField ??= value;
      fileFields.push(value);
    }
    if (flag === "--input") input = value ?? "";
    if (flag === "--header" && /method-override/i.test(value ?? "")) override = true;
  };
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === "--") {
      if (endpoint === undefined) endpoint = args[i + 1];
      break;
    }
    if (a.startsWith("--")) {
      const eq = /^(--[\w-]+)=(.*)$/s.exec(a);
      const flag = eq ? eq[1] : a;
      if (GH_API_LONG_VALUE.has(flag)) take(flag, eq ? eq[2] : args[++i]);
      else if (!GH_API_LONG_BOOL.has(flag)) unknown ??= a;
    } else if (a.startsWith("-") && a.length > 1) {
      for (let k = 1; k < a.length; k++) {
        const c = a[k];
        if (GH_API_SHORT_BOOL.has(c)) continue;
        if (!GH_API_SHORT_VALUE.has(c)) {
          unknown ??= a;
          break;
        }
        const value = k + 1 < a.length ? a.slice(k + 1).replace(/^=/, "") : args[++i];
        take(GH_API_SHORT_LONG[c] ?? `-${c}`, value);
        break;
      }
    } else if (endpoint === undefined) endpoint = a;
  }
  if (override) method = "OVERRIDE"; // whatever `-X` says, before or after it
  return { method, endpoint, fileField, fileFields, input, unknown, fields };
}

/** `gh workflow run` inputs: `-F`/`--field` `@file` reads a local file into an input (`ghField`);
 * a token-like variable (`-f x="$GITHUB_TOKEN"`) sends a secret (`secret`). */
function workflowInputs(args) {
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (/^--json(=|$)/.test(a)) return "ghField"; // inputs read from stdin (a file or a pipe)
    const m = /^(-F|--field|-f|--raw-field)(?:=?(.*))?$/s.exec(a);
    if (!m) {
      if (/[$`]/.test(a) && SECRET_VAR_RE.test(a)) return "secret";
      continue;
    }
    const value = m[2] ? m[2] : (args[++i] ?? "");
    if (SECRET_VAR_RE.test(value)) return "secret";
    if ((m[1] === "-F" || m[1] === "--field") && /^[^=]*=@/.test(value)) return "ghField";
  }
  return null;
}

/** The workflow `gh workflow run` gets: its first positional (a value flag's value skipped). */
function workflowArg(args) {
  for (let n = 0; n < args.length; n++) {
    if (/^(-[rfFR]|--(ref|field|raw-field|repo))$/.test(args[n])) n++;
    else if (!args[n].startsWith("-")) return args[n];
  }
  return undefined;
}

/** Workflow file names the guard treats as release workflows (never started from here):
 * `GUARD_RELEASE_WORKFLOWS` (a case-insensitive regex, set in .claude/settings.json `env`), else
 * release, publish or deploy. */
function releaseWorkflow(name) {
  let re = /release|publish|deploy/i;
  try {
    if (process.env.GUARD_RELEASE_WORKFLOWS)
      re = new RegExp(process.env.GUARD_RELEASE_WORKFLOWS, "i");
  } catch {
    // a bad pattern keeps the default
  }
  return re.test(String(name));
}

/** `gh api` that does what `gh workflow run` and `gh run rerun` do: a POST to the endpoint (the
 * first positional) that dispatches a literal non-release workflow file or re-runs a run or job,
 * with no file read into the body. */
const WORKFLOW_RUN_RE =
  /^\/?repos\/[^/\s]+\/[^/\s]+\/actions\/(workflows\/([\w.-]+\.ya?ml)\/dispatches|runs\/\d+\/(rerun|rerun-failed-jobs)|jobs\/\d+\/rerun)\/?$/;
function workflowRun(args) {
  const { method = "POST", endpoint, fileField, input } = ghApiParse(args);
  const m = WORKFLOW_RUN_RE.exec(endpoint ?? "");
  return (
    String(method).toUpperCase() === "POST" &&
    !!m &&
    fileField === undefined &&
    input === undefined &&
    !(m[2] && releaseWorkflow(m[2]))
  );
}

/** `gh api` that opens a PR: a POST to `repos/<owner>/<repo>/pulls` (exactly; no `..`, query or
 * sub-path such as `/merge`), on github.com, whose only file read is `-F body=@<repo file>`. Like
 * `gh pr create --body-file`. */
const PR_CREATE_RE =
  /^\/?repos\/(?:\{owner\}|(?!\.\.?\/)[\w.-]+)\/(?:\{repo\}|(?!\.\.?\/)[\w.-]+)\/pulls\/?$/;
function prCreate(args) {
  const { method = "POST", endpoint, fileFields, input } = ghApiParse(args);
  if (args.some((a) => /^--hostname(=|$)/.test(a))) return false; // another GitHub host
  return (
    String(method).toUpperCase() === "POST" &&
    PR_CREATE_RE.test(endpoint ?? "") &&
    fileFields.every((f) => /^body=@/.test(f)) && // the file itself: repoFile
    input === undefined
  );
}

/** File names that hold credentials or configuration a body must not carry. */
const CRED_NAME_RE =
  /^(environ|.*credential.*|.*secret.*|.*token.*|.*netrc|.*npmrc|.*pypirc|.*yarnrc.*|config(\.json)?|.*\.(pem|key|p12|pfx|kdbx)|id_\w+|keys\.txt|.*service[-_.]?account.*|.*\.tfstate(\..*)?|.*\.tfvars(\.json)?|auth\.json|\.?dev\.vars(\..*)?)$/i;

/** A body file the PR / issue routes may read: written literally as a relative path with no
 * dotfile, `..`, credential or config segment; on disk a regular file inside the repo reached
 * with no symlink; not written by this command (`ln -sf .env pr.md && ...`). Fail closed. */
function repoFile(p, projectDir, cwd, chain) {
  if (!p || p === "-" || /^[/~]/.test(p) || /[$`*?[{\\]/.test(p)) return false;
  const parts = p.split("/").filter((x) => x && x !== ".");
  if (!parts.length || parts.some((x) => x.startsWith(".") || CRED_NAME_RE.test(x))) return false;
  if (secretLike(p) || !chain.later || chain.later.opaque || chain.state?.opaque) return false;
  try {
    const root = realpathSync(projectDir);
    const abs = resolve(realpathSync(cwd), p);
    if (!abs.startsWith(`${root}/`) || realpathSync(abs) !== abs) return false;
    if (!lstatSync(abs).isFile()) return false;
    const written = [...chain.later.names, ...chain.made, ...chain.tainted];
    return !written.some((w) => w === abs || w === resolve(cwd, p) || abs.startsWith(`${w}/`));
  } catch {
    return false; // missing or unreadable
  }
}

/** Env prefixes that point gh at another host or token (`GH_HOST=x gh api ...`). */
const GH_ENV_RE =
  /^(GH_\w*|GITHUB_\w*|\w*TOKEN\w*|HTTPS?_PROXY|https?_proxy|ALL_PROXY|all_proxy|NO_PROXY|no_proxy|SSL_CERT_(FILE|DIR)|CURL_CA_BUNDLE|XDG_CONFIG_HOME)=/;

/** Text that may point gh at another host, repo, proxy, CA, config or token anywhere in the
 * command (`export GH_HOST=x; gh ...`, `declare -x`, `read GH_REPO`, `MY_TOKEN=x`,
 * `HTTPS_PROXY=x`), or set a variable whose name is built at run time
 * (`declare -x ${G}_HOST=x`, `export "$G"_TOKEN=x`, `printf -v "$N" ...`). */
const GH_TAINT_RE = new RegExp(
  [
    String.raw`\b(?:GH|GITHUB)_\w+|\w*TOKEN\w*=`,
    String.raw`\b(?:HTTPS?_PROXY|https?_proxy|ALL_PROXY|all_proxy|NO_PROXY|no_proxy|SSL_CERT_(?:FILE|DIR)|CURL_CA_BUNDLE|XDG_CONFIG_HOME)\b`,
    String.raw`\b(?:export|declare|typeset|readonly|local|read|env|printf\s+-v)\b[^\n;&|]*?\s["']?[$\x60\\]`,
  ].join("|"),
);

/** The gh write routes the guard opens (a PR, a workflow dispatch or re-run, an issue) apply only
 * to a command the guard read in full: every word literal (no `$VAR`, `"$@"`, `${...}`, `$(...)`
 * or backtick, a -f / --body value too), no GH_* / GITHUB_* / token variable set anywhere in the
 * command (or the script that runs it), no directory change before it and no `-R`/`--repo`. */
const ghLiteral = (t, chain) =>
  chain.ghClean === true &&
  !t.ghEnv &&
  !t.some((w) => /[$`]/.test(w) || /^(-R|--repo)(=|$)|^-R./.test(w));

/** The `gh pr|issue create` body file (`--body-file p`, `-F p`, `--body-file=p`, `-Fp`). */
function bodyFiles(args) {
  const out = [];
  for (let i = 0; i < args.length; i++) {
    const m = /^(?:--body-file|-F)(?:=?(.*))?$/s.exec(args[i]);
    if (m) out.push(m[1] ? m[1] : (args[++i] ?? ""));
  }
  return out;
}

const ADMIN_API_RE =
  /\/(branches\/[^/\s]+\/protection|rulesets?|environments|actions\/(secrets|variables|permissions)|hooks|keys|collaborators|transfer|rename)\b/;

const CHDIR_FLAG_RE =
  /^(?:(?:-[A-Za-z]*[CtdoO]|--chdir|--directory|--work-tree|--prefix|--cwd|--dir|--target-directory|--output|--output-document)(?:=(.*))?|-[A-Za-z]*[CtdoO](.+))$/;

/** Directories a command changes into or writes under with a flag (`env -C`, `tar -xC`, `make -C`,
 * `--work-tree=`, `cp -t`, `unzip -d`, `curl -o`). */
function chdirs(tokens) {
  const out = [];
  for (let i = 0; i < tokens.length; i++) {
    const m = CHDIR_FLAG_RE.exec(tokens[i]);
    if (!m) continue;
    const v = m[1] ?? m[2] ?? tokens[++i];
    if (v) out.push(v);
  }
  return out;
}

/** Tools whose printed shell code (`eval "$(tool ...)"`) sets up their own environment. */
const ENV_PRINTERS = new Set(
  "ssh-agent fnm rbenv pyenv nodenv conda brew opam starship zoxide dircolors lesspipe keychain".split(
    " ",
  ),
);

/** Variables whose value is a program git, a pager or an editor runs. */
const PROGRAM_ENV_RE =
  /^(GIT_(SSH|SSH_COMMAND|PAGER|EDITOR|SEQUENCE_EDITOR|EXTERNAL_DIFF|ASKPASS|PROXY_COMMAND|EXEC_PATH|TEMPLATE_DIR)|PAGER|MANPAGER|LESSOPEN|LESSCLOSE|EDITOR|VISUAL|SSH_ASKPASS|PROMPT_COMMAND)=(.*)$/s;
/** Programs that are safe as a pager, editor or ssh command: they run nothing the guard misses. */
const SAFE_PROGRAM_RE =
  /^(cat|less|more|true|false|:|ssh|vi|vim|nvim|nano|emacs|code|head|tail|diff)?$/;
/** Config keys whose value git runs as a program or that pull in more config. */
const PROGRAM_KEY_RE =
  /^(core\.(sshcommand|pager|editor|fsmonitor|askpass|gitproxy|alternaterefscommand)|sequence\.editor|diff\.(external|[^.=]+\.(textconv|command))|merge\.[^.=]+\.driver|filter\.|credential\.|gpg\.([^.=]+\.)?program|pager\.|uploadpack\.packobjectshook|ssh\.variant|remote\.[^.=]+\.(uploadpack|receivepack|proxy)|protocol\.|include\.|includeif\.)/i;
const safeProgram = (v) => SAFE_PROGRAM_RE.test(base(String(v).trim().split(/\s+/)[0] ?? ""));

/** True when an assignment makes git or a pager run a program the guard cannot read. */
function envRunsProgram(x) {
  const m = PROGRAM_ENV_RE.exec(x);
  const value = x.slice(x.indexOf("=") + 1);
  if (m) return /PATH|DIR$/.test(m[1]) ? value !== "" : !safeProgram(value);
  const c = /^GIT_CONFIG(_PARAMETERS|_COUNT|_KEY_\d+|_VALUE_\d+|_GLOBAL|_SYSTEM)?=(.*)$/s.exec(x);
  return !!c && !(/_GLOBAL|_SYSTEM/.test(c[1] ?? "") && c[2] === "/dev/null");
}

/** `git -c key=value` or `git config key value` that runs a program. */
function programKey(key, value) {
  if (!PROGRAM_KEY_RE.test(key)) return false;
  return /^(credential|filter|include|includeif|protocol)/i.test(key)
    ? value !== ""
    : !safeProgram(value);
}

const RUNS_ENV_RE =
  /^(GIT_[A-Z0-9_]+|PAGER|LESSOPEN|LESSCLOSE|LD_PRELOAD|LD_LIBRARY_PATH|BASH_ENV|ENV|EDITOR|VISUAL|NODE_OPTIONS|PERL5OPT|PYTHONSTARTUP)=/;

/** `x>file` is `x` plus a redirect `>file`; a bare fd prefix (`2>/dev/null`) stays one token. */
function splitRedirect(tok) {
  const m = /^(.+?)((?:\d*|&)[<>].*)$/s.exec(tok);
  if (!m || /^(\d+|&)$/.test(m[1]) || /^[<>&]/.test(tok)) return [tok];
  return [m[1], m[2]];
}

/** Tools whose -d/-o/-t/-C/-O flags never name a directory or an output file. */
const NO_DIR_FLAGS = new Set("ls grep egrep fgrep head tail wc cat du stat diff".split(" "));
/** Tools that only read; a redirect still counts as a write. Not tree/less (-o writes). */
const READ_ONLY_PROGS = new Set(
  (
    "ls cat head tail wc grep egrep fgrep rg ag stat du diff cmp jq pwd echo printf true false " +
    "test [ cd pushd popd basename dirname realpath readlink sha256sum md5sum"
  ).split(" "),
);
const READ_ONLY_GIT = new Set(["status", "log", "diff", "show", "ls-files", "blame", "rev-parse"]);
/** Flags that write a file or run a program even in a read-only tool. */
const RUNS_OR_WRITES_RE = /^(--output|--pre\b|--config-env|--exec-path|--ext-diff)/;

/** True when a command cannot write: a read-only tool, no redirect except to /dev/null. */
function readOnly(t) {
  const prog = base(t[0]);
  if (prog === "git" ? !READ_ONLY_GIT.has(gitSub(t).sub) : !READ_ONLY_PROGS.has(prog)) return false;
  if (t.slice(1).some((x) => RUNS_OR_WRITES_RE.test(x))) return false;
  // git -c/-C (config can run a program, -C moves the repo).
  if (prog === "git" && t.slice(1).some((x) => /^-[cC]/.test(x))) return false;
  return !t.some((x, i) => {
    if (!writesTarget(t, i)) return false;
    return (
      !/^(\d*>[>|]?|&>>?)?\/dev\/null$/.test(x) &&
      !/^\d*>&\d$/.test(x) &&
      !(/^\d*>[>|]?$|^&>>?$/.test(x) && t[i + 1] === "/dev/null")
    );
  });
}

const dirOf = (cwd, x) => resolve(cwd, stripTok(x));

function bashRule(
  t,
  projectDir,
  cwd = projectDir,
  chain = { made: new Set(), tainted: new Set() },
) {
  const prog = base(t[0]);
  const args = t.slice(1);
  if (prog === "@deny") return t[1];
  if (prog === "@evade") return RISKY_RE.test(t[1]) ? "evasion" : null;
  if (prog === "@evade-exec") return EXEC_RE.test(t[1]) ? "evasion" : null;
  if (prog === "@runs-env") return chain.guarded ? "protected" : null;
  if (prog === "@chdir") return guardedDir(t[1], projectDir, cwd) ? "protected" : null;
  if (["cd", "pushd"].includes(prog)) {
    const dirs = args.filter((a) => !a.startsWith("-"));
    // `cd "$(echo .claude)"`, `cd $X`: the target is unknown, so it is refused.
    // In a script the guard reads, `cd "$(dirname "$0")"` is common: a known gap, as before
    // scripts were read.
    if (!chain.script && dirs.some((d) => d && /[$`]/.test(d))) return "protected";
    // Into a guarded dir: allowed, but the rest of the chain must only read (relative names
    // there would hide a protected write).
    if (dirs.some((d) => d && guardedDir(d, projectDir, cwd))) chain.guarded = true;
  }
  if (chain.guarded && !readOnly(t)) return "protected";
  if (prog === "patch" || (prog === "git" && gitSub(t).sub === "apply")) {
    // The diff names the files it writes: read it (file operand, <redirect or heredoc). Piped or
    // unreadable input cannot be checked, so it is refused; apply it from a file in the repo.
    const texts = [...(t.docs ?? [])];
    let read = texts.length > 0;
    for (const f of args.map((x) => x.replace(/^<+/, "")).filter((x) => x && !x.startsWith("-"))) {
      const abs = dirOf(cwd, f);
      if (chain.made.has(abs)) {
        read = true; // written earlier in this chain by git diff: the repo's own changes
        continue;
      }
      if (chain.tainted.has(abs)) return "protected"; // content made by the chain cannot be read
      try {
        texts.push(readFileSync(resolve(cwd, f), "utf8"));
        read = true;
      } catch {
        // a value such as the 1 of `-p 1`, or a missing file
      }
    }
    if (!read || texts.some(patchTouchesProtected)) return "protected";
  }
  const run = args.findIndex((a) => a === "run" || a === "run-script");
  if (
    MANAGERS.has(prog) &&
    (args.some((a) => a === "publish" || a === "unpublish" || a === "dist-tag") ||
      args[0] === "release" ||
      (run >= 0 && args[run + 1] === "release"))
  )
    return "publish";
  if (
    prog === "cargo" &&
    (args.includes("publish") || args.find((a) => !a.startsWith("+")) === "release")
  )
    return "publish";
  if (
    ["make", "gmake", "just", "task"].includes(prog) &&
    args.some((a) => /^(publish|release)$/.test(a))
  )
    return "publish";
  if (prog === "changeset" && args.includes("publish")) return "publish";
  if (prog === "gh") {
    const [a0, a1] = args;
    if (a0 === "api") {
      const api = ghApiParse(args.slice(1));
      if (api.unknown !== undefined) return "ghFlag";
      if (api.fileField !== undefined && !prCreate(args.slice(1))) return "ghField";
      if (api.method === "OVERRIDE") return "ghApi";
      if (api.fields.some((f) => SECRET_VAR_RE.test(f))) return "secret"; // a token sent as input
      if (ghApiWrites(args.slice(1))) {
        if (ADMIN_API_RE.test(`/${api.endpoint ?? ""}`)) return "ghAdmin";
        if (!workflowRun(args.slice(1)) && !prCreate(args.slice(1))) return "ghApi";
        if (!ghLiteral(t, chain)) return "ghLiteral";
        const files = api.fileFields.map((f) => f.slice(6));
        if (!files.every((f) => repoFile(f, projectDir, cwd, chain))) return "ghLiteral";
      } else if (api.input !== undefined) return "ghField";
    }
    // `new` is gh's alias of `create` (`gh pr new`, `gh issue new`, `gh release new`).
    if (["pr", "issue"].includes(a0) && ["create", "new"].includes(a1)) {
      if (!ghLiteral(t, chain)) return "ghLiteral";
      if (!bodyFiles(args.slice(2)).every((f) => repoFile(f, projectDir, cwd, chain)))
        return "ghLiteral";
    }
    if (a0 === "pr" && a1 === "merge") {
      if (args.includes("--admin")) return "ghMerge";
      if (!args.includes("--auto") && !args.includes("--disable-auto")) return "ghMerge";
    }
    if (a0 === "release" && ["create", "new", "upload"].includes(a1)) return "publish";
    // Approving a PR is the owner's (or bosun's reviewers') step, never this session's.
    if (
      a0 === "pr" &&
      a1 === "review" &&
      args.some((x) => /^(--approve|-[a-zA-Z]*a[a-zA-Z]*)$/.test(x))
    )
      return "ghReview";
    if (a0 === "release" && !["list", "view", "download", undefined].includes(a1)) return "ghAdmin";
    if (a0 === "workflow" && ["run", "enable", "disable"].includes(a1)) {
      if (args.slice(2).some(releaseWorkflow)) return "publish";
      // `gh workflow run <id>`: a number does not name its workflow (it may be a release one)
      if (a1 === "run" && /^\d+$/.test(workflowArg(args.slice(2)) ?? "")) return "ghApi";
      if (a1 === "run") return workflowInputs(args.slice(2));
      return null;
    }
    if (a0 === "secret") return "ghAdmin";
    if (a0 === "alias" && a1 !== undefined && !["list", "delete"].includes(a1)) return "alias";
    if (a0 === "repo" && ["edit", "delete", "archive", "rename", "transfer"].includes(a1))
      return "ghAdmin";
    if (a0 === "auth" && a1 === "token") return "secret";
  }
  if (prog === "git") {
    const { sub, args: rest, cfg } = gitSub(t);
    if (cfg.some((c) => /^core\.hookspath=/i.test(c))) return "protected";
    for (const c of cfg) {
      const eq = c.indexOf("=");
      if (programKey(eq < 0 ? c : c.slice(0, eq), eq < 0 ? "x" : c.slice(eq + 1)))
        return "gitProgram";
    }
    if (sub === "push" && cfg.some((c) => /^(remote\.[^=]*\.push|push\.default)=/i.test(c)))
      return "pushDefault";
    const alias = cfg
      .map((c) => /^alias\.([^=]+)=(.*)$/i.exec(c))
      .find((m) => m && m[1].toLowerCase() === String(sub).toLowerCase());
    if (alias) {
      if (alias[2].startsWith("!")) {
        for (const inner of commands(`${alias[2].slice(1)} ${rest.join(" ")}`)) {
          const r = bashRule(inner, projectDir);
          if (r) return r;
        }
        return null;
      }
      return bashRule(["git", ...split(alias[2]).flat(), ...rest], projectDir);
    }
    if (sub === "push") return pushRule(rest, projectDir);
    if (sub === "config" && rest.some((a) => /^(core\.hookspath|alias\.)/i.test(a)))
      return "protected";
    if (
      sub === "config" &&
      !rest.some((a) => /^(--get|--get-all|--get-regexp|-l|--list|--show-origin)$/.test(a))
    ) {
      const ops = rest.filter((a) => !a.startsWith("-"));
      if (
        ops.length &&
        PROGRAM_KEY_RE.test(ops[0]) &&
        (ops.length > 1 || rest.some((a) => /^--(add|replace-all)$/.test(a)))
      )
        if (programKey(ops[0], ops.slice(1).join(" "))) return "gitProgram";
    }
    if (sub === "update-ref") {
      if (rest.some((a) => a === "--stdin" || a.startsWith("refs/tags/"))) return "tag";
      if (rest.some((a) => a.startsWith("refs/heads/") && isDefault(a, projectDir)))
        return "rewriteDefault";
    }
    if (sub === "tag" && rest.length && !TAG_READ_RE.test(rest[0])) return "tag";
    const names = rest.filter((a) => !a.startsWith("-"));
    if (
      sub === "branch" &&
      rest.some(
        (a) => /^-[A-Za-z]*[dDfmMcC][A-Za-z]*$/.test(a) || /^--(delete|force|move|copy)$/.test(a),
      ) &&
      names.some((n) => isDefault(n, projectDir))
    )
      return "rewriteDefault";
    if (
      (sub === "checkout" || sub === "switch") &&
      rest.some((a) => ["-B", "-C", "--force-create"].includes(a)) &&
      names.some((n) => isDefault(n, projectDir))
    )
      return "rewriteDefault";
    if (sub === "reset" && rest.includes("--hard")) {
      const cur = currentBranch(projectDir);
      if (cur && isDefault(cur, projectDir)) return "rewriteDefault";
    }
  }
  if (["echo", "printf"].includes(prog) && args.some((a) => SECRET_VAR_RE.test(a))) return "secret";
  if (
    prog === "printenv" &&
    (args.length === 0 || args.some((a) => /KEY|TOKEN|SECRET|PASSW|CREDENTIAL/i.test(a)))
  )
    return "secret";
  if (prog === "export" && (args.length === 0 || args.includes("-p"))) return "secret";
  if (
    (prog === "declare" || prog === "typeset") &&
    args.some((a) => /^-[a-z]*[xp]/.test(a)) &&
    args.every((a) => a.startsWith("-"))
  )
    return "secret";
  if (prog === "set" && args.length === 0) return "secret";
  if (
    prog === "sops" &&
    args.some((a) => ["-d", "--decrypt", "decrypt", "exec-env", "exec-file"].includes(a))
  )
    return "secret";
  if (prog === "age" && args.some((a) => a === "-d" || a === "--decrypt")) return "secret";
  if (prog === "hoist" && args[0] === "secrets" && args[1] === "exec") {
    const dd = args.indexOf("--");
    const inner = dd >= 0 ? args.slice(dd + 1) : args.slice(2);
    const it = stripPrefixes(inner);
    const SHELLISH = [
      "env",
      "printenv",
      "export",
      "set",
      "declare",
      "sh",
      "bash",
      "zsh",
      "dash",
      "ksh",
    ];
    if (
      inner.slice(0, 4).some((x) => SHELLISH.includes(base(x)) || SHELLS.has(base(x))) ||
      (INTERPRETERS.test(base(it[0] ?? "")) && it.some((a, n) => n > 0 && INLINE_RE.test(a))) ||
      /ENVIRON|environ|process\.env/.test(inner.join(" "))
    )
      return "secret";
  }
  // A token with whitespace is prose (a quoted message or label), not a file name.
  const firstOperand = t.findIndex((y, m) => m > 0 && !y.startsWith("-"));
  const files = ["grep", "egrep", "fgrep", "rg", "ag"].includes(prog)
    ? t.filter((x, n) => n === 0 || x.startsWith("-") || n > firstOperand) // the first operand is the pattern
    : t;
  if (files.some((x) => !/\s/.test(x) && secretLike(x))) return "secret";
  for (let i = 0; i < t.length; i++) {
    const x = stripTok(t[i]);
    if (LOCK_RE.test(x) && writesTarget(t, i)) return "lock";
    if (!writesTarget(t, i)) continue;
    if (isProtected(x)) return "protected";
    const real = realRel(x, projectDir, cwd);
    if (real && LOCK_RE.test(real)) return "lock";
    if (real && isProtected(real)) return "protected";
  }
  if (prog === "find" && findWrites(t)) {
    const roots = findRoots(t).map((i) => stripTok(t[i]));
    for (const r of roots.length ? roots : ["."]) {
      // A root the guard cannot name (a substitution, variable, glob or `~`) fails closed.
      if (/[$`*?[]|^~/.test(r)) return "protected";
      const real = realRel(r, projectDir, cwd);
      if (real === undefined || holdsGuarded(real)) return "protected";
    }
  }
  return null;
}

/** Guarded files a directory may hold, to judge a recursive delete of the directory. */
const GUARDED_SAMPLES = [
  ".claude/hooks/x",
  ".claude/settings.json",
  ".github/workflows/release.yml",
  ".bosun/x",
  ".git/hooks/x",
  ".git/config",
  ".splice/lock.toml",
  "pins.toml",
  "scripts/release-pre.sh",
];

/** A repo-relative dir that is guarded, holds a guarded file, or holds the repo itself (`..`). */
function holdsGuarded(rel) {
  const r = rel.replace(/\/+$/, "").toLowerCase();
  if (r === "" || /^\.\.(\/\.\.)*$/.test(r) || isProtected(r) || LOCK_RE.test(r)) return true;
  if (r.startsWith("../")) return false; // outside the repo
  return GUARDED_SAMPLES.some((g) => g.startsWith(`${r}/`)) || /(^|\/)\.(claude|bosun)$/.test(r);
}

const GUARDED_DIR_RE =
  /^(\.claude(\/hooks)?|\.bosun(\/.*)?|\.github\/workflows|\.git(\/hooks)?)\/?$/i;

/** `cd`/`git -C` into a guarded dir would turn later relative writes into unguarded names. */
function guardedDir(d, projectDir, cwd) {
  const x = stripTok(d);
  if (/[*?[{]/.test(x) && globHitsGuarded(x, projectDir, cwd)) return true;
  return [x, realRel(x, projectDir, cwd)].some(
    (r) => r && (GUARDED_DIR_RE.test(r) || isProtected(r) || LOCK_RE.test(r)),
  );
}

const GUARDED_DIRS = [
  ".claude",
  ".claude/hooks",
  ".bosun",
  ".github/workflows",
  ".git",
  ".git/hooks",
];

/** True when a glob (`.cla*`, `.?laude`, `.claud{e,x}`) could expand to a guarded dir. */
function globHitsGuarded(glob, projectDir, cwd) {
  const rel = relPath(glob, projectDir, cwd).replace(/\/+$/, "");
  const re = (g) => {
    let out = "";
    for (let i = 0; i < g.length; i++) {
      const c = g[i];
      if (c === "*") out += g[i + 1] === "*" ? (i++, ".*") : "[^/]*";
      else if (c === "?") out += "[^/]";
      else if (c === "[") {
        const e = g.indexOf("]", i + 2);
        if (e < 0) out += "\\[";
        else {
          out += `[${g.slice(i + 1, e).replace(/^!/, "^")}]`;
          i = e;
        }
      } else if (c === "{") {
        // stripPrefixes may have cut a trailing `}`: read to the end then
        const e = g.indexOf("}", i) < 0 ? g.length : g.indexOf("}", i);
        {
          out += `(?:${g
            .slice(i + 1, e)
            .split(",")
            .map((a) => a.replace(/[.+^$()|\\]/g, "\\$&"))
            .join("|")})`;
          i = e;
        }
      } else out += c.replace(/[.+^$()|\\]/g, "\\$&");
    }
    return out;
  };
  try {
    const r = new RegExp(`^${re(rel)}$`, "i");
    return GUARDED_DIRS.some((g) => r.test(g));
  } catch {
    return true; // unreadable glob: refuse
  }
}

/** True when a unified diff or patch writes a protected path (any `-p` strip level). */
function patchTouchesProtected(text) {
  for (const m of String(text).matchAll(
    /^(?:\+\+\+|---|diff --git|rename (?:to|from)|copy (?:to|from)|\*\*\*)[ \t]+(.*)$/gm,
  )) {
    for (const path of m[1].split(/[ \t]+/)) {
      const parts = path.replaceAll("\\", "/").split("/");
      for (let i = 0; i < parts.length; i++) {
        const sub = parts.slice(i).join("/");
        if (isProtected(sub) || LOCK_RE.test(sub)) return true;
      }
    }
  }
  return false;
}

/** Repo-relative path with symlinks resolved through the deepest existing ancestor, or undefined. */
function realRel(p, projectDir, cwd = projectDir) {
  try {
    let cur = resolve(cwd, String(p));
    const tail = [];
    while (!existsSync(cur) && dirname(cur) !== cur) {
      tail.unshift(basename(cur));
      cur = dirname(cur);
    }
    return posix.relative(realpathSync(projectDir), join(realpathSync(cur), ...tail));
  } catch {
    return undefined;
  }
}

function relPath(p, projectDir, cwd = projectDir) {
  const rel = posix.relative(
    posix.resolve(projectDir),
    posix.resolve(cwd, String(p).replaceAll("\\", "/")),
  );
  return rel;
}

/** The text a deny names: the risky word of an unreadable body, else the simple command. */
function matched(t) {
  if (t[0] === "@evade") return RISKY_RE.exec(t[1])?.[0] ?? t[1];
  if (t[0] === "@evade-exec") return EXEC_RE.exec(t[1])?.[0] ?? t[1];
  if (t[0] === "@chdir") return t[1];
  if (t[0] === "@deny") return t[2];
  return t.join(" ");
}

/** Check shell text: each simple command, then each script it runs. `opts.seen` stops cycles;
 * a nested script shares its parent's chain (files written earlier stay unreadable) and binds its
 * literal arguments to `$1`, `$@` (a trampoline `bash "$1"` runs the file it is given). */
function checkShell(text, projectDir, cwd, opts) {
  const { seen, parent, script = false, scriptDir } = opts;
  let argv = opts.argv;
  const chain = {
    made: parent?.made ?? new Set(),
    tainted: parent?.tainted ?? new Set(),
    path: parent?.path ?? [],
    state: parent?.state ?? { opaque: null }, // the first earlier command that may write any file
    guarded: opts.guarded,
    script,
    lost: opts.lost ?? false, // a `cd` the guard cannot follow: relative script paths are unknown
    vars: varValues(text, parent?.vars),
  };
  const cmds = commands(text);
  // gh's PR / issue / workflow routes: no GH_* / GITHUB_* / token variable and no directory
  // change anywhere in this text or the one that runs it (`ghLiteral`).
  chain.ghClean =
    (parent ? parent.ghClean === true : true) &&
    !GH_TAINT_RE.test(text) &&
    !cmds.some((t) => ["cd", "pushd", "popd", "chdir", "@chdir"].includes(base(t[0])));
  // Writes anywhere in this text (a later command can run first: `(sleep 1; bash f) & cp x f`).
  const later = {
    names: new Set(parent?.later?.names ?? []),
    opaque: parent?.later?.opaque ?? null,
  };
  const async = /(^|[^&|>])&(?![&>])|(^|[^|>])\|(?!\|)|\bcoproc\b/.test(text);
  const all = { made: new Set(), tainted: new Set() };
  for (const t of cmds) {
    if (String(t[0]).startsWith("@")) continue;
    noteWrites(t, cwd, all);
    if (async && !later.opaque && opaqueWriter(t)) later.opaque = t.join(" ");
  }
  for (const x of [...all.made, ...all.tainted]) later.names.add(x);
  chain.later = later;
  // A `cd` into a guarded dir anywhere in the command makes all of it read-only: substitutions
  // are listed before the command that holds them.
  chain.guarded ||= cmds.some(
    (t) =>
      ["cd", "pushd"].includes(base(t[0])) &&
      t.slice(1).some((d) => !d.startsWith("-") && guardedDir(d, projectDir, cwd)),
  );
  let here = cwd; // where a later script path resolves after a literal `cd`
  for (const raw of cmds) {
    const t = argv ? positional(raw, argv) : raw;
    if (argv && base(t[0]) === "shift") argv = argv.slice(Number(t[1] ?? 1) || 1);
    if (argv && base(t[0]) === "set" && t[1] === "--") argv = t.slice(2);
    if (t[0] === "@path") {
      if (!notePath(t[1], here, chain)) return { rule: "unscanned", shown: `PATH=${t[1]}` };
      continue;
    }
    const rule = bashRule(t, projectDir, cwd, chain);
    if (rule) return { rule, shown: matched(t) };
    const dirs = [...new Set([here, cwd, scriptDir].filter(Boolean))];
    const hit = runScript(t, projectDir, dirs, chain, seen);
    if (hit) return hit;
    noteWrites(t, cwd, chain);
    if (!chain.state.opaque && opaqueWriter(t)) chain.state.opaque = t.join(" ");
    if (["cd", "pushd", "popd", "chdir"].includes(base(t[0]))) {
      const ops = t.slice(1).filter((d) => !/^-[LPe@]+$/.test(d) && !/^\d*[<>&]/.test(d));
      let to = ops[0];
      if (base(t[0]) === "popd" || to === "-" || ops.length > 1) to = undefined;
      else if (ops.length === 0) to = homedir();
      else
        to = to
          .replace(/^(~|\$\{?HOME\}?)(?=\/|$)/, homedir())
          .replace(
            /^\$\(\s*dirname\s+"?\$\{?(0|BASH_SOURCE(\[0\])?)\}?"?\s*\)(?=\/|$)/,
            scriptDir ?? "$",
          );
      if (to === undefined || /[$`*?[{~]/.test(to)) chain.lost = true;
      else {
        if (to.startsWith("/")) chain.lost = false;
        here = dirOf(here, to);
      }
    }
  }
  return null;
}

/** A script's `$1`..`$9`, `$@`, `$*` as whole words replaced by the literal arguments it got. */
function positional(t, argv) {
  const out = t.flatMap((x) => {
    const n = /^\$\{?([1-9])\}?$/.exec(x);
    if (n) return argv[n[1] - 1] !== undefined ? [argv[n[1] - 1]] : [x];
    if (/^\$\{?[@*]\}?$/.test(x)) return argv.length ? argv : [x];
    return [x];
  });
  if (t.docs) out.docs = t.docs;
  return wrap(out, t.wrapped);
}

const UNSET = "\uE003"; // a variable with no value (`${X:-w}` and `${X-w}` give `w`)
const SAVED_PATH = "\uE004"; // `_OLD=$PATH`: the PATH the shell started with
/** Every value a variable named in `text` can hold, for PATH entries (`PATH=$VENV/bin:$PATH`,
 * `PATH=${_OLD_PATH:-}`): its environment value, each literal the text assigns, empty after
 * `unset`. Any other mention (`read X`, `for X in`, `X+=`, a non-literal value) makes it unknown. */
function varValues(text, parent) {
  const vars = new Map(parent ?? []);
  vars.assigned = new Set(parent?.assigned ?? []); // `$X` alone needs X assigned in the text
  const assigned = [...String(text).matchAll(/(?<![\w$])([A-Za-z_]\w*)=/g)].map((m) => m[1]);
  for (const n of assigned) vars.assigned.add(n);
  const defaulted = [...String(text).matchAll(/\$\{([A-Za-z_]\w*):?-/g)].map((m) => m[1]);
  const names = new Set([...assigned, ...defaulted].filter((n) => n !== "PATH"));
  // A nameref, `((`/`let` arithmetic, `${X:=w}` or a sourced/eval'd text can change any variable.
  const opaque =
    /(^|[\s;&|(])(source|\.|eval)\s|\b(declare|local|typeset|readonly)\s+-\w*n|\(\(|\blet\s|\$\{!/.test(
      String(text),
    );
  for (const n of names) {
    if (opaque || new RegExp(`\\$\\{${n}:?[=?]`).test(text)) {
      vars.set(n, null);
      continue;
    }
    let vals = vars.has(n)
      ? vars.get(n) && new Set(vars.get(n))
      : new Set([process.env[n] ?? UNSET]);
    const re = new RegExp(`(?<![$\\w{#!])${n}(?!\\w)`, "g");
    for (const m of String(text).matchAll(re)) {
      if (!vals) break;
      const before = text.slice(0, m.index);
      const rest = text.slice(m.index + n.length);
      const v =
        /^=("[^"`$]*"|'[^']*'|"\$\{?PATH\}?"|\$\{?PATH\}?(?=[\s;&|)]|$)|[^\s;&|()<>"'`$\\]*(?=[\s;&|)]|$))/.exec(
          rest,
        );
      if (v) {
        const raw = v[1].replace(/^(["'])(.*)\1$/s, "$2");
        vals.add(/^\$\{?PATH\}?$/.test(raw) ? SAVED_PATH : raw);
      } else if (/(^|[\s;&|({])(export|readonly)\s+((-\w+|[A-Za-z_]\w*)\s+)*$/.test(before))
        continue;
      else if (/(^|[\s;&|({])unset\s+(-v\s+)?((-\w+|[A-Za-z_]\w*)\s+)*$/.test(before))
        vals.add(UNSET);
      else vals = null;
    }
    vars.set(n, vals);
  }
  return vars;
}

/** Each string a PATH value can be, its variables expanded from `vars`; null when unknown. */
function expandPath(value, vars) {
  let out = [String(value)];
  for (let round = 0; round < 4; round++) {
    const next = [];
    let changed = false;
    for (const s of out) {
      const m = /\$\{([A-Za-z_]\w*)(:?-([^}$`]*))?\}|\$([A-Za-z_]\w*)/.exec(s);
      if (!m) {
        next.push(s);
        continue;
      }
      const n = m[1] ?? m[4];
      if (n === "PATH" || n === "HOME" || n === "PWD") {
        // left for notePath (`$PATH` entries, `$HOME/x`, `$PWD/x`)
        next.push(s.replace(m[0], m[0].replace("$", "\uE005")));
        changed = true;
        continue;
      }
      const vals = vars?.get(n);
      if (!vals || (!m[2] && !vars.assigned.has(n))) return null;
      for (const v of vals) {
        let x = v;
        if (m[2] && (v === UNSET || (v === "" && m[2].startsWith(":")))) x = m[3];
        else if (v === UNSET) x = "";
        if (/[$`]/.test(x)) return null;
        next.push(
          s.replace(m[0], x === SAVED_PATH ? "$PATH" : x).replaceAll("$PATH", "\uE005PATH"),
        );
      }
      changed = true;
    }
    if (next.length > 16) return null;
    out = next;
    if (!changed) return out.map((s) => s.replaceAll("\uE005", "$"));
  }
  return null;
}

/** Add the literal dirs of a PATH value to the chain; false when a dir is not literal. */
function notePath(value, here, chain) {
  const all = expandPath(value, chain.vars);
  if (!all) return false;
  if (all.length !== 1 || all[0] !== value)
    return all.every((v) => notePath(v, here, { ...chain, vars: null }));
  for (const e of String(value).split(":")) {
    if (/^\$\{?PATH\}?$/.test(e)) continue;
    const dir = e
      .replace(
        /^(?:\$\((?:npm|pnpm|yarn) bin\)|`(?:npm|pnpm|yarn) bin`)(?=\/|$)/,
        `${here}/node_modules/.bin`,
      )
      .replace(/^(~|\$\{?HOME\}?)(?=\/|$)/, homedir())
      .replace(/^(\$\{?PWD\}?|\$\(pwd\)|`pwd`)(?=\/|$)/, here);
    if (/[$`*?[{]/.test(dir)) return false;
    const abs = resolve(here, dir || ".");
    // a system dir is on PATH already: what runs from it is not a script the command added
    if (!/^\/(usr\/(local\/)?)?s?bin$/.test(abs)) chain.path.push(abs);
  }
  return true;
}

/** True when a file is a shell script: a shell `.sh`-style name or a shell `#!`. */
function looksShell(abs) {
  if (/\.(sh|bash|zsh|ksh|mksh|dash|ash)$/.test(abs)) return true;
  try {
    const i = shebang(head(abs));
    return i !== null && SHELLS.has(i);
  } catch {
    return false;
  }
}

let shellBins;
/** True when a binary is a shell (or busybox/toybox) by real name, or a copy of one in /etc/shells. */
function shellBinary(abs) {
  try {
    const real = base(realpathSync(abs));
    if (SHELLS.has(real) || /^(busybox|toybox)/.test(real)) return true;
    if (!shellBins) {
      shellBins = [];
      let list = [];
      try {
        list = readFileSync("/etc/shells", "utf8").split("\n");
      } catch {
        // no /etc/shells
      }
      for (const p of [...list, "/bin/busybox", "/usr/bin/busybox", "/bin/sh", "/bin/bash"]) {
        try {
          if (p.startsWith("/")) shellBins.push(realpathSync(p.trim()));
        } catch {
          // not installed
        }
      }
    }
    const st = statSync(abs);
    for (const sh of new Set(shellBins)) {
      const ss = statSync(sh);
      if (ss.size !== st.size) continue;
      if (ss.ino === st.ino && ss.dev === st.dev) return true;
      if (readFileSync(sh).equals(readFileSync(abs))) return true;
    }
  } catch {
    // unreadable: not known as a shell
  }
  return false;
}

const MAX_SCRIPT = 512 * 1024;
/** Shell startup files in the home dir (relative to it). */
const RC_RE =
  /^(\.(bashrc|bash_profile|bash_login|profile|zshrc|zprofile|zshenv|zlogin|kshrc|mkshrc|cshrc|tcshrc)|\.config\/fish\/config\.fish)$/;
const MAX_SCRIPTS = 32;
const SHELL_OPT_VALUE = new Set([
  "-o",
  "+o",
  "-O",
  "+O",
  "--rcfile",
  "--init-file",
  "-rcfile",
  "-init-file",
]);
// `-rcfile` is bash's one-dash long option (it takes a file), not `-r -c`.
const SHELL_C_RE = /^-(?!rcfile$)[a-zA-Z]*c[a-zA-Z]*$/;

/** Where a shell command's code comes from: `{kind: "c"}` (a `-c` string), `"doc"`, `"herestring"`,
 * `"file"` (a script operand, its arguments), `"info"` (`--version`, `--help`) or `"stdin"` (with
 * the file of a `<file` redirect, if any). */
function shellSource(t) {
  const ops = [];
  let stdinFile;
  for (let n = 1; n < t.length; n++) {
    const a = t[n];
    if (a.startsWith("<<<")) return { kind: "herestring" };
    const r = /^0?<(?![<(&])(.*)$/s.exec(a);
    if (r) {
      stdinFile = r[1] || t[++n];
      continue;
    }
    if (/^\d*[<>&]/.test(a)) {
      if (/^(\d*|&)[<>]+[&|]?$/.test(a)) n++; // `2> f`, `> f`: the next word is the target
      continue;
    }
    ops.push(a);
  }
  if (t.docs?.length) return { kind: "doc" };
  // Parse only, exactly `bash|sh -n <one literal file>`: any other option may undo noexec
  // (`+n`, `+o noexec`) or start an interactive shell that ignores it.
  if (
    /^(bash|sh)$/.test(base(t[0])) &&
    ops.length === 2 &&
    ops[0] === "-n" &&
    stdinFile === undefined &&
    /^[^-+$`{}*?[~\\]/.test(ops[1]) &&
    !/[$`{}*?[\\]/.test(ops[1])
  )
    return { kind: "parse" };
  let i = 0;
  while (i < ops.length && /^[-+]./.test(ops[i])) {
    if (ops[i] === "--") {
      i++;
      break;
    }
    // `-rcfile f` is bash's long option with one dash, not `-r -c`.
    if (SHELL_OPT_VALUE.has(ops[i])) {
      i += 2;
      continue;
    }
    if (SHELL_C_RE.test(ops[i])) return { kind: "c" };
    if (/^-[a-zA-Z]*s[a-zA-Z]*$/.test(ops[i])) return { kind: "stdin", stdinFile }; // args follow
    if (/^--(version|help)$/.test(ops[i])) return { kind: "info" };
    i += SHELL_OPT_VALUE.has(ops[i]) ? 2 : 1;
  }
  if (i < ops.length && ops[i] !== "-")
    return { kind: "file", file: ops[i], args: ops.slice(i + 1) };
  return { kind: "stdin", stdinFile };
}

/** Command arguments without redirects (`2>/dev/null`, `> f`). */
const operands = (args) =>
  args.filter((a, n, all) => !/^\d*[<>&]/.test(a) && !/^(\d*|&)[<>]+[&|]?$/.test(all[n - 1] ?? ""));

/** The script files a command runs: `{file, shell, args}`, `shell` when a shell reads it whatever
 * its `#!` (`bash f`, `bash < f`, `source f`, `. f`), else a path run directly (`./f`,
 * `scripts/f`, `/tmp/f`, or a bare name found in a dir the chain added to PATH). */
function scriptRuns(t, chain) {
  if (/^\d*[<>&]/.test(t[0])) return []; // a bare redirect (`2>/dev/null`) runs nothing
  const prog = base(t[0]);
  const runs = [];
  if (prog === "source" || prog === ".") {
    const [file, ...args] = operands(t.slice(1));
    if (file) runs.push({ file, shell: true, args });
  } else if (SHELLS.has(prog)) {
    // `./bin/bash f` runs that file: read it unless it is a real shell (shellBinary)
    if (String(t[0]).includes("/"))
      runs.push({ file: t[0], shell: false, args: [], wrapped: !!t.wrapped, asShell: true });
    const src = shellSource(t);
    if (src.kind === "file") runs.push({ file: src.file, shell: true, args: src.args });
    if (src.kind === "stdin" && src.stdinFile)
      runs.push({ file: src.stdinFile, shell: true, args: [] });
  } else if (String(t[0]).includes("/") && !t[0].startsWith("@") && !INTERPRETERS.test(prog))
    runs.push({ file: t[0], shell: false, args: operands(t.slice(1)), wrapped: !!t.wrapped });
  if (!String(t[0]).includes("/") && !t[0].startsWith("@"))
    for (const d of chain.path) {
      const abs = join(d, t[0]);
      if (existsSync(abs))
        runs.push({ file: abs, shell: false, args: operands(t.slice(1)), wrapped: !!t.wrapped });
    }
  // A `$VAR` program or an unknown runner (`parallel`, `docker run`, ...) may run a shell script
  // it is given: such operands are read when they are shell scripts.
  if (!t[0].startsWith("@") && !SHELLS.has(prog) && !ARGS_ARE_DATA.has(prog))
    for (const f of operands(t.slice(1)))
      if (!f.startsWith("-") && !/[$`]/.test(f))
        runs.push({ file: f.replace(/^[^=]*=/, ""), shell: true, args: [], optional: true });
  // `source name` searches PATH for a name without a slash.
  const src = runs[0];
  if ((prog === "source" || prog === ".") && src && !src.file.includes("/"))
    for (const d of chain.path) {
      const abs = join(d, src.file);
      if (existsSync(abs)) runs.push({ ...src, file: abs });
    }
  // stdin and process substitution are refused in commands() (`@deny`).
  return runs.filter((r) => !/^(-|\/dev\/|\/proc\/|<\()/.test(r.file));
}

/** The program a `#!` line names, through `env [-S] [opts] [VAR=x]` and `busybox`. */
function shebang(text) {
  const m = /^#!\s*(\S+)([^\n]*)/.exec(text);
  if (!m) return null;
  let prog = base(m[1]);
  const rest = m[2].trim().split(/\s+/).filter(Boolean);
  if (prog === "env") {
    prog = "";
    while (rest.length) {
      const o = rest.shift();
      if (/^-S./.test(o)) {
        prog = base(o.slice(2));
        break;
      }
      if (["-u", "--unset", "-C", "--chdir"].includes(o)) rest.shift();
      else if (!o.startsWith("-") && !/^[A-Za-z_]\w*=/.test(o)) {
        prog = base(o);
        break;
      }
    }
  }
  if (prog === "busybox") prog = base(rest.shift() ?? "");
  return prog;
}

/** `#!` interpreters of languages the guard knows are not a shell (their code is a known gap). */
const KNOWN_LANG_RE =
  /^(?:python[\d.]*|node|nodejs|bun|deno|ruby|perl|php|awk|gawk|mawk|nawk|sed|make|gmake|tsx|ts-node)$/;

const ELF_MACHINE = { x64: 62, arm64: 183, ia32: 3, arm: 40, riscv64: 243, ppc64: 21, s390x: 22 };
const MACHO_CPU = { x64: 0x01000007, arm64: 0x0100000c };
/** True when the kernel runs `top` (a file's first bytes) as a host binary. Anything else is read
 * as text: a file the kernel refuses (ENOEXEC) is run by sh (bash, execvp in env/timeout/xargs),
 * so a fake magic number (`\x7fELF\ngit push ...`) is a shell script. A valid header for this
 * host (ELF on Linux, Mach-O on macOS: class, byte order, version, type, machine) with a NUL
 * before the first newline counts. */
function hostBinary(top) {
  const b = Buffer.from(top, "latin1");
  const nl = b.indexOf(10);
  if (b.subarray(0, nl < 0 ? b.length : nl).indexOf(0) < 0) return false;
  if (process.platform === "darwin") {
    if (b.length < 28 || b.readUInt32LE(0) !== 0xfeedfacf) return false;
    return b.readUInt32LE(4) === MACHO_CPU[process.arch] && b.readUInt32LE(12) === 2; // MH_EXECUTE
  }
  if (b.length < 64 || b.toString("latin1", 0, 4) !== "\x7fELF") return false;
  if (![1, 2].includes(b[4]) || ![1, 2].includes(b[5]) || b[6] !== 1) return false;
  const u16 = (o) => (b[5] === 1 ? b.readUInt16LE(o) : b.readUInt16BE(o));
  return [2, 3].includes(u16(16)) && u16(18) === ELF_MACHINE[process.arch];
}

/** Build tools: what they write is a build output, not a checked-out or unpacked tree. */
const BUILD_RE =
  /^(\S*\/)?(cargo|go|make|gmake|cmake|ninja|rustc|cc|gcc|g\+\+|clang|clang\+\+|zig|meson|bazel)\s/;

/** True for a host binary (hostBinary) that is not a shell under another name. */
function nativeBinary(abs) {
  try {
    return hostBinary(head(abs)) && !shellBinary(abs);
  } catch {
    return false;
  }
}

/** First bytes of a file as latin1 text (for `#!` and magic numbers). */
function head(abs) {
  const fd = openSync(abs, "r");
  try {
    const buf = Buffer.alloc(512);
    return buf.toString("latin1", 0, readSync(fd, buf, 0, 512, 0));
  } finally {
    closeSync(fd);
  }
}

/** Comments the tokenizer finds (not `#` inside quotes, heredocs or substitutions) blanked. */
function stripComments(text) {
  let out = "";
  let at = 0;
  for (const [a, b] of split(text).comments) {
    out += text.slice(at, a);
    at = b;
  }
  return out + text.slice(at);
}

/** A script the command runs is checked like typed commands. The guard reads all of it or refuses
 * (fail closed): too large, NUL bytes, unreadable, nested more than MAX_SCRIPTS deep, a path from
 * a variable or glob, or missing at hook time (`scriptLater`). Binaries and known-language `#!`
 * scripts run directly are not read. A script written earlier in the same command is refused. */
function runScript(t, projectDir, dirs, chain, seen) {
  for (const run of scriptRuns(t, chain)) {
    // `$HOME`, `$PWD` and `$CLAUDE_PROJECT_DIR` at the start are known; other variables are not.
    const name = stripTok(run.file)
      .replace(/^~\+(?=\/|$)/, dirs[0])
      .replace(/^(~|\$\{?HOME\}?)(?=\/)/, homedir())
      .replace(/^\$\{?PWD\}?(?=\/)/, dirs[0])
      .replace(/^\$\{?CLAUDE_PROJECT_DIR\}?(?=\/)/, projectDir);
    const refuse = (why) => ({ rule: "unscanned", shown: `${name}: ${why}` });
    // A glob runs what it matches now (none: refused as missing); `$`, backticks and braces
    // (`find -exec bash {}`) name a file the guard cannot know.
    const glob = /[*?[]/.test(name);
    if (/^~|[$`{]/.test(name) || (glob && typeof nodeFs.globSync !== "function")) {
      if (run.optional) continue;
      return refuse("a path the guard cannot read");
    }
    if (chain.lost && !name.startsWith("/")) {
      if (run.optional) continue;
      return refuse("relative, after a cd the guard cannot follow: use an absolute path");
    }
    const paths = dirs.flatMap((d) =>
      glob ? nodeFs.globSync(name, { cwd: d }).map((m) => resolve(d, m)) : [resolve(d, name)],
    );
    let found = false;
    const under = (set) => [...set].some((w) => abs0 === w || abs0.startsWith(`${w}/`));
    let abs0;
    for (const abs of paths) {
      abs0 = abs;
      let st;
      try {
        st = statSync(abs);
      } catch (e) {
        if (e?.code !== "ENOENT" && e?.code !== "ENOTDIR") return refuse("unreadable");
      }
      // An operand of an unknown runner or `$VAR` counts only when it is a shell script.
      if (run.optional && !(st?.isFile() && looksShell(abs))) continue;
      if (under(chain.tainted) || under(chain.made))
        return { rule: "evasion", shown: `${name}: written by this command` };
      if (under(chain.later.names))
        return { rule: "scriptLater", shown: `${name}: written in this same command` };
      // Whatever an earlier (or concurrent) command wrote is read only at run time: refuse.
      const opaque = chain.state.opaque ?? chain.later.opaque;
      // An existing ELF/Mach-O binary run directly (`cargo build && ./target/debug/x`) is not read
      // anyway; a script stays refused, even a tracked one: the writer (build scripts, install
      // hooks, `git pull`/`checkout` and their hooks) may change it before it runs (guard.md).
      // Through a wrapper or in a script a binary is refused anyway (below).
      const direct = !run.shell && !run.wrapped && !chain.script;
      if (opaque && !(st?.isFile() && direct && BUILD_RE.test(opaque) && nativeBinary(abs)))
        return { rule: "scriptLater", shown: `${name} with ${opaque}` };
      if (!st) continue;
      found = true;
      if (!st.isFile()) continue; // a directory: the shell fails
      const key = `${abs}\0${run.args.join("\0")}`;
      if (seen.has(key)) continue;
      let text;
      try {
        if (!run.shell) {
          const top = head(abs);
          if (hostBinary(top)) {
            if (shellBinary(abs)) {
              if (run.asShell) continue; // `/bin/bash -c ...`: the shell itself, read as one
              return refuse("a shell under another name: run it by its name");
            }
            // Only bash or zsh running it directly refuse a binary the kernel refuses; a wrapper
            // (execvp) or a script's sh hands it to /bin/sh. The guard does not judge loadability.
            if (run.wrapped || chain.script)
              return refuse("a binary through a wrapper, runner or script: run it directly");
            continue; // run directly: bash refuses it if the kernel does
          }
          const interp = shebang(top);
          if (interp !== null && !SHELLS.has(interp)) {
            if (KNOWN_LANG_RE.test(interp)) continue; // another language: a known gap
            return refuse(`#! names ${interp || "nothing"}, an interpreter the guard cannot read`);
          }
        }
        if (st.size > MAX_SCRIPT) return refuse(`larger than ${MAX_SCRIPT} bytes`);
        text = readFileSync(abs, "utf8");
      } catch {
        return refuse("unreadable");
      }
      if (text.includes("\0")) return refuse("holds NUL bytes");
      if (seen.size >= MAX_SCRIPTS) return refuse(`more than ${MAX_SCRIPTS} scripts`);
      seen.add(key);
      const hit = checkShell(stripComments(text), projectDir, dirs[0], {
        seen,
        parent: chain,
        guarded: chain.guarded,
        lost: chain.lost,
        script: true,
        scriptDir: dirname(abs),
        argv: run.args,
      });
      // A home startup file the guard cannot pass: it already ran for this shell; say so.
      if (hit && RC_RE.test(posix.relative(homedir(), abs)))
        return { rule: "rcfile", shown: `${name}: ${hit.shown}` };
      if (hit) return { rule: hit.rule, shown: `${name}: ${hit.shown}` };
    }
    // A script missing now could run whatever is written there later (a shell fails on it anyway).
    if (!found && !run.optional && (run.shell || chain.script || chain.state.opaque))
      return { rule: "scriptLater", shown: `${name}: missing when the guard ran` };
  }
  return null;
}

/**
 * @param {{ tool_name: string, tool_input: Record<string, unknown> }} input
 * @param {{ lockText?: string, projectDir: string }} ctx
 * @returns {null | { decision: "deny" | "ask", reason: string }}
 */
function decideHead(input, ctx) {
  const name = input?.tool_name;
  const ti = input?.tool_input ?? {};
  const deny = (key, hit) => {
    const shown = String(hit ?? "")
      .replace(/\s+/g, " ")
      .trim()
      .slice(0, 160);
    return { decision: "deny", reason: `${REASON[key]} (rule ${key}, matched \`${shown}\`)` };
  };
  const cwd = [ctx.cwd, input?.cwd].find((x) => typeof x === "string" && x) ?? ctx.projectDir;
  if (name === "Bash") {
    // A cwd persisted inside a guarded dir (an earlier `cd .bosun`) starts the chain read-only.
    const guarded = cwd !== ctx.projectDir && guardedDir(cwd, ctx.projectDir, ctx.projectDir);
    const hit = checkShell(String(ti.command ?? ""), ctx.projectDir, cwd, {
      guarded,
      seen: new Set(),
    });
    return hit ? deny(hit.rule, hit.shown) : null;
  }
  if (!FILE_TOOLS.has(name)) return null;
  const candidates = [targetPath(ti), ti.glob];
  if (name === "Glob") candidates.push(ti.pattern);
  const secretHit = candidates.find(secretLike);
  if (secretHit) return deny("secret", secretHit);
  if (!EDIT_TOOLS.has(name)) return null;
  const target = targetPath(ti);
  if (typeof target !== "string" || !target) return null;
  const rel = relPath(target, ctx.projectDir, cwd);
  if (LOCK_RE.test(rel)) return deny("lock", target);
  if (isProtected(rel)) return deny("protected", target);
  const real = realRel(target, ctx.projectDir, cwd);
  if (real && LOCK_RE.test(real)) return deny("lock", target);
  if (real && isProtected(real)) return deny("protected", target);
  if (ctx.lockText && ownedPaths(ctx.lockText).has(rel)) {
    return { decision: "ask", reason: ownedReason(rel) };
  }
  return null;
}

/** A word of an enumerated allow: letters, digits and `-_./:=@,+%` only (no quote, `$`, backquote,
 * glob, redirect, separator, escape or newline). */
const PLAIN_WORD_RE = /^[A-Za-z0-9\-_./:=@,+%]+$/;

/** `gh api -X POST repos/<o>/<r>/pulls -f k=v ... [-F body=@<file>]`: the documented PR-create
 * route. Only -X/--method POST, -f/--raw-field `k=v`, -F/--field `body=@<file>` and the one
 * endpoint; guard.mjs's own checks (literal, taint, repo body file) ran before. */
function ghApiPullsShape(w) {
  if (w[0] !== "gh" || w[1] !== "api") return false;
  let post = 0;
  const ends = [];
  for (let i = 2; i < w.length; i++) {
    const x = w[i];
    if (x === "-X" || x === "--method") {
      if (w[++i] !== "POST") return false;
      post++;
    } else if (x === "-XPOST" || x === "--method=POST") post++;
    else if (x === "-f" || x === "--raw-field") {
      if (!/^[a-z_]+=/.test(w[++i] ?? "")) return false;
    } else if (x === "-F" || x === "--field") {
      if (!/^body=@[A-Za-z0-9_][\w./-]*$/.test(w[++i] ?? "")) return false;
    } else if (x.startsWith("-")) return false;
    else ends.push(x);
  }
  return (
    post === 1 &&
    ends.length === 1 &&
    /^\/?repos\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\/pulls$/.test(ends[0])
  );
}

/** The enumerated intended allows: the only Bash commands guard.mjs lets through although the
 * legacy judge (guard-legacy.mjs, main's guard) denies them. Each is a literal shape with its own
 * test (tests/guard-no-regress.test.mjs); adding one needs evidence that it is intended. Shapes
 * legacy already allows (`gh pr|issue create|new`, `gh workflow run <non-release file>`) need no
 * entry. */
const INTENDED_ALLOWS = [{ name: "gh-api-pulls", shape: ghApiPullsShape }];

/** The name of the enumerated allow `command` is exactly, or null. */
export function intendedAllow(command) {
  const s = String(command ?? "");
  if (/[\n\r]/.test(s)) return null;
  const w = s.split(/[ \t]+/).filter(Boolean);
  if (!w.length || !w.every((x) => PLAIN_WORD_RE.test(x))) return null;
  return INTENDED_ALLOWS.find((a) => a.shape(w))?.name ?? null;
}

/** The legacy judge's decision, as main's hook made it: an internal error (or a missing judge)
 * denies a call that looks like push, publish, release, gh or secrets. */
function legacyDecision(input, ctx) {
  try {
    if (!LEGACY) throw new Error("guard-legacy.mjs did not load");
    return LEGACY.decideLegacy(input, ctx);
  } catch {
    const raw = JSON.stringify(input ?? null);
    return (LEGACY?.failClosedLegacy ?? failClosed)(raw)
      ? { decision: "deny", reason: `${REASON.failClosed} (rule failClosed, legacy judge)` }
      : null;
  }
}

/**
 * The hook's decision, with the no-regression rule: deny when guard.mjs's rules deny or when the
 * legacy judge (main's guard) denies, unless the Bash command is exactly an enumerated intended
 * allow (`intendedAllow`). guard.mjs runs first, so its credential and taint rules apply inside
 * the allow shapes too.
 * @param {{ tool_name: string, tool_input: Record<string, unknown> }} input
 * @param {{ lockText?: string, projectDir: string }} ctx
 * @returns {null | { decision: "deny" | "ask", reason: string }}
 */
/** Input size (JSON chars) up to which a legacy deny also runs the new rules for their reason. */
const QUICK_INPUT = 2000;

export function decide(input, ctx) {
  // The legacy judge runs first: whatever main denies is denied as fast as main denies it, so
  // the new rules' extra work can never push such a call past the hook's timeout.
  const old = legacyDecision(input, ctx);
  if (old?.decision === "deny") {
    const bashCmd = input?.tool_name === "Bash" ? String(input?.tool_input?.command ?? "") : null;
    if (bashCmd === null || intendedAllow(bashCmd) === null) {
      // A short call (fast for the new rules) gets the new rules' more precise reason.
      if (JSON.stringify(input ?? null).length <= QUICK_INPUT) {
        const head = decideHead(input, ctx);
        if (head?.decision === "deny") return head;
      }
      return { decision: "deny", reason: `${REASON.legacy} Main's guard says: ${old.reason}` };
    }
  }
  const head = decideHead(input, ctx);
  if (head?.decision === "deny") return head;
  if (old?.decision === "deny") return head; // an enumerated allow
  return head ?? old ?? null; // an `ask` from either
}

/** True when a git path argument could name a protected file or a dir that holds one. */
function riskyPathspec(p) {
  const x = stripTok(p).replace(/^\.\//, "");
  if (!x || x === "." || /^[:/]|(^|\/)\.\.(\/|$)|[*?[{\\]/.test(x)) return true;
  const dir = x.replace(/\/+$/, "");
  return (
    isProtected(dir) ||
    isProtected(`${dir}/x`) ||
    LOCK_RE.test(dir) ||
    [
      ".claude/settings.json",
      ".github/workflows/release.yml",
      ".bosun/x",
      ".splice/lock.toml",
    ].some((s) => s === dir || s.startsWith(`${dir}/`))
  );
}

/** A patch the repo itself yields: git diff/show/format-patch with an explicit `--` pathspec that
 * names no protected path. Plain `diff`, `--no-index`, prefixes and no pathspec are not. */
function isRepoPatch(t) {
  if (base(t[0]) !== "git" || !["diff", "show", "format-patch"].includes(gitSub(t).sub))
    return false;
  if (t.some((x) => /^--(no-index|src-prefix|dst-prefix|relative)(=|$)/.test(x))) return false;
  const dd = t.indexOf("--");
  const specs = dd < 0 ? [] : t.slice(dd + 1).filter((x) => !/^(\d*>|--output)/.test(x));
  return specs.length > 0 && !specs.some((x) => riskyPathspec(x) || /^\d*>/.test(x));
}

/** Programs whose arguments are data, never a command or script they run. */
const ARGS_ARE_DATA = new Set(
  (
    "ls cat head tail wc grep egrep fgrep rg ag stat du diff cmp jq pwd echo printf true false test [ " +
    "cd pushd popd basename dirname realpath readlink sha256sum md5sum mkdir cp mv ln install tee touch " +
    "rm rmdir unlink chmod chown truncate export local declare readonly typeset unset set shift sleep " +
    "date sort uniq cut tr nl tac rev comm join paste fold fmt column od hexdump file which type hash " +
    "id whoami hostname uname seq git gh npm pnpm yarn bun cargo rustc rustup hoist sops age splice " +
    "bosun keel berth hoard tsc vp turbo node nodejs deno python python3 ruby perl php alias trap " +
    "source . eval for while until if case select function return exit kill wait printenv curl wget find"
  ).split(" "),
);

/** Commands whose file writes the guard can name (targets tracked by noteWrites) or that write none. */
const TRANSPARENT = new Set(
  (
    "mkdir cp mv ln install tee touch rm rmdir unlink chmod chown truncate dd export set shift unset " +
    "local declare readonly typeset sleep date test [[ : for while until if fi done esac case " +
    "return exit break continue wait trap sort uniq cut tr nl tac rev comm join paste fold fmt " +
    "column od hexdump file which type hash umask id whoami hostname uname env printenv seq " +
    "yes tree less more source ."
  ).split(" "),
);
/** git subcommands that write no work-tree file and run no hook (commit runs hooks). */
const GIT_SAFE = new Set(
  "status log diff show ls-files blame rev-parse fetch push branch tag remote ls-remote describe shortlog cat-file rev-list merge-base symbolic-ref for-each-ref add grep reflog name-rev var version help config".split(
    " ",
  ),
);

/** True when `t` may write files whose names or content the guard cannot know before it runs: a
 * download, archive, decoder, patch, build tool, package manager or interpreter. */
function opaqueWriter(t) {
  const prog = base(t[0] ?? "");
  if (!prog || prog.startsWith("@") || /^\d*[<>&]/.test(t[0])) return false;
  if (READ_ONLY_PROGS.has(prog) || SHELLS.has(prog)) return false;
  if (prog === "git")
    return !GIT_SAFE.has(gitSub(t).sub) || t.some((x) => /^(-o|--output)/.test(x));
  // `-t dir` writes names the guard does not track
  if (
    ["cp", "mv", "ln", "install"].includes(prog) &&
    t.some((x) => /^(-[a-zA-Z]*t|--target-directory)/.test(x))
  )
    return true;
  if (
    MANAGERS.has(prog) &&
    /^(bin|root|view|info|ls|list|why|-v|--version|config)$/.test(t[1] ?? "")
  )
    return false;
  if (prog === "gh")
    return t.some((x) => ["download", "clone", "checkout", "extension"].includes(x));
  if (prog === "curl")
    return t.some((x) =>
      /^-[a-zA-Z]*[oOJK]|^--(output|remote-name|output-dir|config|create-dirs)/.test(x),
    );
  if (prog === "find") return t.some((x) => /^-(fprint|fprintf|fls|delete)/.test(x));
  // An output file the guard does not track: `sort -o F`, `--output[=]F` (or a prefix: `--out`),
  // `tree -o F`, `less -o/-O F`, `--log-file`, `file -C` (writes magic.mgc), uniq's 2nd operand.
  const args = operands(t.slice(1));
  if (["sort", "tree"].includes(prog)) return args.some((x) => /^-[a-zA-Z]*o|^--o/.test(x));
  if (prog === "less") return args.some((x) => /^-[a-zA-Z]*[oO]|^--(log|LOG)/.test(x));
  if (prog === "file") return args.some((x) => /^-[a-zA-Z]*C|^--comp/.test(x));
  if (prog === "uniq") return args.filter((x) => x === "-" || !x.startsWith("-")).length > 1;
  return !TRANSPARENT.has(prog);
}

/** Track files written earlier in a chain: a repo patch (`git diff -- src > f`) is the repo's own
 * change; any other write leaves content the guard cannot read. */
function noteWrites(t, cwd, chain) {
  const repoPatch = isRepoPatch(t);
  for (let i = 0; i < t.length; i++) {
    let name = "";
    if (/^--output=/.test(t[i])) name = t[i].slice(9);
    else if (writesTarget(t, i) && !t[i].startsWith("-")) name = stripTok(t[i]);
    else continue;
    if (!name || /^\d+$/.test(name)) continue;
    const abs = dirOf(cwd, name);
    const viaRedirect =
      /^\d*>/.test(t[i]) || /^\d*>[>|]?$/.test(t[i - 1] ?? "") || t[i].startsWith("--output=");
    const append = />>/.test(t[i]) || />>$/.test(t[i - 1] ?? "");
    if (repoPatch && viaRedirect && !append) {
      chain.made.add(abs);
      chain.tainted.delete(abs);
    } else {
      chain.made.delete(abs);
      chain.tainted.add(abs);
    }
  }
}

/** True when a call that failed inside the guard still looks like push, publish, release or secrets. */
export function failClosed(raw) {
  return RISKY_RE.test(String(raw ?? "")) || FAIL_CLOSED_RE.test(String(raw ?? ""));
}

const emit = (decision, reason) =>
  process.stdout.write(
    `${JSON.stringify({
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: decision,
        permissionDecisionReason: reason,
      },
    })}\n`,
  );

function main() {
  let raw = "";
  try {
    raw = readFileSync(0, "utf8");
    const input = JSON.parse(raw);
    const projectDir = process.env.CLAUDE_PROJECT_DIR || process.cwd();
    let lockText;
    try {
      lockText = readFileSync(resolve(projectDir, ".splice/lock.toml"), "utf8");
    } catch {
      lockText = undefined;
    }
    const d = decide(input, { lockText, projectDir });
    if (d) emit(d.decision, d.reason);
  } catch (err) {
    process.stderr.write(
      `guard.mjs: internal error: ${String(err?.message ?? err).split("\n")[0]}\n`,
    );
    if (failClosed(raw)) emit("deny", REASON.failClosed);
  }
}

/** Entry check, fail-safe: only a different real file (an import) skips main(). */
const isMain = () => {
  const entry = process.argv[1];
  if (!entry) return true;
  try {
    return realpathSync(entry) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return true;
  }
};
if (isMain()) main();
