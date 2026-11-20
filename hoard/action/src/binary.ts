// Downloads, verifies and caches the hoard binary (spec §10). Never runs a binary whose
// SHA-256 is not baked into this action build.
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { access, chmod, rm } from "node:fs/promises";
import { join } from "node:path";
import { pipeline } from "node:stream/promises";
import { setTimeout as delay } from "node:timers/promises";
import * as tc from "@actions/tool-cache";
import type { Target } from "./platform.js";

// Downloads come from the action's repo at tag v<version>. The monorepo (keelstackhq/keelstack)
// releases hoard from its first release after v0.21.0; every hoard release up to v0.21.0 lives
// in the old repo keelstackhq/hoard (the last is v0.6.1), so such a version is fetched there
// even from the monorepo (a workspace `uses: ./hoard` before the first monorepo release).
const MONOREPO = "keelstackhq/keelstack";
const OLD_REPO = "keelstackhq/hoard";
const LAST_PRE_MONOREPO = [0, 21, 0];
// owner/repo; "." and ".." segments would change the URL path, so they are refused too.
const REPO_RE = /^[\w.-]+\/[\w.-]+$/;
const isRepo = (s: string) => REPO_RE.test(s) && s.split("/").every((p) => p !== "." && p !== "..");

/** True for a version released before the monorepo (<= 0.21.0, pre-releases of those too). */
export function preMonorepo(version: string): boolean {
  const v = version.split(/[.+-]/).slice(0, 3).map(Number);
  if (v.length < 3 || v.some((n) => !Number.isInteger(n))) return false;
  const i = v.findIndex((n, k) => n !== LAST_PRE_MONOREPO[k]);
  return i < 0 || v[i] < LAST_PRE_MONOREPO[i];
}

/** Release asset URL, from the action's own repository when GitHub names it. */
export function releaseUrl(target: Target, version: string, env: NodeJS.ProcessEnv): string {
  const fromEnv = env.GITHUB_ACTION_REPOSITORY ?? "";
  let repo = isRepo(fromEnv) ? fromEnv : MONOREPO;
  if (repo.toLowerCase() === MONOREPO && preMonorepo(version)) repo = OLD_REPO;
  return `https://github.com/${repo}/releases/download/v${version}/hoard-${target}.tar.gz`;
}

/**
 * The release archive could not be downloaded (after one retry, an HTTP 4xx, or the 90 s cap). Transient (network, a
 * github.com outage), so main goes on without the cache instead of failing the build (§10).
 * A checksum mismatch or a missing checksum is never this error.
 */
export class BinaryDownloadError extends Error {
  override name = "BinaryDownloadError";
}

/** Download timing; injectable for tests. */
export interface DownloadOptions {
  /** Wait before the one retry (default 2 s). */
  retryDelayMs?: number;
  /** Cap for the whole download, retry included (default 90 s). */
  timeoutMs?: number;
  sleep?: (ms: number) => Promise<unknown>;
}

const msText = (ms: number) => (ms % 1000 === 0 ? `${ms / 1000}s` : `${ms}ms`);

/** An HTTP 4xx from @actions/tool-cache (HTTPError.httpStatusCode): not transient. */
function isClientError(e: unknown): boolean {
  const code = (e as { httpStatusCode?: unknown } | null)?.httpStatusCode;
  return typeof code === "number" && code >= 400 && code < 500;
}

const asDownloadError = (e: unknown) =>
  new BinaryDownloadError(e instanceof Error ? e.message : String(e), { cause: e });

/**
 * One attempt, plus one retry unless the server answered 4xx. tc.downloadTool already retries
 * internally (10–20 s apart), so more outer attempts would only stall the job.
 */
async function downloadWithRetry(
  url: string,
  download: (url: string) => Promise<string>,
  retryDelayMs: number,
  sleep: (ms: number) => Promise<unknown>,
): Promise<string> {
  try {
    return await download(url);
  } catch (e) {
    if (isClientError(e)) throw asDownloadError(e);
    await sleep(retryDelayMs);
  }
  try {
    return await download(url);
  } catch (e) {
    throw asDownloadError(e);
  }
}

/** `downloadWithRetry`, but a BinaryDownloadError once `timeoutMs` has passed. */
async function downloadCapped(
  url: string,
  download: (url: string) => Promise<string>,
  opts: DownloadOptions,
): Promise<string> {
  const timeoutMs = opts.timeoutMs ?? 90_000;
  const attempt = downloadWithRetry(
    url,
    download,
    opts.retryDelayMs ?? 2_000,
    opts.sleep ?? ((ms) => delay(ms)),
  );
  let timer: NodeJS.Timeout | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new BinaryDownloadError(`no answer within ${msText(timeoutMs)}`)),
      timeoutMs,
    );
  });
  try {
    return await Promise.race([attempt, timeout]);
  } catch (e) {
    // A download that lands after the cap is not used; delete what it wrote.
    attempt.then((f) => rm(f, { force: true })).catch(() => {});
    throw e;
  } finally {
    clearTimeout(timer);
  }
}

async function sha256(file: string): Promise<string> {
  const hash = createHash("sha256");
  await pipeline(createReadStream(file), hash);
  return hash.digest("hex");
}

/**
 * Returns the path of a verified hoard executable for `target`. The tool cache is keyed by the
 * target (used as the tool-cache "arch"); an entry there was verified before it was stored, and
 * it is only reused on GitHub-hosted runners.
 */
export async function ensureBinary(
  target: Target,
  version: string,
  checksums: Partial<Record<Target, string>>,
  download: (url: string) => Promise<string> = (url) => tc.downloadTool(url),
  env: NodeJS.ProcessEnv = process.env,
  opts: DownloadOptions = {},
): Promise<string> {
  const expected = checksums[target];
  if (!expected) {
    throw new Error(`no verified hoard binary for ${target} in this action build (${version})`);
  }
  const exeName = target.endsWith("-windows-msvc") ? "hoard.exe" : "hoard";

  // Only a GitHub-hosted runner's tool cache is fresh per job; on a self-hosted runner it
  // persists and anything could have been put there, so the binary is downloaded and verified.
  if (env.RUNNER_ENVIRONMENT === "github-hosted") {
    const cached = tc.find("hoard", version, target);
    if (cached) return join(cached, exeName);
  }

  const archive = await downloadCapped(releaseUrl(target, version, env), download, opts);
  try {
    if ((await sha256(archive)) !== expected.toLowerCase()) {
      throw new Error(`checksum mismatch for ${target}`);
    }

    const dir = await tc.extractTar(archive);
    const exe = join(dir, exeName);
    try {
      await access(exe);
    } catch {
      throw new Error(`hoard-${target}.tar.gz does not contain ${exeName}`);
    }
    if (exeName === "hoard") await chmod(exe, 0o755);
    const toolDir = await tc.cacheDir(dir, "hoard", version, target);
    return join(toolDir, exeName);
  } finally {
    await rm(archive, { force: true });
  }
}
