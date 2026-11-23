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

| Need                       | Use                                                                                                                  |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Tests, globs, regex        | `[ -f x ] && ...`, `grep -E 'a[bc]{2,}' f`, `ls *.md`, `"$(git rev-parse HEAD)"`                                     |
| A script                   | `bash scripts/x.sh` or `./scripts/x.sh`: the guard reads it like typed commands                                      |
| Start or re-run a workflow | `gh workflow run ci.yml --ref <branch>`, `gh run rerun <run-id> --failed` (or their `gh api` POST, values with `-f`) |

A script does not hide a command: the guard reads a shell script before it runs (also `bash < f`,
scripts it runs, with `$1`/`$@` bound to the literal arguments) and refuses it when a line in it
would be refused; it also refuses a script the same command (or a parent script) just wrote.
Release workflows (file names with `release`, `publish` or `deploy`; the human can set the
`GUARD_RELEASE_WORKFLOWS` regex in `.claude/settings.json` `env`) never start from here.

The guard refuses a script it cannot read in full; run a literal path to a text script instead:
larger than 512 KiB, NUL bytes, unreadable, missing when a script runs it, more than 32 scripts,
a path from a variable or `{}` (`bash "$F"`, `find -exec bash {} +`, `xargs bash`), a shell fed
by a pipe, stdin or an expanding here-string or heredoc (`cat f | bash`, `bash <<< "$(cat f)"`),
or a PATH dir that is not literal. `alias x=...` is refused: run the command. `gh api -F x=@file`
and `--input` read a file into the request: pass values with `-f`.

After `cd` into `.claude/`, `.claude/hooks/`, `.bosun/` or `.github/workflows/`, the rest of the
command may only read (`ls`, `cat`, `grep`, `head`, `git log`, ..., redirects to `/dev/null`
only; `x>file` counts as a write, and so do `GIT_*=`/`PAGER=` prefixes). Write from the repo root
with the full path instead, so the guard sees the target.

## Rails that stay

- No push to the default branch, no force push, no tag push, no `npm|pnpm|cargo publish`, no
  `gh release`, no `gh pr merge` without `--auto`: open a PR and run `gh pr merge --auto`.
- No reads of secret files (`.env*`, keys, `~/.ssh`, `~/.config/gh`) or whole-environment dumps.
- No edits of `.claude/hooks/`, `.claude/settings*.json`, release workflows, `.bosun/`,
  `pins.toml` (release workflows install its versions), `.splice/lock.toml`; template-owned files ask first (change them with `splice update`).
- No `gh api` writes to repo settings (rulesets, environments, secrets): those are owner steps.
- A target the guard cannot read (`cd "$D"`, `eval`) is refused when it could hide one of the
  above; a pipe into `sh` is always refused: spell the command out.

## Gaps that stay open

A rail, not a sandbox. Not read: scripts in other languages and binaries; `$VAR` or `"$@"` in
command position (only checked for risky words); a glob script path that matches nothing at hook
time but a file made later; a `cd` to a variable inside a script; edits of guarded files from an
interpreter. A re-run by id (`gh run rerun <id>`, `actions/runs/<id>/rerun`, `jobs/<id>/rerun`)
cannot tell a release run from another: release workflows must stay environment-gated with a
required human reviewer, so a re-run waits for that approval.
