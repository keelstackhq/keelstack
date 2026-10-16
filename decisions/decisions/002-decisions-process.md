# D-002: This repo records cross-repo decisions

- Date: 2026-10-16
- Scope: all
- Status: accepted
- Supersedes: —

## Context

Agents work in several repos at once and need the same decisions without reading every repo or
loading long documents into their context.

## Decision

- `DECISIONS.md` is the index: one table row per settled decision, at most 60 lines (CI enforces it).
- Each row links `decisions/NNN-title.md` with the reasoning (template: `decisions/000-template.md`).
- A superseded decision leaves the index; its file stays, marked `superseded by D-NNN`.
- Open topics are discussed in shared docs, issues or PRs; only the outcome lands here.
- Changes arrive by PR; the owner (Adam) merges. Agents propose, never merge.
- Every repo commits a snapshot of the index at `docs/agents/org-decisions.md`, imported by its
  `CLAUDE.md` and refreshed by a session-start hook. A new repo adopts it in its first PR with
  `bash ../decisions/adopt/adopt.sh .` (see `adopt/README.md`).

## Consequences

An agent in any repo starts each session with all current decisions (about 60 lines). Repos render
from hull's template do not get this automatically: hull's template ships to people outside the org.
