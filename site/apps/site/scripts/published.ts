// Prints the first candidate version (arguments, in order) that is fully published on npm, or an
// empty line (deploy-site.yml: the page then shows the next lower release, else legacy). Why a
// candidate failed goes to stderr.
//   node apps/site/scripts/published.ts 0.23.0 0.22.0
import { findRoot } from "../src/data/facts.ts";
import { launchersFor, resolvePublished } from "../src/lib/published.ts";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const BACKOFF = [1000, 3000];

/** 200 -> JSON, 404 -> undefined; 429, 5xx, network errors and timeouts retry (1s, 3s), then throw. */
async function fetchJson(path: string): Promise<unknown | undefined> {
  const seg = path.split("/");
  const name = path.startsWith("@") ? `${seg[0]}%2f${seg[1]}` : seg[0];
  const version = seg[path.startsWith("@") ? 2 : 1];
  const url = `https://registry.npmjs.org/${name}${version ? `/${version}` : ""}`;
  let last: unknown;
  for (let attempt = 0; attempt <= BACKOFF.length; attempt++) {
    if (attempt > 0) await sleep(BACKOFF[attempt - 1]!);
    try {
      const res = await fetch(url, {
        headers: { accept: "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8" },
        signal: AbortSignal.timeout(15000),
      });
      if (res.status === 404) return undefined;
      if (res.status === 200) return await res.json();
      last = new Error(`${url}: HTTP ${res.status}`);
      // Other 4xx answers will not change: stop; 429 and 5xx retry.
      if (res.status !== 429 && res.status < 500) throw last;
    } catch (e) {
      if (e === last) {
        if (!(e instanceof Error) || !e.message.endsWith("HTTP 429")) {
          if (!/HTTP 5\d\d/.test((e as Error).message)) throw e;
        }
      }
      last = e;
    }
  }
  throw last;
}

const resolved = await resolvePublished(
  process.argv.slice(2),
  launchersFor(findRoot()),
  fetchJson,
  (m) => console.error(`published: ${m}`),
);
console.log(resolved);
