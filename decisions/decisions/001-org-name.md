# D-001: Org and npm scope are keelstackhq

- Date: 2026-10-15
- Scope: all
- Status: accepted
- Supersedes: —

## Context

The repos started out naming `keelstack/*` and `@keelstack/*`, but the GitHub org and the npm scope
that exist are `keelstackhq`. splice proposed the rename in the splice ⇄ hull contract-proposals doc
and hull agreed.

## Decision

Repos are `keelstackhq/<name>`, npm packages `@keelstackhq/<name>`, template sources
`gh:keelstackhq/<repo>//<dir>`. "keelstack" stays as the product name in prose.

## Consequences

Every repo replaces old `keelstack/` and `@keelstack/` references. hull did so on 2026-10-15
(keelstackhq/hull#1); the shared `docs/template-format.md` follows in the joint hull + splice PR.
