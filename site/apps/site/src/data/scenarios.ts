// The copy-paste blocks of /docs/start, /migrate, /docs/cloud, /docs/risk, /docs/update, the
// landing "Pick your path" cards and llms.txt. Commands are copied from the bosun, hull and splice
// docs (see sources below); no imports, so scripts/adopt.mjs can use the same builders.
// Sources: bosun main docs/quickstart.md, docs/migrate-booster.md, docs/update.md,
// docs/adopt-existing-repo.md, docs/spec/08-risk.md, docs/spec/10-slack.md,
// src/cli/setup_snippet.rs; hull docs/adopt.md, docs/start-checklist.md; splice README.

export interface Versions {
  hull: string;
  splice: string;
  bosun: string;
  hoist: string;
}

export interface Scenario {
  id: "a" | "b" | "c" | "d" | "e" | "f";
  title: string;
  summary: string;
}

export const scenarios: Scenario[] = [
  { id: "a", title: "New repo", summary: "A new repo from hull, run by bosun." },
  {
    id: "b",
    title: "Adopt a repo",
    summary: "An existing repo: hull assess and splice adopt, or bosun alone.",
  },
  {
    id: "c",
    title: "Change mode",
    summary: "gatekeeper, steady, booster, autonomous: one PR per move.",
  },
  { id: "d", title: "Upgrade", summary: "Move bosun, hull and splice to the newest releases." },
  {
    id: "e",
    title: "Where agents run",
    summary: "Claude Code CLI, Claude Code on the web, or Claude Tag in Slack.",
  },
  { id: "f", title: "Approvals", summary: "What merges alone, what the owner approves, and how." },
];

/** The cloud environment's setup script, pasted once: `bosun setup-snippet` prints it as data.short. */
export const SETUP_SHORT = [
  "for d in . ./*/; do",
  '  [ -f "$d/.bosun/version" ] && [ -f "$d/docs/setup-script.sh" ] || continue',
  '  (cd "$d" && timeout -k 10 1800 bash docs/setup-script.sh </dev/null) || echo "bosun: $d/docs/setup-script.sh failed" >&2',
  "done; exit 0",
].join("\n");

export const NETWORK =
  "Network access: Limited, with Allow package managers ticked. Add only the hosts your repo's docs/setup-script.sh downloads from: a Rust repo needs github.com and release-assets.githubusercontent.com (prebuilt cargo tools); a repo that only uses npm needs none. No secrets in environment variables and no PAT: GitHub comes from the Claude GitHub App.";

export const CLOUD_TOML =
  '[cloud]\nenvironment_id = "env_..."   # the cloud environment the spawned children run in';

/** The prompt for a mode move, the same in the CLI, on the web and to Claude Tag. */
export const MIGRATE_PROMPT = [
  "Run `bosun migrate --to booster --direct`; do what its `next` and `data.then` say; stop when it asks for the owner.",
  "After the merge run `bosun migrate --finish`.",
].join("\n");

export const MIGRATE_COMMANDS = [
  "bosun migrate --to <mode> --dry-run   # survey and planned edits; writes nothing",
  "bosun migrate --to <mode> [--direct]  # one PR, local gates, ready + auto-merge; --direct skips rungs",
  "bosun migrate --to <mode> --no-ready  # instead: the draft PR only",
  "bosun pr --pr <n> --ready             # only after --no-ready; n = data.pr; after the gates",
  "bosun migrate --finish                # after the merge: join, plan amend",
  "bosun doctor                          # verify",
].join("\n");

export const MODE_LADDER: readonly (readonly [string, string, string])[] = [
  ["gatekeeper", "No helpers. The default without [mode].", "as needed"],
  [
    "steady",
    "Gates before push, red-PR notices, revert PRs on red main, daily digest, budget warnings.",
    "5 a day or fewer",
  ],
  [
    "booster",
    "Steady plus fast-lane approval (autonomous policy), auto spawn, fixer sessions, budget asks, curator proposals.",
    "3 a day or fewer",
  ],
  [
    "autonomous",
    "Booster plus auto-merge of exact-inverse reverts (needs a clean verdict); owner-class PRs still need an approval; about one owner touch a day (the digest).",
    "1 a day or fewer",
  ],
];

