# keelstackhq/decisions

Settled decisions that apply across keelstackhq repos, kept small enough for every agent to load
at session start.

- **Read:** [`DECISIONS.md`](DECISIONS.md), one row per decision (at most 60 lines), each linking
  its reasoning in [`decisions/`](decisions/).
- **Use in a repo (also every new repo):** `bash ../decisions/adopt/adopt.sh .` from the repo, then
  commit; see [`adopt/README.md`](adopt/README.md). It adds a committed snapshot imported by
  `CLAUDE.md` and a session-start hook that refreshes it.
- **Change:** discuss an open topic in a shared doc, issue or PR in the repos concerned; once it is
  settled, open a PR here adding `decisions/NNN-title.md` (from `decisions/000-template.md`) and
  one index row. `bash scripts/check-index.sh` must pass; the owner merges.
