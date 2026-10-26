---
layout: ../../layouts/Docs.astro
---

Two things change a repo after it is set up: your own releases, and updates to the templates and
channels it came from. Agents take part in both, but they never tag: tags come from release
workflows, or from the owner where a repo is tagged by hand (org decision D-006).

## Releasing with hoist

The flow of the keelstack tools themselves (hoist, berth, bosun) and of hull's `template-rust`
(unreleased v0.3.0).
`.github/workflows/release.yml` runs `hoist release` on every push to `main`:

1. **Release pull request.** Job `prepare` runs `hoist release prepare`: it computes the next
   versions from conventional commits and opens or updates a pull request on branch
   `hoist/release` with the version bumps, dependents, `CHANGELOG.md` and lockfiles. Merging that
   pull request is the decision to release.
2. **Detect and build.** On the merge, `detect` finds a version without a tag, and `build` makes
   the binaries.
3. **Publish, after the owner approves.** `publish` runs in the environment `release`, which waits
   for the owner's approval. `hoist release publish` creates the tag `v<version>` through the API,
   a draft GitHub Release with the changelog section and the binaries plus `SHA256SUMS`, publishes
   the release, then the npm packages and crates through trusted publishing (no tokens stored).
4. `prepare` then opens, updates or closes the next release pull request.

Nothing releases until the owner switches it on with a repository variable (`HOIST_RELEASE` in
hull's `template-rust` on hull `main`, `BOSUN_RELEASE` in bosun): until then a push to `main` runs `detect` only.

- If `publish` fails, fix the cause outside the repo (a registry setting, a trusted publisher, a
  token) and **re-run the failed job**; do not push a fix commit. Finished steps are skipped.
- The optional secret `HOIST_RELEASE_TOKEN` (Contents and Pull requests: write) makes the release
  pull request run CI. Pull requests opened with the default `GITHUB_TOKEN` do not trigger
  workflows.
- `hoist release plan` shows the next versions and writes nothing.

### The first release (once, by the owner)

npm and crates.io can register a trusted publisher only for a package that already exists, so the
first publish needs a person:

1. `hoist apply --area release` creates the `release` environment before the first run. Make the
   owner its required reviewer and limit it to `main`.
2. Optionally add `HOIST_RELEASE_TOKEN`, then set the release variable to `enabled`.
3. Let the workflow tag the first version. Its `publish` job stops at the packages that do not
   exist yet (`first-publish:<name>`). This failure is expected.
4. On your machine, logged in to npm (npm 11.15.0 or later) and crates.io, in a checkout of the
   tag:

   ```bash
   hoist release bootstrap --dry-run   # what is missing; writes nothing
   hoist release bootstrap             # lists the writes, asks, then runs them
   ```

   It publishes each missing npm package once as a placeholder `0.0.0`, publishes each missing
   crate at its tag, and registers the npm trusted publishers. A crates.io trusted publisher stays
   a manual step in the crate's settings (repository, workflow `release.yml`, environment
   `release`).

5. **Within 2 days**, re-run the failed `publish` job. npm expires a trusted publisher that has
   not been used for 2 days.

Never publish a real npm version by hand: npm versions are immutable.

### A tag on merge, without a release pull request

splice's own `release.yml` (since 2026-10-26) skips the release pull request: a push to `main`
whose version has no tag yet runs a `tag` job that creates the tag, and the same run releases it,
after the `release` environment approval. The tag has to be made in the same run because a tag
pushed with `GITHUB_TOKEN` starts no other workflow.

## Releasing from hull's `template`

hull's `template` (v0.2.2) uses a different `release.yml`: a person pushes a tag
`<name>-v<version>` (for example `cli-v1.2.0`) after the version bump has merged, and the workflow
publishes that package (npm through trusted publishing with provenance, and GitHub Releases for
CLIs). Human steps first: a `release` environment with a tag rule, the first publish of each new
npm name by hand, and an npm trusted publisher (the repo, `release.yml`, environment `release`),
then a release within 2 days. Agents bump the version and stop there.

## Pinning bosun after a bosun release

A repo pins the bosun release it runs in `.bosun/version`; its workflows install that version
from npm. A new channel version is a bosun release tag, and it brings the matching pin, so the
splice update bot moves the channel files and the pin in one pull request. That pull request is
R-gov and needs the owner's approval. After a pin change, paste `docs/setup-script.sh` into the
cloud environment again, since it installs the pinned bosun.

## Template and channel updates

The splice update bot is a workflow (`.github/workflows/splice.yml`, which the bosun channel
seeds) using `keelstackhq/splice/action@v1`, weekly and on demand. It runs `splice update --all`
when the repo has channels and opens or refreshes one pull request on branch `splice/update`:

| Result      | The bot                                                    |
| ----------- | ---------------------------------------------------------- |
| `updated`   | commits to the branch and opens or edits the pull request  |
| `conflicts` | the same, with a title starting `[conflicts]`              |
| `current`   | closes an open bot pull request                            |
| `drift`     | fails: an owned file was edited locally; nothing is pushed |

- Pass a token other than `GITHUB_TOKEN` (`SPLICE_BOT_TOKEN`: a fine-grained token with Contents,
  Pull requests and Workflows read and write, or a GitHub App token). With `GITHUB_TOKEN` the bot
  cannot push changes to workflow files, and its pull requests run no CI.
- In a bosun repo the update pull request always touches `.splice/**`, so it is R-gov: the owner
  approves it with `/bosun approve <head sha>` every week.
- By hand: `splice check` (exit 1 when an update is available or owned files drifted),
  `splice diff` (writes nothing), `splice update` (clean tree needed). On conflicts, resolve the
  diff3 markers, run `splice check`, then commit.

## The decisions channel

Org decisions are a short table (`DECISIONS.md`, at most 60 lines) with one file of reasoning per
decision. Each repo gets a copy as `docs/agents/org-decisions.md` through the channel
`gh:keelstackhq/decisions//channel`, added with `--track main`, so the update bot brings every
merged change. Commit such an update alone, as `chore: sync org decisions`.

To change a decision, open a pull request in the decisions repo that adds
`decisions/NNN-title.md` and one index row, and copies `DECISIONS.md` to the channel snapshot
(`channel/pieces/snapshot/files/docs/agents/org-decisions.md`); the owner merges. Superseded
decisions leave the index; their files stay, marked superseded.

Current keelstackhq decisions (D-001 to D-006): the org and npm scope are `keelstackhq`; the
decisions repo records settled cross-repo decisions; hull and splice share
`docs/template-format.md`; org decisions reach repos as this channel; shared template rules
bind both renderers; agents never merge, tag or force-push.
