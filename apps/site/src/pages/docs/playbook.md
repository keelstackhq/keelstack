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
bosun repo) or the owner merges. This is org decision D-008 (2026-10-26, supersedes D-006).

### Child sessions

One per independent work stream, each in its own repo or area. Inside a child:

- **Practice:** model routing with haiku to search, sonnet to implement and opus to review; test
  first; one task per commit. (bosun's own defaults differ for review: `read = "haiku"`,
  `build = "sonnet"`, `review = "sonnet"`, `hard = "opus"`.)
- Tasks of at most 400 changed lines (bosun).
- Never merge, tag, release or force-push.
- **Practice, owner-gated pull requests:** finish every review and fix first, then report the
  final head SHA exactly once and freeze the head: never push to it again, since a push voids the
  approval, which binds to one SHA. Never merge `main` into a pull request after its approve line
  went to the owner.
- **One push per pull request.** Run the local checks (in a bosun repo, every gate) before the
  push and do review rounds locally, so a pull request goes up once, done and green. Push again
  only for review feedback or a CI failure that cannot be reproduced locally. Do not edit a pull
  request's body or title just to retry CI. This is bosun's AGENTS.md rule and org decision
  D-007.
- **Practice, security-sensitive pull requests** get two independent reviewer sessions, not one.
- **Practice, blocked pushes:** a child session's permission classifier can refuse a push when
  the instruction to push came from another session (the orchestrator). The owner then confirms
  the push in that child session.

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

- **Approve a pull request:** in the orchestrator's chat (a signed approval, below), or with a
  comment `/bosun approve <head sha>` typed in the GitHub app or posted from the owner's own
  machine. An unsigned comment from a Claude cloud session is re-attributed by the proxy and never
  counts.
- **Release:** one approval of the `release` environment.
- **Repo settings:** in the GitHub settings, or `hoist apply`.
- **Human-only steps** are listed under "Blocked" in `docs/PROGRESS.md`, each with the exact
  action.

### Approvals by chat

Since bosun 0.2.1. Set up once: the orchestrator makes an ed25519 key
in its own container, and the owner approves the pull request that enrolls its public key in
`.bosun/approvers.toml` on GitHub (steps in [Risk and review](/docs/risk/)). Then, per pull request:

1. **Practice:** a child finishes reviews and fixes, reports the final head SHA once, and freezes
   the head.
2. The orchestrator runs `bosun approve --list` and shows the owner the ready pull requests (number,
   title, risk, files, head).
3. **Practice:** the owner answers in the chat, for example "approve 222".
4. The orchestrator runs `bosun approve 222@<head> --sign-key <pem>`. It posts
   `/bosun approve <head>` with a `bosun-sig ed25519 <key id> <signature>` line, and `risk-check`
   turns green. If the head moved, nothing is signed; the child reports the new head and the owner
   approves again.

Pull requests that change `.bosun/approvers.toml`, `governance/owners.toml` or a `CODEOWNERS` file,
and pull requests bosun cannot see whole, still need the owner's own GitHub approval.

### Delegated decisions

Since bosun 0.2.1 a repo can opt rules into `approval = "delegate"` and enroll a second key with
`role = "delegate"`. A delegate session then approves those high-risk changes itself
(`bosun approve <pr>@<head> --sign-key <pem> --as delegate`) when an independent verdict approves
with confidence of at least 0.9 and no blocker or major findings. Critical changes (workflows,
governance, the approval and risk system, secrets, production writes, an unseen diff, and in bosun
its own code) always go to the owner. bosun's own repo delegates R-ci, R-gate and R-deps (since
0.2.3). To stop delegation at once, remove the delegate key in a pull request the owner approves.

### Known gaps

The two-job design for ts-pnpm stacks (bosun #234) shipped in bosun 0.2.3. **Practice:** open
bosun issues among #235 to #238, as the orchestrator lists them: pnpm's `auditConfig`, cargo-deny
by path, and how review verdicts of several sessions are aggregated (#238).

## Releases

**Practice:** a version-bump pull request; merging it starts the release workflow, which tags the
version and then waits for the owner's approval of the `release` environment before it publishes.
splice's release workflow does exactly that since 2026-10-26: a push to `main` whose version has
no tag yet creates the tag and releases it in the same run (a tag pushed with `GITHUB_TOKEN`
would start no other workflow). With hoist, `hoist release prepare` opens the version-bump pull
request, and the `publish` job, after the approval, creates the tag and publishes.
