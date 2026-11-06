---
layout: ../../layouts/Docs.astro
---

How keelstackhq works with coding agents, in eight principles, and which part of each one bosun
checks today. The mapping follows bosun's operating model (`docs/operating-model.md`, bosun pull
request #421); a principle that bosun does not check yet says so.

## Words used here

- **Boundary files**: the files that decide what passes and who approves: risk rules, limits and
  budgets, governance, workflows and gate code. Agents may propose changes to them; humans
  approve them.
- **Loop**: one cycle of change and check. The loops nest: local gates, then CI, then checks of
  `main`.
- **Risk rule**: a path or content pattern in `.bosun/risk.toml` that makes a pull request high
  risk. Each rule names who may approve: the owner, a **delegate** (one Claude session with its
  own signing key, plus one clean review verdict), or **two verdicts** from two reviewer sessions.
- **Mainfix**: a pull request that repairs a red `main`; it may merge while `main` is red.

## The manifest

1. **Tend the codebase; it is not a factory.** Every change stays and becomes the ground for the
   next one, so types, module limits, gates, CI and docs exist to keep that growth healthy.
2. **Agents write the code, within limits.** They build features and may improve the code and
   its structure, but they do not decide the boundary files.
3. **Humans own the limits.** People decide what to build, how we know it is good, who approves
   what, which checks run, what agents may touch and how much they may spend.
4. **Widen the limits on measured evidence.** Agents get more room when numbers show it is safe,
   never on confidence and never because agents agree with each other.
5. **Scrutiny follows blast radius and reversibility.** A change that reaches far or is hard to
   undo waits for a human; a small one that rolls back cleanly does not.
6. **Every escaped bug teaches a check.** A defect that reached `main` ends as a test, a gate or a
   rule, so the same mistake is caught earlier next time.
7. **Answer decisions, not transcripts.** Agents bring the owner a question with options; the
   owner never has to read a session to find out what is needed.
8. **One step stops everything.** The owner can halt all agent work at once, and any merged
   change can be rolled back without the loop that made it.

## How bosun enforces it

Status: **enforced** means CI, a hook or the bosun binary refuses; **partial** means some of it
is enforced, or it has a known gap; **planned** means a ranked candidate in bosun's operating
model, not built yet.

