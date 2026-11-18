import { expect, test } from "vite-plus/test";
import { installBlock } from "../data/scenarios.ts";
import { docRefs, tagCommits, versions } from "../data/versions.ts";
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
  expect(ctx["short.bosun"]).toHaveLength(7);
  expect(ctx["ref.bosunSpec"]).toBe(docRefs.bosunSpec);
  expect(ctx["ref.spliceAction"]).toBe(docRefs.spliceAction);
  expect(ctx["ref.spliceActionVersion"]).toBe(docRefs.spliceActionVersion);
  // The action pin is what bosun's channel seeds: a full commit SHA, and the splice tag's own
  // commit while bosun seeds the current splice release.
  expect(ctx["ref.spliceAction"]).toMatch(/^[0-9a-f]{40}$/);
  if (docRefs.spliceActionVersion === versions.splice) {
    expect(ctx["ref.spliceAction"]).toBe(tagCommits.splice);
  }
  expect(ctx.verified).toContain("Last verified");
  expect(ctx["block.adoptHullBlock"]).toBeUndefined();
  const withAssess = buildCtx(v, { ...docRefs, assessUrl: "/assess/a.mjs", assessSha: "abc" });
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

test("commit, short and verified derive from the arguments", () => {
  const c = buildCtx(
    { ...v, bosun: "9.9.9" } as unknown as typeof versions,
    docRefs,
    { ...tagCommits, bosun: "abcdef1234" },
    "2030-01-01",
  );
  expect(c["commit.bosun"]).toBe("abcdef1234");
  expect(c["short.bosun"]).toBe("abcdef1");
  expect(c.verified).toBe(
    `Last verified 2030-01-01 against bosun 9.9.9, hull ${v.hull}, splice ${v.splice}.`,
  );
});
