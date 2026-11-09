---
layout: ../../layouts/Docs.astro
---

How to bring a repo up to the newest hull and bosun releases. Each path follows the tool's own
guide, linked at the end of its section.

## Update hull

Work on a branch, so the update is one reviewable change:

1. `git switch -c chore/splice-update`
2. `splice check --json` # exit 1 = update available, drift or markers
3. `splice diff` # preview, writes nothing
4. `splice update --all` # template + channels
5. `git grep -n '<<<<<<<'` # resolve conflicts; owned files: move edits to seed files
6. `splice check` # must exit 0
7. `pnpm install && pnpm run ci`
8. `git add -A && git commit -m "chore: splice update to hull vX.Y.Z"`

- Normally you do not run this: the weekly splice workflow (`splice update --all`) opens the PR on `splice/update`.
- Needs splice 0.3.0 or later (`pnpm dlx @keelstackhq/splice` works without installing it).
- Merge only when `ci-ok` is green.
- Full guide: [hull's `docs/updating.md`](https://github.com/keelstackhq/hull/blob/720442f7557168f139d8d836ceddd8aebe4e7952/docs/updating.md) at commit `720442f`.

## Update bosun

1. `bosun doctor` # step 17 "update" gives the fix; warns, never fails
2. `bosun pin --latest --check` # list only, writes nothing
3. `bosun pin --latest --pr` # verifies npm sha512, branch bosun/pin, draft PR (default branch, clean tree)
4. The owner approves; auto-merge lands it on green.
5. `npm i -g @keelstackhq/bosun@<new> && bosun --version` # your machine, after the merge

- Cloud: nothing to do. Paste `bosun setup-snippet` once; never again.
- Channel repos: `splice update --channel bosun`.
- Full guide: [bosun's `docs/update.md`](https://github.com/keelstackhq/bosun/blob/main/docs/update.md).

Machine-readable: this page is https://keelstack.dev/docs/update.md.
