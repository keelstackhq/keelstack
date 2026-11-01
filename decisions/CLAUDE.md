# keelstackhq/decisions

The record of settled decisions that apply across keelstackhq repos. Agents in every repo load
`DECISIONS.md` through the splice channel in `channel/` (D-004), so keep it short and exact.

## Rules

- `DECISIONS.md` stays at or under 60 lines: one table row per decision, newest decisions last.
- Every row links `decisions/NNN-title.md` (copy `decisions/000-template.md`); IDs are never reused.
- Superseding: add the new decision, mark the old file `Status: superseded by D-NNN`, remove the
  old row from the index.
- Record only settled outcomes. Open questions belong in shared docs, issues or PRs elsewhere.
- Changes go through PRs; the owner merges. Never push to `main`.
- Keep the table aligned (`npx prettier --write DECISIONS.md`): repos format-check their snapshot.
- Scope `all orgs` marks a decision that also applies to repos outside keelstackhq; anything else
  (`all` = every keelstackhq repo, or a repo list) stays out of the `portable` channel piece.
  Mark `all orgs` only when the row names nothing keelstackhq-specific.
- The channel snapshot `channel/pieces/snapshot/files/docs/agents/org-decisions.md.jinja` is
  generated: run `bash scripts/gen-snapshot.sh` (check-index.sh does) and commit it.
- Before a PR: `bash scripts/check-index.sh` must pass.
- A decision is written so an agent can follow it without context: name the repos, files and
  commands it affects.
