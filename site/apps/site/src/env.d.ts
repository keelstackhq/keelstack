declare module "*.css";

// Minimal typing for the docs test (the site has no @types/node).
declare module "node:fs" {
  export function readdirSync(dir: URL): string[];
  export function readFileSync(file: URL, encoding: "utf8"): string;
}

// Vite's import.meta.glob, used by src/lib/docs-md.ts (the site has no vite/client types).
interface ImportMeta {
  glob(
    pattern: string,
    options?: { query?: string; import?: string; eager?: boolean },
  ): Record<string, unknown>;
}
