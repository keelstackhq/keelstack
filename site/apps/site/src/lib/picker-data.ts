// Build-time data for the piece picker, from data/facts.ts (node only).
import { facts } from "../data/facts";
import { bare, sources, versions } from "../data/versions";
import type { PickerData, Piece } from "./command";

/** Editorial: which hull pieces the picker offers, and which are on by default. */
export const OFFERED = [
  "preact-app",
  "hono-worker",
  "ui-storybook",
  "astro-site",
  "rust-tool",
  "tauri-app",
  "docs",
] as const;
/** Pieces deliberately not offered. facts.test.ts fails on a piece in neither list. */
export const NOT_OFFERED = [
  "base",
  "circleci",
  "circleci-only",
  "secrets",
  "npm-cli",
  "rust-cli",
  "ts-lib",
] as const;
export const DEFAULT_PIECES = ["preact-app", "hono-worker"] as const;

const pieces: Piece[] = OFFERED.map((id) => {
  const p = facts.pieces.find((x) => x.template === "template" && x.id === id);
  if (!p) throw new Error(`picker: piece ${id} is not in hull/template/pieces`);
  return { piece: id, name: p.instance };
});

export const pickerData: PickerData = {
  splicePackage: facts.tools.find((t) => t.name === "splice")!.npm!,
  // Pins that resolve today (data/versions.ts): the workspace version once released.
  spliceVersion: bare(versions.splice),
  hullSource: `${sources.hull}/template`,
  hullRef: versions.hull,
  pieces,
  defaultOn: pieces
    .filter((p) => (DEFAULT_PIECES as readonly string[]).includes(p.piece))
    .map((p) => p.name),
};
