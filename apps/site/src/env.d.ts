declare module "*.css";

// Minimal typing for the docs test (the site has no @types/node).
declare module "node:fs" {
  export function readdirSync(dir: URL): string[];
  export function readFileSync(file: URL, encoding: "utf8"): string;
}
