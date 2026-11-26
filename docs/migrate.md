# Move a repo to a new keelstack release

Run this in the repo, as a human or as an agent. The repo does not need to be part of an orchestrator session.

```bash
npx @keelstackhq/keel migrate --pr
```

`keel migrate [--to X.Y.Z] [--json] [--dry-run] [--pr] [--mode MODE]` (default target: the release of the keel you run).
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
`--dry-run`, work remains, 2 usage, 3 environment (dirty tree, not a git checkout, no keelstack piece found), 5 a human step is pending.

Typical answer after a real run: exit 5 with the checklist `approve-pr` (phone). Hand that to the owner and stop;
approvals and merges follow the org's rules, keel does neither.

## What a human may still have to do

| Checklist id | When | What |
|---|---|---|
| `approve-pr` | always after `--pr` | approve the PR (the owner's chat approval, D-018) and merge it by the repo's rules |
| `open-pr` | run without `--pr` | `keel migrate --pr` pushes the branch and opens the PR |
| `setup-snippet` | only when the bootstrap text changed (or could not be compared) | paste `.keel/cloud-setup.sh` into claude.ai/code > Environments > Setup script. It lists the line counts and `.keel/cloud-setup.previous.sh` to `diff` against. It holds no secret. When it did not change, never paste again: each checkout's own `docs/setup-script.sh` carries the new pins |
| `hoist-apply` | hoist reports drift (for example the `release-tags` environment or `[release] reviewers`) | `hoist apply --area github` with an admin token, on a computer |
| `bosun-doctor`, `hoist-check`, `hoist-init` | a check could not run or found work | the command in the entry |
| `bosun-mode` | `--mode MODE` | after the PR merges: `bosun migrate --to MODE` (its own gated PR) |

## Safety

- Exit 3 means keel changed nothing and tells you the fix: not a git checkout; `-C` is not the repo root (the check and the rollback cover the whole repo); HEAD detached or no commit; a dirty tree, tracked or untracked (checked with `--untracked-files=all` over the whole repo, whatever `status.showUntrackedFiles` says: git is the undo button, `git stash -u`); the branch `keel/migrate-vX.Y.Z` already exists and is neither merged, a squash merge of the same tree, nor the branch of a merged PR (`git switch` it to resume, or `git branch -D` it); `gh` missing or logged out with `--pr`; no splice, bosun or hoist piece found.
- Files git ignores here (`.env`, `node_modules/`) are recorded before the first step and are never committed, pushed or deleted. A step that would un-ignore one (a template that rewrites `.gitignore`) fails with the paths named, unstages them and rolls back; keep them ignored or move them out of the repo, then rerun.
- A branch left by an earlier migration is replaced silently Exception: a branch with commits beyond its merged PR's head is never deleted; exit 3 with the `git log` and `git branch -D` advice. when it is merged into HEAD, has the same tree as HEAD (a squash merge), or its PR is merged on GitHub.
- Any failed step before the PR rolls back to the untouched starting branch and deletes the migration branch. The rollback never runs `git clean`: it deletes only the untracked files that appeared during the run (none existed before, the preflight proved it), and resets only keel's own branch; the JSON
  `recovery` says exactly what was undone and what to run next. A repo that is already current changes nothing and exits 0.
- `--dry-run` installs nothing, writes nothing, creates no branch and pushes nothing.
- `--pr` opens the PR against the repo's default branch (`--base`); a checked-out branch that is merely ahead of origin, with nothing to migrate, gets no PR and exit 0.
- A tool that is not at the target is installed with keel's embedded sha512 when keel has them for that version. Otherwise keel takes the integrity from `npm view`, which is the registry vouching for itself: it catches a corrupted download, not a compromised registry. Prefer a keel release whose embedded pins cover the target.
- `--mode`: if `bosun migrate --to MODE --dry-run` fails, the migration stays and the checklist says to run it by hand.
- keel never merges, tags, releases, publishes or force-pushes.

More: `keel/README.md`, spec section 11 (`keel/docs/superpowers/specs/2026-11-20-keel-design.md`).
