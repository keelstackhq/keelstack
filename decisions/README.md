# keelstackhq/decisions

Settled decisions that apply across keelstackhq repos, kept small enough for every agent to load
at session start.

- **Read:** [`DECISIONS.md`](DECISIONS.md), one row per decision (at most 60 lines), each linking
  its reasoning in [`decisions/`](decisions/).
- **Use in a repo (also every new repo):** `splice channel add gh:keelstackhq/decisions//channel
--track main --piece import-project` (`--piece import-claude` without
  `docs/agents/project.md`); see D-004.
- **Change:** discuss an open topic in a shared doc, issue or PR in the repos concerned; once it is
  settled, open a PR here adding `decisions/NNN-title.md` (from `decisions/000-template.md`) and
  one index row, and run `bash scripts/gen-snapshot.sh` (it writes
  `channel/pieces/snapshot/files/docs/agents/org-decisions.md.jinja`). `bash scripts/check-index.sh`
  must pass; the owner merges.
- **A repo in another org:** add `--piece portable` to take only the decisions scoped `all orgs`
  (the rest name keelstackhq's own repos and process):
  `splice channel add gh:keelstackhq/decisions//channel --track main --piece portable --piece import-claude`.

## Your org's own decisions repo

Use the same shape, so your repos get your decisions the same way:

1. Copy `DECISIONS.md` (with your rows), `decisions/000-template.md`, `channel/` and `scripts/` into
   a new repo, e.g. `<org>/decisions`.
2. In `channel/splice.toml` set `name = "<org>-decisions"`. Rename the snapshot file to
   `channel/pieces/snapshot/files/docs/agents/<org>-decisions.md.jinja`, set that path in
   `channel/pieces/snapshot/piece.toml` (`owned`), in `scripts/gen-snapshot.sh` and
   `scripts/check-index.sh` (`out`, `snap`), and in the `@docs/agents/...` line of each
   `channel/pieces/import-*/files/sections/org-decisions.md`.
3. In your repos: `splice channel add gh:<org>/decisions//channel --as <org>-decisions --track main
--piece import-claude`. It sits next to keelstackhq's portable channel: the files and the
   section markers (`splice:<channel>:begin`) differ, so neither touches the other.
