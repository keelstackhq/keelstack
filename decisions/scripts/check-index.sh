#!/usr/bin/env bash
# Keep DECISIONS.md small and consistent: <= 60 lines, unique IDs, every row links an existing record.
set -euo pipefail
cd "$(dirname "$0")/.."
fail=0
lines=$(wc -l < DECISIONS.md)
if [ "$lines" -gt 60 ]; then echo "DECISIONS.md has $lines lines (limit 60)"; fail=1; fi
# Repos format-check their snapshot (oxfmt/prettier align tables), so the table must be aligned:
# every table line the same length. Fix with e.g. `npx prettier --write DECISIONS.md`.
widths=$(grep -E '^\|' DECISIONS.md | awk '{ print length }' | sort -u | wc -l)
if [ "$widths" -gt 1 ]; then echo "DECISIONS.md table is not aligned; format it (npx prettier --write DECISIONS.md)"; fail=1; fi
ids=$(grep -oE '^\| D-[0-9]{3} ' DECISIONS.md | tr -d '| ' || true)
dupes=$(printf '%s\n' "$ids" | sort | uniq -d)
if [ -n "$dupes" ]; then echo "duplicate IDs: $dupes"; fail=1; fi
while IFS= read -r row; do
  id=$(printf '%s' "$row" | grep -oE 'D-[0-9]{3}' | head -1)
  link=$(printf '%s' "$row" | grep -oE '\(decisions/[^)]+\.md\)' | tr -d '()' || true)
  if [ -z "$link" ]; then echo "$id: no link to decisions/NNN-title.md"; fail=1; continue; fi
  if [ ! -f "$link" ]; then echo "$id: $link does not exist"; fail=1; continue; fi
  num=${id#D-}
  case "$(basename "$link")" in "$num"-*) ;; *) echo "$id: file $link does not start with $num-"; fail=1 ;; esac
  if grep -q 'Status: superseded' "$link"; then echo "$id: $link is superseded; remove it from the index"; fail=1; fi
done < <(grep -E '^\| D-[0-9]{3} ' DECISIONS.md)
# The channel template cannot hold symlinks, so it carries a copy of DECISIONS.md; keep it identical.
snap=channel/pieces/snapshot/files/docs/agents/org-decisions.md
if ! cmp -s DECISIONS.md "$snap"; then echo "$snap differs from DECISIONS.md; run: cp DECISIONS.md $snap"; fail=1; fi
if [ "$fail" -ne 0 ]; then exit 1; fi
echo "DECISIONS.md ok: $lines lines, $(printf '%s\n' "$ids" | grep -c . ) decisions"
