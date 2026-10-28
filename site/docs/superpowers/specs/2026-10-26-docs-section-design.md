# Docs section (design)

Date: 2026-10-26 · Status: accepted (decided autonomously; the orchestrator relays the owner's
goal, the owner was not asked)

## Goal

Owner's words: "The landing page must contain a proper docs section for all of the mechanics so
I can run it in an external repo as well."

A reader with an empty or existing GitHub repo outside keelstackhq adopts the whole system (hull,
splice, bosun, hoist, hoard, berth, the decisions channel) end to end from `/docs/` alone.

## Sources and grounding

Copy comes only from these repos, at the commits read on 2026-10-26. Where docs and code
disagree, the page follows the code and says so.

| Repo        | Commit    | Files read                                                                                                                                                     |
| ----------- | --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| hull        | `a7ebb3f` | README, CHANGELOG, docs/pieces.md, docs/PROGRESS.md                                                                                                            |
| splice      | `e0984d9` | README, docs/getting-started.md, docs/PROGRESS.md                                                                                                              |
| bosun       | `0b448e9` | README, docs/spec.md, docs/defaults.md, docs/plan-format.md, docs/releasing.md, docs/PROGRESS.md, CHANGELOG, AGENTS.md, REVIEW.md, channel/**, src/cli/init.rs |
| hoist       | `a007f72` | README, CHANGELOG, design spec §12                                                                                                                             |
| hoard       | `2f46250` | README                                                                                                                                                         |
| berth       | `44453a5` | README, docs/integration.md                                                                                                                                    |
| decisions   | `b440e67` | README, DECISIONS.md, decisions/001-006                                                                                                                        |
| rust-canary | `94295b3` | docs/canary-findings.md (a real external-repo trial, 2026-10-25)                                                                                               |

Released versions on 2026-10-26 (git tags): hull v0.2.2 (`template-rust` is on main only, in the
unreleased v0.3.0), splice 0.2.0 (main is 0.2.1, untagged; `update --ref` is unreleased), bosun
0.2.0, hoist 0.2.0, hoard v0.3.0 (pin `@v0`), berth 0.1.0.

Findings that shape the copy:

- Many rust-canary findings are fixed on bosun main but sit under "Unreleased" in its CHANGELOG
  (docs-only fast path, `own_ci` running only test-integrity, `init --branch`, human merges
  moving tasks to merged, the R-gov additions for `**/AGENTS.md` and `**/CLAUDE.md`). Pages mark
  each such behaviour "on bosun main, not in 0.2.0 yet". A consumer's channel version is a bosun
  release tag, so a 0.2.0 consumer gets the 0.2.0 behaviour.
- Signed chat approvals (`bosun-sig ed25519`) exist only on bosun branch
  `feat/signed-approvals` (not merged): documented as upcoming, one paragraph, no commands.
- hull's README still says "pre-0.1, no release tag" and splice's getting-started still says
  "until the first release"; the tags and npm packages exist, so the pages follow the tags.
- No source says tags created with `GITHUB_TOKEN` do not trigger workflows. The sourced pitfall
  is "pushes and PRs made with `GITHUB_TOKEN` do not trigger workflows" (splice README, hoist
  README); the troubleshooting page uses that wording.

## Decisions

| Topic         | Decision                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| URLs          | `/docs/` overview, `/docs/getting-started/`, `/docs/tasks/`, `/docs/ci/`, `/docs/risk/`, `/docs/releases/`, `/docs/troubleshooting/`. Short, stable, one topic per page.                                                                                                                                                                                                                                                   |
| Authoring     | Markdown pages in `apps/site/src/pages/docs/*.md` with `layout:` frontmatter (Astro renders `.md` pages natively; no new dependency, no MDX).                                                                                                                                                                                                                                                                              |
| Page data     | `src/data/docs.ts` holds per page `slug`, `title`, `summary`, `sources` (`<repo>/<path>` strings) and `ready`; the `.md` frontmatter holds only `layout`. The layout finds its entry by URL, renders the title as the `h1`, the summary as the meta description and `sources` as a "Sources" list linking `github.com/keelstackhq/<repo>/blob/main/<path>`, so every claim is checkable. Tests read one TS file, not YAML. |
| Layout        | `src/layouts/Docs.astro`: the site `Nav`, a sidebar (page list from `src/data/docs.ts`, current page marked), the prose column, then Sources and prev/next links. No footer CTA block. On narrow screens the sidebar is a `<details>` disclosure above the content. No JavaScript.                                                                                                                                         |
| Code blocks   | `markdown.syntaxHighlight: false` in `astro.config.mjs` (template-owned, already diverging for React): plain `<pre><code>` styled by `docs.css`, light, no inline token styles, smaller HTML.                                                                                                                                                                                                                              |
| Styles        | `src/styles/docs.css`, unlayered, existing tokens only. Tables scroll inside themselves (`display: block; overflow-x: auto`), `pre` scrolls horizontally, so the page never scrolls sideways at 360 px. Light only.                                                                                                                                                                                                        |
| Nav           | The top nav gets "Docs" (`/docs/`) before "GitHub ↗". Footer unchanged.                                                                                                                                                                                                                                                                                                                                                    |
| `/llms.txt`   | New "## Docs" section built from `src/data/docs.ts`: one line per page with its absolute URL and summary.                                                                                                                                                                                                                                                                                                                  |
| Gaps          | Where a reader would be stuck, the page says "Not documented in the sources yet." The `TODO(copy)` markers live in `docs/agents/project.md` (one list for all pages), not in HTML comments, which reach the built page.                                                                                                                                                                                                    |
| Status labels | Unreleased behaviour: "On bosun main, not released yet." Upcoming (unmerged): "Upcoming." Versions are written out (`@keelstackhq/splice@0.2.0`), never "latest".                                                                                                                                                                                                                                                          |
| Budget        | Same as `/`: HTML + CSS + fonts under ~150 KB transferred per docs page; no island on docs pages.                                                                                                                                                                                                                                                                                                                          |
| Tests         | `src/data/docs.test.ts`: every nav entry has a page file and vice versa, each page has title, description and at least one source, sources point at the eight repos, internal `/docs/` links resolve, no old org names. `llms.test.ts` covers the Docs section.                                                                                                                                                            |
| Work split    | Infra and tests through an implementer subagent; page copy written by the orchestrating session, which read the sources; an opus reviewer checks every PR's copy against the sources for invented claims.                                                                                                                                                                                                                  |

## Page contents

1. **Overview** (`/docs/`): one paragraph per tool (what it is, released version, install line),
   how they fit (hull renders through splice; channels: rigging, decisions, bosun; bosun runs on
   GitHub state and calls berth, hoist, hoard; D-006 agents never merge or tag), and a reading
   order.
2. **Getting started** (`/docs/getting-started/`): prerequisites; path A, new repo from hull
   (`splice new`, first commit, push); path B, existing repo (decisions channel, bosun channel
   without hull); then bosun (`bosun init`, setup PR, owner steps), repo settings (`hoist apply`
   or manual ruleset), the cloud environment setup script, hoard in CI, berth locally, Routines,
   first plan and task, releases (pointer to `/docs/releases/`).
3. **Tasks** (`/docs/tasks/`): plans, items, tasks and UCs; labels; the state machine table;
   leases, heartbeat, claim conflicts; sessions, spawn checks, waves, `wait`, results; stop rules
   and tier escalation; reap and resume; main-red, process-red; spend guard; the JSON envelope and
   exit codes.
4. **CI and gates** (`/docs/ci/`): gate contract, profiles (rust, ts-pnpm, generic), required
   checks (`ci-ok`, `bosun-ok`, `risk-check`, `review-verdict`, `main-open`), `own_ci`,
   docs-only fast path, test-integrity, workflows that run the pinned release, hoard and
   rust-cache.
5. **Risk and review** (`/docs/risk/`): risk rules and classes (R-gov, R-ci, R-gate, R-secrets,
   R-deps, R-prod-write, two-verdict R-ci-review, built-in R-unseen), rules read from the base;
   verdicts (`verdict post`, `verdict comment` from cloud sessions, the verdict workflow); owner
   approvals (comment, review, `bosun approve` locally, the cloud proxy limit, signed approvals
   upcoming); kill switch; `bosun ask`.
6. **Releases and updates** (`/docs/releases/`): hoist release flow (release PR → merge →
   `detect`/`build` → `publish` in environment `release` with the owner's approval → tag and
   registries), first publish with `hoist release bootstrap` and the 2-day trusted-publisher
   window, pinning bosun after a release (R-gov PR); the splice update bot, the decisions
   channel and how to change a decision.
7. **Troubleshooting** (`/docs/troubleshooting/`): one entry per known pitfall with symptom,
   cause, fix and source: private repo on a free plan; approvals from cloud sessions; bot PRs
   with `GITHUB_TOKEN`; setup PR cannot get bosun checks; weekly update PR is R-gov; `bosun init`
   branch in cloud sessions; hull `template-rust` untagged (update path); 403 on commit statuses
   from cloud sessions; GraphQL refused (auto-merge fallback); doctor "cannot verify"; main-red
   blocks feature PRs; exit codes per tool.

## PR series

Each PR is green on its own (`pnpm run ci`, build, screenshots at 390 and 1280 px) and about
400 changed lines or fewer.

1. Spec, plan, `astro.config.mjs` change, `Docs.astro`, `docs.css`, `docs.ts`, nav link,
   `/llms.txt` Docs section, tests, the Overview page.
2. Getting started.
3. Tasks and CI.
4. Risk and Releases.
5. Playbook (`/docs/playbook/`, added on 2026-10-26 at the owner's request): how keelstackhq
   bootstraps and runs a repo with an orchestrator session, child sessions and waves. Parts that
   come only from the orchestrator's brief, not from a repo, are labelled **Practice**.
6. Troubleshooting, final TODO(copy) list in the project notes.

## Addendum (2026-10-27): docs for AI agents

The owner asked for docs an AI can be pointed at. Decisions:

- `/llms.txt` stays the index (llmstxt.org shape); its Docs section links each page's Markdown
  version and names `/llms-full.txt`. It also lists bosun, hoist and berth (`moreTools` in
  `src/data/tools.ts`) next to the three tools with cards.
- `/docs/<page>.md` (overview: `/docs/index.md`) and `/llms-full.txt` are static endpoints built
  from the same `.md` files as the HTML pages through `import.meta.glob` (`src/lib/docs-md.ts`): the
  frontmatter is dropped, the title and summary come from `src/data/docs.ts`, site-relative
  `/docs/...` links become absolute links to the Markdown pages, and the sources are listed at
  the end. No hand-copied duplicate.
- `/docs/` opens with a "For AI agents" note naming both files.
- Tests fail when a ready page is missing from `/llms.txt` or `/llms-full.txt`.

## Addendum (2026-10-27): docspack

- The docs are also a docspack documentation package, `@keelstackhq/docspack` (a name docspack
  discovers in a project's dependencies).
  `apps/site/scripts/docspack.mjs` runs after `astro build`: `docspack build --from` the generated
  `dist/docs/*.md` (the same Markdown as `/llms-full.txt`), a check that every page is the source
  of a chunk, then `npm pack` into `dist/docspack/`, served at keelstack.dev/docspack/.
- Version: a date (`2026.10.27`) in `apps/site/docspack.json`, bumped with doc changes. The file
  also holds `sources`, a sha256 of the packed Markdown; the build fails when the docs change and
  the version does not, since a consumer's lockfile pins the tarball's checksum.
- A repo installs it from the tarball URL (`pnpm add -D docspack@1.5.0 "@keelstackhq/docspack@<url>"`),
  then `docspack sync` and `docspack ask --json`. Only the current version is served, so a pinned
  install needs the npm package: `.github/workflows/docspack-release.yml` publishes each new
  version (trusted publishing, environment `release`) after a one-time owner bootstrap; a hull
  `docs/packs.json` entry follows once it is on npm.

## Addendum (2026-10-27): docspack as a workspace package

At the owner's request the package moved out of the site build into `packages/docspack`, released
the regular hull way: `version` in its `package.json`, `"hull": { "release": "ts-lib" }`, a human
pushes `docspack-v<version>`, and hull's template-owned `release.yml` and `release-plan.mjs`
(main `c355525`) publish it from the `release` environment with provenance. Its build copies
`apps/site/dist/docs/*.md` and runs `docspack build` with the settings in its `package.json`.
`docspack-release.yml` is gone. The site keeps serving its tarball, and Getting started keeps
installing it, until 0.1.0 is on npm; a follow-up PR then switches Getting started and `/llms.txt`
to npm and removes the site's packing (`apps/site/scripts/docspack.mjs`, `docspack.json`).
