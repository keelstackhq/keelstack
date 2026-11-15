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
- Needs splice 0.3.0 or later. Without a global install write `pnpm dlx @keelstackhq/splice@0.5.1` in place of `splice`.
- Do bosun first, then hull: each is its own PR, each is R-gov, and the owner approves each head.
- Merge only when `ci-ok` is green.
- Full guide: [hull's `docs/updating.md`](https://github.com/keelstackhq/hull/blob/5a72442649f6958e91a50283a91e985f45e10a09/docs/updating.md) at tag `v0.12.0` (`5a72442`).

## Update bosun

1. `bosun doctor` # step 17 "update" gives the fix; warns, never fails
2. `bosun pin --latest --check` # lists tools behind, downloads and writes nothing; run on the default branch
3. `bosun pin --latest --pr` # verifies npm sha512, branch bosun/pin, draft PR (default branch, clean tree)
4. The owner approves; auto-merge lands it on green.
5. `npm i -g @keelstackhq/bosun@<new> && bosun --version` # your machine, after the merge; <new> is the version pin --check printed

- Cloud: nothing to do. Paste `bosun setup-snippet` once; never again.
- A repo set up with `bosun init` or hull has `.splice/channels/bosun.toml`: `bosun pin` refuses there; use `splice update --channel bosun` (the update bot opens that PR). The steps above are for a repo set up with `bosun adopt`.
- Full guide: [bosun's `docs/update.md`](https://github.com/keelstackhq/bosun/blob/main/docs/update.md).

Machine-readable: this page is https://keelstack.dev/docs/update.md.
