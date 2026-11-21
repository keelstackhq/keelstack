import { describe, expect, it } from "vite-plus/test";
import { missingPages } from "./pages.ts";

const chunk = (page: string) => `<!-- docspack: from ${page} -->\n\n## Heading\n`;

describe("missingPages", () => {
  it("is empty when every page is the source of a chunk", () => {
    expect(missingPages(["a.md", "b.md"], [chunk("a.md"), chunk("b.md"), chunk("a.md")])).toEqual(
      [],
    );
  });

  it("lists the pages no chunk comes from", () => {
    expect(missingPages(["a.md", "b.md", "c.md"], [chunk("b.md")])).toEqual(["a.md", "c.md"]);
  });

  it("does not match a page name inside another one", () => {
    expect(missingPages(["ci.md"], [chunk("docs-ci.md")])).toEqual(["ci.md"]);
  });
});
