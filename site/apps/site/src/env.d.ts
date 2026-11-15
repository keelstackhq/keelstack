declare module "*.css";

// Minimal typing for the docs test (the site has no @types/node).
declare module "node:fs" {
  export function readdirSync(dir: URL): string[];
  export function readFileSync(file: URL, encoding: "utf8"): string;
  export function readFileSync(file: URL): Uint8Array;
  export function existsSync(file: URL): boolean;
}

// Minimal typing for the budget test's gzip size.
declare module "node:zlib" {
  export function gzipSync(data: Uint8Array, options?: { level?: number }): Uint8Array;
}

// Minimal typing for the start-checklist test's sha256.
declare module "node:crypto" {
  export function createHash(algorithm: "sha256"): {
    update(data: string): { digest(encoding: "hex"): string };
  };
}

// Vite's import.meta.glob, used by src/lib/docs-md.ts (the site has no vite/client types).
interface ImportMeta {
  glob(
    pattern: string,
    options?: { query?: string; import?: string; eager?: boolean },
  ): Record<string, unknown>;
}
