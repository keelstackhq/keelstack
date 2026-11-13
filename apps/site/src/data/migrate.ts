// Move a repo to bosun booster: copy from bosun docs/migrate-booster.md (bosun PR #576, branch
// docs/migrate-booster at f6260ce) and docs/quickstart.md. Re-sync when #576 merges.
// bosun 0.14.0 and hull v0.9.0 are released. Names no client org or repo.

export const RUNBOOK_URL = "https://github.com/keelstackhq/bosun/blob/main/docs/migrate-booster.md";
export const QUICKSTART_URL = "https://github.com/keelstackhq/bosun/blob/main/docs/quickstart.md";

export const migrate = {
  title: "Move a repo to bosun booster",
  summary:
    "For a repo that already runs hull and bosun: what booster changes, the steps, and a prompt for Claude Code locally, on the web, or in Slack.",
  forWho:
    "For a repo that already runs hull and bosun, on gatekeeper or steady. Booster is steady plus a fast lane. It needs bosun 0.14.0 or newer; an older release refuses the [mode] key. Go one step at a time, gatekeeper to steady to booster, and never skip steady.",
  owner:
    "The owner still approves every PR that touches .bosun/** or .claude/**, including the [mode] PR and the pin PR. A lighter mode is R-weaken: the owner approves it too. Agents never merge, tag or force-push; they turn on auto-merge.",
  fastLane:
    "Fast lane: the orchestrator approves a PR that changes only src/, tests/, docs/ or changelog.d/. Every verdict on the exact head must be clean, one verdict must come from an independent reviewer session, and the PR must close an open task its author leases. Targets: red-CI rate under 20 %, at most 3 owner touches a day.",
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
  steps: [
    [
      "Survey",
      "bosun mode, bosun metrics --days 30, bosun doctor. Write the metrics on the first PR as the baseline.",
    ],
    [
      "Pin the new release",
      "bosun pin --latest --pr. The owner approves the exact head; turn on auto-merge.",
    ],
    [
      "Pin the registry",
      "Commit .bosun/registry with the number of the open bosun:registry issue.",
    ],
    [
      "Epic markers",
      "bosun plan check <plan> and bosun plan amend <plan> --dry-run for every plan.",
    ],
    [
      "Steady, then booster",
      'One PR with [mode] name = "steady"; watch two weeks; then a PR with name = "booster". Both are R-gov and R-weaken.',
    ],
    ["Join", "After the merge the orchestrator runs bosun join."],
    ["Amend the plans", "bosun plan amend <plan> for every plan, now that the knob is on."],
    [
      "Verify",
      'bosun mode shows "booster"; bosun metrics --days 7 shows data.brake. Watch two weeks.',
    ],
  ] as [string, string][],
  stepsAfter:
    "Then, per the runbook: hull splice update to v0.9.0, the owner's Routine prompts (janitor, curator, digest), and bosun wave tick <epic> to run. Rolling back is one PR that sets [mode] name to steady or gatekeeper.",

  local: {
    heading: "Claude Code locally",
    intro: "Claude Code CLI on your machine. Setup once:",
    setup: [
      "npm i -g @keelstackhq/bosun",
      "gh auth login                           # once, if gh is not signed in",
      "bosun doctor                            # each missing step with its fix",
      "bosun trailers install                  # the SessionStart hook does it too",
    ].join("\n"),
    notes: [
      "Login: set the environment variable GITHUB_TOKEN to the output of gh auth token (scopes: repo and workflow). Only if bosun cannot read the origin remote, also set BOSUN_REPO to owner/name. Never write either in a file in the repo.",
      "Hooks: .claude/settings.json holds bosun hook session-start, stop and pre-tool-use. They refuse a git push without gate stamps and every merge, tag or force-push. Do not edit that file; it is R-gov.",
      "Session id: set BOSUN_SESSION=<session>/<task> for each task and run bosun heartbeat at least every 20 minutes.",
      "Update the binary after the pin PR merges: npm i -g @keelstackhq/bosun@<new>, then bosun --version.",
    ],
    gatesLabel:
      "Gates before every push (TypeScript stack: add types), then push as its own command:",
    gates:
      'for g in fmt lint test deps test-integrity ci-minutes; do bosun gate "$g" || break; done\ngit push -u origin <branch>',
    prompt: [
      "Migrate this repo to bosun booster mode. Read docs/migrate-booster.md. You work in my local Claude Code CLI.",
      "Rules: follow error.fix on every refusal; never merge, tag or force-push; make only the calls bosun returns in next.",
      "1. Do section 1 (checks). Stop and tell me if bosun 0.14.0 is not on npm.",
      "2. Give me ONE checklist of the steps only I can do, each marked phone or computer, computer first.",
      "3. Do section 2, steps 1 to 8, one PR per step. After each PR ask me with a chat form: repo, PR, head SHA. Turn on auto-merge when I approve.",
      "4. Mark TODO items and ask me. Do not guess flags, hosts or settings.",
      "5. At the end report: PR numbers, bosun mode output, bosun metrics --days 7 data.brake.",
    ].join("\n"),
  },

  web: {
    heading: "Claude Code on the web",
    intro:
      "A cloud container. The environment is shared; the owner sets it up once per repo in the cloud environment menu (session title bar, Edit).",
    setupLabel:
      "1. Setup script. Run this, then paste data.script into the environment's Setup script field, once:",
    setup: "bosun setup-snippet",
    setupNote:
      "It does not change between releases. It runs as root on each fresh container, after the repos are cloned and before Claude starts, and it cannot read the environment's variables or secrets. It is idempotent and always exits 0. It runs each checkout's own docs/setup-script.sh, which pins the tools. The pin PR sets the new bosun version there. Add nothing else to the snippet.",
    network:
      "2. Network access. Choose Limited with Allow package managers ticked. GitHub goes through the session's GitHub proxy and the Claude GitHub App, not the host list. Add only hosts the repo's docs/setup-script.sh downloads from and that package managers do not cover. In bosun's own script they are:",
    hosts: [
      ["registry.npmjs.org", "the pinned tools; covered by package managers."],
      ["deb.nodesource.com", "Node 24, only when the container has no Node 24 yet."],
      ["github.com", "the plugin marketplace clone; it only warns on failure."],
      [
        "tuf-repo-cdn.sigstore.dev",
        "only when the checkout has no repo script (the bootstrap checks npm signatures).",
      ],
    ] as [string, string][],
    hostsNote:
      "A denied host shows as a proxy refusal that names the host. Read the repo's own script before you add one.",
    env: "3. Environment variables are for non-secret values only. bosun names one: CLAUDE_CODE_AUTO_COMPACT_WINDOW. No GitHub PAT, no owner credential and no age key in the environment. An API key is added as a network credential in the environment settings. Never put a secret in the script, a variable or a file.",
    cloud:
      "4. Tell bosun the environment. A PR (R-gov, the owner approves; needs the pinned release):",
    cloudToml:
      '# .bosun/bosun.toml\n[cloud]\nenvironment_id = "env_..."     # the id of the environment the children run in',
    cloudNote: "bosun wave tick and bosun spawn put this id in each create_session call.",
    janitor: "5. Janitor Routine: step 10 of the runbook.",
    cannot: [
      "Write commit statuses (the proxy answers 403): use bosun verdict comment --from -, not bosun verdict post.",
      "Approve with BOSUN_OWNER_TOKEN (it exits 5): approve by chat form.",
      'Use GraphQL for ready or auto-merge: bosun falls back by itself. If bosun pr <issue> --ready returns data.ready: "pending", make the call in next, then run it again.',
      "Merge, tag or force-push: the hooks refuse it.",
    ],
    prompt: [
      "Migrate this repo to bosun booster mode. Read docs/migrate-booster.md. You run in a Claude Code cloud environment.",
      "Rules: follow error.fix on every refusal; never merge, tag or force-push; no PAT; use bosun verdict comment, not verdict post.",
      "1. Do section 1. Stop and tell me if bosun 0.14.0 is not on npm.",
      "2. Check this environment against section 4: setup script pasted, network hosts, [cloud] environment_id. Tell me what is missing.",
      "   I set the environment in claude.ai/code. Do not put a secret in the script or in an environment variable.",
      "3. Give me ONE checklist of my steps, each marked phone or computer.",
      "4. Do section 2, steps 1 to 8, one PR per step. Ask for each approval with a chat form: repo, PR, head SHA.",
      "5. Mark TODO items and ask me. Do not guess hosts or settings.",
      "6. At the end report: PR numbers, bosun mode output.",
    ].join("\n"),
  },

  slack: {
    heading: "Claude Tag in Slack",
    intro:
      "Slack only starts and reports; the steps run in a spawned session, in the CLI or cloud setup above. Once, the owner makes a private channel (no guests, not shared) and puts their Slack ID in governance/owners.toml. Then a PR (R-gov):",
    toml: '# .bosun/bosun.toml\n[slack]\nchannel = "C0123456789"\nspawners = "owners"\nmay_spawn = true',
    tomlNote: "may_build stays false. Code comes from spawned sessions.",
    session: "The Claude Tag session labels itself:",
    commands:
      "bosun join --origin slack --slack-channel C0123456789 [--slack-thread <ts>]\nbosun notify --channel-info -     # posts due: halt, main-red, asks",
    notes: [
      "An owner posts the request in a thread. The session runs bosun status, then bosun spawn --slack-thread <ts> --slack-user <U...> --request - for work that has no epic yet. The user must be an owner's Slack ID in governance/owners.toml.",
      "It makes the create_session call in next, then runs bosun spawn <epic> --record --from -.",
      "Approvals never come from Slack. bosun answers needs-human in the thread; the owner approves by chat form or on GitHub.",
    ],
    prompt: [
      "Migrate this repo to bosun booster mode. Read docs/migrate-booster.md, sections 1, 2 and 5.",
      "Join with: bosun join --origin slack --slack-channel <channel id> --slack-thread <ts>.",
      "Run bosun status. Then spawn one orchestrator session for the migration with bosun spawn --slack-thread <ts> --slack-user <my id> --request -.",
      "Make the create_session call in next, then record it. Post progress in this thread through bosun notify.",
      "Ask me for approvals in the thread; I answer by chat form or on GitHub, never in Slack.",
      "Mark TODO items and ask me. Never merge, tag or force-push.",
    ].join("\n"),
  },
};
