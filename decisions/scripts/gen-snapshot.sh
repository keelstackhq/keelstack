#!/usr/bin/env bash
# Write the channel snapshot from DECISIONS.md. Rows whose Scope is not `all orgs` apply only inside
# keelstackhq: they are left out when the consumer selects the `portable` piece. Without it the
# render is byte-identical to DECISIONS.md. Uses only the template subset (if/else on `pieces`).
set -euo pipefail
cd "$(dirname "$0")/.."
out=channel/pieces/snapshot/files/docs/agents/org-decisions.md.jinja
mkdir -p "$(dirname "$out")"
awk '
  BEGIN { g = "{% if \"portable\" in pieces %}"; open = 0 }
  # A guard keeps the newline inside its branch, so the next line starts with the endif.
  function emit(text) { printf "%s%s", (open ? "{% endif %}" : ""), text; open = 0 }
  /^\| ID / && !noted {
    emit(g "Portable subset: only decisions with scope `all orgs`; keep your org'"'"'s own in its own decisions repo (README).\n\n")
    open = 1; noted = 1
  }
  /^\| D-[0-9]+ / {
    split($0, cell, "|"); scope = cell[4]; gsub(/^ +| +$/, "", scope)
    if (scope != "all orgs") { emit(g "{% else %}" $0 "\n"); open = 1; next }
  }
  { emit($0 "\n") }
  END { if (open) printf "%s", "{% endif %}" }
' DECISIONS.md > "$out.tmp"
mv "$out.tmp" "$out"
