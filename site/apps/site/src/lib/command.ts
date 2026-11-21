// Browser-safe: no fs, no facts import. The data comes from lib/picker-data.ts at build time and
// reaches the island as a prop.
export interface Piece {
  piece: string;
  name: string;
}

export interface PickerData {
  splicePackage: string;
  spliceVersion: string;
  hullSource: string;
  hullRef: string;
  pieces: readonly Piece[];
  defaultOn: readonly string[];
}

export const caption = (d: PickerData): string =>
  `Pinned to hull ${d.hullRef}. Drop --ref to get the latest hull release.`;

// Selected pieces, in `pieces` order regardless of insertion order.
function selectedArgs(d: PickerData, on: ReadonlySet<string>): string[] {
  return d.pieces.filter((p) => on.has(p.name)).map((p) => `--piece ${p.piece}:${p.name}`);
}

export function buildCommand(d: PickerData, on: ReadonlySet<string>): string {
  const lines = [
    `pnpm dlx ${d.splicePackage}@${d.spliceVersion} new`,
    `${d.hullSource} my-repo --ref ${d.hullRef}`,
  ];
  const pieces = selectedArgs(d, on);
  if (pieces.length > 0) lines.push(pieces.join(" "));
  return lines.join(" \\\n  ");
}
