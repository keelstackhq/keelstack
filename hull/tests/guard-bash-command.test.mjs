// A Bash call whose `command` is not a string is never an enumerated intended allow: guard.mjs
// does not coerce it (`String(["gh api ..."])` would read as the allowed shape), so a legacy deny
// stays final. Not reachable from Claude Code (its Bash command is always a string); run against
// both template copies.
import assert from "node:assert/strict";
import { test } from "node:test";

const COPIES = ["template", "template-rust"];
const ALLOWED = "gh api -X POST repos/o/r/pulls -f title=t -F body=@pr.md";
const legacy = { judgeRaw: () => ({ decision: "deny", reason: "legacy says no" }) };
const head = () => null;
const raw = (command) => JSON.stringify({ tool_name: "Bash", tool_input: { command } });

for (const copy of COPIES) {
  const mod = await import(`../${copy}/pieces/base/files/.claude/hooks/guard.mjs`);

  test(`${copy}: a string command of the allowed shape passes the legacy deny`, () => {
    assert.equal(mod.main(raw(ALLOWED), { legacy, head }), null);
  });

  test(`${copy}: a non-string command is never coerced into an intended allow`, () => {
    for (const command of [[ALLOWED], { toString: () => ALLOWED }, 42, null]) {
      const got = mod.main(raw(command), { legacy, head });
      assert.equal(got?.decision, "deny", JSON.stringify(command));
    }
  });
}
