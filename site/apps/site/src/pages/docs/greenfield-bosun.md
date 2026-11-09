---
layout: ../../layouts/Docs.astro
---

A new repo, rendered from hull v0.8.0, that bosun runs from the first task. This page shows the
files you end up with, where the product spec and the plan go, who does which step, and how the
plan turns into work. It follows bosun 0.11.0 (2026-11-09): the file tree and the example plan come from running
`bosun init --no-pr` and `bosun plan check` 0.11.0 on a repo rendered from hull v0.8.0, and every
command and flag below was checked against `bosun --help`, each subcommand's `--help` and bosun's
docs. Every owner step, with the check that
verifies it, is in [Start a project](/docs/start-project/); the details of each step are in
[Getting started](/docs/getting-started/).

## The repo after `bosun init`

On top of what hull renders (see [Getting started](/docs/getting-started/)), `bosun init` adds
the bosun channel:

```text
.bosun/bosun.toml          project, stacks, review mode, [approvals], optional [slack]
.bosun/limits.toml         caps and budget (seed)
.bosun/risk.toml           risk rules (seed)
.bosun/session-rules.toml  session rules (seed)
.bosun/intake.toml         the owner's upfront answers (seed, written once; `bosun intake check`)
.bosun/version             the pinned bosun release (R-gov)
.bosun/fold-fragments.sh   folds changelog.d/ fragments (run it with bash)
governance/owners.toml     the owners whose approval counts (GitHub logins, Slack IDs)
hoist.toml                 the repo settings and required checks for `hoist apply`
.npmrc                     ts-pnpm stack config (seed)
deny.toml, rust-toolchain.toml   rust stack only (seed)
.github/CODEOWNERS
.github/workflows/         bosun.yml, risk.yml, verdict.yml, main-open.yml, main-health.yml
AGENTS.md, REVIEW.md       bosun's sections (agent rules, review rules for the verdict)
docs/setup-script.sh       the cloud environment's setup script
.splice/channels/bosun.toml   the channel record, kept fresh by the splice update bot
plans/<name>.md            the plan: the only input bosun reads (`--plan` copies yours here)
```

