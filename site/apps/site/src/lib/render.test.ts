import { expect, test } from "vite-plus/test";
import { installBlock } from "../data/scenarios.ts";
import { links, sources, versions } from "../data/versions.ts";
import { buildCtx, renderTemplate } from "./render.ts";

const v = versions;
const ctx = buildCtx();

test("replaces a token", () => {
  expect(renderTemplate("splice {{ks.splice}}", ctx, "x.md")).toBe(`splice ${v.splice}`);
});

test("block tokens render the fenced builder output", () => {
  expect(renderTemplate("{{ks.block.installBlock}}", ctx, "x.md")).toBe(
    "```bash\n" + installBlock(v) + "\n```",
  );
});

test("derived keys", () => {
  expect(ctx["bare.hull"]).toBe(v.hull.replace(/^v/, ""));
  // Sources and links come from versions.ts (derived from facts.ts), one token each.
  for (const [k, val] of Object.entries(sources)) expect(ctx[`src.${k}`]).toBe(val);
  for (const [k, val] of Object.entries(links)) expect(ctx[`link.${k}`]).toBe(val);
  expect(ctx["link.main"]).toBe("https://github.com/keelstackhq/keelstack/blob/main");
  expect(ctx["commit.hull"]).toBeUndefined();
  expect(ctx["block.adoptHullBlock"]).toBeUndefined();
  const withAssess = buildCtx(v, { assessUrl: "/assess/a.mjs", assessSha: "abc" });
  expect(withAssess["block.adoptHullBlock"]).toContain("/assess/a.mjs");
  expect(withAssess["assess.url"]).toBe("/assess/a.mjs");
  expect(withAssess["assess.sha"]).toBe("abc");
});

test("GitHub expressions pass through", () => {
  const s = "token: ${{ secrets.X }}";
  expect(renderTemplate(s, ctx, "x.md")).toBe(s);
});

test("unknown token throws with file and line", () => {
  expect(() => renderTemplate("a\nb\nc {{ks.nope}}", ctx, "x.md")).toThrow(
    "gen-docs: unknown token {{ks.nope}} in docs-src/x.md:3",
  );
});

test("output has no tokens left", () => {
  expect(renderTemplate("{{ks.hull}} {{ks.bosun}}", ctx, "x.md")).not.toContain("{{ks.");
});

test("a token with inner spaces is not accepted", () => {
  expect(() => renderTemplate("{{ks. splice }}", ctx, "x.md")).toThrow("unknown token");
});

test("versions and verified derive from the arguments", () => {
  const c = buildCtx({ ...v, bosun: "9.9.9" }, {}, "2030-01-01");
  expect(c.bosun).toBe("9.9.9");
  expect(c["bare.hull"]).toBe(v.hull.replace(/^v/, ""));
  expect(c.verifiedOn).toBe("2030-01-01");
});
