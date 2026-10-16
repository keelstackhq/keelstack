# keelstackhq/decisions

The record of settled decisions that apply across keelstackhq repos. Agents in every repo load
`DECISIONS.md` through a committed snapshot (see `adopt/`), so keep it short and exact.

## Rules

- `DECISIONS.md` stays at or under 60 lines: one table row per decision, newest decisions last.
- Every row links `decisions/NNN-title.md` (copy `decisions/000-template.md`); IDs are never reused.
- Superseding: add the new decision, mark the old file `Status: superseded by D-NNN`, remove the
  old row from the index.
- Record only settled outcomes. Open questions belong in shared docs, issues or PRs elsewhere.
- Changes go through PRs; the owner merges. Never push to `main`.
- Before a PR: `bash scripts/check-index.sh` must pass.
- A decision is written so an agent can follow it without context: name the repos, files and
  commands it affects.
