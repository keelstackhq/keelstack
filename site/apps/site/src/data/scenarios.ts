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

/** Extra hosts a Rust repo's docs/setup-script.sh downloads its prebuilt cargo tools from. */
export const EXTRA_HOSTS = ["github.com", "release-assets.githubusercontent.com"] as const;

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
  "bosun migrate --to <mode> [--direct]  # one governance PR (draft); --direct skips the rungs between",
  "bosun pr --pr <n> --ready             # after the local gates: ready + auto-merge",
  "bosun migrate --finish                # after the merge: join, plan amend (idempotent)",
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
  "One rung per PR unless the owner passes --direct; the PR body records that choice. Never move lighter after a red week.",
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
  "The owner approves the exact head of a high-risk PR. That is any PR that touches governance paths (R-gov: .bosun/**, governance/**, .claude/**, .splice/**, docs/setup-script.sh, ...), loosens a gate (R-weaken) or adds a secret (R-secrets). R-ci, R-deps, R-prod-write, R-unseen, R-bundle, R-plan and R-revert are owner-class by default too. Exceptions: R-ci-review takes two independent verdicts, a delegate-class rule can be decided by a delegate, and under the autonomous policy allowlisted paths take the fast lane.";

export const APPROVE_HOW = [
  "Chat form: Claude asks in the Claude Code chat (repo, PR, head SHA; approve or hold) and posts the signed approval.",
  `GitHub: comment ${APPROVE_COMMENT} from the owner account; a new push needs a new comment.`,
] as const;

// --- Scenario blocks (versions come from data/versions.ts) ---

export const newRepoBlock = (v: Versions): string =>
  [
    `pnpm dlx @keelstackhq/splice@${v.splice} new gh:keelstackhq/hull//template my-repo --ref ${v.hull} --piece preact-app:web --piece hono-worker:api`,
    "cd my-repo && pnpm install && pnpm run ci",
    `npm i -g @keelstackhq/bosun@${v.bosun} && export GITHUB_TOKEN=$(gh auth token)`,
    "bosun init --stack ts-pnpm --plan plans/<name>.md --owner <login>   # opens the setup PR",
    "bosun doctor",
  ].join("\n");

export const NEW_REPO_OWNER =
  "Owner: create and push the GitHub repo before bosun init (Start a project, item 1); approve the setup PR's head and merge it; then run hoist apply.";

export const adoptHullBlock = (v: Versions, assessUrl: string, sha: string): string =>
  [
    `curl -fsSL https://keelstack.dev${assessUrl} -o /tmp/assess.mjs`,
    `echo "${sha}  /tmp/assess.mjs" | sha256sum -c`,
    "node /tmp/assess.mjs --assess --json",
    `npx @keelstackhq/splice@${v.splice} adopt gh:keelstackhq/hull//<template> --ref ${v.hull} --piece <id>:<name> ... --dry-run --json`,
  ].join("\n");

export const ADOPT_HULL_NOTE =
  "Read the change list, then run the same splice command without --dry-run on a new branch with a clean tree. Never pass --force unless the owner names the files it may replace.";

export const adoptBosunBlock = (v: Versions): string =>
  [
    `npm i -g @keelstackhq/bosun@${v.bosun} && export GITHUB_TOKEN=$(gh auth token)`,
    "bosun adopt --dry-run                    # writes nothing",
    "bosun adopt --owner <login>              # writes bosun's files; commits nothing",
    "bosun doctor",
  ].join("\n");

export const ADOPT_BOSUN_OWNER =
  "Owner: run bosun keygen --enroll first (in the orchestrator's container) and commit .bosun/approvers.toml in the setup PR; run the git commands in data.commands (branch bosun-setup), open the setup PR and merge it yourself.";

export const cliBlock = (v: Versions): string =>
  [
    `npm i -g @keelstackhq/bosun@${v.bosun}`,
    "export GITHUB_TOKEN=$(gh auth token)   # in your shell, never in a file",
    "bosun doctor                           # each missing step with its fix",
  ].join("\n");
