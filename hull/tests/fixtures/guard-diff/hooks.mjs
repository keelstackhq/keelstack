// Module hooks for guard-differential.test.mjs: every template guard.mjs is loaded with its
// exported `decide` wrapped, so each call the hull guard tests make is also judged by the legacy
// judge (guard-legacy.mjs, main's guard). One JSON line per call goes to GUARD_DIFF_OUT: whether
// guard.mjs denied whatever legacy denied (or the command is an enumerated intended allow).

const WRAP = `
import { appendFileSync as __diffAppend } from "node:fs";
import * as __diffLegacy from "./guard-legacy.mjs";
function __diffLegacyDecision(input, ctx) {
  try {
    return __diffLegacy.decideLegacy(input, ctx);
  } catch {
    // main's hook: an internal error denies a risky call
    return __diffLegacy.failClosedLegacy(JSON.stringify(input)) ? { decision: "deny" } : null;
  }
}
export function decide(input, ctx) {
  const got = __diffDecideHead(input, ctx);
  const old = __diffLegacyDecision(input, ctx);
  const command = input?.tool_name === "Bash" ? String(input?.tool_input?.command ?? "") : null;
  const allow = command === null ? null : intendedAllow(command);
  const ok = old?.decision !== "deny" || got?.decision === "deny" || allow !== null;
  if (process.env.GUARD_DIFF_OUT)
    __diffAppend(
      process.env.GUARD_DIFF_OUT,
      JSON.stringify({ ok, tool: input?.tool_name, command, legacy: old?.decision ?? null, head: got?.decision ?? null, allow }) + "\\n",
    );
  return got;
}
`;

export async function load(url, context, nextLoad) {
  const r = await nextLoad(url, context);
  if (!url.endsWith("/pieces/base/files/.claude/hooks/guard.mjs")) return r;
  const src = String(r.source);
  const head = "export function decide(input, ctx) {";
  if (!src.includes(head)) throw new Error(`guard-diff hooks: no decide() in ${url}`);
  return { ...r, source: src.replace(head, "function __diffDecideHead(input, ctx) {") + WRAP };
}
