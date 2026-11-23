# Guard hook: safe patterns

`.claude/hooks/guard.mjs` checks every Bash, Read, Grep, Glob and edit call. It is a rail, not a
sandbox: branch rulesets and environment-gated releases are the real protection. A deny names its
rule and the allowed alternative. Use the patterns below; they pass.

## Reads that pass

| Need                      | Use                                                                                     |
| ------------------------- | --------------------------------------------------------------------------------------- |
| GitHub API read           | `gh api repos/<o>/<r>/pulls`, `gh api -X GET ...`, `curl -s https://api.github.com/...` |
| Remote refs               | `git ls-remote origin \| grep refs/tags`, `git ls-remote --tags <url>`                  |
| Fetch, then look          | `git fetch origin main && git log --oneline origin/main -3`                             |
| Look inside a guarded dir | `cd .bosun && ls && cat bosun.toml` (only reads after the `cd`)                         |
| Globs                     | `ls -d .*`, `cat .bosun/*.toml`, `grep -n push .github/workflows/*.yml`                 |
| Variable names of secrets | `cat .env.example`; run with values: `hoist secrets exec -- <cmd>`                      |

## Runs that pass

| Need                        | Use                                                                                                                                                                                                                |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Tests, globs, regex         | `[ -f x ] && ...`, `grep -E 'a[bc]{2,}' f`, `ls *.md`, `"$(git rev-parse HEAD)"`                                                                                                                                   |
| A script                    | `bash scripts/x.sh` or `./scripts/x.sh`: the guard reads it like typed commands                                                                                                                                    |
| Check script syntax         | `bash -n scripts/x.sh`, `for f in scripts/*.sh; do bash -n "$f"; done` (`-n` only parses)                                                                                                                          |
| A script another step makes | run the step (`curl -o`, `tar x`, `pnpm build`, ...) first, then the script in a separate Bash call                                                                                                                |
| Push the current branch     | `git push -u origin HEAD` or the literal name (`git push -u origin my-feature`), never `"$B"`                                                                                                                      |
| Start or re-run a workflow  | `gh workflow run ci.yml --ref <branch>`, `gh run rerun <run-id> --failed` (or their `gh api` POST, values with `-f`)                                                                                               |
| Open a PR                   | `gh pr create --title t --body-file f`, or (cloud: `gh api` only) `gh api -X POST repos/<o>/<r>/pulls -f title=t -f head=<branch> -f base=main -F body=@f` (every word literal; `f` a non-secret file in the repo) |

A script does not hide a command: the guard reads a shell script before it runs (any shell by
name: `bash`, `sh`, `rbash`, `mksh`, `busybox sh`, ...; also `bash < f`, `< f bash`, scripts it
runs, with `$1`/`$@` bound to the literal arguments, shell functions, `trap` and `coproc` bodies,
wrappers such as `sudo`, `timeout`, `flock`, `nice`, `su -c`, `script -c`, runners such as
`git rebase --exec`, `git bisect run`, `git submodule foreach`, `npm exec`, `at`, `expect -c`,
`nix-shell --run`, `nix develop -c`, and startup files:
`BASH_ENV`, `ENV`, `--rcfile`) and refuses it when a line in it would be refused. A script that
does not exist when the guard runs is refused (a shell would fail on it anyway), and so is any
script run after an earlier command in the same call that may write files the guard cannot name
(downloads, archives, decoders, patches, builds, package managers, interpreters); an existing
host binary (a valid ELF header on Linux, Mach-O on macOS) run directly after a build tool
(`cargo build && ./target/debug/x`) passes. A file with a fake magic number is a shell script
(the kernel refuses it and sh runs it) and is read like one. A binary is not read, so it passes
only when run directly from the typed command (bash refuses a binary the kernel refuses): through
a wrapper or runner (`env`, `timeout`, `nice`, `nohup`, `sudo`, `xargs`, `find -exec`, `sh -c`,
`git bisect run`, ...) or from a script, /bin/sh may run it as a script, so it is refused.
`bash -n <file>` and `sh -n <file>` (one literal file, no other option) only parse and pass; any
other `-n` form reads the script. `find -exec {}`, `xargs env` and `xargs -I{} {}` (no literal
program) are refused.
Release workflows (file names with `release`, `publish` or `deploy`; the human can set the
`GUARD_RELEASE_WORKFLOWS` regex in `.claude/settings.json` `env`) never start from here.

The guard refuses a script it cannot read in full; run a literal path to a text script instead:
larger than 512 KiB, NUL bytes, unreadable, more than 32 scripts,
a path from a variable or `{}` (`bash "$F"`, `find -exec bash {} +`, `xargs bash`), a shell fed
by a pipe, stdin or an expanding here-string or heredoc (`cat f | bash`, `bash <<< "$(cat f)"`),
a `#!` interpreter it does not know, or a PATH dir that is not literal (`$HOME`, `$PWD`, `$(pwd)`,
`$(npm bin)` are known; with a venv, run `.venv/bin/<tool>` instead of `source
.venv/bin/activate`). `alias x=...` is refused: run the command. `source ~/.bashrc` is refused
when the file holds code the guard cannot read: the Bash tool already starts from your shell, so
drop it. `gh api -F x=@file`, `gh workflow run -F x=@file` and `--input` read a file into the
request: pass values with `-f`; a token-like variable (`$*TOKEN*`, `$*SECRET*`, `$*KEY*`) as a value
is refused. A wildcard refspec that can match the default branch (`refs/heads/*`) is refused.
`gh api` flags are read like gh (`-iX PUT` is `-i -X PUT`); a flag gh api does not have is refused.
A method-override header (`-H 'X-HTTP-Method-Override: ...'`) is refused, whatever `-X` says.

