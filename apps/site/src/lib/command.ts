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

// The versions the hero output was recorded with; bump both together.
export const SPLICE_VERSION = "0.2.1";
export const HULL_REF = "v0.2.0";

export const CAPTION = `Pinned to hull ${HULL_REF}. Drop --ref to get the latest hull release.`;

// Selected pieces, in PIECES order regardless of insertion order.
function selectedArgs(on: ReadonlySet<string>): string[] {
  return PIECES.filter((p) => on.has(p.name)).map((p) => `--piece ${p.piece}:${p.name}`);
}

export function buildCommand(on: ReadonlySet<string>): string {
  const lines = [
    `pnpm dlx @keelstackhq/splice@${SPLICE_VERSION} new`,
    `gh:keelstackhq/hull//template my-repo --ref ${HULL_REF}`,
  ];
  const pieces = selectedArgs(on);
  if (pieces.length > 0) lines.push(pieces.join(" "));
  return lines.join(" \\\n  ");
}
