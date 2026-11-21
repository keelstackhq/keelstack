// Reads what bosun's copied setup script pins (data/bosun/), so the docs that describe it are derived.
import { readFileSync } from "node:fs";

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

export const readSetupScript = (): SetupScript =>
  parseSetupScript(
    readFileSync(new URL("../data/bosun/setup-script.sh", import.meta.url), "utf8"),
    readFileSync(new URL("../data/bosun/setup-script.json", import.meta.url), "utf8"),
  );