Descriptions follow bosun's [adopt guide](https://github.com/keelstackhq/bosun/blob/01770b8/docs/adopt-existing-repo.md)
(section 3, "What lands in the repo") and its
[layout spec](https://github.com/keelstackhq/bosun/blob/01770b8/docs/spec/03-layout.md). For
`session-rules.toml` see the bosun docs; the intake schema is in the layout spec. A seed is
written once and then belongs to you; an owned file is replaced by each channel update.

- In a hull repo, hull owns `ci.yml`, `CLAUDE.md` and `.claude/settings.json`, and its
  `splice.yml` stays. `hoist.toml`, `splice.yml`, `deny.toml`, `rust-toolchain.toml` and the other
  channel seeds are skipped when the repo already has them; `bosun init` adds bosun's required
  checks to an existing `hoist.toml`. bosun's aggregate
  check is then `bosun-ok`, next to hull's `ci-ok`, and the bosun hooks come with hull.
- `docs/lessons.toml` is bosun's lessons store (spec §3); `docs/agents/org-decisions.md` comes
  from the decisions channel (D-004), not from bosun.
- Tasks, waves and claims are not files. They live on GitHub: one issue per epic and task, the
  labels `bosun:state/*`, `bosun:area/*` and `bosun:kind/*`, and the registry issue.

## Where the spec and the plan go

bosun reads only `plans/*.md`. It has no convention for a brief or a product spec.

- **Practice:** keep the product spec where hull keeps specs,
  `docs/superpowers/specs/<date>-<name>.md`, and list it in the `touch:` of the tasks that update
  it.
- The plan goes in `plans/<name>.md`, in bosun's format
  ([plan-format.md](https://github.com/keelstackhq/bosun/blob/main/docs/plan-format.md)). Its
  `project:` must equal `project` in `.bosun/bosun.toml`. A minimal plan, which
  `bosun plan check` (0.11.0) accepts with no warnings. A task that touches `apps/api/src/` also
  lists a test under `apps/api/tests/`, or `plan check` warns:

```markdown
# Plan: notes

project: notes

## N1: Notes API

area: api
depends: seed

Create, list and delete notes over HTTP. The product spec is
docs/superpowers/specs/2026-10-29-notes.md.

### Tasks

- T1: Note store and the create endpoint
  touch: apps/api/src/notes/store.ts, apps/api/src/notes/create.ts, apps/api/tests/notes_create.test.ts
  size: 250
- T2: List and delete endpoints
  touch: apps/api/src/notes/list.ts, apps/api/src/notes/delete.ts, apps/api/tests/notes_list.test.ts
  depends: T1
  size: 200

### Done when

- UC-N1-1: POST /notes returns 201 and the note, and GET /notes lists it.
- UC-N1-2: DELETE /notes/:id removes the note, and a second DELETE returns 404.
```

The real output of `bosun plan check plans/notes.md` (0.11.0, exit 0, reformatted for reading), with no warnings:

```json
{
  "command": "plan check",
  "data": {
    "file": "plans/notes.md",
    "items": [
      {
        "area": "api",
        "gated": [],
        "id": "N1",
        "shape": "single",
        "shape_reasons": ["tasks form a chain of 2: no parallel gain"],
        "size": 450,
        "tasks": 2,
        "title": "Notes API",
        "ucs": 2
      }
    ],
    "project": "notes",
    "title": "Plan: notes",
    "warnings": []
  },
  "error": null,
  "exit_code": 0,
  "next": [],
  "ok": true,
  "version": 1
}
```

Each `## <ID>: <title>` item needs an `area:` and becomes one epic. Each task needs a `touch:` and
stays at most 400 lines (`size:`); `kind:` and `risk: high` are optional. Every item needs at least
one `UC-<ID>-<n>` line that a test can check. Human steps a plan needs go in a
`## Prerequisites` section (`bosun prereqs` lists them up front).

## The order

1. **Owner:** create the repo and install the [Claude GitHub App](https://github.com/apps/claude).
2. **Agent:** render hull v0.8.0
   (`pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template <repo> --ref v0.8.0 --piece …`,
   or `//template-rust`; both ship `ready`),
   then run the `ready` skill: it asks the onboarding questions as forms; answer `bosun` with `yes`.
3. **Agent:** write the plan first: turn the brief into `plans/<name>.md` (the spec stays in
   `docs/superpowers/specs/`). This first plan goes in through `bosun init`:
   `bosun init --stack rust|ts-pnpm --plan <file> --owner <login>` (bosun 0.11.0; `--stack` is
   required unless you pass `--intake <file>`, which derives `--stack`, `--owner` and `--plan` from the
   owner's intake file). It opens one setup pull request, which is R-gov.
4. **Owner:**
   - check `governance/owners.toml`;
   - approve the setup pull request's head (the Claude Code form; `/bosun approve <sha>` as the
     fallback) and merge it: while `main` has no bosun workflows, `risk-check`, `review-verdict`
     and `main-open` cannot run on it;
   - then run `hoist apply` (branch rules, auto-merge, the required checks);
   - create the environment `splice-bot` (branch `main` only) with `SPLICE_BOT_TOKEN`;
   - `gh label create bosun:halt --color B60205 --repo <owner>/<repo>` (the kill switch; `bosun doctor`
     prints this exact command while it is missing);
   - paste the output of `bosun setup-snippet` once into the cloud environment ([Cloud environment](/docs/cloud/));
   - create the Routines janitor (hourly) and curator (daily), and with `[slack]` notify (hourly):
     `bosun doctor` prints their prompts in `data.routines`; then set `BOSUN_ROUTINES_OK=1`.
5. **Optional, owner:** enroll an approval key: `bosun keygen --enroll` makes it and adds its public
   half to `.bosun/approvers.toml` (run it only on the machine that signs; if the orchestrator signs
   for you, its session runs it and opens a pull request with the file). The owner approves that
   pull request on GitHub; after it, approvals come as chat forms
   ([Risk and review](/docs/risk/#signed-approvals-from-chat)).
6. **Verify:** `bosun doctor`, then `bosun doctor --remote` (owner); `ready` exits 0.

Once the setup pull request is merged, agents turn on auto-merge when they open a pull request
(`bosun pr <task> --ready`: it needs the pushed HEAD and the `bosun gate` stamps, marks the draft
pull request ready and turns on auto-merge; `--auto-merge` is an old alias), and the ruleset merges
it once the checks pass. The owner merges by hand only when GitHub reports a pull request as
already clean.

## From plan to work

```bash
bosun plan check plans/notes.md    # offline: the format rules and the overlap warnings
bosun plan import plans/notes.md   # an epic per item, an issue per task, a <ID>/P plan task per item
bosun wave plan <epic>             # the tasks of an epic that can run in parallel now
bosun spawn <epic>                 # the checks, the epic claim and the create_session call
bosun wait --pr <pr>               # or `bosun status`: sessions, leases, tasks by state
```

The plan tasks are ready at once; the owner approves the plan pull requests. Tasks become ready
on the import run after their plan task merged. When the plan file on the default branch is already
byte-identical to the one imported, import closes the plan tasks (`--keep-plan-tasks-open` keeps
them open); `bosun plan import --dry-run` shows what it would create. New work enters as an amendment:
`bosun plan pr plans/<name>.md "<why>" --source human` opens the intake and the plan pull request
in one step, then, after it merges,
`bosun plan amend plans/<name>.md`. `allow:` on a task is optional, only for tasks that must
delete or skip tests (`delete-test`, `skip`, `only`, `fewer-asserts`). A child that hands an epic
back unfinished reports `bosun result <epic> --outcome partial`. Details: [Tasks and sessions](/docs/tasks/) and
the [Playbook](/docs/playbook/).

## A first session prompt

bosun documents no first prompt. **Practice:** this is ours; it uses only the commands above.

```text
Run the ready skill and ask me the onboarding questions as forms. If plans/ has no plan,
write plans/<name>.md in bosun's plan format from the spec in docs/superpowers/specs/, run
bosun plan check, then bosun init --stack <stack> --plan plans/<name>.md --owner <my login>
and tell me the setup pull request to approve and merge. Once it is merged, run bosun plan
import, run the plan tasks, then bosun spawn one session per epic (area), with auto-merge on
every pull request (bosun pr <task> --ready). Coordinate with bosun wait and bosun status; for
new work use bosun plan pr.
Ask me through forms whenever a step needs the owner.
```
