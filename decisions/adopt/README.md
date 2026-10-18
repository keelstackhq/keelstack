# Adopt keelstackhq/decisions in a repo

> **Deprecated (D-004).** Use the splice channel (splice 0.2.0 or later) and delete the
> `sync-decisions.sh` hook and the hand-written "Org decisions" block first:
>
> ```bash
> splice channel add gh:keelstackhq/decisions//channel --track main --piece import-project
> ```
>
> Use `--piece import-claude` when the repo has no `docs/agents/project.md`. The rest of this page
> describes the old script, kept until every org repo has the channel.

One command, one commit. Afterwards every agent in that repo loads the current decisions at
session start, about 60 lines. Every new keelstackhq repo does this in its first PR.

```bash
git clone --depth 1 https://github.com/keelstackhq/decisions ../decisions   # or git -C ../decisions pull
bash ../decisions/adopt/adopt.sh .
git checkout -b chore/adopt-org-decisions && git add -A && git commit -m "chore: adopt org decisions"
```

`adopt.sh` is idempotent (run it again any time) and does the three steps below. In a repo
generated from hull, whose `CLAUDE.md`, `.claude/settings.json` and session-start hook are
template-owned, it writes the import into `docs/agents/project.md` and the hook into
`.claude/hooks/session-start.d/`, which hull's session-start hook runs (hull templates from
2026-10-16 on); it does not touch `.claude/settings.json`. Doing it by hand:

1. Copy `adopt/sync-decisions.sh` to `.claude/hooks/sync-decisions.sh` and the current
   `DECISIONS.md` to `docs/agents/org-decisions.md` (a committed snapshot: CI, offline sessions
   and agents without access to this private repo still see it).
2. In `.claude/settings.json`, add to `hooks.SessionStart` (keep existing hooks):

   ```json
   {
     "matcher": "startup|resume",
     "hooks": [
       {
         "type": "command",
         "command": "bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/sync-decisions.sh\"",
         "timeout": 60
       }
     ]
   }
   ```

3. Append to the repo's `CLAUDE.md` (in a template-owned `CLAUDE.md`, use the file it imports for
   repo notes instead, e.g. `docs/agents/project.md`):

   ```markdown
   ## Org decisions

   Cross-repo decisions from keelstackhq/decisions; follow them. To change one, open a PR there.
   The hook refreshes this snapshot; commit an update alone as `chore: sync org decisions`.

   @docs/agents/org-decisions.md
   ```

Check: start a new session and run `/memory` or ask "which org decisions apply here?"; the agent
should list the D-IDs.

## Prompt for the repo's agent

> Adopt keelstackhq/decisions in this repo: clone it to ../decisions and run
> `bash ../decisions/adopt/adopt.sh .` (see its adopt/README.md). Review the diff against this
> repo's own conventions, run the repo's checks, and open one PR `chore: adopt org decisions`.
> Then read docs/agents/org-decisions.md and list, in the PR description, any decision this repo
> does not follow yet (file and line), without fixing it.
