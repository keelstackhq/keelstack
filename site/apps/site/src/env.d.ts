declare module "*.css";

// Minimal typing for the docs test (the site has no @types/node).
declare module "node:fs" {
  export function readdirSync(dir: URL): string[];
  export function readdirSync(
    dir: string,
    options: { withFileTypes: true },
  ): { name: string; isDirectory(): boolean }[];
  export function readFileSync(file: URL | string, encoding: "utf8"): string;
  export function readFileSync(file: URL): Uint8Array;
  export function existsSync(file: URL): boolean;
  export function mkdtempSync(prefix: string): string;
  export function mkdirSync(dir: string, options?: { recursive?: boolean }): void;
  export function writeFileSync(file: string, data: string): void;
  export function rmSync(path: string, options?: { recursive?: boolean; force?: boolean }): void;
}

// Minimal typing for the docs generator (src/lib/generate.ts) and its drift test.
declare module "node:path" {
  export function dirname(path: string): string;
  export function join(...parts: string[]): string;
  export function relative(from: string, to: string): string;
}
declare module "node:os" {
  export function tmpdir(): string;
}
declare module "node:url" {
  export function fileURLToPath(url: string | URL): string;
}
declare module "node:child_process" {
  type Options = { stdio?: "pipe"; cwd?: string };
  export function execFileSync(
    file: string,
    args: string[],
    options: Options & { encoding: "utf8" },
  ): string;
  export function execFileSync(file: string, args: string[], options?: Options): Uint8Array;
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
