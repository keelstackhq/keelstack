---
layout: ../../layouts/Docs.astro
---

How to bring a repo rendered from hull up to the newest hull release. The steps follow hull's
[`docs/updating.md`](https://github.com/keelstackhq/hull/blob/main/docs/updating.md), the full
guide.

## hull

Work on a branch, so the update is one reviewable change:

1. `git switch -c chore/splice-update`
2. `splice check --json`: exit 1 = update available, drift or markers
3. `splice diff`: preview, writes nothing
4. `splice update --all`: template + channels
5. `git grep -n '<<<<<<<'`: resolve conflicts; owned files: move edits to seed files
6. `splice check`: must exit 0
7. `pnpm install && pnpm run ci`
8. `git commit -am "chore: splice update to hull vX.Y.Z"`

The weekly splice workflow runs steps 2 to 4 and opens the PR for you. The full guide is
[hull's `docs/updating.md`](https://github.com/keelstackhq/hull/blob/main/docs/updating.md).

Machine-readable: this page is https://keelstack.dev/docs/update.md.
