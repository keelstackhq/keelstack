import { expect, test } from "vite-plus/test";
import { scenarios } from "../data/scenarios";
import { startScenarioMd } from "./start-md";

test("each scenario file holds Setup plus its own block only", () => {
  for (const s of scenarios) {
    const md = startScenarioMd(s.id);
    expect(md).toContain("npm i -g @keelstackhq/bosun@");
    expect(md).toContain(`# Start ${s.id.toUpperCase()}. `);
    for (const o of scenarios.filter((x) => x.id !== s.id)) {
      expect(md).not.toContain(`## ${o.id.toUpperCase()}. `);
    }
    expect(md).not.toMatch(/\]\(#[a-z]\)/);
    expect(md).not.toContain("](#setup)");
  }
  expect(startScenarioMd("a")).toContain("splice@");
  expect(startScenarioMd("e")).toContain("setup-script.sh");
});
