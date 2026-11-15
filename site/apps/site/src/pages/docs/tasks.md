---
layout: ../../layouts/Docs.astro
---

bosun keeps all coordination on GitHub: issues, labels, comments and pull requests. This page
describes how work is cut up, which states a task goes through, how sessions avoid stepping on
each other, and what stops work. This page follows bosun 0.15.0 (2026-11-14). Long details are in
bosun's specs: [lifecycle](https://github.com/keelstackhq/bosun/blob/834c4c83762a5b42fff05d7964092a43b1729ac3/docs/spec/04-lifecycle.md),
[spawning and waves](https://github.com/keelstackhq/bosun/blob/834c4c83762a5b42fff05d7964092a43b1729ac3/docs/spec/05-spawn.md) and
[cost and main-health](https://github.com/keelstackhq/bosun/blob/834c4c83762a5b42fff05d7964092a43b1729ac3/docs/spec/09-main-health.md).

## Every command speaks JSON

A session calls `bosun <command>` and acts on the one JSON object on stdout, also on errors:

```json
{
  "version": 1,
  "command": "claim",
  "ok": false,
  "exit_code": 3,
  "data": {},
  "error": {
    "kind": "conflict",
    "message": "crates/x.rs is leased by session_02 until 10:30Z",
    "fix": "pick another task with `bosun wave plan`, or wait for the lease to end"
  },
  "next": []
}
```

- `error.fix` is never empty; agents do what it says and never work around a refusal.
- `next` lists tool calls the session must make itself, such as `create_session` or
  `send_message`. bosun cannot call them; the session makes the call and passes the result back
  with `--from -`.
- A GitHub 5xx ends a command with exit 1 and `error.kind` `upstream`; every other command stops
  with a `fix` after 120 s of GitHub calls.
- After a refusal with exit 4, 5 or 6 (3 from `claim`), the hook shows its fix on the session's
  next tool call; while halted it refuses writes and pushes. The same bosun call five times in a
  row with no other tool call between is refused with exit 6.
- `bosun schema [<command>]` prints the envelope and each command's `data` as JSON Schema
  (`--format ts` for TypeScript declarations).

| Exit code | Name        | Meaning                                                                       |
| --------- | ----------- | ----------------------------------------------------------------------------- |
| 0         | ok          | The command did its work.                                                     |
| 1         | error       | An unexpected failure (API, I/O, bad config).                                 |
| 2         | usage       | Bad arguments.                                                                |
| 3         | conflict    | A claim or path overlaps a live lease, or another orchestrator owns the area. |
| 4         | halted      | The kill switch, main-red or process-red stops the command.                   |
| 5         | needs-human | The command needs an owner decision.                                          |
| 6         | limit       | A cap or a budget refuses the command.                                        |

## Plans, items, tasks and UCs

Work enters as a plan file in `plans/` ([format](https://github.com/keelstackhq/bosun/blob/main/docs/plan-format.md)).
`bosun plan import <file>` turns it into issues:

| Plan part                                 | Becomes                                                  | Limits                                                           |
| ----------------------------------------- | -------------------------------------------------------- | ---------------------------------------------------------------- |
| Item (`## M1: Title`)                     | One epic issue; one area, one orchestrator               | Critical path of at most 4 tasks                                 |
| Task (`- T1: Title`, with `touch:` globs) | One issue linked to its epic                             | `size:` is a plan, not a cap; a `risk: high` task over 100 warns |
| Plan task (`<item>/P`)                    | One issue per item, ready at once, touches the plan file | —                                                                |
| "Done when" line                          | One use case `UC-<item>-<n>`, listed in the epic         | One line, one test                                               |

A second import of the same plan creates nothing new. Tasks become ready only on the import run
after their item's plan task merged. New work enters only as a plan amendment: `bosun intake`
opens a plan task that asks for it, and after the amendment merges, `bosun plan amend <file>`
imports the new work and closes what the plan dropped as "not planned". Since 0.4.0,
`bosun plan pr <plan> "<why>" --source <s>` opens the intake, claims the plan file and opens the
amendment's pull request in one step. It is for amendments only: the first plan goes in through
`bosun init --plan`.

Item and task fields are in bosun's
[plan format](https://github.com/keelstackhq/bosun/blob/834c4c83762a5b42fff05d7964092a43b1729ac3/docs/plan-format.md):
an item takes `area:`, `depends:`, `shape:` (`single`, `split` or `auto`) and `budget_usd:`; a task
takes `touch:`, `depends:`, `size:`, `kind:`, `risk:`, `allow:` and `done-by: #N` (a task whose PR
already merged). A PR over twice the planned `size:` of its tasks gets a drift note in the review
pack. `bosun plan check <file>` lists every plan error without GitHub, and warns when two tasks
touch overlapping files with no dependency between them, or when a task touches a crate's `src/`
but none of its `tests/`.

A task may carry `allow:` (`delete-test`, `skip`, `only`, `fewer-asserts`) for the changes
test-integrity would otherwise refuse ([CI and gates](/docs/ci/#test-integrity)). It reaches the
task only from the plan as merged on the default branch, which the owner approved, never from an
unmerged local plan.

## Labels and states

| Prefix          | Values                                                                                  |
| --------------- | --------------------------------------------------------------------------------------- |
| `bosun:state/`  | planned, ready, claimed, building, review, fixing, queued, merged, blocked, needs-human |
| `bosun:kind/`   | feature, plan, mainfix, bookkeeping, flake, process-fix                                 |
| `bosun:area/`   | one per area                                                                            |
| `bosun:source/` | plan, slack, lesson, flake, human                                                       |

| From                     | To          | When                                                                                                         |
| ------------------------ | ----------- | ------------------------------------------------------------------------------------------------------------ |
| planned                  | ready       | its plan task merged                                                                                         |
| ready                    | claimed     | `bosun claim`; lease written                                                                                 |
| claimed                  | building    | first commit pushed                                                                                          |
| building                 | review      | `bosun pr`; gates green                                                                                      |
| review                   | fixing      | verdict requests changes                                                                                     |
| fixing                   | review      | fix pushed                                                                                                   |
| review                   | queued      | `bosun pr --ready` (alias `--auto-merge`) marked the draft ready and turned auto-merge on                    |
| queued                   | merged      | GitHub merged it (auto-merge), never an agent                                                                |
| building, review, fixing | merged      | a human merged the pull request (since bosun 0.2.1)                                                          |
| any                      | blocked     | a stop rule, conflict or budget (a child's `bosun result --outcome blocked` sets it on the epic since 0.2.1) |
| any                      | needs-human | high risk, an owner question, tier escalation used up                                                        |
| needs-human              | ready       | `bosun answer --chat` counted an owner's signed answer (not while halted or main is red)                     |

PRs are draft-first. `bosun pr <issue>` opens a draft, and CI jobs skip drafts. `bosun pr <issue> --ready`
needs the local HEAD pushed and a local gate stamp for every ready gate (run `bosun gate <name>`
for each), then marks the PR ready and turns on auto-merge at once; it exits 5 with `already_clean`
when the PR is already mergeable, and bosun never merges. `bosun hold` and `bosun ask` put the PR
back to draft. A PR with no task (a governance PR) goes through `bosun pr --pr <n>`. A single-shape
item bundles its ready tasks into one PR: `bosun wave plan` lists the `bundle`, and
`bosun pr <lead> --bundle <issue>` closes them together; a gated task (`risk: high` or a risk path)
is never bundled. `bosun pr` also exits 3 when the PR changes files outside the session's lease
touch.

Only bosun commands change a state label. The PreToolUse hook (label-lock) guards against
accidents and is not a sandbox. Read-only commands and heredoc prose that mention a state label
pass; any other command naming one is refused (in 0.2.0 even read-only ones were). Since 0.2.1 a
task that a person merged by hand moves to merged too (`bosun pr`, `bosun reap` and
`bosun result --outcome merged --pr N` see it); in 0.2.0 it stayed in `review`.

## Claims and leases

`bosun claim <issue>` writes a lease as an issue comment:

```json
{
  "bosun": "lease",
  "session": "session_...",
  "origin": "spawned",
  "expires": "2026-10-22T10:30:00Z",
  "touch": ["src/core/lease.rs"]
}
```

- The lease lasts 30 minutes. `bosun heartbeat` extends it; run it at least every 20 minutes.
- `bosun release <issue>` ends a lease when you stop before the task is done.
- `bosun reap` (run by the optional janitor Routine, hourly) ends expired leases and sets the
  task back to ready. A dead session blocks a task for at most about 90 minutes.
- After a container restart, `bosun resume` renews the session's leases and lists, per held task,
  the state, the open pull request, its branch and the next action.
- `bosun claim <issue> --touch <glob>` on a task you hold widens the lease after the same overlap
  check.
- `bosun hold <issue> "<what the owner must do>"` records a human step nobody listed: the owner's
  checklist gets a box and the task goes back to ready, instead of the agent stopping to ask.
- Lease and registry records count only from commenters with write access.

Three layers keep sessions apart:

1. **Registry.** `bosun join` registers each session (origin spawned, slack, local or routine) on
   the registry issue. The SessionStart hook joins for you.
2. **Claim check.** `bosun claim` exits 3 when the task's touch paths overlap a live lease, or
   another orchestrator owns the area.
3. **Edit hook.** The PreToolUse hook blocks Edit and Write on a path that another live session
   leased.

Wave agents run inside their orchestrator's session, so each one sets
`BOSUN_SESSION=<session>/<task>` in its berth workspace; `S/x` and `S/y` then conflict like two
sessions.

## Sessions, spawns and waves

An epic gets one session, its area orchestrator. Leaf tasks run as wave agents inside it, each in
its own workspace from `berth up <task>`.

`bosun spawn <epic>` checks, in this order, and returns the `create_session` call in `next`:

1. Main is not red (mainfix work is exempt) and process is not red (mainfix and process-fix work
   is exempt).
2. Today's spend is below 80 % of the daily budget.
3. The new session's depth is at most `max_depth`.
4. Live sessions (all origins) are fewer than `max_live_sessions`.
5. The parent has fewer children than `max_children`.
6. No conflict with a live lease or area owner (exit 3).
7. Claim the epic.

After the call, `bosun spawn <epic> --record --from -` registers the child. A refused check is
exit 6 with the check in the JSON.

- `bosun wave plan <epic>` lists the tasks that can run in parallel now: no overlapping touch
  paths, all dependencies merged, at most `waves.max_parallel`.
- `bosun wait --pr <n>` (or `--issue`, `--child`) polls GitHub with backoff inside one call
  (5 s doubling to 60 s, default timeout 30 min) and returns the change; `--once` reads the state
  once and returns at once. It renews the caller's leases and registry record while it polls,
  reaps expired leases and members, and `--pr` shows only the newest run of each check (or
  `no checks yet`).
- A child reports with `bosun result <epic> --outcome merged|blocked|needs-human|partial [--pr N]`,
  an issue comment the parent reads with `bosun wait --child`. The result lists the epic's merged
  tasks for every outcome, `partial` hands the epic back as ready, and `data.ready_to_merge` lists
  green PRs GitHub cannot auto-merge, with a `send_message` to the parent (the orchestrator merges
  those). A spawned child that holds a live lease must post a result before it stops: the Stop
  hook blocks it once.
- `bosun wave tick <epic>` spawns the ready wave entries for you when the mode allows it
  (`spawn_auto`; see Modes below).
- A root hands over with `bosun handoff` (at most 40 lines), then `bosun spawn --root --handoff`
  and `--record`; the next root runs `bosun resume --handoff`. This happens after
  `waves.relay_waves` waves (default 6). A child tells its parent a PR is ready with
  `bosun handoff-pr <pr>`.
- `bosun msg <session> <text>` returns a `send_message` call; text over 500 characters is
  refused.

Defaults in `.bosun/limits.toml`, read from the default branch (a local copy never raises a cap):

```toml
[sessions]
max_depth = 2          # root -> area orchestrator -> no further sessions
max_live_sessions = 4
max_children = 3
[waves]
max_parallel = 6
[turns]
build = 100
build_cap = 150
review = 40
fix_round = 40
[models]
read = "haiku"
build = "sonnet"
review = "sonnet"
hard = "opus"
[budget]
daily_usd = 150
per_task_usd = 25
```

Most values are estimates; their evidence is in
[defaults.md](https://github.com/keelstackhq/bosun/blob/main/docs/defaults.md). With
`max_live_sessions = 1` the root session does all the work in waves.

## Stop rules

- The same failure signature 3 times in a row: the task goes to blocked.
- A fix round with 0 changed lines: blocked at once.
- On blocked, bosun escalates one model tier (haiku, sonnet, opus). When opus is used up, the
  task goes to needs-human (exit 5).

`bosun fail <issue> --signature S [--changed-lines N] [--model M]` records a failure and applies
these rules. Each stop also opens or hits a lesson.

## Main-red and process-red

- The main-health workflow runs on each push to `main` and weekly (nightly before bosun 0.2.2). A failure opens one
  `bosun:main-red` issue; a green run closes it.
- While main is red, the required check `main-open` fails every pull request that is not a
  mainfix (label `bosun:kind/mainfix` on the pull request or an open linked issue), and spawn,
  claim and wave plan pause feature work.
- `bosun main-triage --good <rev>` bisects the merged pull requests and names the first bad one.
  The default fix is `bosun revert <pr>`, which opens a mainfix pull request; a forward fix is
  allowed only when it is smaller.
- Every escaped defect (a new main-red, a reverted PR, or `bosun gate-missed`) opens one "Which
  gate missed this?" issue labelled `bosun:process-fix`.
- Process-red is an open issue labelled `bosun:process-red`, set by a lesson triaged as
  blocking. Like main-red, it pauses everything except mainfix and process-fix work.

## Money

- At 80 % of `daily_usd`, spawn and claim refuse new work (exit 6); mainfix is exempt.
- At 100 %, `bosun budget check` (run by the janitor, if enabled) interrupts the live sessions.
- With a mode whose `budget_feature` knob is on, an epic also has a feature budget (`budget_usd:`
  on the plan item, else its tasks times `per_task_usd`): claim and spawn warn at 80 %, and under
  `ask` exit 5 at 100 % until the owner raises it in a plan PR or with a signed digest reply
  (`bosun budget raise`). It needs `.bosun/registry` on the default branch.
- `bosun budget record <issue>` (or `--pr <n>` for PRs without a task) records a session's spend;
  `bosun budget report --month YYYY-MM` lists the spend of every PR of a month.
- A task whose recorded spend passes `per_task_usd` goes to blocked and opens a lesson.
- The day resets at 00:00 UTC. Only the owner raises a budget, with a change to
  `.bosun/limits.toml` (high risk).
- `bosun progress [--days N]` reports cost per merged task, the headline metric, and
  `cost_per_held_task_usd` that leaves out tasks whose PR a merged `bosun/revert-<n>` PR undid.

## Lessons

A lesson is a failure that is not fixed in code yet. bosun opens one as an issue (`bosun:lesson`)
on a stop rule, a blocked task, a flake, a repeated review ask or a task over budget; the same
signature adds a hit instead of a new issue. The curator Routine triages each one with
`bosun lesson triage <issue> --as blocking|mechanizable|advice`: blocking sets process-red,
mechanizable opens a process-fix task, advice adds an entry to `docs/lessons.toml` (at most 40,
8 of them global). `bosun lesson add` needs `--evidence #N` and a rule of at most 160 characters. A review pack carries only the lessons whose scope matches the task's paths, plus the global
ones.

## Modes

An optional `[mode]` table in `.bosun/bosun.toml` (`gatekeeper`, the default, `steady`, `booster`
or `autonomous`) sets how much bosun does for you: gates before push (`push_gates`), self-heal
(`bosun heal`: a notice or a fixer session for a red PR, a revert PR for a red `main`), a daily
digest (`bosun digest`), auto spawn (`bosun wave tick`) and a brake that turns the helpers down
after a red `main`, reverts or a high red-CI rate. `bosun mode` prints the knobs in force and
`bosun metrics` the numbers. A less strict mode is an owner-approved R-weaken change; budgets and
limits are never part of a mode. `bosun migrate --to <mode>` opens the one PR for a move, and
`--finish` after the merge. Details: [modes spec](https://github.com/keelstackhq/bosun/blob/834c4c83762a5b42fff05d7964092a43b1729ac3/docs/spec/18-modes.md) and bosun's
[quickstart](https://github.com/keelstackhq/bosun/blob/834c4c83762a5b42fff05d7964092a43b1729ac3/docs/quickstart.md).
