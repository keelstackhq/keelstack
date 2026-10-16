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
    status: "pre-0.1",
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
    status: "under construction",
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
    status: "not released yet",
    statusStyle: "dashed",
    pitch:
      "A Turborepo (and Vite+) remote cache for GitHub Actions. The cache never fails your build: an error is a miss.",
    detail: "GitHub Actions cache by default, or your own Cloudflare R2 bucket",
    detailMono: false,
    repo: `${org.url}/hoard`,
  },
];
