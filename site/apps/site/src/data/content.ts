// Page copy from the design reference. Names follow D-001 (`keelstackhq`).
import { buildCommand } from "../lib/command";
import { pickerData } from "../lib/picker-data";
import { facts } from "./facts";
import { bare, links, released, versions } from "./versions";
import { scenarios } from "./scenarios";
import { org, roles, tools } from "./tools";

export interface HeroVariant {
  lead: string;
  highlight: string;
}

const bosunNpm = facts.tools.find((t) => t.name === "bosun")!.npm!;
const bosunCrate = facts.tools.find((t) => t.name === "bosun")!.crate!;
const keel = facts.tools.find((t) => t.name === "keel")!;

export const hero = {
  eyebrow: "An AI-native monorepo, bootstrapped and kept current",
  variants: {
    seconds: { lead: "Seconds to a repo. Then it’s ", highlight: "all your logic." },
    skip: { lead: "Skip the setup. ", highlight: "Start with the logic." },
    agent: { lead: "Your agent’s first commit is ", highlight: "product, not plumbing." },
  } satisfies Record<"seconds" | "skip" | "agent", HeroVariant>,
  active: "seconds" as "seconds" | "skip" | "agent",
  body: "keelstack renders an opinionated monorepo from a template: stack, tool versions, CI and agent config included. It keeps all of that current as the template moves. You and your agent start on the app.",
};

// The hero shows the default `splice new` command and what it renders. Both are generated: the
// command from lib/picker-data.ts, the rows from hull's piece folders (one row per piece: its target
// and how many template files it contributes, from data/facts.ts). Nothing is recorded by hand.
const defaultPieces = [
  { id: "base", target: "." },
  ...pickerData.pieces
    .filter((p) => pickerData.defaultOn.includes(p.name))
    .map((p) => ({ id: p.piece, target: `${p.piece === "docs" ? "docs" : p.name}/` })),
];
const total = defaultPieces.reduce((n, p) => n + (facts.templateFiles[p.id] ?? 0), 0);
export const terminal = {
  title: `~ · splice ${bare(versions.splice)}`,
  command: buildCommand(pickerData, new Set(pickerData.defaultOn))
    .split("\n")
    .map((l) => l.trim()),
  output: defaultPieces.map((p) => ({
    path: `${p.target}  (${p.id})`,
    cls: `${facts.templateFiles[p.id] ?? 0} files`,
  })),
  summary: `# … ${total} template files from ${defaultPieces.length} pieces; then pnpm install`,
  turn: "→ your turn: write the app",
};

export const stats: { value: string; caption: string }[] = [
  { value: "1 binary", caption: "splice ships static. No Python, no Rust toolchain." },
  { value: "1 CI job", caption: "Turbo runs only the tasks a change affects." },
  { value: "1 line of YAML", caption: "Remote cache for CI. No account, no server." },
  { value: "1 pull request", caption: "per template update, opened by a bot." },
];

export const split = {
  handles: [
    "pnpm workspace",
    "Turborepo",
    `TypeScript ${facts.stack.typescript.split(".")[0]}`,
    "Vite+",
    "tool versions",
    "config",
    "CI + ci-ok check",
    "remote cache",
    "CLAUDE.md",
    "subagents",
    "skills",
    "/loop default",
  ],
  youWrite:
    "Routes, components, workers, domain code. Seed files are rendered once and stay yours from then on.",
};

export const agentRows: { key: string; text: string; href?: string }[] = [
  {
    key: "CLAUDE.md",
    text: "Lean by default. Skills and a /loop default ship with the template; hull ships the subagents too.",
  },
  {
    key: "ready",
    text: "hull’s ready skill asks the onboarding questions as forms and checks every owner step before agents start work alone.",
  },
  {
    key: "bosun",
    text: "Claims, gates, risk checks and review verdicts on GitHub. Agents may open PRs, never merge, tag or release. One big PR is fine: bosun bundles many tasks and reviews it in chunks. Owner teams approve high-risk changes (their own review or a `/bosun approve <sha>` comment); each verdict is bound to its run.",
  },
  {
    key: "hoist",
    text: `Settings as code. A required check can pin the App that posts it. \`hoist org ${facts.hoistOrg.join("|")}\` runs the org control plane.`,
  },
  { key: "--json --yes", text: "Every splice command takes both. No prompts, parseable results." },
  { key: "ci-ok", text: "One job, one check to wait on. Turbo runs only affected tasks." },
  { key: "update bot", text: "Template moves; you get one pull request to review." },
  {
    key: "Manifest",
    text: "How we work with agents: eight principles and what bosun enforces today.",
    href: "/docs/operating-model/",
  },
];

