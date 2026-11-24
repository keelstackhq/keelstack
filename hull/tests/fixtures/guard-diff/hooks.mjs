// Module hooks for guard-differential.test.mjs: every template guard.mjs is loaded with its
// exported `decide` wrapped to record each call the hull guard tests make (the hook input and the
// project dir, one JSON line per call to GUARD_DIFF_OUT). Recording only: the differential test
// replays each call as the hook's raw stdin through both judges.

const WRAP = `
import { appendFileSync as __diffAppend } from "node:fs";
export function decide(input, ctx) {
  if (process.env.GUARD_DIFF_OUT)
    __diffAppend(
      process.env.GUARD_DIFF_OUT,
      JSON.stringify({ input, projectDir: ctx?.projectDir ?? null }) + "\\n",
    );
  return __diffDecide(input, ctx);
}
`;

export async function load(url, context, nextLoad) {
  const r = await nextLoad(url, context);
  if (!url.endsWith("/pieces/base/files/.claude/hooks/guard.mjs")) return r;
  const src = String(r.source);
  const head = "export function decide(input, ctx) {";
  if (!src.includes(head)) throw new Error(`guard-diff hooks: no decide() in ${url}`);
  return { ...r, source: src.replace(head, "function __diffDecide(input, ctx) {") + WRAP };
}