export const MIGRATE_NOTES = [
  "One rung per PR unless the owner passes --direct; the PR body records that choice. Never move to a less strict mode after a red week.",
  "Before the next rung (bosun metrics --days 7): steady needs red-CI under 30 % and 5 owner touches a day or fewer for two weeks; booster under 20 % and 3; autonomous under 15 % and 1.",
  "Rollback: going tighter is the same command (bosun migrate --to steady): no R-weaken, still R-gov. Deleting the [mode] table returns to gatekeeper.",
];

export const SLACK_TOML = [
  "[slack]",
  'channel = "C0123456789"',
  'spawners = "owners"',
  "may_spawn = true",
].join("\n");

export const SLACK_COMMANDS = [
  "bosun join --origin slack --slack-channel C0123456789 [--slack-thread <ts>]",
  "bosun notify --channel-info -     # posts due: halt, main-red, asks",
  "bosun spawn --slack-thread <ts> --slack-user <U...> --request -",
].join("\n");

export const SLACK_NOTIFY_DIGEST =
  'notify = ["owner-question", "main-red", "budget-halt", "halt", "thread-result", "digest"]';

export const APPROVE_COMMENT = "/bosun approve <40-character head SHA>";

export const SIGNER_STEPS = [
  'Pin a release that reads the key, then set `signer = true` under [approvals] and [checks] mode = "checks" (with the gate App) in .bosun/bosun.toml, in an owner-approved PR (R-weaken). It needs the autonomous policy.',
  "bosun keygen --role signer --id signer-ci --out <pem outside the repo> --enroll",
  "Merge that .bosun/approvers.toml PR (R-gov: the owner approves).",
  "Create the GitHub environment bosun-signer (deployment branches: the default branch only) with the secret BOSUN_SIGNER_KEY set to the PEM, then delete the PEM.",
] as const;

export const NO_APPROVAL =
  "A normal-risk PR (no rule in .bosun/risk.toml matches) merges on green CI and a clean review verdict. No approval.";

export const OWNER_APPROVES =
  "The owner approves the exact head of a high-risk PR. That is any PR that touches governance paths (R-gov: `.bosun/**`, `governance/**`, `.claude/**`, `.splice/**`, `docs/setup-script.sh`, ...), loosens a gate (R-weaken) or adds a secret (R-secrets). R-ci, R-deps, R-prod-write, R-unseen, R-bundle, R-plan and R-revert are owner-class by default too. Exceptions: R-ci-review takes two independent verdicts, a delegate-class rule can be decided by a delegate, and under the autonomous policy allowlisted paths take the fast lane.";

export const APPROVE_HOW = [
  "Chat form: the owner tells the signing session (repo, PR, head SHA; approve or hold) and it posts the signed approval below.",
  `GitHub: a plain ${APPROVE_COMMENT} comment counts only when posted with a plain user token (typed in the GitHub web or mobile app, or from the owner's own machine); comments posted by the Claude app or a cloud session never count. A new push needs a new comment. Agent PRs are opened under the owner's login and GitHub forbids self-review, so the owner uses a plain comment, a signed approval, or merges.`,
] as const;

// --- Scenario blocks (versions come from data/versions.ts) ---

/** The one bosun install, shown once at the top of /docs/start; every scenario block assumes it. */
export const installBlock = (v: Versions): string =>
  [
    `npm i -g @keelstackhq/bosun@${v.bosun}`,
    "export GITHUB_TOKEN=$(gh auth token)   # in your shell, never in a file",
  ].join("\n");

export const newRepoBlock = (v: Versions): string =>
  [
    `pnpm dlx @keelstackhq/splice@${v.splice} new gh:keelstackhq/hull//template my-repo --ref ${v.hull} --piece <id>:<name> ...`,
    "cd my-repo && pnpm install && pnpm run ci",
    'git add -A && git commit -m "chore: render from hull"   # commit the render and its lockfile (`splice new --commit` skips the lockfile)',
    "gh repo create <owner>/my-repo --private --source . --push   # before bosun init; <owner> from the brief",
    "bosun init --stack ts-pnpm --plan plans/<name>.md --owner <login>   # opens the setup PR",
    "bosun doctor",
  ].join("\n");

export const NEW_REPO_OWNER =
  "The agent runs every command above, gh repo create included (as the owner's gh login). Owner steps, in order, each with the check an agent runs; the hoist commands are step 4, run on the owner's machine in a checkout of the default branch:";

