// Page copy for the tool cards. Names follow D-001 (org and npm scope `keelstackhq`).

export const org = {
  name: "keelstackhq",
  url: "https://github.com/keelstackhq",
} as const;

export interface Tool {
  name: "hull" | "splice" | "hoard";
  role: string;
  status: string;
  statusStyle: "filled" | "outline" | "dashed";
  pitch: string;
  detail: string;
  detailMono: boolean;
  repo: string;
}

export const tools: Tool[] = [
  {
    name: "hull",
    role: "01 · monorepo template",
    status: "v0.3.0",
    statusStyle: "filled",
    pitch:
      "An opinionated, AI-native monorepo. Pick the pieces you need; hull owns structure, stack, tool versions and config.",
    detail: "pnpm, Turborepo, TypeScript 7, Vite+, Preact, Astro, Rust, Tauri, Hono on Cloudflare",
    detailMono: false,
    repo: `${org.url}/hull`,
  },
  {
    name: "splice",
    role: "02 · bootstrap + update",
    status: "v0.2.1",
    statusStyle: "outline",
    pitch:
      "Start a repo from versioned template pieces and keep it up to date. A Copier alternative in one static binary.",
    detail: "new · add · update · check · diff · explain",
    detailMono: true,
    repo: `${org.url}/splice`,
  },
  {
    name: "hoard",
    role: "03 · cached CI",
    status: "v0.3.0",
    statusStyle: "dashed",
    pitch:
      "A Turborepo (and Vite+) remote cache for GitHub Actions. The cache never fails your build: an error is a miss.",
    // The README pins `@v0` while hoard is 0.x.
    detail: "uses: keelstackhq/hoard@v0 · GitHub Actions cache or your own R2 bucket",
    detailMono: true,
    repo: `${org.url}/hoard`,
  },
];

// The other keelstack tools, listed in /llms.txt (the landing page shows cards for the three above).
export const moreTools: { name: string; status: string; pitch: string; repo: string }[] = [
  {
    name: "bosun",
    status: "v0.2.3",
    pitch:
      "Orchestration for agent-built repos: claims and leases, waves, gates, risk checks, review verdicts, approvals and cost.",
    repo: `${org.url}/bosun`,
  },
  {
    name: "hoist",
    status: "v0.2.0",
    pitch: "Provisions and verifies a repo's GitHub and Cloudflare setup, and runs releases.",
    repo: `${org.url}/hoist`,
  },
  {
    name: "berth",
    status: "v0.1.0",
    pitch: "An isolated workspace per parallel agent: worktree, env files, ports and a database.",
    repo: `${org.url}/berth`,
  },
];
