# keelstackhq/decisions

Settled decisions that apply across keelstackhq repos, kept small enough for every agent to load
at session start.

- **Read:** [`DECISIONS.md`](DECISIONS.md), one row per decision (at most 60 lines), each linking
  its reasoning in [`decisions/`](decisions/).
- **Use in a repo (also every new repo):** `splice channel add gh:keelstackhq/decisions//channel
--track main --piece import-project` (`--piece import-claude` without
  `docs/agents/project.md`); see D-004. `adopt/adopt.sh` is deprecated.
- **Change:** discuss an open topic in a shared doc, issue or PR in the repos concerned; once it is
  settled, open a PR here adding `decisions/NNN-title.md` (from `decisions/000-template.md`) and
  one index row, and copy `DECISIONS.md` to
  `channel/pieces/snapshot/files/docs/agents/org-decisions.md`. `bash scripts/check-index.sh` must pass; the owner merges.
