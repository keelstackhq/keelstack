---
layout: ../../layouts/Docs.astro
---

How to bootstrap a repo with agents and keep it running: who plans, who builds, how work is cut
into waves, and what the owner still does. bosun's spec covers the mechanics (spawning, waves,
results, approvals). The rest is how keelstackhq runs its own repos today; it is not yet written
down in a repo, and each such part is marked **Practice**.

## The roles

### Orchestrator session

One Claude Code session that writes no code. **Practice:** it plans, starts one child session per
work stream with a self-contained prompt (the goal, why, the constraints and the report format),
checks in on the children, and merges pull requests once CI is green (see Merging below). It keeps a running state
log, one line per action, and tells the owner only about blockers.

In bosun terms this is the root session: it runs `bosun plan import`, the wave of plan tasks,
then `bosun spawn <epic>` once per area, and coordinates with `bosun wait` and `bosun status`.
bosun cannot call session tools itself, so `spawn` returns the `create_session` call in `next`;
the orchestrator makes the call and records the child with
`bosun spawn <epic> --record --from -`. Children report back with `bosun result`, which the
orchestrator reads with `bosun wait --child <session>`.

**Practice, check-ins:** after about 50 minutes, then about every 4 hours, with scheduled
check-ins, stopping after 3 check-ins in a row that found nothing new. A session that hit its
usage limit resumes after the reset; the orchestrator checks it again then.

**Merging.** **Practice:** the owner gives the orchestrator session standing authority to merge
a pull request once its required checks are green and any owner approval is in. It merges one at
a time, and only when a pull request's CI ran on the current `main` or its files are disjoint
from what merged since; otherwise it checks the combination locally (format, lint, tests) first.
Child sessions never merge. Without an orchestrator, use auto-merge (`bosun pr --auto-merge` in a
bosun repo) or the owner merges. Org decision D-006 still says agents never merge; a decisions
pull request that replaces it with this rule is in progress.

### Child sessions

One per independent work stream, each in its own repo or area. Inside a child:

- **Practice:** model routing with haiku to search, sonnet to implement and opus to review; test
  first; one task per commit. (bosun's own defaults differ for review: `read = "haiku"`,
  `build = "sonnet"`, `review = "sonnet"`, `hard = "opus"`.)
- Tasks of at most 400 changed lines (bosun).
- Never merge, tag, release or force-push.
- **Practice, owner-gated pull requests:** finish every review and fix first, then report the
  final head SHA exactly once and never push to it again: a push voids the approval, which binds
  to one SHA. Never merge `main` into a pull request after its approve line went to the owner.

Inside an area orchestrator, leaf tasks run as wave agents, each in its own berth workspace with
`BOSUN_SESSION=<session>/<task>`. bosun's limits keep the tree small: depth 2 (root, then area
orchestrators), at most 4 live sessions, 3 children per parent, 6 wave agents in parallel.

## Waves

**Practice:**

1. **Wave 1, the base.** Render the repo from hull and set up bosun (`bosun init`).
2. **Wave 2, parallel streams.** Two or three independent streams at most, on files that barely
   overlap.
3. **A trial in an external repo.** Run the result in a fresh repo the way a user would, write the
   findings down, and fix them in further waves. rust-canary's `docs/canary-findings.md` is such a
   trial.
4. **Close every wave with a docs pull request:** `docs/PROGRESS.md` and `CHANGELOG.md`.

bosun's own pieces apply inside each wave: `bosun wave plan <epic>` lists the tasks that can run
in parallel (no overlapping touch paths, dependencies merged), and the critical path of an item
stays at most 4 tasks.

## What the owner does

The owner sets the repo up, owns the risk decisions and approves high-risk pull requests;
everything else goes through bosun, GitHub and CI. **Practice:** the orchestrator collects open
decisions and asks each as one short question with a recommended option, and makes each
owner-only action as small as possible:

- **Approve a pull request:** a comment `/bosun approve <head sha>`, typed in the GitHub app or
  posted from the owner's own machine. Comments from Claude cloud sessions are re-attributed by the
  proxy and never count.
- **Release:** one approval of the `release` environment.
- **Repo settings:** in the GitHub settings, or `hoist apply`.
- **Human-only steps** are listed under "Blocked" in `docs/PROGRESS.md`, each with the exact
  action.

### Upcoming

Neither is merged in bosun yet:

- **Signed chat approvals.** The orchestrator holds a key enrolled in `.bosun/approvers.toml`; the
  owner says "approve" in the orchestrator chat, and the orchestrator posts the approve comment
  with a signature that `risk-check` verifies (bosun branch `feat/signed-approvals`).
- **Delegated decisions.** A delegate session would approve a high-risk change itself when an
  independent verdict approves with confidence of at least 0.9 and no blocker or major findings,
  while changes to the approval and risk system, secrets and production writes always go to the
  owner (bosun branch `feat/delegate-approvals`).

## Releases

**Practice:** a version-bump pull request; merging it starts the release workflow, which tags the
version and then waits for the owner's approval of the `release` environment before it publishes.
splice's release workflow does exactly that since 2026-10-26: a push to `main` whose version has
no tag yet creates the tag and releases it in the same run (a tag pushed with `GITHUB_TOKEN`
would start no other workflow). With hoist, `hoist release prepare` opens the version-bump pull
request, and the `publish` job, after the approval, creates the tag and publishes.
