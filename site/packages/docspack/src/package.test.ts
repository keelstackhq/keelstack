import { readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";

const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")) as {
  name: string;
  version: string;
  private?: boolean;
  files: string[];
  publishConfig: { access: string; provenance: boolean };
  hull: { release: string };
  docspack: { from: string };
  repository: { url: string; directory: string };
  license: string;
};

describe("package.json", () => {
  it("is a docspack vendor package released like a hull ts-lib", () => {
    expect(pkg.name).toBe("@keelstackhq/docspack");
    expect(pkg.version).toMatch(/^\d+\.\d+\.\d+$/);
    expect(pkg.private).toBeUndefined();
    expect(pkg.hull.release).toBe("ts-lib");
    expect(pkg.publishConfig).toEqual({ access: "public", provenance: true });
  });

  it("ships only the docspack payload", () => {
    expect(pkg.files).toEqual([".llms", "llms.txt"]);
    expect(pkg.docspack.from).toBe("dist/pages");
  });

  it("is MIT licensed like the other keelstackhq packages, with the license file shipped", () => {
    expect(pkg.license).toBe("MIT");
    expect(readFileSync(new URL("../LICENSE", import.meta.url), "utf8")).toMatch(/^MIT License\n/);
  });

  it("names this repo for npm provenance", () => {
    expect(pkg.repository.url).toBe("git+https://github.com/keelstackhq/keelstack.git");
    expect(pkg.repository.directory).toBe("packages/docspack");
  });
});
