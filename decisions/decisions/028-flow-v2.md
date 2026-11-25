# D-028: Flow v2, progress over gates

- Date: 2026-11-25
- Scope: all
- Status: accepted
- Supersedes: —

## Context

This decision amends D-021 (one PR per requirement), D-022 and D-027 (two clean verdicts become one; only blockers block); it supersedes none.

On 2026-11-25 the owner decided in chat: progress matters more than gates. The two-verdict quorum
(D-022), blocking major findings, a red main that pauses all work, and many small PRs each
waiting on reviews made agents spend their time on process, not on the requirement. The checks
that guard security, data, main and releases stay; the rest must stop holding work back.

## Decision

- One requirement is one issue, one branch and one big PR. Related follow-ups (see below) are
  collected into the PR of the area they touch. No line cap (D-021 stays).
- Work runs in the session with `implementer` subagents by default. A child session starts only
  for a separate requirement, and the orchestrator checks on it every 20 to 30 minutes.
- One clean, independent verdict on the exact head is enough: approve, confidence >= 0.9, from
  a reviewer session that is neither the author, the requester nor the signer. bosun reads it
  from `[review] verdicts_needed = 1` in `.bosun/bosun.toml` at the PR's base (default 2, so
  repos that do not set it keep D-022).
- Only blocker findings block: security, data loss, a broken main, a broken release. Major,
  minor and nit findings do not. `[review] block_on = "blocker"` sets it (default `major`). Every
  non-blocking finding goes to the one rolling open issue labelled `bosun:followups` (`bosun
  followups add <pr> --from <verdict>`), and the next PR in that area picks the items up and ticks
  them.
- main-open warns instead of blocking: `[main_health] block = false` (default `true`). A red main
  still gets a `bosun:kind/mainfix` PR first, but other work keeps moving: claims, waves, spawns
  and PR merges are not stopped by it.
- The orchestrator merges as soon as the required checks are green. The signer (quorum
  signature) approves every PR it may; the owner still approves, batched in one message, the PRs
  the signer refuses (`.github/**`, scripts, R-ci, secrets, environments) and keeps the `release`
  environment tap.
- keelstackhq/keelstack turns on `verdicts_needed = 1`, `block_on = "blocker"` and
  `[main_health] block = false` in `.bosun/bosun.toml`.

## Consequences

- The CI signer keeps covering normal PRs: only build and test-runner config (`*.config.*`, `.pnpmfile.*`, nextest, `tox.ini`, `tsconfig*.json`, toolchain, `.cargo/config`, `Makefile`, `build.rs`, and the steering keys of `Cargo.toml`, `pyproject.toml`, `package.json`) is a CI input that sends a PR to the orchestrator key or the owner. Test files are not: editing tests is normal and test-integrity guards weakening. Lockfiles are not either; R-deps judges dependencies.
- D-022's "two approving verdicts (>= 0.85, no blocker/major) from two different reviewer
  sessions" and D-027's "two clean verdicts" become "one clean verdict, no blocker" in repos that
  set the keys; the rest of both decisions stands (trust root, floor, halt, secrets). No earlier
  row is superseded as a whole, so none is removed from the index.
- D-021's "one PR per epic or a parallel split" becomes one PR per requirement; its no-cap rule
  and chunked review stay.
- Other repos keep today's flow until they set the keys. A key that an older pinned bosun does
  not know is a config error there, so set it only once `.bosun/version` pins a release that
  reads it.
- Follow-ups are not blocking, so an unread follow-up issue is the cost: `bosun followups list`
  is how a session reads it before it starts in an area.
