#!/usr/bin/env bash
# Adopt keelstackhq/decisions in a repo: snapshot + SessionStart hook + CLAUDE.md import.
# Usage: bash <decisions-checkout>/adopt/adopt.sh <target-repo-dir>   (idempotent; run again any time)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
src_repo="$(dirname "$here")"
target="$(cd "${1:?usage: adopt.sh <target-repo-dir>}" && pwd)"
cd "$target"

# 1. Hook and snapshot.
mkdir -p .claude/hooks docs/agents
cp "$here/sync-decisions.sh" .claude/hooks/sync-decisions.sh
cp "$src_repo/DECISIONS.md" docs/agents/org-decisions.md
echo "adopt: wrote .claude/hooks/sync-decisions.sh and docs/agents/org-decisions.md"

# 2. SessionStart hook in .claude/settings.json (kept if already present).
merge='
const fs = require("fs"); const p = ".claude/settings.json";
const s = fs.existsSync(p) ? JSON.parse(fs.readFileSync(p, "utf8")) : {};
const cmd = "bash \"$CLAUDE_PROJECT_DIR/.claude/hooks/sync-decisions.sh\"";
s.hooks ??= {}; s.hooks.SessionStart ??= [];
const has = s.hooks.SessionStart.some((e) => (e.hooks || []).some((h) => h.command === cmd));
if (!has) s.hooks.SessionStart.push({ matcher: "startup|resume", hooks: [{ type: "command", command: cmd, timeout: 60 }] });
fs.writeFileSync(p, JSON.stringify(s, null, 2) + "\n");
console.log(has ? "adopt: settings.json already has the hook" : "adopt: added the SessionStart hook to .claude/settings.json");
'
if command -v node >/dev/null 2>&1; then
  node -e "$merge"
elif command -v python3 >/dev/null 2>&1; then
  python3 - <<'PY'
import json, os
p = ".claude/settings.json"
s = json.load(open(p)) if os.path.exists(p) else {}
cmd = 'bash "$CLAUDE_PROJECT_DIR/.claude/hooks/sync-decisions.sh"'
ss = s.setdefault("hooks", {}).setdefault("SessionStart", [])
has = any(h.get("command") == cmd for e in ss for h in e.get("hooks", []))
if not has:
    ss.append({"matcher": "startup|resume", "hooks": [{"type": "command", "command": cmd, "timeout": 60}]})
json.dump(s, open(p, "w"), indent=2); open(p, "a").write("\n")
print("adopt: settings.json already has the hook" if has else "adopt: added the SessionStart hook to .claude/settings.json")
PY
else
  echo "adopt: neither node nor python3 found; add the hook from adopt/README.md step 2 by hand" >&2
fi

# 3. Import the snapshot. A template-owned CLAUDE.md (hull-generated repos) imports
#    docs/agents/project.md; put the section there instead so template updates keep it.
notes="CLAUDE.md"
if [ -f CLAUDE.md ] && grep -q '^@docs/agents/project.md' CLAUDE.md && [ -f docs/agents/project.md ]; then
  notes="docs/agents/project.md"
fi
if grep -qs '@docs/agents/org-decisions.md' "$notes"; then
  echo "adopt: $notes already imports the snapshot"
else
  {
    [ -s "$notes" ] && printf '\n'
    cat <<'MD'
## Org decisions

Cross-repo decisions from keelstackhq/decisions; follow them. To change one, open a PR there.
The hook refreshes this snapshot; commit an update alone as `chore: sync org decisions`.

@docs/agents/org-decisions.md
MD
  } >>"$notes"
  echo "adopt: added the Org decisions import to $notes"
fi
echo "adopt: done. Review the diff and commit it as 'chore: adopt org decisions'."
