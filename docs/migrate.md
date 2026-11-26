# Move a repo to a new keelstack release

Run this in the repo, as a human or as an agent. The repo does not need to be part of an orchestrator session.

```bash
npx @keelstackhq/keel migrate --pr
```

`keel migrate [--to X.Y.Z] [--json] [--dry-run] [--pr] [--base REF] [--mode MODE]` (default target: the release of the keel you run).
It skips every piece the repo does not use and does, in order, one commit each, on the branch `keel/migrate-vX.Y.Z`:

1. **tools**: installs splice, bosun and hoist at the target version (checksum verified, never an unverified tarball).
2. **splice**: `splice update --channel decisions` alone (`chore: sync org decisions`), then `splice update --all` (hull template, rigging and every other channel).
3. **bosun-pin**: `bosun pin --to <target>` (exactly that published release; `.bosun/version` and the cloud setup script `docs/setup-script.sh`). Skipped (not an error) when the repo has no `.bosun/`, has the bosun channel (`splice update` already moved it), or has no `docs/setup-script.sh`.
4. **hoist**: `hoist init` refreshes an unedited `release.yml`.
5. **setup-snippet**: compares the cloud-environment bootstrap of the old and the new bosun.
6. **verify** (read-only): `splice check`, `bosun doctor`, `hoist check`.
7. **pr** (`--pr`): pushes the branch and opens ONE PR. keel never merges.

## For an agent

`keel migrate --json --pr` is enough. Read the one JSON object: `status` (`done`, `human`, `failed`, `planned`),
`steps[]` (`id`, `action`, `detail`), `checklist[]` (what only a human can do, `device` `computer` or `phone`, computer
first) and, on `failed`, `recovery`. Exit codes: 0 done or nothing to do, 1 a step failed (rolled back) or, with
`--dry-run`, work remains, 2 usage, 3 environment (see Safety), 5 a human step is pending.

Typical answer after a real run: exit 5 with the checklist `approve-pr` (phone). Hand that to the owner and stop;
approvals and merges follow the org's rules, keel does neither.

## What a human may still have to do

| Checklist id | When | What |
|---|---|---|
| `approve-pr` | always after `--pr` | approve the PR (the owner's chat approval, D-018) and merge it by the repo's rules |
| `open-pr` | run without `--pr` | `keel migrate --pr` pushes the branch and opens the PR |
| `setup-snippet` | only when the bootstrap text changed (or could not be compared) | paste the file named in the entry (`<git-dir>/keel/cloud-setup.sh`) into claude.ai/code > Environments > Setup script. The entry lists the line counts and the previous file to `diff` against. It holds no secret. When it did not change, never paste again: each checkout's own `docs/setup-script.sh` carries the new pins |
| `hoist-apply` | hoist reports drift (for example the `release-tags` environment or `[release] reviewers`) | `hoist apply --area github` with an admin token, on a computer |
| `bosun-doctor`, `hoist-check`, `hoist-init` | a check could not run or found work | the command in the entry |
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