export interface HumanStep {
  when: string;
  what: string;
  check: string;
}

/** The owner's steps for a bosun repo, in the order they happen: one truth for agents/README.md and /docs/start. */
export const HUMAN_STEPS: readonly HumanStep[] = [
  {
    when: "Before setup PR",
    what: "Install the Claude GitHub App on the repo",
    check: "`bosun doctor` step 2, once the app has acted",
  },
  {
    when: "Before setup PR",
    what: "Signing key (optional): `bosun keygen --enroll`",
    check: "`gh pr diff <n> --name-only`: approvers.toml",
  },
  {
    when: "Setup PR open",
    what: "Approve the setup PR's head and merge it",
    check: "`gh pr view <n> --json state`",
  },
  {
    when: "After that merge",
    what: "`hoist apply --area github` (admin rights)",
    check: "`hoist check --area github`",
  },
  {
    when: "After that merge",
    what: "Paste the setup script into the cloud environment",
    check: "`bosun doctor` step 6; else ask",
  },
];

export const HUMAN_STEPS_NOTE =
  "The owner merges; agents never merge. Step 4 also sets branch protection and auto-merge; an existing repo takes `--adopt`. Setup script: Start E, https://keelstack.dev/docs/start/#e.";

/** Bosun alone has no hoist.toml: bosun adopt's own owner step for branch protection. */
export const bosunAloneHoist = (v: Versions): string =>
  `Bosun alone, step 4 instead, after the setup PR merges: create hoist.toml with \`ci-ok\`, \`risk-check\`, \`review-verdict\`, \`main-open\` in \`[github] required_checks\`, run \`npm i -g @keelstackhq/hoist@${v.hoist}\`, then \`hoist apply --area github --adopt --yes\`.`;

export const humanStepsTable = (steps: readonly HumanStep[] = HUMAN_STEPS): string =>
  [
    "| When | Owner step | Agent check |",
    "| --- | --- | --- |",
    ...steps.map((s) => `| ${s.when} | ${s.what} | ${s.check} |`),
  ].join("\n");

export const humanStepsList = (steps: readonly HumanStep[] = HUMAN_STEPS): string =>
  steps.map((s, i) => `${i + 1}. ${s.when}: ${s.what}. Agent check: ${s.check}.`).join("\n");

export const hoistBlock = (v: Versions): string =>
  [
    `npm install -g @keelstackhq/hoist@${v.hoist}`,
    "hoist plan --area github          # what would change; writes nothing",
    "hoist apply --area github --yes   # write it, then verify like `hoist check`",
  ].join("\n");

export const adoptHullBlock = (v: Versions, assessUrl: string, sha: string): string =>
  [
    `curl -fsSL https://keelstack.dev${assessUrl} -o /tmp/assess.mjs`,
    `echo "${sha}  /tmp/assess.mjs" | sha256sum -c`,
    "node /tmp/assess.mjs --assess --json",
    `pnpm dlx @keelstackhq/splice@${v.splice} adopt gh:keelstackhq/hull//<template> --ref ${v.hull} --piece <id>:<name> ... --dry-run --json`,
  ].join("\n");

/** After the owner confirmed the dry run: the same command for real, then the check. */
export const adoptHullRunBlock = (v: Versions): string =>
  [
    `pnpm dlx @keelstackhq/splice@${v.splice} adopt gh:keelstackhq/hull//<template> --ref ${v.hull} --piece <id>:<name> ...   # the dry-run command without --dry-run`,
    `pnpm dlx @keelstackhq/splice@${v.splice} check --offline   # conflict markers fail it`,
  ].join("\n");

export const ADOPT_HULL_NOTE =
  "Read the change list, then run the same splice command without --dry-run. Never pass --force unless the owner names the files it may replace. Open the PR; the owner merges it. After the merge, for a Rust or ts-pnpm stack run bosun init --stack rust|ts-pnpm --plan plans/<name>.md --owner <login> (the plan comes from the owner's brief, as in A; bosun adopt below is for repos without hull) and the hoist steps of A, but on an existing repo run `hoist plan --area github --adopt`, then `hoist apply --area github --adopt --yes` (`--adopt` keeps stricter live settings and never loosens them; hoist apply sets the ci-ok ruleset).";