After `cd` into `.claude/`, `.claude/hooks/`, `.bosun/` or `.github/workflows/`, the rest of the
command may only read (`ls`, `cat`, `grep`, `head`, `git log`, ..., redirects to `/dev/null`
only; `x>file` counts as a write, and so do `GIT_*=`/`PAGER=` prefixes). Write from the repo root
with the full path instead, so the guard sees the target.

## Rails that stay

- No push to the default branch, no force push, no tag push, no `npm|pnpm|cargo publish`, no
  `gh release`, no `gh pr merge` without `--auto`: open a PR and run `gh pr merge --auto`.
- No reads of secret files (`.env*`, keys, `~/.ssh`, `~/.config/gh`) or whole-environment dumps.
- No edits of `.claude/hooks/`, `.claude/settings*.json`, release workflows, `.bosun/`,
  `.git/hooks/`, `.git/config`,
  `pins.toml` (release workflows install its versions), `.splice/lock.toml`; template-owned files ask first (change them with `splice update`).
- No `gh api` writes but opening a PR (`POST repos/<o>/<r>/pulls`) and starting or re-running a
  workflow: no merge, auto-merge, review, tag, release or repo settings (owner steps).
- A target the guard cannot read (`cd "$D"`, `eval`) is refused when it could hide one of the
  above; a pipe into `sh` is always refused: spell the command out.
- A push, publish, merge, tag, release or secrets command with a word the shell changes when it
  runs is refused: `$(...)`, backticks, `${...}`, `$'...'`, `$"..."`, a brace expansion
  (`HEAD:{main,x}`) or a line continuation inside a word, a `gh` title or body value too. Write
  the word plainly. `gh workflow run <id>` is refused: name the workflow file.
- Opening a PR or issue (`gh pr|issue create`, `gh api` POST `repos/<o>/<r>/pulls`) and a
  `gh api` workflow dispatch or re-run pass only with every word literal: no `$VAR`, `"$@"`,
  `${...}`, `$(...)` or backticks anywhere, no `GH_*`, `GITHUB_*` or `*TOKEN*` variable set
  anywhere in the command (prefix, `export`, `declare`), no `cd`/`pushd` before it, no
  `--hostname` or `-R`/`--repo`. Put the body in a regular file in the repo, named from the repo
  root: `--body-file pr.md` or `-F body=@pr.md` (no `~`, `/`, `..`, stdin, dotfile, symlink or
  credential/config name; not written by the same command).
  A `$(...)` runs after the `cd` and `PATH=` before it (`cd d && x=$(./s.sh)` reads `d/s.sh`).

## Gaps that stay open

A rail, not a sandbox. These stay open:

- A token copied into another variable first (`t=$GITHUB_TOKEN; gh workflow run w.yml -f x=$t`):
  only token-like names in the input value itself are refused.
- Code in other languages (Python, Node, Ruby, Perl, PHP, awk, make files, `#!` scripts in those
  languages) and binaries: only checked for secret names and exec text, not run like a shell.
- Programs that run other programs from their own config: `npm run`/`pnpm` scripts, `make`
  targets, `cargo` build scripts, git hooks of a cloned repo, `.envrc`, editors.
- `$VAR` or `"$@"` in command position: only checked for risky words (its `-c` code and the shell
  scripts it is given are read). `eval "$(tool)"` passes only for env-setup tools (`ssh-agent`,
  `fnm`, `rbenv`, ...); other `eval`, `bash -c "$X"` and `$(...)` as the program are refused.
- Hook files a hooks manager already points git at (`.husky/`, lefthook config): `git commit` runs them.
- Wrappers the guard does not know, given a command as separate words (`foo git push ...` is
  refused only when its text names push, publish or release next to git, gh, npm, ...).
- A `cd` to a variable inside a script, and a dir from a variable in a flag value (`tar -C "$D"`).
- `find <dir> -delete` (or `-exec <writer>`) is refused when `<dir>` is guarded, holds a guarded
  root path (`.`, `.claude`, `.github`, `.git`) or holds the repo; a guarded dir nested deeper
  (`find apps -delete` over `apps/x/.claude/`) is not found.
- Edits of guarded files from an interpreter (`python -c "open('.claude/...','w')"`).
- A file written in an earlier Bash call is read when a later call runs it, as it is at that
  moment: the guard judges each call alone and reads every script that call runs. A binary
  written earlier is not read; it passes only when run directly, never through a wrapper.
- A default branch that `origin/HEAD` does not name (besides `main` and `master`).
- A script after a writer the guard cannot name in the same call (`pnpm install && bash
  scripts/gen.sh`, `git pull && bash s.sh`, `git checkout main && bash s.sh`) stays refused, even
  when it is tracked and unchanged: install scripts, build scripts, the pulled or checked-out
  commit and git hooks can rewrite it before it runs. Run it in a separate Bash call. A host
  binary run directly is allowed only after a build tool (`cargo build && ./target/debug/x`).
- `expect` Tcl beyond `spawn`/`exec`/`system` words and `expect` script files; `at` fed by a pipe
  (refused); `npm exec` with an option the guard does not know and no `--` (refused: add `--`).
- PATH entries from variables: `$X` counts only when this text assigns X a literal (or `$PATH`)
  and changes it no other way (no `${X:=w}`, nameref, `((`, `let`, `source` or `eval` in the
  text); `${X:-w}` of an unassigned X uses the guard's own environment. System dirs (`/bin`,
  `/usr/bin`, `/usr/local/bin`, ...) are on PATH already and are not tracked.
- A re-run by id (`gh run rerun <id>`, `actions/runs/<id>/rerun`, `jobs/<id>/rerun`) cannot tell a
  release run from another: release workflows must stay environment-gated with a required human
  reviewer, so a re-run waits for that approval.
