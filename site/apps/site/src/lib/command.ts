export type Mode = "today" | "splice";

export interface Piece {
  piece: string;
  name: string;
}

export const PIECES: readonly Piece[] = [
  { piece: "preact-app", name: "web" },
  { piece: "hono-worker", name: "api" },
  { piece: "ui-storybook", name: "ui" },
  { piece: "astro-site", name: "site" },
  { piece: "rust-tool", name: "cli" },
  { piece: "tauri-app", name: "desktop" },
  { piece: "docs", name: "docs" },
];

export const DEFAULT_ON: readonly string[] = ["web", "api"];

export const CAPTIONS: Record<Mode, string> = {
  today: "In a hull checkout.",
  splice: "Preview: flags unconfirmed until splice 0.1 ships.",
};

// Selected pieces, in PIECES order regardless of insertion order.
function selectedArgs(on: ReadonlySet<string>): string[] {
  return PIECES.filter((p) => on.has(p.name)).map((p) => `--piece ${p.piece}:${p.name}`);
}

export function buildCommand(on: ReadonlySet<string>, mode: Mode): string {
  const pieces = selectedArgs(on);
  if (mode === "today") {
    const args = ["--piece base", ...pieces, "--data scope=acme"].join(" ");
    return [
      "node scripts/render.mjs new --template template --out ../my-repo \\",
      `  ${args}`,
      "cd ../my-repo && git init && pnpm install && pnpm run ci",
    ].join("\n");
  }
  const head = "pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template my-repo";
  return pieces.length === 0 ? head : `${head} \\\n  ${pieces.join(" ")}`;
}
