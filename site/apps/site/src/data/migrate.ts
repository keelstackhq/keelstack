// Move a repo to bosun booster: copy from bosun docs/migrate-booster.md (bosun main at 834c4c8),
// Quick start at its top. Needs bosun 0.15.0 (pairs with hull v0.10.0, per the bosun changelog); versions come from versions.ts. Names no client org or repo.

import { MIGRATE_NOTES, MIGRATE_PROMPT, MODE_LADDER } from "./scenarios";
import { bare, versions } from "./versions";

export const RUNBOOK_URL = "https://github.com/keelstackhq/bosun/blob/main/docs/migrate-booster.md";
export const APPENDIX_URL = `${RUNBOOK_URL}#appendix-a-the-manual-path`;
export const QUICKSTART_URL = "https://github.com/keelstackhq/bosun/blob/main/docs/quickstart.md";

export const migrate = {
  title: "Move a repo to bosun booster",
  summary:
    "For a repo that already runs hull and bosun: one setup script, a two-line prompt, one PR to approve. Works in Claude Code locally, on the web, and in Slack.",
  forWho: `For a repo that already runs hull and bosun. It needs bosun ${bare(versions.bosun)} or newer. bosun ${bare(versions.bosun)} pairs with hull ${versions.hull}. Booster is steady plus a fast lane.`,
  owner:
    "The owner approves one PR: the exact head (repo, PR, head SHA), in the way bosun asks. Agents never merge, tag or force-push; they turn on auto-merge. Routines (scheduled Claude Code sessions such as janitor and curator, set up at claude.ai/code; see /docs/getting-started/) stay human-only; the PR body lists them.",
  quick: {
    setupLabel:
      "1. Web and Slack only (in the CLI, skip to the prompt): set up the cloud environment once (setup script, Network access, no secrets) with Start block E:",
    setupId: "e",
    promptLabel: "2. The prompt. The same in the CLI, on the web and to Claude Tag in Slack:",
    prompt: MIGRATE_PROMPT,
    promptNote:
      "This prompt goes straight to booster: --direct is the owner's choice, and the PR body records it. Drop --direct to move one step (gatekeeper to steady).",
    approve:
      "3. You approve one PR: the exact head. Auto-merge follows. risk-check still waits for the owner (R-gov always, R-weaken for a less strict mode).",
  },
  ladder: MODE_LADDER,
  ladderHead: ["Mode", "What bosun does for you", "Owner touches"] as const,
  notes: MIGRATE_NOTES,
  knobsUrl:
    "https://github.com/keelstackhq/bosun/blob/main/docs/quickstart.md#0-set-the-mode-all-modes-all-environments",
  does: {
    intro: "What bosun migrate does (the exact commands are in Start block C):",
    startId: "c",
    notes: [
      "Survey: pinned version, current mode, registry issue, epics without a bosun:epic marker, hull lock, and a 30-day metrics baseline (best effort: a missing baseline never blocks).",
      "One PR with every file step: the .bosun/version pin (when the release is on npm), the .bosun/registry pin and [mode] name. An open PR for the same step is reused.",
      "After the merge, bosun migrate --finish runs bosun join, then bosun plan amend for each plan an open epic names. Run it twice: the second run changes nothing.",
    ],
  },
  fastLane:
    "Fast lane: the orchestrator approves a PR whose paths are on the autonomous policy's allowlist (D-017, an org decision; see /docs/operating-model/) and not on its deny list. Every verdict on the exact head must be clean, one verdict must come from an independent reviewer session, and the PR must close an open task its author leases. An explicit [approvals] policy in the file wins. Targets: red-CI rate under 20 %, at most 3 owner touches a day.",

  local: {
    heading: "Claude Code on your machine (CLI)",
    steps: [
      `[computer] npm i -g @keelstackhq/bosun@${bare(versions.bosun)}, and gh auth login once.`,
      "Set GITHUB_TOKEN to gh auth token in your shell. Never in a file.",
      "Open Claude Code in the repo, on the default branch, clean tree. Paste the prompt.",
      "[phone] Approve the PR head when Claude asks.",
      "[computer] Do the Routine steps the PR body lists.",
    ],
  },

  web: {
    heading: "Claude Code on the web",
    steps: [
      "[computer] Environment (session title bar, Edit), once per repo: set it up once with Start block E (setup script, Network access, no secrets, no PAT).",
      "Start a session on the repo in that environment. Paste the prompt.",
      "[phone] Approve the PR head when Claude asks.",
      "[computer] Do the Routine steps the PR body lists.",
    ],
  },

  slack: {
    heading: "Claude Tag in Slack",
    steps: [
      "[computer] Once: a private channel, your Slack ID in governance/owners.toml, [slack] channel and may_spawn = true in .bosun/bosun.toml (an R-gov PR). Claude Tag runs each thread on the environment set for the channel in its settings; use the one from the web steps. [cloud] environment_id applies only to relay or wave spawns; otherwise children inherit the caller's environment.",
      "In the channel, start a thread and post the prompt to Claude. Slack is the only entry here.",
      "[phone] Approve the PR head on GitHub (web or mobile app); Slack never approves, it only links the PR.",
      "[computer] Do the Routine steps the PR body lists.",
    ],
  },
};
