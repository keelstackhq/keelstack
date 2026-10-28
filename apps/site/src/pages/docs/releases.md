---
layout: ../../layouts/Docs.astro
---

Two things change a repo after it is set up: your own releases, and updates to the templates and
channels it came from. Agents take part in both, but they never tag: tags come from release
workflows, or from the owner where a repo is tagged by hand.

## Releasing with hoist

The flow of the keelstack tools themselves (hoist, berth, bosun) and of hull's `template-rust`
(since hull v0.3.0).
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
hull's `template-rust`, `BOSUN_RELEASE` in bosun): until then a push to `main` runs `detect` only.

- If `publish` fails, fix the cause outside the repo (a registry setting, a trusted publisher, a
  token) and **re-run the failed job**; do not push a fix commit. Finished steps are skipped.
- The optional secret `HOIST_RELEASE_TOKEN` (Contents and Pull requests: write) makes the release
  pull request run CI. Pull requests opened with the default `GITHUB_TOKEN` do not trigger
  workflows. In bosun's own `release.yml` (since 0.2.2), `prepare` runs only when that secret is
  set: without it the job failed on every push to `main`, since `GITHUB_TOKEN` may not open pull
  requests there, so now it is skipped with a warning.
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
after the `release` environment approval. hull's own `release.yml` (since v0.3.0) works the same
way for a template: a push to `main` whose `CHANGELOG.md` top heading is a new dated
`## v<X.Y.Z> — YYYY-MM-DD` is tagged and released after the approval; `— unreleased` releases
nothing. The tag has to be made in the same run because a tag
pushed with `GITHUB_TOKEN` starts no other workflow.

### Container images

hoist does not push OCI images yet, and a `[deploy]` script cannot either (the `publish` job has
no `packages: write`). hoist's README documents the pattern: a separate `image` job in
`release.yml` that runs after `publish`, only in a run that released, from the release tag, with
`packages: write`, `attestations: write` and `id-token: write`. It pushes
`ghcr.io/<owner>/<image>:<version>` for `linux/amd64` and `linux/arm64` only when that tag does
not exist yet, attests the digest it built with `actions/attest-build-provenance`, and you deploy
by digest.

## Releasing from hull's `template`

hull's `template` (v0.3.0; unchanged since v0.2.x) uses a different `release.yml`: a person pushes a tag
`<name>-v<version>` (for example `cli-v1.2.0`) after the version bump has merged, and the workflow
publishes that package (npm through trusted publishing with provenance, and GitHub Releases for
CLIs). Human steps first: a `release` environment with a tag rule, the first publish of each new
npm name by hand, and an npm trusted publisher (the repo, `release.yml`, environment `release`),
then a release within 2 days. Agents bump the version and stop there.

### Changelog fragments

Since bosun 0.2.2, pull requests in bosun (and in repos with its channel) never edit
`CHANGELOG.md` or the Done list of `docs/PROGRESS.md`. Each adds a one-line fragment,
`changelog.d/<branch>.md` or `docs/progress.d/<branch>.md` (the branch name with `/` as `-`), so
merging `main` into a pull request never conflicts on them. Before a release, a docs pull request
to `main` runs `scripts/fold-fragments.sh` (the channel ships it as `.bosun/fold-fragments.sh`),
which appends them under `## Unreleased` in `CHANGELOG.md` and `## Done` in
`docs/PROGRESS.md` and deletes them.

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

- Pass a token other than `GITHUB_TOKEN`. With `GITHUB_TOKEN` the bot cannot push changes to
  workflow files, and its pull requests run no CI. hull v0.4.0 (not tagged yet) tries, in order: a GitHub App
  (variable `SPLICE_APP_CLIENT_ID`, secret `SPLICE_APP_KEY`; Contents, Pull requests and
  Workflows write; no personal token), then `SPLICE_BOT_TOKEN` (a fine-grained token with the same
  permissions), then `github.token`. Keep them in the environment `splice-bot` with deployment
  branches `main` only. Session mode: set the repository variable `SPLICE_BOT=off` and run
  `splice update` in an agent session that opens the pull request.
