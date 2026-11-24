// Which release the site can show: a version counts only when the npm packages its commands install
// were published at it. Pure, no network: the registry lookup is injected (scripts/published.ts).
// .ts import extensions so Node runs it directly.
import { loadFacts } from "../data/facts.ts";

const SEMVER = /^\d+\.\d+\.\d+$/;

/** The npm launcher packages the site's commands install (from the built commit). */
export function launchersFor(root: string): string[] {
  return loadFacts(root, "")
    .tools.map((t) => t.npm)
    .filter((n): n is string => !!n);
}

const parts = (v: string): number[] => v.split(".").map(Number);
function lte(a: string, b: string): boolean {
  const [x, y] = [parts(a), parts(b)];
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return (x[i] ?? 0) < (y[i] ?? 0);
  return true;
}

/**
 * GET a registry document: the parsed JSON, `undefined` for 404 (not there), a throw for anything
 * else (the caller counts that as not published). `path` is `<package>` or `<package>/<version>`.
 */
export type FetchJson = (path: string) => Promise<unknown | undefined>;

type Doc = { versions?: Record<string, unknown>; optionalDependencies?: Record<string, string> };

/**
 * Whether `version` is fully published. A launcher is required when npm lists a version of it at or
 * below `version` (the tool existed by then; a tool added later is not); each required launcher must
 * have `version`, and so must every platform package its own manifest at that version names. At
 * least one launcher must be required. Throws on a registry failure.
 */
export async function isPublished(
  version: string,
  launchers: string[],
  fetchJson: FetchJson,
  why: (msg: string) => void = () => {},
): Promise<boolean> {
  let required = 0;
  for (const launcher of launchers) {
    const packument = (await fetchJson(launcher)) as Doc | undefined;
    const existed = Object.keys(packument?.versions ?? {}).some(
      (v) => SEMVER.test(v) && lte(v, version),
    );
    if (!existed) continue;
    required++;
    const manifest = (await fetchJson(`${launcher}/${version}`)) as Doc | undefined;
    if (!manifest) {
      why(`${launcher}@${version} is not published`);
      return false;
    }
    for (const dep of Object.keys(manifest.optionalDependencies ?? {})) {
      if (!(await fetchJson(`${dep}/${version}`))) {
        why(`${dep}@${version} is not published`);
        return false;
      }
    }
  }
  if (required === 0) why(`no launcher existed at ${version}`);
  return required > 0;
}

/**
 * The first candidate (in the given order; only X.Y.Z counts) that is fully published, else "".
 * A throwing or rejecting `fetchJson` counts as "not published": never throws.
 */
export async function resolvePublished(
  candidates: string[],
  launchers: string[],
  fetchJson: FetchJson,
  why: (msg: string) => void = () => {},
): Promise<string> {
  for (const version of candidates) {
    if (!SEMVER.test(version)) continue;
    try {
      if (await isPublished(version, launchers, fetchJson, why)) return version;
    } catch (e) {
      why(`${version}: registry check failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  return "";
}
