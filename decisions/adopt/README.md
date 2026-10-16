# Adopt keelstackhq/decisions in a repo

Three changes, one commit (`chore: adopt org decisions`). Afterwards every agent in that repo loads
the current decisions at session start, about 60 lines.

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

> Adopt keelstackhq/decisions in this repo by following
> https://github.com/keelstackhq/decisions/blob/main/adopt/README.md exactly (hook, snapshot,
> CLAUDE.md import). Then read docs/agents/org-decisions.md and list any decision this repo does
> not follow yet, with the file and line, without fixing it. Open one PR.
