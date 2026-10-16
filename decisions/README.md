# keelstackhq/decisions

Settled decisions that apply across keelstackhq repos, kept small enough for every agent to load
at session start.

- **Read:** [`DECISIONS.md`](DECISIONS.md), one row per decision (at most 60 lines), each linking
  its reasoning in [`decisions/`](decisions/).
- **Use in a repo:** follow [`adopt/README.md`](adopt/README.md): a committed snapshot imported by
  `CLAUDE.md`, refreshed by a session-start hook.
- **Change:** discuss an open topic in a shared doc, issue or PR in the repos concerned; once it is
  settled, open a PR here adding `decisions/NNN-title.md` (from `decisions/000-template.md`) and
  one index row. `bash scripts/check-index.sh` must pass; the owner merges.
