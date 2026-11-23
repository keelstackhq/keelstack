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
// A shell script the command runs (`bash f`, `bash < f`, `source f`, `./f` with a shell, `env -S`
// or busybox shell, or no `#!`; a bare name in a dir the command adds to PATH) is read and judged
// like typed commands, nested scripts, function bodies and startup files (`BASH_ENV`, `ENV`,
// `--rcfile`) too: they share the chain (a file written earlier is
// refused) and `$1`/`$@` stand for the literal arguments. Fail closed: a script the guard cannot
// read in full (> 512 KiB, NUL bytes, unreadable, missing inside a script, > 32 scripts, a path
// from a variable or `{}`, a shell fed by a pipe, stdin or an expanding here-string, `xargs bash`)
// is refused; so are `alias x=...` and a PATH dir that is not literal ($HOME, $PWD, $(pwd) known).
// `gh api` with an explicit GET and graphql queries are reads; a POST whose endpoint (the first
// positional) dispatches a literal workflow file that is not a release workflow (release, publish,
// deploy; GUARD_RELEASE_WORKFLOWS overrides) or re-runs a run/job matches `gh workflow run` /
// `gh run rerun`. `-F x=@file` and `--input` read a file into the request: refused (use `-f`).
// `cd` into a guarded dir is allowed when the rest of the chain only reads (docs/agents/guard.md
// lists the safe patterns). A deny names its rule and the match.
// Known gaps: scripts in other languages, binaries, a `$VAR`/`"$@"` in command position (only
// text-checked), a glob script path that matches nothing at hook time, a re-run by run/job id of a
// release run (the id does not name its workflow), a `cd` to a variable inside a script, editing
// guarded files from an interpreter, a dir reached through a variable or substitution in a flag
// value (`tar -C "$D"`; `cd "$D"` itself is refused), a patch piped in (refused) or made by a
// producer other than `git diff|show|format-patch -- <paths>` (refused), and a default branch
// that origin/HEAD does not name. Node built-ins only.
// An internal error denies when the call mentions push, publish, release, gh, secret or sops (retry),
// else allows and prints one stderr line. Settings deny rules only cover force pushes and publish
// commands: the guard is the one layer for default-branch pushes and secrets.
import {
  closeSync,
  existsSync,
  openSync,
  readFileSync,
  readSync,
  realpathSync,
  statSync,
} from "node:fs";
import nodeFs from "node:fs"; // `globSync` (Node 22+), looked up at run time
import { homedir } from "node:os";
import { basename, dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SECRET_SAFE = new Set([".env.example", ".env.sample", ".env.template", ".dev.vars.example"]);
const MANAGERS = new Set(["npm", "pnpm", "yarn", "bun"]);
const SHELLS = new Set(["bash", "sh", "zsh", "dash", "ksh", "ash", "fish"]);
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
  publish: "Releases run in CI from a tag a human pushes (release.yml); do not publish from here.",
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
    "gh api writes are blocked: use plain GET reads; start a workflow with `gh workflow run <file> --ref <branch>` and re-run one with `gh run rerun <run-id>` (release, publish and deploy workflows excepted); for anything else ask the human (a PR merges by its owner).",
  ghField:
    "gh api -F/--field @file and --input read a local file into the request: pass literal values with -f/--raw-field (`-f ref=main -f inputs[x]=1`).",
  unscanned:
    "The guard must read a script before a shell runs it, and cannot read this one (too large, binary bytes, missing, unreadable, nested too deep, fed on stdin or a pipe, or named by a variable, glob or {}): run it as `bash <path>` with a literal path to a readable text script, or run the command directly.",
  alias:
    "Aliases hide the command they run from the guard: run the command directly (git push -u origin <feature-branch>).",
  ghMerge:
    "gh pr merge only with --auto (merges once required checks pass); never --admin or an immediate merge: ask the human.",
  ghAdmin:
    "Releases, repo settings, branch protection, rulesets and secrets belong to the human: ask the human to run it; gh pr create, view, checks and merge --auto stay open.",
  evasion:
    "Hidden or indirect push, publish or release is blocked: run the command directly (git push -u origin <feature-branch>) so the guard can see it.",
  lock: "only splice writes the lock (.splice/lock.toml): run `splice update` or `splice add` instead.",
  protected:
    "Guard rails (.claude/hooks, .claude/settings*.json, release workflows, .bosun/, pins.toml, git hooksPath and aliases) belong to the human: ask the human; put local rules in docs/agents/project.md. Use paths from the repo root (no cd, -C, --chdir or --work-tree into guarded dirs) and apply patches from a file in the repo.",
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

/** End index of the substitution opened at `i` (`$(`, `<(`, `>(` or a backtick), or the text's end. */
function subEnd(cmd, i) {
  if (cmd[i] === "`") {
    for (let j = i + 1; j < cmd.length; j++) {
      if (cmd[j] === "\\") j++;
      else if (cmd[j] === "`") return j;
    }
    return cmd.length;
  }
  let depth = 1; // `i` is the opening `(`
  let cases = 0; // inside `case ... esac`, a pattern's `)` does not close the substitution
  let q = "";
  // A keyword in command position: after `(`, `;`, `&`, `|` or a newline (spaces between).
  const word = (j, w) =>
    cmd.startsWith(w, j) &&
    /[(;&|\n]$/.test(cmd.slice(i, j).replace(/[ \t]+$/, "")) &&
    /^[\s;&|)]?$/.test(cmd[j + w.length] ?? "");
  for (let j = i + 1; j < cmd.length; j++) {
    const c = cmd[j];
    if (q) {
      if (c === q) q = "";
      else if (c === "\\" && q === '"') j++;
    } else if (c === "'" || c === '"') q = c;
    else if (c === "\\") j++;
    else if (word(j, "case")) cases++;
    else if (word(j, "esac")) cases = Math.max(0, cases - 1);
    else if (cases && (c === "(" || c === ")")) continue;
    else if (c === "(") depth++;
    else if (c === ")" && --depth === 0) return j;
  }
  return cmd.length;
}

/** Bodies of `$(...)`, `<(...)`, `>(...)` and backticks in heredoc text (quotes there are literal). */
function docSubs(body) {
  const out = [];
  for (let i = 0; i < body.length; i++) {
    if (body[i] === "\\") i++;
    else if (body[i] === "`" || (body[i] === "$" && body[i + 1] === "(")) {
      const e = subEnd(body, body[i] === "`" ? i : i + 1);
      out.push(body.slice(i + (body[i] === "`" ? 1 : 2), e));
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
  const segments = [];
  const subs = [];
  let tokens = [];
  let cur = "";
  let has = false;
  let quote = "";
  let pending = [];
  let docs = [];
  let expand = false; // a heredoc with an unquoted delimiter holds `$` or a backtick
  const comments = [];
  const endToken = () => {
    if (has) tokens.push(cur);
    cur = "";
    has = false;
  };
  const endSegment = () => {
    endToken();
    if (tokens.length) {
      if (docs.length) tokens.docs = docs;
      if (expand) tokens.docExpand = true;
      segments.push(tokens);
    }
    tokens = [];
    docs = [];
    expand = false;
  };
  for (let i = 0; i < cmd.length; i++) {
    const c = cmd[i];
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
    } else if (!quote && c === "#" && !has) {
      // A comment runs nothing: skip to the end of the line.
      const nl = cmd.indexOf("\n", i);
      comments.push([i, nl < 0 ? cmd.length : nl]);
      if (nl < 0) break;
      i = nl - 1;
    } else if (open) {
      const at = c === "`" ? i : i + 1;
      const e = subEnd(cmd, at);
      subs.push(cmd.slice(at + 1, e));
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
    } else if (c === "<" && cmd[i + 1] === "<" && cmd[i + 2] !== "<") {
      const m = /^<<(-?)[ \t]*((?:'[^']*'|"[^"]*"|\\.|[^\s;&|<>()'"\\])+)/.exec(cmd.slice(i));
      if (m) {
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
        while (at < cmd.length) {
          let end = cmd.indexOf("\n", at);
          if (end < 0) end = cmd.length;
          const line = cmd.slice(at, end);
          at = end + 1;
          if ((strip ? line.replace(/^\t+/, "") : line) === word) break;
          body += `${line}\n`;
        }
        docs.push(body);
        if (!data) subs.push(...docSubs(body)); // an unquoted delimiter runs $(...) and backticks
        if (!data && /[$`]/.test(body)) expand = true;
      }
      pending = [];
      i = at - 1;
      endSegment();
    } else if (c === "|" && cur.endsWith(">")) {
      cur += c; // `>|` forces a redirect
    } else if (c === ";" || c === "|" || c === "\n") {
      endSegment();
    } else if (c === "&" && !cur.endsWith(">") && !cur.endsWith("<")) {
      endSegment();
    } else if (c === " " || c === "\t") {
      endToken();
    } else {
      cur += c;
      has = true;
    }
  }
  endSegment();
  segments.subs = subs;
  segments.comments = comments;
  return segments;
}

function stripPrefixes(tokens, inCase = false) {
  // A function definition runs its body when called: `f() { cmd; }`, `function f { cmd; }`.
  if (tokens[0] === "function") tokens = tokens.slice(2);
  else if (/^[^\s()$`'"]+\(\)[{(]?$/.test(tokens[0] ?? "")) tokens = tokens.slice(1);
  if (tokens[0] === "()") tokens = tokens.slice(1);
  // Inside `case`, a lone `pat)` on its own line is a pattern, not a command.
  if (inCase && tokens.length === 1 && /^[^()]*\)$/.test(tokens[0])) return [];
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
  for (;;) {
    const raw = t[0];
    if (raw === undefined) return t;
    const a = base(raw);
    const b = t[1];
    if (/^[A-Za-z_][A-Za-z0-9_]*=/.test(raw)) t = t.slice(1);
    else if (
      [
        "then",
        "do",
        "else",
        "elif",
        "!",
        "exec",
        "command",
        "builtin",
        "time",
        "nohup",
        "setsid",
      ].includes(a)
    ) {
      t = t.slice(1);
      skipOptions();
    } else if (a === "env") {
      t = t.slice(1);
      skipEnvOptions();
    } else if (a === "npx") {
      t = t.slice(1);
      skipOptions();
    } else if (a === "sudo") {
      t = t.slice(1);
      skipOptions(["-u", "-g", "-h", "-p", "-C", "-D", "-R", "-T", "-U"]);
    } else if (a === "nice") {
      t = t.slice(1);
      skipOptions(["-n", "--adjustment"]);
    } else if (a === "ionice") {
      t = t.slice(1);
      skipOptions(["-c", "-n", "-p", "-P", "-u", "--class", "--classdata", "--pid"]);
    } else if (a === "stdbuf") {
      t = t.slice(1);
      skipOptions(["-i", "-o", "-e", "--input", "--output", "--error"]);
    } else if (a === "doas") {
      t = t.slice(1);
      skipOptions(["-u", "-C"]);
    } else if (a === "busybox") {
      t = t.slice(1);
    } else if (a === "watch") {
      t = t.slice(1);
      skipOptions(["-n", "--interval"]);
      if (t.length === 1 && /\s/.test(t[0])) t = split(t[0]).flat();
    } else if (a === "xargs") {
      t = t.slice(1);
      skipOptions(["-n", "-I", "-L", "-P", "-s", "-d", "-E"]);
    } else if (a === "timeout") {
      t = t.slice(1);
      skipOptions(["-s", "-k"]);
      t = t.slice(1);
    } else if (a === "pnpm" && (b === "exec" || b === "dlx")) {
      t = t.slice(2);
      skipOptions();
    } else return t;
  }
}

/** Simple commands in `cmd` as token lists, prefixes stripped, `bash -c '<s>'` expanded recursively.
 * Pseudo-commands `@evade` (text with a risky word is refused) and `@evade-exec` (same, for code
 * bodies) mark constructs the guard cannot read. */
const fed = (x) => /^(\/dev\/(stdin|fd\/\d+)|\/proc\/[^/]+\/fd\/\d+|-|<\(.*)$/.test(x);

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
  for (const sub of segs.subs) for (const inner of commands(sub)) out.push(inner);
  let caseDepth = 0;
  for (const seg of segs) {
    if (base(seg[0].replace(/^[({]+/, "")) === "case") caseDepth++;
    const t = stripPrefixes(seg, caseDepth > 0)
      .flatMap(splitRedirect)
      .map(unmark);
    if (seg.some((x) => /^esac\b/.test(x)) && caseDepth > 0) caseDepth--;
    // An assignment that makes git or a pager run a program (`GIT_EXTERNAL_DIFF=x git diff`).
    if (seg.some((x) => RUNS_ENV_RE.test(x))) out.push(["@runs-env", seg.join(" ")]);
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
    // `xargs git ...` takes its last arguments from stdin; `find -exec cmd ... ;` runs cmd.
    const viaXargs = base(seg[0].replace(/^[({]+/, "")) === "xargs";
    if (viaXargs) out.push(["@evade", text]); // args come from the pipe
    if (viaXargs && (SHELLS.has(prog) || prog === "source" || prog === "."))
      out.push(["@deny", "unscanned", segText]); // the script path comes from the pipe
    if (prog === "alias" && t.slice(1).some((x) => x.includes("=")))
      out.push(["@deny", "alias", segText]);
    if (SHELLS.has(prog))
      for (let n = 1; n < t.length; n++) {
        const rc = /^--(?:rcfile|init-file)(?:=(.*))?$/s.exec(t[n]);
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
    if (prog === "find") {
      const e = t.findIndex((x) => x === "-exec" || x === "-execdir" || x === "-ok");
      if (e >= 0) {
        const end = t.findIndex((x, n) => n > e && (x === ";" || x === "+"));
        const inner = stripPrefixes(t.slice(e + 1, end < 0 ? undefined : end));
        if (inner.length) out.push(inner);
        const ci = inner.findIndex((x, n) => n > 0 && SHELL_C_RE.test(x));
        if (SHELLS.has(base(inner[0] ?? "")) && ci > 0 && inner[ci + 1] !== undefined)
          for (const x of commands(inner[ci + 1])) out.push(x);
      }
    }
    if (/^[$`]|\$\(/.test(t[0])) out.push(["@evade", segText]);
    // `git $SUB ...` hides the subcommand; `git -C "$D" push` hides where it runs. A variable after
    // the subcommand is an argument.
    if (prog === "git") {
      const { sub, args } = gitSub(t);
      const hidden = (x) => /^[$`]|\$\(/.test(x ?? "");
      if (hidden(sub)) out.push(["@evade", text]);
      else if (t.slice(1, t.length - args.length - 1).some(hidden)) out.push(["@evade", segText]);
    }
    if (prog === "eval") {
      const body = t.slice(1).join(" ");
      out.push(["@evade", body]);
      for (const inner of commands(body)) out.push(inner);
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
      if (SHELLS.has(prog)) for (const inner of commands(body)) out.push(inner);
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
        for (const inner of commands(body)) out.push(inner);
      }
      const i = t.findIndex((x, n) => n > 0 && SHELL_C_RE.test(x));
      if (i > 0 && t[i + 1] !== undefined) {
        for (const inner of commands(t[i + 1])) out.push(inner);
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
    if (!a.startsWith("-") && /[$`]/.test(a)) return "evasion";
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
  const tok = t[i];
  if (/^\d*>[>|]?/.test(tok) || /^&>>?/.test(tok) || /^\d*<>/.test(tok)) return true;
  if (/^\d*>[>|]?$|^&>>?$|^\d*<>$/.test(t[i - 1] ?? "")) return true;
  const prog = base(t[0]);
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

/** `gh api` that writes: a non-GET method, or with no method a body/field flag (these imply POST);
 * graphql writes on a mutation or a query the guard cannot read (a file, stdin or a variable). */
function ghApiWrites(args) {
  let method;
  let body = false;
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === "-X" || a === "--method") method = args[i + 1];
    else if (a.startsWith("--method=")) method = a.slice(9);
    else if (/^-X./.test(a)) method = a.slice(2);
    if (/^(-[fF]|--field|--raw-field|--input)(=|$)/.test(a) || /^-[fF][^-]/.test(a)) body = true;
  }
  if (args.includes("graphql"))
    return args.some(
      (a) => /\bmutation\b/i.test(a) || /^--input(=|$)/.test(a) || /query=[@$]/.test(a),
    );
  if (method !== undefined) return String(method).toUpperCase() !== "GET";
  return body;
}

/** `gh api` flags that take a value (the next argument unless written `--flag=value`). */
const GH_API_VALUE = new Set(
  "-X --method -f --raw-field -F --field -H --header -q --jq -t --template --input --hostname --cache -p --preview".split(
    " ",
  ),
);

/** `gh api` arguments read like gh: the endpoint is the first positional (flag values skipped);
 * `fileField` is a `-F`/`--field` value read from a file (`@path`) and `input` a `--input` body. */
function ghApiParse(args) {
  let method;
  let endpoint;
  let fileField;
  let input;
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    let flag = a;
    let value;
    const eq = /^(--[\w-]+)=(.*)$/s.exec(a);
    if (eq) [flag, value] = [eq[1], eq[2]];
    else if (/^-[XfFHqtp]./.test(a)) [flag, value] = [a.slice(0, 2), a.slice(2)];
    else if (GH_API_VALUE.has(a)) value = args[++i];
    else if (!a.startsWith("-") && endpoint === undefined) endpoint = a;
    if (flag === "-X" || flag === "--method") method = value;
    if ((flag === "-F" || flag === "--field") && /^[^=]*=@/.test(value ?? "")) fileField = value;
    if (flag === "--input") input = value ?? "";
    if ((flag === "-H" || flag === "--header") && /method-override/i.test(value ?? ""))
      method = "OVERRIDE"; // the header changes the method: never a plain POST
  }
  return { method, endpoint, fileField, input };
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

const RUNS_ENV_RE =
  /^(GIT_[A-Z0-9_]+|PAGER|LESSOPEN|LESSCLOSE|LD_PRELOAD|LD_LIBRARY_PATH|BASH_ENV|ENV|EDITOR|VISUAL|NODE_OPTIONS|PERL5OPT|PYTHONSTARTUP)=/;

/** `x>file` is `x` plus a redirect `>file`; a bare fd prefix (`2>/dev/null`) stays one token. */
function splitRedirect(tok) {
  const m = /^(.+?)((?:\d*|&)>.*)$/s.exec(tok);
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
      if (api.fileField !== undefined) return "ghField";
      if (api.method === "OVERRIDE") return "ghApi";
      if (ghApiWrites(args.slice(1))) {
        if (ADMIN_API_RE.test(`/${api.endpoint ?? ""}`)) return "ghAdmin";
        if (!workflowRun(args.slice(1))) return "ghApi";
      } else if (api.input !== undefined) return "ghField";
    }
    if (a0 === "pr" && a1 === "merge") {
      if (args.includes("--admin")) return "ghMerge";
      if (!args.includes("--auto") && !args.includes("--disable-auto")) return "ghMerge";
    }
    if (a0 === "release" && ["create", "upload"].includes(a1)) return "publish";
    if (a0 === "release" && !["list", "view", "download", undefined].includes(a1)) return "ghAdmin";
    if (a0 === "workflow" && ["run", "enable", "disable"].includes(a1))
      return args.slice(2).some(releaseWorkflow) ? "publish" : null;
    if (a0 === "secret") return "ghAdmin";
    if (a0 === "repo" && ["edit", "delete", "archive", "rename", "transfer"].includes(a1))
      return "ghAdmin";
    if (a0 === "auth" && a1 === "token") return "secret";
  }
  if (prog === "git") {
    const { sub, args: rest, cfg } = gitSub(t);
    if (cfg.some((c) => /^core\.hookspath=/i.test(c))) return "protected";
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
      inner.slice(0, 4).some((x) => SHELLISH.includes(base(x))) ||
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
  return null;
}

const GUARDED_DIR_RE = /^(\.claude(\/hooks)?|\.bosun(\/.*)?|\.github\/workflows)\/?$/i;

/** `cd`/`git -C` into a guarded dir would turn later relative writes into unguarded names. */
function guardedDir(d, projectDir, cwd) {
  const x = stripTok(d);
  if (/[*?[{]/.test(x) && globHitsGuarded(x, projectDir, cwd)) return true;
  return [x, realRel(x, projectDir, cwd)].some(
    (r) => r && (GUARDED_DIR_RE.test(r) || isProtected(r) || LOCK_RE.test(r)),
  );
}

const GUARDED_DIRS = [".claude", ".claude/hooks", ".bosun", ".github/workflows"];

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
    guarded: opts.guarded,
    script,
  };
  const cmds = commands(text);
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
    const to = ["cd", "pushd"].includes(base(t[0])) && t.slice(1).find((d) => !d.startsWith("-"));
    if (to && !/[$`*?[{~]/.test(to)) here = dirOf(here, to);
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
  return out;
}

/** Add the literal dirs of a PATH value to the chain; false when a dir is not literal. */
function notePath(value, here, chain) {
  for (const e of String(value).split(":")) {
    if (/^\$\{?PATH\}?$/.test(e)) continue;
    const dir = e
      .replace(/^(~|\$\{?HOME\}?)(?=\/|$)/, homedir())
      .replace(/^(\$\{?PWD\}?|\$\(pwd\)|`pwd`)(?=\/|$)/, here);
    if (/[$`*?[{]/.test(dir)) return false;
    chain.path.push(resolve(here, dir || "."));
  }
  return true;
}

const MAX_SCRIPT = 512 * 1024;
const MAX_SCRIPTS = 32;
const SHELL_OPT_VALUE = new Set(["-o", "+o", "-O", "+O", "--rcfile", "--init-file"]);
const SHELL_C_RE = /^-[a-zA-Z]*c[a-zA-Z]*$/;

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
  let i = 0;
  while (i < ops.length && /^[-+]./.test(ops[i])) {
    if (ops[i] === "--") {
      i++;
      break;
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
    const src = shellSource(t);
    if (src.kind === "file") runs.push({ file: src.file, shell: true, args: src.args });
    if (src.kind === "stdin" && src.stdinFile)
      runs.push({ file: src.stdinFile, shell: true, args: [] });
  } else if (String(t[0]).includes("/") && !t[0].startsWith("@") && !INTERPRETERS.test(prog))
    runs.push({ file: t[0], shell: false, args: operands(t.slice(1)) });
  if (!String(t[0]).includes("/") && !t[0].startsWith("@"))
    for (const d of chain.path) {
      const abs = join(d, t[0]);
      if (existsSync(abs)) runs.push({ file: abs, shell: false, args: operands(t.slice(1)) });
    }
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

/** Magic numbers of executables the kernel runs itself (ELF, Mach-O, PE, wasm). */
const BINARY_RE =
  /^(\x7fELF|\xfe\xed\xfa[\xce\xcf]|[\xce\xcf]\xfa\xed\xfe|\xca\xfe\xba\xbe|MZ|\0asm)/;

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
 * a variable or glob, or (inside a script) missing at hook time. Binaries and non-shell `#!`
 * scripts run directly are not read. A script written earlier in the same command is refused. */
function runScript(t, projectDir, dirs, chain, seen) {
  for (const run of scriptRuns(t, chain)) {
    // `$HOME`, `$PWD` and `$CLAUDE_PROJECT_DIR` at the start are known; other variables are not.
    const name = stripTok(run.file)
      .replace(/^(~|\$\{?HOME\}?)(?=\/)/, homedir())
      .replace(/^\$\{?PWD\}?(?=\/)/, dirs[0])
      .replace(/^\$\{?CLAUDE_PROJECT_DIR\}?(?=\/)/, projectDir);
    const refuse = (why) => ({ rule: "unscanned", shown: `${name}: ${why}` });
    // A glob runs what it matches now (a case pattern matches nothing); `$`, backticks and braces
    // (`find -exec bash {}`) name a file the guard cannot know.
    const glob = /[*?[]/.test(name);
    if (/[$`{]/.test(name) || (glob && typeof nodeFs.globSync !== "function"))
      return refuse("a path the guard cannot read");
    const paths = dirs.flatMap((d) =>
      glob ? nodeFs.globSync(name, { cwd: d }).map((m) => resolve(d, m)) : [resolve(d, name)],
    );
    let found = glob;
    for (const abs of paths) {
      if (chain.tainted.has(abs))
        return { rule: "evasion", shown: `${name}: written by this command` };
      let st;
      try {
        st = statSync(abs);
      } catch (e) {
        if (e?.code === "ENOENT" || e?.code === "ENOTDIR") continue;
        return refuse("unreadable");
      }
      found = true;
      if (!st.isFile()) continue; // a directory: the shell fails
      const key = `${abs}\0${run.args.join("\0")}`;
      if (seen.has(key)) continue;
      let text;
      try {
        if (!run.shell) {
          const top = head(abs);
          if (BINARY_RE.test(top)) continue; // a binary the kernel runs
          const interp = shebang(top);
          if (interp !== null && !SHELLS.has(interp)) continue; // another language: a known gap
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
        script: true,
        scriptDir: dirname(abs),
        argv: run.args,
      });
      if (hit) return { rule: hit.rule, shown: `${name}: ${hit.shown}` };
    }
    // A script that runs a file missing now could run whatever is written there later.
    if (!found && chain.script) return refuse("missing when the guard ran");
  }
  return null;
}

/**
 * @param {{ tool_name: string, tool_input: Record<string, unknown> }} input
 * @param {{ lockText?: string, projectDir: string }} ctx
 * @returns {null | { decision: "deny" | "ask", reason: string }}
 */
export function decide(input, ctx) {
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
