# Move a repo to a new keelstack release

Run this in the repo, as a human or as an agent. The repo does not need to be part of an orchestrator session.

```bash
npx -y @keelstackhq/keel@<version> migrate --pr
```

`-y` skips npx's interactive "Ok to proceed?" prompt (needed for agents and scripts). An npm warning about an unknown user config comes from your `~/.npmrc`, not from keel, and is harmless.

`keel migrate [--to X.Y.Z] [--json] [--dry-run] [--pr] [--base REF] [--mode MODE]` (default target: the release of the keel you run).
It skips every piece the repo does not use and does, in order, one commit each, on the branch `keel/migrate-vX.Y.Z`:

1. **tools**: installs splice, bosun and hoist at the target version (checksum verified, never an unverified tarball).
2. **splice**: `splice update --channel decisions` alone (`chore: sync org decisions`), then `splice update --all` (hull template, rigging and every other channel).
3. **bosun-pin**: `bosun pin --to <target>` (exactly that published release; `.bosun/version` and the cloud setup script `docs/setup-script.sh`). Skipped (not an error) when the repo has no `.bosun/`, has the bosun channel (`splice update` already moved it), or has no `docs/setup-script.sh`.
4. **hoist**: `hoist init` refreshes an unedited `release.yml`. A repo with nothing to publish (hoist's error kind `nothing-to-publish`) is `skipped: nothing to publish`, with no checklist entry.
5. **setup-snippet**: compares the cloud-environment bootstrap of the old and the new bosun.
6. **verify** (read-only): `splice check`, `bosun doctor`, `hoist check`. keel runs `splice check` on the base before any step: drift already there (edited or deleted owned files, conflict markers) does not fail the migration; only drift a step introduced does (rolled back). Existing drift is `pending` plus the checklist entry `splice-drift`. Real `splice update` refuses owned files with local edits when a source has a newer version, writing nothing; keel then updates every other source on its own (`splice update`, `splice update --channel NAME`), one commit each (splice refuses a dirty tree and keel never passes `--allow-dirty`), marks the refused ones **not updated** (step `splice` is `pending`), and goes on with bosun-pin, hoist and the snippet. A refusal for any path that was not drifted on the base fails and rolls back. Section blocks (`HOST#NAME`) belong to channel `NAME`. The run then ends `human` (exit 5); a PR is opened only if there are commits, and its body says the migration is partial.
7. **pr** (`--pr`): pushes the branch and opens ONE PR. keel never merges.

## For an agent

`keel migrate --json --pr` is enough. Read the one JSON object: `status` (`done`, `human`, `failed`, `planned`),
`steps[]` (`id`, `action`, `detail`), `checklist[]` (what only a human can do, `device` `computer` or `phone`, computer
first) and, on `failed`, `recovery`. Exit codes: 0 done or nothing to do, 1 a step failed (rolled back) or, with
`--dry-run`, work remains, 2 usage, 3 environment (see Safety), 5 a human step is pending.

Typical answer after a real run: exit 5 with the checklist `approve-pr` (phone). Hand that to the owner and stop;
approvals and merges follow the org's rules, keel does neither. When nothing is left to merge (no commits) but a human step remains (`splice-drift`, `setup-snippet`), the answer is `status: "human"`, exit 5, `changed: false`, `pr: null` and no `approve-pr`/`open-pr`: do the checklist, no PR exists.

## What a human may still have to do

| Checklist id | When | What |
|---|---|---|
| `approve-pr` | always after `--pr` | approve the PR (the owner's chat approval, D-018) and merge it by the repo's rules |
| `open-pr` | run without `--pr` | `keel migrate --pr` pushes the branch and opens the PR |
| `setup-snippet` | only when the bootstrap text changed (or could not be compared) | paste the file named in the entry (`<git-dir>/keel/cloud-setup.sh`) into claude.ai/code > Environments > Setup script. The entry lists the line counts and the previous file to `diff` against. It holds no secret. When it did not change, never paste again: each checkout's own `docs/setup-script.sh` carries the new pins |
| `hoist-apply` | hoist reports drift (for example the `release-tags` environment or `[release] reviewers`) | `hoist apply --area github` with an admin token, on a computer |
| `splice-drift` | `splice check` already reported drift on the base (not caused by the migration); the entry says which sources were NOT updated because of it, resolve then rerun `keel migrate` | per path (the entry names its class, source and cause): keep the local version: channel file `splice update --channel NAME --exclude PATH` (replaces the channel's stored filters), main-template file: move the local notes into the seed file the template provides (e.g. `<target>/CLAUDE.md`), then `splice update --force`; take the template's version: `splice update --channel NAME --force` or `splice update --force`; a section block `HOST#NAME` (splice has no command that restores a block of an up-to-date channel): revert your edit between the markers (`git diff <base> -- HOST`); `--force` rewrites it only when the channel has a newer version; to keep other text, change the channel's source. (`--force` does restore edited owned files of an up-to-date main or channel; `--exclude PATH` keeps them.) The JSON `notes[]` says when the baseline check itself could not run. `keel migrate` again afterwards, or `splice check` |
| `bosun-doctor`, `hoist-check` | a check could not run or found work | the command in the entry |
| `hoist-init` | `hoist init` failed for a reason other than "nothing to publish" (for example no GitHub token) | hoist's own message and fix are in the title; no command is suggested because rerunning `hoist init` fails the same way until that is fixed |
| `bosun-mode` | `--mode MODE` | after the PR merges: `bosun migrate --to MODE` (its own gated PR) |

## How it works: your checkout is never touched

keel does all the work in its own temporary git worktree, created from the remote default branch:

- It runs `git fetch origin <default>` (this must succeed: a stale base would migrate old files; exit 3 with the fix, or pass `--base REF` to use a ref you name) and adds a worktree under `<git-dir>/keel/migrate-vX.Y.Z` on branch `keel/migrate-vX.Y.Z` based on `origin/<default>`.
- Every tool (`splice`, `bosun`, `hoist`) runs in that worktree, the commits are made there and `--pr` pushes from there. The worktree is always removed at the end, on success and on failure.
- Your checkout, your current branch, the index, the stash, your uncommitted, untracked and ignored files (`.env`, `node_modules/`), and your unpushed commits are never read as input or changed. A dirty tree is fine. Unpushed local commits are not part of the migration; the JSON says how many (`excluded_commits`) and the PR body mentions it.
- The cloud setup snippet files are written to `<git-dir>/keel/` (`cloud-setup.sh`, `cloud-setup.previous.sh`), not into your checkout; the JSON `setup_snippet.path` is the absolute path.
- A step that rewrites `.gitignore` so that a file the repo ignores (as of the base) would be committed fails (the files named, nothing committed) and the run is rolled back.

## Safety

- Exit 3 means keel changed nothing and tells you the fix: not a git checkout; no `origin` default branch could be found or fetched (fix the remote or pass `--base REF`); `--base REF` does not exist; you are on `keel/migrate-vX.Y.Z` itself (switch to another branch first); that branch exists, is not merged and is behind the base (`git branch -D` it or rebase it); a branch whose PR merged has commits beyond the PR head (`git log`, then `git branch -D`); `gh` missing or logged out with `--pr`; no splice, bosun or hoist piece on the base.
- A branch left by an earlier migration is replaced silently when it is merged into the base, has the same tree as the base (a squash merge), or its PR is merged on GitHub with the branch at the PR's head. Any other existing migration branch is resumed (re-run with `--pr` to push it).
- Any failed step before the PR resets the migration branch to where the run started (the branch is deleted when this run created it) and removes the worktree; the JSON `recovery` says exactly what was undone and what to run next. A repo that is already current changes nothing and exits 0.
- `--dry-run` installs nothing, commits nothing, creates no branch and pushes nothing (it adds and removes a temporary detached worktree and fetches the base).
- `--pr` opens the PR against the repo's default branch (`gh pr create --base`); nothing to migrate means no PR and exit 0, whatever your branch is ahead by.
- A tool that is not at the target is installed with keel's embedded sha512 when keel has them for that version. Otherwise keel takes the integrity from `npm view`, which is the registry vouching for itself: it catches a corrupted download, not a compromised registry. Prefer a keel release whose embedded pins cover the target.
- `--mode`: if `bosun migrate --to MODE --dry-run` fails, the migration stays and the checklist says to run it by hand.
- keel never merges, tags, releases, publishes or force-pushes.

More: `keel/README.md`, spec section 11 (`keel/docs/superpowers/specs/2026-11-20-keel-design.md`).
