import { expect, test } from "vite-plus/test";
import { hasLiteralPin } from "./pins.ts";

const v = { hull: "v0.12.0", splice: "0.5.1" };
const c = {
  hull: "5a72442649f6958e91a50283a91e985f45e10a09",
  splice: "8dcdf9a9faf35cd5b504c326c9107f8da689264e",
};

test("flags a full SHA, a 7-char SHA, --ref vX and @pkg@version", () => {
  expect(hasLiteralPin(`blob/${c.hull}/x`, v, c)).toBe(true);
  expect(hasLiteralPin("at `5a72442` here", v, c)).toBe(true);
  expect(hasLiteralPin("--ref v0.12.0 --piece", v, c)).toBe(true);
  expect(hasLiteralPin("pnpm dlx @keelstackhq/splice@0.5.1 new", v, c)).toBe(true);
});

test("does not flag tokens, longer versions or other text", () => {
  expect(hasLiteralPin("@keelstackhq/splice@0.5.10 new", v, c)).toBe(false);
  expect(hasLiteralPin("--ref v0.12.01", v, c)).toBe(false);
  expect(hasLiteralPin("@keelstackhq/splice@{{ks.bare.splice}}", v, c)).toBe(false);
  expect(hasLiteralPin("5a724421 and x5a72442", v, c)).toBe(false);
});

const all = {
  hull: "v0.13.0",
  splice: "0.6.0",
  bosun: "0.20.0",
  hoist: "0.5.1",
  hoard: "v0.6.1",
  berth: "0.2.1",
};

test("flags bare prose '<tool> X.Y.Z' and '<tool> vX.Y.Z' at the tool's current version", () => {
  for (const line of [
    "This follows bosun 0.20.0.",
    "needs Bosun v0.20.0 or later",
    "splice 0.6.0 adds it",
    "hoard v0.6.1 ships it",
    "hoard 0.6.1 ships it",
    "hull v0.13.0.",
    "@keelstackhq/hoist 0.5.1 apply",
    "berth 0.2.1",
  ])
    expect(hasLiteralPin(line, all, {})).toBe(true);
});

test("does not flag history versions, other tools' versions or tokens", () => {
  for (const line of [
    "since 0.2.1 a task moves",
    "bosun 0.18.10 fixed it",
    "bosun 0.17.0 had it",
    "splice 0.5.1 is old",
    "bosun {{ks.bare.bosun}} follows",
    "hoist 0.6.0",
    "the bosun 0.18.0.1 build",
  ])
    expect(hasLiteralPin(line, all, {})).toBe(false);
});
