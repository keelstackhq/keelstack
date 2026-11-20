# decisions/

Settled decisions that apply across keelstackhq repos, in the `keelstackhq/keelstack` monorepo.
Agents here read `DECISIONS.md` in place; the splice channel `gh:keelstackhq/keelstack//decisions/channel`
(D-004) is only for repos outside the monorepo. Keep the index short and exact.

## Rules

- `DECISIONS.md` stays at or under 60 lines: one table row per decision, newest last.
- Every row links `decisions/NNN-title.md` (copy `decisions/000-template.md`); IDs are never reused.
- Superseding: add the new decision, mark the old file `Status: superseded by D-NNN`, drop its row.
- Record only settled outcomes. Open questions belong in shared docs, issues or PRs elsewhere.
- Changes go through monorepo PRs touching `decisions/` (D-002); merging follows D-022 (orchestrator
  merges on its quorum, trust-root items need the owner). Never push to `main`.
- Keep the table aligned (`npx prettier --write decisions/DECISIONS.md`).
- Scope `all orgs` marks a decision that also applies outside keelstackhq (it goes into the
  `portable` channel piece); only when the row names nothing keelstackhq-specific.
- The channel snapshot `channel/pieces/snapshot/files/docs/agents/org-decisions.md.jinja` is
  generated: `bash decisions/scripts/gen-snapshot.sh` (check-index.sh does); commit it.
- Before a PR, from the repo root: `bash decisions/scripts/check-index.sh` must pass.
- Write a decision so an agent can follow it without context: name the folders, files, commands.