export const adoptBosunBlock: string = [
  "bosun adopt --dry-run                    # writes nothing",
  "bosun adopt --owner <login>              # writes bosun's files; commits nothing",
  "bosun doctor                             # verify",
].join("\n");

export const ADOPT_BOSUN_OWNER =
  "Run the git commands in data.commands (branch bosun-setup) and open the setup PR. Owner steps, in order, each with the check an agent runs (the owner merges the setup PR; agents never merge):";

export const upgradeBlock = (v: Versions): string =>
  [
    "bosun pin --latest --check            # tools behind; writes nothing; run on the default branch",
    "bosun pin --latest --pr               # checks npm sha512, opens a draft PR; the owner approves",
    "# after it merges, on your machine (<new> = the version pin --check printed):",
    "npm i -g @keelstackhq/bosun@<new> && bosun --version",
    "# then hull, on its own branch:",
    "git switch <default-branch> && git pull && git switch -c chore/splice-update",
    `pnpm dlx @keelstackhq/splice@${v.splice} update --all`,
    `pnpm dlx @keelstackhq/splice@${v.splice} check && pnpm install && pnpm run ci`,
    'git add -A && git commit -m "chore: splice update to hull vX.Y.Z" && git push -u origin HEAD',
  ].join("\n");

export const UPGRADE_NOTE =
  "Do bosun first, then hull; each is its own PR and each is R-gov (`.bosun/**`, `.claude/**`, `.splice/**`), so the owner approves each head as in F. A repo set up with bosun init or hull has .splice/channels/bosun.toml and cannot run bosun pin (it refuses, exit 1): there `splice update --all` below also updates the bosun channel (or `splice update --channel bosun` alone), so skip the bosun pin lines and the npm line; install the bosun version the merged channel update put in .bosun/version. The bosun pin lines are for a repo set up with bosun adopt. Cloud sessions run the repo's own setup script, whose tool pins come with the bosun release: bosun 0.19.0's script pins splice 0.6.0, hoist 0.5.1 and berth 0.2.1, so a cloud session runs those until a later bosun release moves them. Merge only when ci-ok is green.";

export const approveBlock: string = [
  "bosun approve --list   # ready PRs and heads",
  "bosun approve <pr>@<40-character head SHA> --sign-key <pem> --print   # shows the signed comment",
  "bosun approve <pr>@<40-character head SHA> --sign-key <pem>   # posts it",
].join("\n");

export const SIGNED_PREREQ =
  "Signed form (works from a cloud session): a bosun release with signed approvals (0.2.1 or later) in .bosun/version, the signer's key enrolled in .bosun/approvers.toml on the default branch (bosun keygen --enroll; the enrolling PR is R-gov, so the owner approves it with a GitHub review or a plain comment, since no signed approval counts on that PR), and the PEM held by the signing session.";

export const cliBlock = "bosun doctor   # each missing step with its fix";

/** Channels only (no hull template): the decisions channel (hull docs/adopt.md, step 10). */
export const channelsBlock = (v: Versions): string =>
  `pnpm dlx @keelstackhq/splice@${v.splice} channel add gh:keelstackhq/decisions//channel --track main --piece import-project`;

/** hoist on an existing repo: only adds, never loosens a stricter live setting. */
export const hoistAdoptBlock: string = [
  "hoist plan --area github --adopt          # what would change; writes nothing",
  "hoist apply --area github --adopt --yes   # write it, then verify",
].join("\n");

/** Checks by timing; none is a failure of an agent step unless its note says so. */
export const verifyBlock = (v: Versions): string =>
  [
    "pnpm run ci   # where the repo has it",
    "bosun doctor   # exit 5: owner steps pending",
    `pnpm dlx @keelstackhq/splice@${v.splice} check --offline   # needs .splice/lock.toml`,
    "hoist check --area github   # after owner step 4 (--adopt on an adopted repo)",
  ].join("\n");

/** Undo, from the least to the most: nothing is merged until the owner merges it. */
export const rollbackBlock: string = [
  "git switch <default-branch> && git branch -D <branch>   # unpushed: drop the branch",
  "gh pr close <n> --delete-branch                         # open PR: close it",
  "bosun migrate --to <tighter-mode>                       # mode too loose: one PR tighter",
].join("\n");
