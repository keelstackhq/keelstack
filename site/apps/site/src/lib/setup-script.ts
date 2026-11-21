// Reads what bosun's copied setup script pins (data/bosun/), so the docs that describe it are derived.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export interface SetupScript {
  bosun: string;
  splice: string;
  hoist: string;
  berth: string;
  commit: string;
  short: string;
}

export function parseSetupScript(sh: string, json: string): SetupScript {
  const pin = (name: string): string => {
    const m = sh.match(new RegExp(`^${name}_VERSION=(\\d+\\.\\d+\\.\\d+)\\s*$`, "m"));
    if (!m) throw new Error(`setup-script.sh: no ${name}_VERSION=X.Y.Z line`);
    return m[1] as string;
  };
  const commit = (JSON.parse(json) as { ref: string }).ref;
  return {
    bosun: pin("BOSUN"),
    splice: pin("SPLICE"),
    hoist: pin("HOIST"),
    berth: pin("BERTH"),
    commit,
    short: commit.slice(0, 7),
  };
}

/** `src/data/bosun/` of this app, found upwards (from the source, or from Astro's bundled chunk). */
function bosunData(): string {
  let dir = dirname(fileURLToPath(import.meta.url));
  for (;;) {
    const d = join(dir, "src", "data", "bosun");
    if (existsSync(join(d, "setup-script.json"))) return d;
    const up = dirname(dir);
    if (up === dir) throw new Error("setup-script: src/data/bosun not found");
    dir = up;
  }
}

export const readSetupScript = (): SetupScript => {
  const d = bosunData();
  return parseSetupScript(
    readFileSync(join(d, "setup-script.sh"), "utf8"),
    readFileSync(join(d, "setup-script.json"), "utf8"),
  );
};