export const current: { label: string; title: string; text: string; accent?: boolean }[] = [
  {
    label: "owned files",
    title: "Replaced",
    text: "Files hull owns are swapped for the newer version.",
  },
  {
    label: "shared files",
    title: "3-way merged",
    text: "Template changes and your changes land together.",
  },
  {
    label: "seed files",
    title: "Stay yours",
    text: "Rendered once as a starting point, then left alone.",
    accent: true,
  },
];

// The update paths (hull's docs/updating.md, bosun's docs/update.md). Shown on the home page under
// "Stays current", on /docs/update/ and in /llms.txt; update.test pins the three to one list.
interface UpdatePath {
  heading: string;
  /** `cmd` is a command, `text` a plain step, `note` a trailing comment. */
  steps: { cmd?: string; text?: string; note?: string }[];
  notes: string[];
  guide: string;
}
export const update: {
  href: string;
  /** The home page shows only this; the full steps are on /docs/update/ and in /llms.txt. */
  short: { label: string; cmd?: string; text?: string }[];
  hull: UpdatePath;
  bosun: UpdatePath;
} = {
  href: "/docs/update/",
  short: [
    { label: "hull", cmd: "splice update --all" },
    { label: "bosun", cmd: "bosun pin --latest --pr" },
    { label: "cloud", text: "nothing to do" },
  ],
  hull: {
    heading: "Update hull",
    steps: [
      { cmd: "git switch -c chore/splice-update" },
      { cmd: "splice check --json", note: "exit 1 = update available, drift or markers" },
      { cmd: "splice diff", note: "preview, writes nothing" },
      { cmd: "splice update --all", note: "template + channels" },
      {
        cmd: "git grep -n '<<<<<<<'",
        note: "resolve conflicts; owned files: move edits to seed files",
      },
      { cmd: "splice check", note: "must exit 0" },
      { cmd: "pnpm install && pnpm run ci" },
      { cmd: 'git add -A && git commit -m "chore: splice update to hull vX.Y.Z"' },
    ],
    notes: [
      "Normally you do not run this: the weekly splice workflow (`splice update --all`) opens the PR on `splice/update`.",
      `Needs splice 0.3.0 or later. Without a global install write \`pnpm dlx @keelstackhq/splice@${versions.splice}\` in place of \`splice\`.`,
      "Do bosun first, then hull: each is its own PR, each is R-gov, and the owner approves each head.",
      "Merge only when `ci-ok` is green.",
    ],
    guide: `${links.hull}/docs/updating.md`,
  },
  bosun: {
    heading: "Update bosun",
    steps: [
      { cmd: "bosun doctor", note: 'step 17 "update" gives the fix; warns, never fails' },
      {
        cmd: "bosun pin --latest --check",
        note: "lists tools behind, downloads and writes nothing; run on the default branch",
      },
      {
        cmd: "bosun pin --latest --pr",
        note: "verifies npm sha512, branch bosun/pin, draft PR (default branch, clean tree)",
      },
      { text: "The owner approves; auto-merge lands it on green." },
      {
        cmd: `npm i -g ${bosunNpm}@<new> && bosun --version`,
        note: "your machine, after the merge; <new> is the version pin --check printed",
      },
    ],
    notes: [
      "Cloud: nothing to do. Paste `bosun setup-snippet` once; never again.",
      "A repo set up with `bosun init` or hull has `.splice/channels/bosun.toml`: `bosun pin` refuses there; use `splice update --channel bosun` (the update bot opens that PR). The steps above are for a repo set up with `bosun adopt`.",
    ],
    guide: `${links.main}/bosun/docs/update.md`,
  },
};

// `body` is plain text; the 05 intro mentions `pnpm install` as inline code in the reference.
const NUMBER_WORDS = [
  "Zero",
  "One",
  "Two",
  "Three",
  "Four",
  "Five",
  "Six",
  "Seven",
  "Eight",
  "Nine",
  "Ten",
];

export const sections: Record<
  "split" | "tools" | "agents" | "current" | "start",
  {
    label: string;
    heading: string;
    body?: string;
    links?: { label: string; href: string; text: string }[];
  }
