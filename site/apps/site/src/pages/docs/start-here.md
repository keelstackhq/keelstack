---
layout: ../../layouts/Docs.astro
---

One ordered path from an empty GitHub repo to the first merged task. Each step names who does it
and links the page with the details. Versions as of 2026-10-29: hull v0.5.0, splice 0.3.1, bosun
0.3.0, hoist 0.3.0, berth 0.1.0, hoard v0.5.0.

Machine-readable: this page is https://keelstack.dev/docs/start-here.md, and every page is in
https://keelstack.dev/llms-full.txt.

## Before step 1 (owner)

- A GitHub repo you administer; public, or a paid plan (a private repo on the free plan has no
  rulesets, so required checks and auto-merge do not work).
- The [Claude GitHub App](https://github.com/apps/claude) installed on it.
- A dedicated Claude Code cloud environment with bosun's setup script and network allow-list:
  [Cloud environment](/docs/cloud/).

## The path

1. **Render the template** (agent). `pnpm dlx @keelstackhq/splice@0.3.1 new
   gh:keelstackhq/hull//template my-repo --piece …` (or `//template-rust`), then commit.
   [Getting started, step 1](/docs/getting-started/#1-render-the-template-new-repo).
2. **Add the channels** (agent). `splice channel add gh:keelstackhq/hull//rigging --piece claude`
   and `splice channel add gh:keelstackhq/decisions//channel --track main --piece import-project`.
   [Getting started, step 2](/docs/getting-started/#2-add-the-channels).
3. **Set up bosun** (agent). `bosun init --stack rust|ts-pnpm --owner <login>` opens one setup
   pull request. The owner reviews it, approves its head with `/bosun approve <sha>` in the GitHub
   app and merges it by hand, once.
   [Getting started, step 3](/docs/getting-started/#3-set-up-bosun).
4. **Apply the repo settings** (owner, own machine). `hoist apply --area github --yes` (auto-merge,
   the ruleset with the required checks), the `bosun:halt` label, the update bot's token
   (`SPLICE_BOT_TOKEN`, or a GitHub App from hull v0.4.0, in the main-only environment `splice-bot`),
   `governance/owners.toml`, and the Routines that `bosun doctor` prints.
   [Getting started, step 4](/docs/getting-started/#4-apply-the-repo-settings).
5. **Re-paste the setup script** (owner). The repo now has its own `docs/setup-script.sh`; paste it
   into the environment. [Cloud environment](/docs/cloud/#2-paste-the-setup-script).
6. **Enroll the orchestrator's signing key** (orchestrator makes the key, owner approves the pull
   request on GitHub). After this, the owner approves in chat.
   [Risk and review](/docs/risk/#signed-approvals-from-chat).
7. **Write the plan** (agent). `plans/<project>.md` with items, tasks (`touch:` paths, at most 400
   lines each) and "Done when" lines; `bosun plan check` validates it.
   [Tasks and sessions](/docs/tasks/#plans-items-tasks-and-ucs).
8. **Import and merge the plan task** (agent). `bosun join`, `bosun plan import <file>`, claim the
   plan task, `bosun pr`, review, merge. Later work enters through `bosun intake` and a plan
   amendment. [Getting started, step 7](/docs/getting-started/#7-run-the-first-task).
9. **Run the first task** (agent). `bosun plan import` again (the tasks become ready),
   `bosun claim <task>`, test first, run every gate locally, `bosun pr <task>`, a fresh reviewer's
   verdict (`bosun verdict comment` from a cloud session), then `bosun pr <task> --auto-merge`, or
   the orchestrator merges once the checks are green and any owner approval is in.
   [CI and gates](/docs/ci/), [Risk and review](/docs/risk/).
10. **Run it day to day.** One orchestrator session plans and starts child sessions in the
    environment; the owner approves in chat and approves releases.
    [Playbook](/docs/playbook/), [Releases and updates](/docs/releases/).

When something fails, check [Troubleshooting](/docs/troubleshooting/) first.
