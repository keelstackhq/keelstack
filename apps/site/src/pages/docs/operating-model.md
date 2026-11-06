---
layout: ../../layouts/Docs.astro
---

How keelstackhq works with coding agents, in eight principles, and which part of each one bosun
checks today. The mapping follows bosun's operating model (`docs/operating-model.md`, bosun pull
request #421); a principle that bosun does not check yet says so.

## The manifest

1. **Tend the codebase; it is not a factory.** Every change stays and becomes the ground for the
   next one, so types, module limits, gates, CI and docs exist to keep that growth healthy.
2. **Agents write the code, inside a fence.** They build features and may improve the code and
   its structure, but only inside limits that they cannot move.
3. **Humans own the fence.** People decide what to build, how we know it is good, who approves
   what, which checks run, what agents may touch and how much they may spend.
4. **Widen the fence on measured evidence.** Agents get more room when numbers show it is safe,
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
is enforced; **planned** means a ranked candidate in bosun's operating model, not built yet.

| Principle                | bosun mechanism                                                                                                                                                                                                                                                                                                                          | Status   |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| 1. Tend the codebase     | fmt, lint, test and deps gates from `profiles/*.toml`; nested loops: local `bosun gate`, then `ci.yml`, then `main-health.yml`; `main-open` blocks pull requests while `main` is red                                                                                                                                                     | enforced |
| 2. Agents inside a fence | a worker claims a task (`bosun claim`), runs the gates, opens a draft (`bosun pr`) and gets a verdict from a fresh reviewer; lease holders never count as reviewers; a hook denies merge, tag and force-push, also through GitHub API tools                                                                                              | enforced |
| 3. Humans own the fence  | boundary files (`.bosun/**`, `governance/**`, `.claude/**`, workflows, gate code) make a pull request high risk until the owner approves (a delegate key may approve `deny.toml`, the toolchain files and `profiles/**`); limits and budgets live in `.bosun/limits.toml`; risk rules and approver keys are read from the default branch | enforced |
| 3. Humans own the fence  | weakening a gate is caught by path only; no check yet that a threshold went down (a risk rule `R-weaken` is planned)                                                                                                                                                                                                                     | partial  |
| 3. Humans own the fence  | budgets warn at 80 % and stop at 100 % (exit 6); `bosun progress` shows cost per merged task, not yet per outcome that holds or owner minutes                                                                                                                                                                                            | partial  |
| 4. Measured evidence     | the `delegate` approval class is fixed by path, not earned from revert or escape rates                                                                                                                                                                                                                                                   | partial  |
| 4. Measured evidence     | earned autonomy per change class (revert and escape rate per risk rule) and a trust reset when the model changes                                                                                                                                                                                                                         | planned  |
| 5. Blast radius          | `.bosun/risk.toml` rules, read from the base branch, only escalate; each rule asks for `owner`, `delegate` or `two-verdicts`; critical paths and an unseen diff always need the owner                                                                                                                                                    | enforced |
| 5. Blast radius          | challenge in proportion: two levels only (`normal`, `high`)                                                                                                                                                                                                                                                                              | partial  |
| 5. Reversibility         | `bosun revert <pr>` rolls back a merged change without the loop that made it                                                                                                                                                                                                                                                             | enforced |
| 5. Reversibility         | preview and cancel: `--dry-run` on some commands, `bosun hold`, the halt issue                                                                                                                                                                                                                                                           | partial  |
| 5. Approvals             | an owner approval is an ed25519-signed comment bound to the exact head SHA, with a key from the default branch; a new push needs a new approval                                                                                                                                                                                          | enforced |
| 6. Escaped bug → check   | `main-triage` bisects a red `main`, `bosun revert` opens a fix pull request and a red `main` opens a lesson; a mechanizable lesson becomes a task by hand; lessons are capped at 40, so each one is mechanized or dropped                                                                                                                | partial  |
| 6. Escaped bug → check   | after a self-heal, a follow-up task asking which gate missed the bug                                                                                                                                                                                                                                                                     | planned  |
| 7. Decisions             | `bosun ask` and `answer` for decisions, `bosun prereqs` for human-only steps, `bosun notify` replies to the person who asked                                                                                                                                                                                                             | enforced |
| 7. Decisions             | chat and ticket text is evidence, not instruction: `bosun intake` fences it so it cannot forge markers; not every human channel is fenced yet                                                                                                                                                                                            | partial  |
| 8. One step stops        | an open `bosun:halt` issue from an owner, or the repo variable `BOSUN_HALT=1`: commands that start or change work exit 4; it stops the whole repo, not one loop                                                                                                                                                                          | partial  |
| 8. One step stops        | a kill switch per loop and a loop manifest (mandate, identity, budget, stop rule)                                                                                                                                                                                                                                                        | planned  |

The tests that keep the gates honest are enforced too: `test-integrity` refuses deleted tests, new
skips and fewer asserts, and `ci-minutes` lints the workflows.

## How keelstackhq runs today

keelstackhq runs in bootstrap mode, which the owner chose: until 2026-11-30 the orchestrator
session may sign gated approvals itself, after green checks and an independent agent review of
that head (decision D-012; `[approvals] bootstrap_until` in `.bosun/bosun.toml`, with a hard end
date in bosun's code). This trades the rule "agent agreement is not independent evidence" for
speed, and we say so.

What stays human in this mode:

- **The fence itself.** Governance, risk rules, the approval code and the bootstrap setting stay
  owner-only; so do `hoist.toml`, the workflows that use environments and secrets.
- **Tags, force-pushes and merges.** A hook denies merge, tag and force-push to every agent.
  bosun turns on auto-merge and GitHub merges on green; the one exception is a pull request that
  is already mergeable (`already_clean`), which the orchestrator merges (D-008).
- **Releases and secrets.** A bootstrap approval never counts for `release.yml`, `splice.yml`,
  a workflow that names an `environment:`, or a change that matches the secrets rule
  (R-secrets), so the owner approves those.

The owner can end the mode at any time by removing the orchestrator's key. Bootstrap mode ends on
2026-11-30 at the latest (capped in code); after that, `[approvals] policy` (`owner`,
`owner-gated` or `orchestrator`; missing means `owner`, D-014) decides when an orchestrator
approval counts. A later autonomous mode for the org is not in bosun yet.

See [Risk and review](/docs/risk/) for the approval steps and [Playbook](/docs/playbook/) for how
the orchestrator runs.