> = {
  split: {
    label: "01 · The split",
    heading: "hull owns the plumbing. Your app code stays yours.",
  },
  tools: {
    label: `02 · ${NUMBER_WORDS[tools.length] ?? String(tools.length)} tools`,
    heading: tools.map((t) => `${roles[t.name]!.verb}.`).join(" "),
    body: released
      ? `One repo, one version: every tool ships together from ${facts.repoSlug}. Actions are ${facts.repoSlug}/<tool>@vX. The old per-tool repos are archived.`
      : `One repo for every tool: ${facts.repoSlug}. The old per-tool repos are archived.`,
  },
  agents: {
    label: "03 · Built for agents",
    heading: "Your agent opens the repo and gets to work.",
    body: "hull is built for Claude Code. The context is lean, the conventions are written down, and every tool speaks machine-readable output, so agents spend tokens on your product.",
  },
  current: {
    label: "04 · Stays current",
    heading: "The template moves. Your edits are merged, never lost.",
  },
  start: {
    label: "05 · Get started",
    heading: "Pick your pieces.",
    body: `splice renders hull’s base plus the pieces you pick, writes the lock, then runs pnpm install. Needs Node ${facts.stack.node} and pnpm ${facts.stack.pnpm.split(".")[0]}.`,
    links: [
      {
        label: "Start a project",
        href: "/docs/start-project/",
        text: "Every owner step before agents work on a new repo, and how hull’s ready check verifies it.",
      },
      {
        label: "Greenfield repo with bosun",
        href: "/docs/greenfield-bosun/",
        text: "The files, where the spec and the plan go, the step order and a first session prompt.",
      },
      {
        label: "Adopt an existing repo",
        href: "/docs/adopt/",
        text: "A read-only assessment, a Claude Code prompt and hull's adoption guide for a repo that already has code.",
      },
      {
        label: "Move a repo to bosun booster",
        href: "/migrate/",
        text: "For a repo already on hull and bosun: what booster changes, the steps, and a prompt for Claude Code locally, on the web or in Slack.",
      },
    ],
  },
};

export const footerLinks: { label: string; href: string }[] = [
  ...tools.map((t) => ({ label: t.name, href: t.repo })),
  { label: "decisions", href: `${facts.repoUrl}/tree/main/decisions` },
  { label: "Migrate to booster", href: "/migrate/" },
];

export const nav: { label: string; href: string }[] = [
  { label: "Start", href: "#quickstart" },
  { label: "Tools", href: "#tools" },
  { label: "For agents", href: "#agents" },
  { label: "Get started", href: "#start" },
  { label: "Manifest", href: "/docs/operating-model/" },
  { label: "Docs", href: "/docs/" },
  { label: "Migrate", href: "/migrate/" },
  { label: "GitHub ↗", href: org.url },
];

// Start in 5 minutes: bosun README (PR #455), docs/adopt-existing-repo.md, AGENTS.md, spec §8 (keygen).
// adopt's printed commands, keygen and doctor's exact fixes ship in bosun 0.11.0 and later.
export interface QuickStep {
  cmd?: string;
  text: string;
}

export const quickstart = {
  label: "bosun · Start in 5 minutes",
  heading: "From your repo to the first plan in nine steps.",
  body: "For humans, on a computer, in a clean checkout of your repo’s default branch. You need Node, the gh CLI signed in, and admin on the repo.",
  steps: [
    {
      cmd: `npm i -g ${bosunNpm}`,
      text: `Install bosun (or cargo install ${bosunCrate}). ${released ? `To set up every tool in one go, run npx ${keel.npm} up (crate ${keel.crate}).` : ""}`.trim(),
    },
    { cmd: "export GITHUB_TOKEN=$(gh auth token)", text: "A token with repo and workflow scopes." },
    {
      cmd: "bosun adopt --owner <you>",
      text: "Writes the bosun files, makes the labels and the registry issue, and prints the exact git commands. It commits nothing.",
    },
    {
      cmd: "bosun keygen --enroll",
      text: "Your approval key. The public half goes into .bosun/approvers.toml; the private half stays outside every repo.",
    },
    { text: "Run the git commands adopt printed, open the setup PR and merge it yourself." },
    {
      text: "Install the Claude GitHub App, set branch protection with the link adopt prints (required check ci-ok, or bosun-ok if your repo already has a ci-ok job; adopt’s printed step names the right one), and turn on Allow auto-merge.",
    },
    {
      cmd: "bosun setup-snippet",
      text: "Paste its output once as the cloud environment’s setup script. You never paste it again: it runs each repo’s own pinned script.",
    },
    {
      cmd: "bosun doctor",
      text: "Do each fix it names, then run it again, until only plan is left.",
    },
    { text: "Write your first plan. Agents take it from there." },
  ] satisfies QuickStep[] as QuickStep[],
  more: { label: "The full path, with undo", href: "/docs/start/" },
  agents: {
    heading: "For agents",
    body: "Agents call bosun commands and act on the one JSON object each prints. Do what error.fix says; make the calls in next.",
    fields: ["ok", "exit_code", "data", "error.fix", "next"],
    exitCodes: [
      [0, "ok"],
      [1, "error"],
      [2, "usage"],
      [3, "conflict"],
      [4, "halted"],
      [5, "needs-human"],
      [6, "limit"],
    ] as [number, string][],
  },
};

// "Pick your path" cards on the landing page: one per scenario of /docs/start, linking to its anchor.
export const pickPath = {
  label: "Pick your path",
  heading: "One short block for each thing you want to do.",
  cards: scenarios.map((s) => ({ ...s, href: `/docs/start/#${s.id}` })),
};
