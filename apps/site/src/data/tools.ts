// Page copy. Every line comes from the tool's README or design spec; gaps are TODO(copy).
// Names follow D-001 (org and npm scope `keelstackhq`), even where a README still says `keelstack`.

export const org = {
  name: "keelstackhq",
  url: "https://github.com/keelstackhq",
} as const;

export interface Tool {
  name: "hull" | "splice" | "hoard";
  role: string;
  summary: string;
  facts: string[];
  status: string;
  repo: string;
}

export const tools: Tool[] = [
  {
    name: "hull",
    role: "Monorepo template",
    summary:
      "An opinionated, AI-native monorepo template. Pick the pieces you need; hull owns the structure, stack, tool versions and config, and keeps them current. Your app code stays yours.",
    facts: [
      "pnpm, Turborepo, TypeScript 7, Vite+, Preact, Astro, Rust, Tauri, Hono on Cloudflare",
      "One-job CI plus a ci-ok check; Turbo runs only affected tasks",
      "Built for Claude Code: a lean CLAUDE.md, subagents, skills and a /loop default",
    ],
    status: "Pre-0.1, no release tag yet",
    repo: `${org.url}/hull`,
  },
  {
    name: "splice",
    role: "Bootstrap and update",
    summary:
      "Start a repo from versioned template pieces and keep it up to date. A Copier alternative in one static binary: no Python, no Rust toolchain.",
    facts: [
      "splice new, add, update, check, diff and explain",
      "On update, owned files are replaced, shared files are 3-way merged, seed files stay yours",
      "Every command takes --json and --yes; an update bot opens one pull request",
    ],
    status: "Under construction",
    repo: `${org.url}/splice`,
  },
  {
    name: "hoard",
    role: "Cached CI",
    summary:
      "A Turborepo (and Vite+) remote cache for GitHub Actions, in one line of YAML. No account, no server to host.",
    facts: [
      "Artifacts go to the GitHub Actions cache (default, zero config) or your own Cloudflare R2 bucket",
      "Serves the Turborepo remote-cache API and exports TURBO_API, TURBO_TOKEN and TURBO_TEAM",
      "The cache never fails your build: a storage error is a miss or a skipped upload",
    ],
    status: "Not released yet",
    repo: `${org.url}/hoard`,
  },
];

export interface FitStep {
  tool: Tool["name"];
  title: string;
  body: string;
  code: string;
}

export const fitSteps: FitStep[] = [
  {
    tool: "hull",
    title: "Template",
    body: "hull defines the structure, stack, tool versions and config as pieces: base, preact-app, ui-storybook, astro-site, hono-worker, rust-tool, tauri-app, docs.",
    code: "gh:keelstackhq/hull//template",
  },
  {
    tool: "splice",
    title: "Bootstrap and update",
    body: "splice renders the pieces you pick into a new repo, then moves it to newer template versions. Your edits are merged, never lost.",
    code: "splice new · splice update · splice check",
  },
  {
    tool: "hoard",
    title: "Cached CI",
    body: "hoard gives the generated repo's CI a shared Turborepo cache. Later runs replay every unchanged task.",
    code: "uses: keelstackhq/hoard@v1",
  },
];

// From hull's README "Quick start": the only path that works before splice 0.1 ships.
export const quickStart = [
  "node scripts/render.mjs new --template template --out ../my-repo \\",
  "  --piece base --piece preact-app:web --piece hono-worker:api --data scope=acme",
  "cd ../my-repo && git init && pnpm install && pnpm run ci",
].join("\n");

// From hull's README: the planned equivalent once splice 0.1 is out (flags unconfirmed).
export const spliceStart =
  "pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template my-repo --piece ...";
