// Page copy for the tool cards. Names follow D-001 (org and npm scope `keelstackhq`).

export const org = {
  name: "keelstackhq",
  url: "https://github.com/keelstackhq",
} as const;

export interface Tool {
  name: "hull" | "splice" | "hoard" | "bosun" | "hoist" | "berth";
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
    status: "v0.5.0",
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
    status: "v0.3.1",
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
    status: "v0.5.0",
    statusStyle: "dashed",
    pitch:
      "A Turborepo (and Vite+) remote cache for GitHub Actions and CircleCI. The cache never fails your build: an error is a miss.",
    // The README pins `@v0` while hoard is 0.x.
    detail: "uses: keelstackhq/hoard@v0 · GitHub Actions cache or your own R2 bucket",
    detailMono: true,
    repo: `${org.url}/hoard`,
  },
  {
    name: "bosun",
    role: "04 · agent orchestration",
    status: "v0.3.0",
    statusStyle: "outline",
    pitch:
      "Runs an agent-built repo on GitHub: claims and leases, waves, gates, risk checks, review verdicts and approvals. Agents act on one JSON object per command.",
    detail: "claim · pr · gate · risk-check · verdict · approve",
    detailMono: true,
    repo: `${org.url}/bosun`,
  },
  {
    name: "hoist",
    role: "05 · provision + release",
    status: "v0.3.0",
    statusStyle: "outline",
    pitch:
      "Provisions and verifies a repo's GitHub and Cloudflare setup from what the repo already says, and runs its releases.",
    detail: "plan · check · apply · release · secrets",
    detailMono: true,
    repo: `${org.url}/hoist`,
  },
  {
    name: "berth",
    role: "06 · agent workspaces",
    status: "v0.1.0",
    statusStyle: "outline",
    pitch:
      "Gives each parallel agent an isolated workspace: a git worktree, copied env files, a block of ports, a setup hook and its own database.",
    detail: "up · down · ls · env · gc",
    detailMono: true,
    repo: `${org.url}/berth`,
  },
];
