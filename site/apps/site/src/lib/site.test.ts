import { describe, expect, it } from "vite-plus/test";
import { canonicalUrl, SITE_URL } from "./site";

describe("canonicalUrl", () => {
  it("uses the keelstack.dev origin", () => {
    expect(SITE_URL).toBe("https://keelstack.dev");
  });

  it("maps the home page to the bare origin with a trailing slash", () => {
    expect(canonicalUrl("/")).toBe("https://keelstack.dev/");
  });

  it("keeps other paths and drops query and hash", () => {
    expect(canonicalUrl("/llms.txt?x=1#top")).toBe("https://keelstack.dev/llms.txt");
  });
});
