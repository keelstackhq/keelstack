// main's hook entry for a guard-legacy.mjs (argv[2]): reads stdin once and emits what
// `judgeRaw(raw)` (main's main() body verbatim) decides, as main's hook did.
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const { judgeRaw } = await import(pathToFileURL(process.argv[2]).href);
let raw = "";
try {
  raw = readFileSync(0, "utf8");
} catch {
  raw = "";
}
const d = judgeRaw(raw);
if (d)
  process.stdout.write(
    `${JSON.stringify({
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: d.decision,
        permissionDecisionReason: d.reason,
      },
    })}\n`,
  );
