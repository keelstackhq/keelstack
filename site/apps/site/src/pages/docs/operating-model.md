---
layout: ../../layouts/Docs.astro
---

How keelstackhq works with coding agents, in eight principles, and which part of each one bosun
checks today. The mapping follows bosun's operating model (`docs/operating-model.md` on bosun
`main` at `ba4ed9c`, bosun 0.11.0, 2026-11-09); a principle that bosun does not check yet says so.

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

| Principle                | bosun mechanism                                                                                                                                                                                                                                                                                                                                                                                | Status            |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------- |
| 1. Tend the codebase     | fmt, lint, test and deps gates from `profiles/*.toml`; nested loops: local `bosun gate`, then `ci.yml`, then `main-health.yml`; `main-open` fails every pull request except a mainfix while `main` is red                                                                                                                                                                                      | enforced          |
| 2. Agents within limits  | a worker claims a task (`bosun claim`), runs the gates, opens a draft (`bosun pr`) and gets a verdict from a fresh reviewer; but the reviewer session is self-declared and `verdict post` writes the status directly, so an author can post its own verdict; after a merge of `main`, the review pack's delta lists only the pull request's own files (since bosun 0.11.0)                     | partial           |
| 2. Agents within limits  | a hook denies merge, tag and force-push in shell commands and in the GitHub merge and auto-merge tools; bosun only turns on auto-merge; the hook code is now on a risk path (R-gate), but the hook is a guard, not a sandbox                                                                                                                                                                   | partial           |
| 2. Agents within limits  | count only verdicts bound to a reviewer's workflow run (deferred: it needs the owner's change to the verdict workflow)                                                                                                                                                                                                                                                                         | planned           |
| 3. Humans own the limits | boundary files make a pull request high risk until the owner approves (a delegate may approve `deny.toml`, the toolchain files and `profiles/**`); limits and budgets live in `.bosun/limits.toml`; risk rules and approver keys are not read from the pull request; bootstrap mode (below) lets the orchestrator approve part of them                                                         | partial           |
| 3. Humans own the limits | R-weaken compares base and head and flags a pull request that loosens a gate: raised limits, changed profiles, lowered lint or deny levels, lower coverage floors, removed CI steps, fewer tests or more skips; only an owner approval counts on it, never an orchestrator, bootstrap, policy, autonomous or delegate signature; a few gate files are still on no risk path                    | partial           |
| 3. Humans own the limits | budgets refuse new work at 80 % (exit 6) and return stop actions for the caller; `bosun progress` shows cost per merged task and, as `held` and `cost_per_held_task_usd`, per outcome that holds (tasks whose pull request was reverted are left out); owner minutes are not counted                                                                                                           | partial           |
| 4. Measured evidence     | the delegate approval class is fixed by path, not earned from revert or escape rates                                                                                                                                                                                                                                                                                                           | partial           |
| 4. Measured evidence     | autonomy earned per change class from revert and escape rates (deferred: it needs the revert data and an owner-set limit)                                                                                                                                                                                                                                                                      | planned           |
| 4. Measured evidence     | a new model resets trust: verdict records carry the reviewer's model, and `[approvals] review_models` limits which models count for delegate, two-verdict and autonomous approval (since bosun 0.11.0); the model is self-declared like the reviewer session, so it guards against accidents, not a lying author, and it limits only gated approvals, not the low-risk `review-verdict` status | partial           |
| 5. Blast radius          | `.bosun/risk.toml` rules, read from the pull request's base commit, only escalate; each rule asks for the owner, a delegate or two verdicts; a diff that GitHub hid in part always needs the owner; bootstrap gaps (below)                                                                                                                                                                     | partial           |
| 5. Blast radius          | challenge in proportion: two levels only (`normal`, `high`)                                                                                                                                                                                                                                                                                                                                    | partial           |
| 5. Reversibility         | `bosun revert <pr>` rolls back a merged change without the loop that made it                                                                                                                                                                                                                                                                                                                   | enforced          |
| 5. Reversibility         | preview and cancel: `--dry-run` on some commands, `bosun hold`, the halt issue                                                                                                                                                                                                                                                                                                                 | partial           |
| 5. Reversibility         | commit trailers `Bosun-Session`, `Bosun-Task` and `Bosun-Model` trace each change to its session and model; `bosun pr` checks them, and refuses without them when the repo turns on `[trailers] require`                                                                                                                                                                                       | enforced (opt-in) |
| 5. Approvals             | an approval counts only on the exact head commit, by one of three routes: an owner review approval, a plain owner comment, or an ed25519-signed `/bosun approve <head_sha>` comment with a key from the default branch; a new push needs a new approval; `bosun keygen --enroll` makes the key and keeps its private half outside every repo (since bosun 0.11.0)                              | enforced          |
| 6. Escaped bug → check   | `main-triage` bisects a red `main`, `bosun revert` opens a mainfix and a red `main` opens a lesson; a red `main`, a revert and `bosun gate-missed` each open a "Which gate missed this?" task                                                                                                                                                                                                  | enforced          |
| 6. Escaped bug → check   | turning a mechanizable lesson into a process-fix task is a manual step; lessons are capped at 40, so each one is mechanized or dropped                                                                                                                                                                                                                                                         | partial           |
| 7. Decisions             | `bosun ask` and `answer` for decisions, `bosun prereqs` for human-only steps; `bosun doctor` names each owner step left with its exact fix, also without a token (since bosun 0.11.0)                                                                                                                                                                                                          | partial           |
| 7. Decisions             | `bosun notify` returns the thread replies for the caller to post; it posts nothing itself, and the notify routine is a step for a human to set up                                                                                                                                                                                                                                              | partial           |
| 7. Decisions             | chat and ticket text is evidence, not instruction: `bosun intake` wraps it as quoted data so it cannot pose as bosun's own markers; the review pack names its untrusted fields, and both built-in spawn prompts call issue, PR, comment and Slack text data (since bosun 0.11.0); the Routine prompts do not say so yet                                                                        | partial           |
| 8. One step stops        | an open `bosun:halt` issue from an owner, or the repo variable `BOSUN_HALT=1`: commands that start or change work exit 4; it stops the whole repo, not one loop                                                                                                                                                                                                                                | partial           |
| 8. One step stops        | a kill switch per loop and one manifest per loop (mandate, identity, budget, stop rule)                                                                                                                                                                                                                                                                                                        | planned           |

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
  guard, not a sandbox; its code is on a risk path (R-gate).
