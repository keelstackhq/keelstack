---
layout: ../../layouts/Docs.astro
---

How to bring a repo rendered from hull up to the newest hull release. The steps follow hull's
[`docs/updating.md`](https://github.com/keelstackhq/hull/blob/720442f7557168f139d8d836ceddd8aebe4e7952/docs/updating.md), the full
guide, at commit `720442f`.

## hull

Work on a branch, so the update is one reviewable change:

1. `git switch -c chore/splice-update`
2. `splice check --json`: exit 1 = update available, drift or markers
3. `splice diff`: preview, writes nothing
4. `splice update --all`: template + channels
5. `git grep -n '<<<<<<<'`: resolve conflicts; owned files: move edits to seed files
6. `splice check`: must exit 0
7. `pnpm install && pnpm run ci`
8. `git add -A && git commit -m "chore: splice update to hull vX.Y.Z"`

Normally you do not run this: the weekly splice workflow (`splice update --all`) opens the PR on
`splice/update`. The full guide is
[hull's `docs/updating.md`](https://github.com/keelstackhq/hull/blob/720442f7557168f139d8d836ceddd8aebe4e7952/docs/updating.md).

Machine-readable: this page is https://keelstack.dev/docs/update.md.
