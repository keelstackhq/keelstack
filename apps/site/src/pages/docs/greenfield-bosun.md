---
layout: ../../layouts/Docs.astro
---

A new repo, rendered from hull v0.5.0, that bosun runs from the first task. This page shows the
files you end up with, where the product spec and the plan go, who does which step, and how the
plan turns into work. It follows bosun 0.3.0 (2026-10-29). Every owner step, with the check that
verifies it, is in [Start a project](/docs/start-project/); the details of each step are in
[Getting started](/docs/getting-started/).

## The repo after `bosun init`

On top of what hull renders (see [Getting started](/docs/getting-started/)), `bosun init` adds
the bosun channel:

```text
.bosun/bosun.toml          project, stacks, review mode
.bosun/limits.toml         caps and budget
.bosun/risk.toml           risk rules
.bosun/version             the pinned bosun release (R-gov)
governance/owners.toml     the owners whose approval counts
.github/CODEOWNERS
.github/workflows/         bosun.yml, risk.yml, verdict.yml, main-open.yml, main-health.yml
AGENTS.md, REVIEW.md       bosun's sections (agent rules, review rules for the verdict)
docs/setup-script.sh       the cloud environment's setup script
.splice/channels/bosun.toml   the channel record, kept fresh by the splice update bot
plans/<name>.md            the plan: the only input bosun reads (`--plan` copies yours here)
```

- In a hull repo, hull owns `ci.yml`, `CLAUDE.md` and `.claude/settings.json`, and its
  `splice.yml` stays (the channel's copy is a seed that `bosun init` skips when one exists). bosun's aggregate
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
  `bosun plan check` (0.3.0) accepts with no warnings:

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
  touch: apps/api/src/notes/store.ts, apps/api/src/notes/create.ts, apps/api/src/notes/create.test.ts
  size: 250
- T2: List and delete endpoints
  touch: apps/api/src/notes/list.ts, apps/api/src/notes/delete.ts, apps/api/src/notes/list.test.ts
  depends: T1
  size: 200

### Done when

- UC-N1-1: POST /notes returns 201 and the note, and GET /notes lists it.
- UC-N1-2: DELETE /notes/:id removes the note, and a second DELETE returns 404.
```

Each `## <ID>: <title>` item needs an `area:` and becomes one epic. Each task needs a `touch:` and
stays at most 400 lines (`size:`); `kind:` and `risk: high` are optional. Every item needs at least
one `UC-<ID>-<n>` line that a test can check. Human steps a plan needs go in a
`## Prerequisites` section (`bosun prereqs` lists them up front).

## The order

1. **Owner:** create the repo and install the [Claude GitHub App](https://github.com/apps/claude).
2. **Agent:** render hull v0.5.0
   (`pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template <repo> --ref v0.5.0 --piece …`),
   then run the `ready` skill: it asks the onboarding questions as forms; answer `bosun` with `yes`.
3. **Agent:** `bosun init --stack rust|ts-pnpm --plan <file> --owner <login>`. It opens one setup
   pull request, which is R-gov.
4. **Owner:**
   - check `governance/owners.toml`;
   - approve and merge the setup pull request by hand: while `main` has no bosun workflows,
     `risk-check`, `review-verdict` and `main-open` cannot run on it;
   - then run `hoist apply` (branch rules, auto-merge, the required checks);
   - create the environment `splice-bot` (branch `main` only) with `SPLICE_BOT_TOKEN`;
   - `gh label create bosun:halt --color B60205`;
   - paste `docs/setup-script.sh` into the cloud environment ([Cloud environment](/docs/cloud/));
   - create the Routines janitor, curator and notify (`bosun doctor` prints their prompts).
5. **Optional, owner:** add the orchestrator's public key to `.bosun/approvers.toml`. The owner
   approves that pull request on GitHub; after it, approvals come as chat forms
   ([Risk and review](/docs/risk/#signed-approvals-from-chat)).
6. **Verify:** `bosun doctor`, then `bosun doctor --remote` (owner); `ready` exits 0.

## From plan to work

```bash
bosun plan check plans/notes.md    # offline: the format rules and the overlap warnings
bosun plan import plans/notes.md   # an epic per item, an issue per task, a <ID>/P plan task per item
bosun wave plan <epic>             # the tasks of an epic that can run in parallel now
bosun spawn <epic>                 # the checks, the epic claim and the create_session call
bosun wait --pr <pr>               # or `bosun status`: sessions, leases, tasks by state
```

The plan tasks are ready at once; the owner approves the plan pull requests. Tasks become ready
on the import run after their plan task merged. New work enters as an amendment: a plan pull
request, then `bosun plan amend plans/<name>.md`. Details: [Tasks and sessions](/docs/tasks/) and
the [Playbook](/docs/playbook/).

## A first session prompt

bosun documents no first prompt. **Practice:** this is ours; it uses only the commands above.

```text
Run the ready skill and ask me the onboarding questions as forms. If plans/ has no plan,
write plans/<name>.md in bosun's plan format from the spec in docs/superpowers/specs/, run
bosun plan check, and open it as a plan pull request. Once it is merged, run bosun plan import,
run the plan tasks, then bosun spawn one session per epic (area). Coordinate with bosun wait
and bosun status, and ask me through forms whenever a step needs the owner.
```