- **Workflows, releases and secrets.** No orchestrator approval ever counts for any workflow
  change or CI file (GitHub workflows, CircleCI and other CI configs), which covers the release
  and splice workflows, or for a change that matches the secrets rule (R-secrets). The owner
  approves those.
- **Governance and production writes.** The window never covers the rest of the governance paths
  (`.claude/**` with the hook settings, `REVIEW.md`, plans, `docs/agents/**`, agent and Claude
  instruction files) or changes that write to production (R-prod-write).
- **Weakened gates.** A pull request that loosens a gate (R-weaken) needs the owner, in any mode.

The owner can end the mode at any time by removing the orchestrator's key. Bootstrap mode ends on
2026-11-30 at the latest (capped in code); after that, `[approvals] policy` (`owner`,
`owner-gated` or `orchestrator`; missing means `owner`; D-014, draft) decides when an orchestrator
approval counts. bosun also has an `autonomous` policy (D-017), checked first, inside the window
too, after R-weaken only: the orchestrator may approve a gated pull request outside the excluded
paths when the author holds the linked task and a reviewer who is neither a lease holder nor the
signer left a clean verdict on the head. Its weak point is that the reviewer session is
self-declared.

See [Risk and review](/docs/risk/) for the approval steps and [Playbook](/docs/playbook/) for how
the orchestrator runs.