| Principle                | bosun mechanism                                                                                                                                                                                                                                                                                                                        | Status   |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| 1. Tend the codebase     | fmt, lint, test and deps gates from `profiles/*.toml`; nested loops: local `bosun gate`, then `ci.yml`, then `main-health.yml`; `main-open` fails every pull request except a mainfix while `main` is red                                                                                                                              | enforced |
| 2. Agents within limits  | a worker claims a task (`bosun claim`), runs the gates, opens a draft (`bosun pr`) and gets a verdict from a fresh reviewer; but the reviewer session is self-declared and `verdict post` writes the status directly, so an author can post its own verdict                                                                            | partial  |
| 2. Agents within limits  | a hook denies merge, tag and force-push in shell commands and in the GitHub merge and auto-merge tools; bosun only turns on auto-merge; the hook code itself is on no risk path yet                                                                                                                                                    | partial  |
| 2. Agents within limits  | put the hooks on a risk path; count only verdicts bound to a reviewer's workflow run                                                                                                                                                                                                                                                   | planned  |
| 3. Humans own the limits | boundary files make a pull request high risk until the owner approves (a delegate may approve `deny.toml`, the toolchain files and `profiles/**`); limits and budgets live in `.bosun/limits.toml`; risk rules and approver keys are not read from the pull request; bootstrap mode (below) lets the orchestrator approve part of them | partial  |
| 3. Humans own the limits | weakening a gate is caught only where the gate's file is on a risk path, and nothing checks that a threshold went down; a rule that compares base and head values is planned                                                                                                                                                           | partial  |
| 3. Humans own the limits | budgets refuse new work at 80 % (exit 6) and return stop actions for the caller; `bosun progress` shows cost per merged task, not yet per outcome that holds or owner minutes                                                                                                                                                          | partial  |
| 4. Measured evidence     | the delegate approval class is fixed by path, not earned from revert or escape rates                                                                                                                                                                                                                                                   | partial  |
| 4. Measured evidence     | autonomy earned per change class (revert and escape rate per risk rule) and a trust reset when the model changes                                                                                                                                                                                                                       | planned  |
| 5. Blast radius          | `.bosun/risk.toml` rules, read from the pull request's base commit, only escalate; each rule asks for the owner, a delegate or two verdicts; a diff that GitHub hid in part always needs the owner; bootstrap gaps (below)                                                                                                             | partial  |
| 5. Blast radius          | challenge in proportion: two levels only (`normal`, `high`)                                                                                                                                                                                                                                                                            | partial  |
| 5. Reversibility         | `bosun revert <pr>` rolls back a merged change without the loop that made it                                                                                                                                                                                                                                                           | enforced |
| 5. Reversibility         | preview and cancel: `--dry-run` on some commands, `bosun hold`, the halt issue                                                                                                                                                                                                                                                         | partial  |
| 5. Approvals             | an approval counts only on the exact head commit, by one of three routes: an owner review approval, a plain owner comment, or an ed25519-signed `/bosun approve <head_sha>` comment with a key from the default branch; a new push needs a new approval                                                                                | enforced |
| 6. Escaped bug → check   | `main-triage` bisects a red `main`, `bosun revert` opens a mainfix and a red `main` opens a lesson; a mechanizable lesson becomes a task by hand; lessons are capped at 40, so each one is mechanized or dropped                                                                                                                       | partial  |
| 6. Escaped bug → check   | after a self-repair, a follow-up task asking which gate missed the bug                                                                                                                                                                                                                                                                 | planned  |
| 7. Decisions             | `bosun ask` and `answer` for decisions, `bosun prereqs` for human-only steps                                                                                                                                                                                                                                                           | partial  |
| 7. Decisions             | `bosun notify` returns the thread replies for the caller to post; it posts nothing itself, and the notify routine is a step for a human to set up                                                                                                                                                                                      | partial  |
| 7. Decisions             | chat and ticket text is evidence, not instruction: `bosun intake` wraps it as quoted data so it cannot pose as bosun's own markers, but only the prompt for a Slack-thread task calls it data                                                                                                                                          | partial  |
| 8. One step stops        | an open `bosun:halt` issue from an owner, or the repo variable `BOSUN_HALT=1`: commands that start or change work exit 4; it stops the whole repo, not one loop                                                                                                                                                                        | partial  |
| 8. One step stops        | a kill switch per loop and one manifest per loop (mandate, identity, budget, stop rule)                                                                                                                                                                                                                                                | planned  |

The tests that keep the gates honest are enforced too: `test-integrity` refuses deleted tests, new
skips and fewer asserts, and `ci-minutes` lints the workflows.

## How keelstackhq runs today

keelstackhq runs in bootstrap mode, which the owner chose: until 2026-11-30 the orchestrator
session may sign gated approvals itself, after green checks and an independent agent review of
that head (decision D-012; `[approvals] bootstrap_until` in `.bosun/bosun.toml`, with a hard end
date in bosun's code). This trades the rule "agent agreement is not independent evidence" for
speed, and we say so.

What stays human in this mode:

- **The core boundary files.** `.bosun/**`, `governance/**`, CODEOWNERS, `hoist.toml`, the risk
  and verdict workflows and the approval code stay owner-only.
- **Tags and force-pushes.** A hook denies tag, force-push and merge to every agent. It is a
  guard, not a sandbox, and its code is on no risk path yet.
- **Workflows, releases and secrets.** No orchestrator approval ever counts for any workflow
  change or CI file (GitHub workflows, CircleCI and other CI configs), which covers the release
  and splice workflows, or for a change that matches the secrets rule (R-secrets). The owner
  approves those.

Merges: GitHub merges on green after bosun turns on auto-merge; only a pull request that is
already mergeable (`already_clean`) is merged by the orchestrator (D-008).

What the orchestrator can still approve during the window: the rest of the
governance paths (`.claude/**` with the hook settings, `REVIEW.md`, plans, `docs/agents/**`, agent
and Claude instruction files) and changes that write to production (R-prod-write). Closing these
gaps is bosun's second planned change.

The owner can end the mode at any time by removing the orchestrator's key. Bootstrap mode ends on
2026-11-30 at the latest (capped in code); after that, `[approvals] policy` (`owner`,
`owner-gated` or `orchestrator`; missing means `owner`; D-014, draft) decides when an orchestrator
approval counts. A later autonomous mode for the org is not in bosun yet.

See [Risk and review](/docs/risk/) for the approval steps and [Playbook](/docs/playbook/) for how
the orchestrator runs.
