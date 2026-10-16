# D-003: hull and splice share the template contract

- Date: 2026-10-15
- Scope: hull, splice
- Status: accepted
- Supersedes: —

## Context

splice renders hull's template; both implement the same format, and drift would make a template
render differently in the two tools.

## Decision

`docs/template-format.md` exists in both repos and stays byte-identical. A change lands in both
repos in the same week, agreed first (the 2026-10-15 proposals doc is the pattern). Neither repo
edits it alone.

## Consequences

Contract changes need one PR per repo, merged together. hull's dev renderer and splice enforce the
same template-author rules.
