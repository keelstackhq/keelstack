// Page copy from the design reference. Names follow D-001 (`keelstackhq`).
import { buildCommand, DEFAULT_ON, SPLICE_VERSION } from "../lib/command";
import { org } from "./tools";

export interface HeroVariant {
  lead: string;
  highlight: string;
}

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

// Real `splice new` run (splice 0.3.1, hull v0.5.1, the default pieces). `output` is an excerpt of
// its 60 create lines, in output order; `summary` is our `#` note counting them, not splice output.
// The generated repo pins Node 24 (.node-version) and pnpm 12.11.2 (packageManager). Re-record when the versions change.
export const terminal = {
  title: `~ · splice ${SPLICE_VERSION}`,
  command: buildCommand(new Set(DEFAULT_ON))
    .split("\n")
    .map((l) => l.trim()),
  output: [
    { path: ".github/workflows/ci.yml", cls: "owned" },
    { path: "AGENTS.md", cls: "seed" },
    { path: "CLAUDE.md", cls: "owned" },
    { path: "apps/api/src/index.ts", cls: "seed" },
    { path: "apps/web/src/app.tsx", cls: "seed" },
  ] as { path: string; cls: "owned" | "shared" | "seed" }[],
  summary: "# … 60 files: 36 owned, 8 shared, 16 seed; then pnpm install",
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
    "TypeScript 7",
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

export const agentRows: { key: string; text: string }[] = [
  {
    key: "CLAUDE.md",
    text: "Lean by default. Skills and a /loop default ship with the template; subagents come from hull’s rigging channel.",
  },
  {
    key: "ready",
    text: "hull’s ready skill asks the onboarding questions as forms and checks every owner step before agents start work alone.",
  },
  {
    key: "bosun",
    text: "Claims, gates, risk checks and review verdicts on GitHub. Child sessions never merge or tag; the orchestrator or auto-merge merges on green, and owners approve high-risk changes, also by chat.",
  },
  { key: "--json --yes", text: "Every splice command takes both. No prompts, parseable results." },
  { key: "ci-ok", text: "One job, one check to wait on. Turbo runs only affected tasks." },
  { key: "update bot", text: "Template moves; you get one pull request to review." },
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

// `body` is plain text; the 05 intro mentions `pnpm install` as inline code in the reference.
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
    label: "02 · Six tools",
    heading: "Template. Bootstrap. Cache. Orchestrate. Provision. Isolate.",
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
    body: "splice renders hull’s base plus the pieces you pick, writes the lock, then runs pnpm install. Needs Node 24 and pnpm 12.",
    links: [
      {
        label: "Start a project",
        href: "/docs/start-project/",
        text: "Every owner step before agents work on a new repo, and how hull’s ready check (since hull v0.5.0) verifies it.",
      },
      {
        label: "Greenfield repo with bosun",
        href: "/docs/greenfield-bosun/",
        text: "The files, where the spec and the plan go, the step order and a first session prompt.",
      },
    ],
  },
};

export const footerLinks: { label: string; href: string }[] = [
  { label: "hull", href: `${org.url}/hull` },
  { label: "splice", href: `${org.url}/splice` },
  { label: "hoard", href: `${org.url}/hoard` },
  { label: "bosun", href: `${org.url}/bosun` },
  { label: "hoist", href: `${org.url}/hoist` },
  { label: "berth", href: `${org.url}/berth` },
  { label: "decisions", href: `${org.url}/decisions` },
];

export const nav: { label: string; href: string }[] = [
  { label: "Tools", href: "#tools" },
  { label: "For agents", href: "#agents" },
  { label: "Get started", href: "#start" },
  { label: "Docs", href: "/docs/" },
  { label: "GitHub ↗", href: org.url },
];
