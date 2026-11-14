// Move a repo to bosun booster: copy from bosun docs/migrate-booster.md (bosun main at bb4d1ab),
// Quick start at its top. Needs bosun 0.15.0 and hull v0.10.0. Names no client org or repo.

export const RUNBOOK_URL = "https://github.com/keelstackhq/bosun/blob/main/docs/migrate-booster.md";
export const APPENDIX_URL = `${RUNBOOK_URL}#appendix-a-the-manual-path`;
export const QUICKSTART_URL = "https://github.com/keelstackhq/bosun/blob/main/docs/quickstart.md";

export const migrate = {
  title: "Move a repo to bosun booster",
  summary:
    "For a repo that already runs hull and bosun: one setup script, a two-line prompt, one PR to approve. Works in Claude Code locally, on the web, and in Slack.",
  forWho:
    "For a repo that already runs hull and bosun. It needs bosun 0.15.0 or newer and hull v0.10.0 or newer. Booster is steady plus a fast lane.",
  owner:
    "The owner approves one PR: the exact head (repo, PR, head SHA), in the way bosun asks. Agents never merge, tag or force-push; they turn on auto-merge. Routines stay human-only (claude.ai/code, Routines); the PR body lists them.",
  quick: {
    setupLabel:
      "1. Cloud environment setup script. bosun setup-snippet prints it as data.short; paste it once as the environment's Setup script:",
    setup: [
      "for d in . ./*/; do",
      '  [ -f "$d/.bosun/version" ] && [ -f "$d/docs/setup-script.sh" ] || continue',
      '  (cd "$d" && timeout -k 10 1800 bash docs/setup-script.sh </dev/null) || echo "bosun: $d/docs/setup-script.sh failed" >&2',
      "done; exit 0",
    ].join("\n"),
    setupNote:
      "Each repo's own docs/setup-script.sh installs the pinned bosun (sha512 checked) and the rest. A repo without that script needs the long form, data.script.",
    network:
      "Network access: Limited, with Allow package managers ticked. Add github.com and release-assets.githubusercontent.com: a Rust repo downloads its prebuilt cargo tools from them. No secrets, no PAT: GitHub comes from the Claude GitHub App.",
    promptLabel: "2. The prompt. The same in the CLI, on the web and to Claude Tag in Slack:",
    prompt: [
      "Run `bosun migrate --to booster --direct`; do what its `next` and `data.then` say; stop when it asks for the owner.",
      "After the merge run `bosun migrate --finish`.",
    ].join("\n"),
    promptNote:
      "This prompt goes straight to booster: --direct is the owner's choice, and the PR body records it. Drop --direct to move one step (gatekeeper to steady).",
    approve:
      "3. You approve one PR: the exact head. Auto-merge follows. risk-check still waits for the owner (R-gov always, R-weaken for a lighter mode).",
  },
  does: {
    intro: "What bosun migrate does:",
    commands:
      "bosun migrate --to booster --dry-run   # survey and planned edits; writes nothing\nbosun migrate --to booster             # one governance PR (draft)\nbosun migrate --finish                 # after the merge: join, plan amend (idempotent)",
    notes: [
      "Survey: pinned version, current mode, registry issue, epics without a bosun:epic marker, hull lock, and a 30-day metrics baseline (best effort: a missing baseline never blocks).",
      "One PR with every file step: the .bosun/version pin (when the release is on npm), the .bosun/registry pin and [mode] name. An open PR for the same step is reused.",
      "After the merge, bosun migrate --finish runs bosun join, then bosun plan amend for each plan an open epic names. Run it twice: the second run changes nothing.",
    ],
  },
  knobs: [
    ["[approvals] policy", "repo value", "autonomous (when unset)", "Fast-lane approval."],
    ["push_gates", "push", "push", "Same: a git push needs gate stamps."],
    [
      "heal_pr",
      "notify",
      "spawn",
      "bosun heal also returns a create_session fixer when the lease holder is dead.",
    ],
    ["heal_main", "revert-pr", "revert-pr", "Same: a draft revert PR on red main."],
    ["approvals_batch", "daily", "daily", "Same: one digest a day."],
    ["spawn_auto", "off", "on", "bosun wave tick <epic> returns create_session calls."],
    [
      "budget_feature",
      "warn",
      "ask",
      "At 100 % of a feature budget, claim and spawn exit 5 until the owner answers.",
    ],
    ["brake", "on", "on", "Same."],
    ["curator_loosen", "off", "propose", "The curator Routine may open one draft PR a week."],
  ] as [string, string, string, string][],
  knobHead: ["Knob", "steady", "booster", "What it means"] as const,
  fastLane:
    "Fast lane: the orchestrator approves a PR whose paths are on the D-017 allowlist and not on its deny list. Every verdict on the exact head must be clean, one verdict must come from an independent reviewer session, and the PR must close an open task its author leases. An explicit [approvals] policy in the file wins. Targets: red-CI rate under 20 %, at most 3 owner touches a day.",

  local: {
    heading: "Claude Code on your machine (CLI)",
    steps: [
      "[computer] npm i -g @keelstackhq/bosun (a release where bosun migrate --help works), and gh auth login once.",
      "Set GITHUB_TOKEN to gh auth token in your shell. Never in a file.",
      "Open Claude Code in the repo, on the default branch, clean tree. Paste the prompt.",
      "[phone] Approve the PR head when Claude asks.",
      "[computer] Do the Routine steps the PR body lists.",
    ],
  },

  web: {
    heading: "Claude Code on the web",
    steps: [
      "[computer] Environment (session title bar, Edit), once per repo: paste the setup script; Network access: Limited with Allow package managers; add the hosts your docs/setup-script.sh downloads from (a Rust repo needs github.com and release-assets.githubusercontent.com). No secrets, no PAT: GitHub comes from the Claude GitHub App.",
      "Start a session on the repo in that environment. Paste the prompt.",
      "[phone] Approve the PR head when Claude asks.",
      "[computer] Do the Routine steps the PR body lists.",
    ],
  },

  slack: {
    heading: "Claude Tag in Slack",
    steps: [
      "[computer] Once: a private channel, your Slack ID in governance/owners.toml, [slack] channel, may_spawn = true and [cloud] environment_id in .bosun/bosun.toml (an R-gov PR). The environment is the one from the web steps.",
      "In the channel, start a thread and post the prompt to Claude. Slack is the only entry here.",
      "[phone] Approve the PR head when bosun asks.",
      "[computer] Do the Routine steps the PR body lists.",
    ],
  },
};
