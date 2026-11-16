// Finds literal current pins in a docs-src line (they must be `{{ks.*}}` tokens instead).
const escape = (s: string): string => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

export function hasLiteralPin(
  line: string,
  versions: Record<string, string>,
  commits: Record<string, string>,
): boolean {
  const shas = Object.values(commits).flatMap((c) => [c, c.slice(0, 7)]);
  if (shas.some((s) => new RegExp(`\\b${s}\\b`).test(line))) return true;
  const literals = [
    ...Object.entries(versions).map(([k, v]) => `@keelstackhq/${k}@${v}`),
    `--ref ${versions.hull}`,
  ];
  if (literals.some((l) => new RegExp(`${escape(l)}(?![\\w.])`).test(line))) return true;
  // Bare prose: `bosun 0.18.0`, `hoard v0.6.1`, `@keelstackhq/splice 0.6.0` at the tool's current version.
  return Object.entries(versions).some(([tool, v]) =>
    new RegExp(`\\b${tool}\\s+v?${escape(v.replace(/^v/, ""))}(?!\\w|\\.\\d)`, "i").test(line),
  );
}