- `splice update` (0.3.1) no longer suggests `--data` for a variable that only a new piece uses.
- In a bosun repo the update pull request always touches `.splice/**`, so it is R-gov: the owner
  approves it with `/bosun approve <head sha>` every week.
- By hand: `splice check` (exit 1 when an update is available or owned files drifted),
  `splice diff` (writes nothing), `splice update` (clean tree needed). On conflicts, resolve the
  diff3 markers, run `splice check`, then commit.

## Secrets

hoist 0.3.0 (not released yet) adds `hoist secrets`:
values encrypted with sops and age in a private secrets repo, mapped into each consumer repo by a
`secrets.toml` that holds no values. hull v0.4.0's opt-in `secrets` piece seeds that file and a
`deploy.yml` that runs on push to `main` in the environment `prod`.

| Command                                    | Who and where                                                                   |
| ------------------------------------------ | ------------------------------------------------------------------------------- |
| `hoist secrets repo init OWNER/NAME`       | owner: creates the private secrets repo with its ruleset                        |
| `hoist secrets edit FILE`                  | owner, secrets repo: `sops` edit with your own age key                          |
| `hoist secrets grant OWNER/REPO --env E`   | owner: a consumer environment's key; creates `preview` and `prod`, main-only    |
| `hoist secrets sync`                       | owner, secrets repo: `.sops.yaml` from `access.toml`, re-keys changed files     |
| `hoist secrets revoke NAME`                | owner, secrets repo: removes a recipient, new data keys, lists values to rotate |
| `hoist secrets rotate-key NAME`            | owner, secrets repo: routine rotation of a consumer key                         |
| `hoist secrets check [--remote]`           | anyone: validates `access.toml` or `secrets.toml`; never decrypts               |
| `hoist secrets exec --env E --keys A,B --` | CI only: runs a command with those keys, output redacted                        |

- **Keys live only in main-only environments.** `SOPS_AGE_KEY` is an environment secret of
  `preview` and `prod` (deployment branches: `main`), never a repository secret.
- **Pull requests never get secrets.** `secrets exec` refuses outside CI, in an agent session, on
  a pull request or fork, and off the default branch or a release tag. A repo whose release runs
  decrypt secrets tags `<name>-v<version>`.
- **Agents never decrypt.** They add a key by editing `secrets.toml` and ask the owner to store the
  value; on bosun's `main` the hook refuses `sops`, `age -d`, `hoist secrets edit|sync|grant|revoke|rotate*`
  and reading the age key, and those files are R-secrets.

## The decisions channel

Org decisions are a short table (`DECISIONS.md`, at most 60 lines) with one file of reasoning per
decision. Each repo gets a copy as `docs/agents/org-decisions.md` through the channel
`gh:keelstackhq/decisions//channel`, added with `--track main`, so the update bot brings every
merged change. Commit such an update alone, as `chore: sync org decisions`.

To change a decision, open a pull request in the decisions repo that adds
`decisions/NNN-title.md` and one index row, and copies `DECISIONS.md` to the channel snapshot
(`channel/pieces/snapshot/files/docs/agents/org-decisions.md`); the owner merges. Superseded
decisions leave the index; their files stay, marked superseded.

Current keelstackhq decisions (D-001 to D-005, D-007, D-008): the org and npm scope are
`keelstackhq`; the decisions repo records settled cross-repo decisions; hull and splice share
`docs/template-format.md`; org decisions reach repos as this channel; shared template rules bind
both renderers. The two newest, both settled on 2026-10-26:

- **D-007, CI minutes.** Run the repo's local CI mirror before every push; one push per pull
  request, with review rounds done locally; workflows skip comment, label and edit events at job
  level; small jobs fold into one that keeps `ci-ok`; Windows and macOS only in release workflows;
  scheduled workflows at most weekly.
- **D-008, merges and tags** (supersedes D-006). Child sessions never merge, push or move tags, or
  force-push; they get the required checks green and may enable auto-merge. The orchestrator
  session may merge once the required checks are green and any owner approval is in. Release
  workflows may tag on a version-bump merge, and the owner still approves `release`; tag-only repos
  without such a workflow are tagged by the owner; the decisions repo stays owner-merged.
