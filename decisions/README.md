# decisions/

Settled decisions that apply across keelstackhq repos, kept small enough for every agent to load
at session start. This folder lives in the `keelstackhq/keelstack` monorepo.

- **Read:** [`DECISIONS.md`](DECISIONS.md), one row per decision (at most 60 lines), each linking
  its reasoning in [`decisions/`](decisions/). Agents inside the monorepo read it in place.
- **Use in a repo outside the monorepo (also every new one):**
  `splice channel add gh:keelstackhq/keelstack//decisions/channel --track main --piece import-project`
  (`--piece import-claude` without `docs/agents/project.md`); see D-004.
- **Change:** discuss an open topic in a shared doc, issue or PR in the repos concerned; once it is
  settled, open a monorepo PR adding `decisions/decisions/NNN-title.md` (from `000-template.md`)
  and one index row, and run `bash decisions/scripts/gen-snapshot.sh` (it writes
  `channel/pieces/snapshot/files/docs/agents/org-decisions.md.jinja`).
  `bash decisions/scripts/check-index.sh` must pass; merging follows D-002 and D-022.
- **A repo in another org:** add `--piece portable` to take only the decisions scoped `all orgs`
  (the rest name keelstackhq's own repos and process):
  `splice channel add gh:keelstackhq/keelstack//decisions/channel --track main --piece portable --piece import-claude`.

- **The catalog of alternatives (D-031):** `--piece catalog` adds `catalog.toml` at the repo root
  (capabilities and alternatives with docs, keyless options and migration notes); `hoist system
  explain` reads it next to `system.toml` and lists each provider's alternatives in `SYSTEM.md`.

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
