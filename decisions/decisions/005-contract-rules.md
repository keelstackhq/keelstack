# D-005: shared template rules and how the contract versions

- Date: 2026-10-21
- Scope: hull, splice
- Status: accepted
- Supersedes: —

## Context

splice and hull read points the contract left open differently (identifiers, path safety,
whitespace, raw `{{`, which shell runs commands). They settled them in the splice ⇄ hull proposals
doc (2026-10-15) and the owner approved the rest on 2026-10-21. The rules landed in both repos'
`docs/template-format.md` together: keelstackhq/hull#6 and keelstackhq/splice#8.

## Decision

- `docs/template-format.md` §9.15–26 are binding for every template hull or splice handles:
  required/default keys, reserved and keyword names, `else` and comment rules, ASCII tag
  whitespace, no symlinks, portable path segments and case collisions per source, `[[remove]]`
  with the from-version, `{% raw %}` blocks, stray `#}`, POSIX `sh -c` for `[[post]]`/`[[step]]`,
  and `[sections]` for channel templates.
- `format` stays 1. It is bumped only when a change makes a valid template invalid or renders it
  differently; syntax a template opts into (raw blocks, `[sections]`) is gated by `min_splice`
  (0.2.0 or later).
- Both renderers enforce the rules alike: splice (MiniJinja) and hull's dev renderer (Nunjucks)
  share the conformance corpus in splice `tests/fixtures/subset/`, vendored byte for byte in hull
  `tests/fixtures/conformance/`.

## Consequences

- hull: templates may emit `${{ … }}` through raw blocks once they set `min_splice = "0.2.0"`;
  workflow files that used runtime guards for this can become `.jinja` later.
- splice: templates that relied on the old leniency (names like `con.txt`, a var named `none`,
  `{#- … #}`) now exit 3; `update` still reads an older tag leniently as its merge base.
- A new rule follows D-003: same text in both repos in the same week, plus a corpus case when it
  touches rendering.
