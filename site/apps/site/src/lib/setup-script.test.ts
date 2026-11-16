import { readFileSync } from "node:fs";
import { expect, test } from "vite-plus/test";
import { buildCtx } from "./render.ts";
import { parseSetupScript } from "./setup-script.ts";

const text = readFileSync(new URL("../data/bosun/setup-script.sh", import.meta.url), "utf8");
const json = readFileSync(new URL("../data/bosun/setup-script.json", import.meta.url), "utf8");

test("finds the four tool pins and the source commit of the copied script", () => {
  const s = parseSetupScript(text, json);
  for (const k of ["bosun", "splice", "hoist", "berth"] as const)
    expect(text).toContain(`${k.toUpperCase()}_VERSION=${s[k]}\n`);
  expect(s.commit).toBe(JSON.parse(json).ref);
  expect(s.short).toBe(s.commit.slice(0, 7));
});

test("throws when a pin is missing", () => {
  expect(() => parseSetupScript(text.replace(/^BERTH_VERSION=.*$/m, ""), json)).toThrow(/BERTH/);
});

test("the context exposes them as script.* tokens", () => {
  const s = parseSetupScript(text, json);
  const ctx = buildCtx();
  expect(ctx["script.bosun"]).toBe(s.bosun);
  expect(ctx["script.splice"]).toBe(s.splice);
  expect(ctx["script.hoist"]).toBe(s.hoist);
  expect(ctx["script.berth"]).toBe(s.berth);
  expect(ctx["script.commit"]).toBe(s.commit);
  expect(ctx["script.short"]).toBe(s.short);
});
