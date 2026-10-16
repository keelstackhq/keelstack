import { renderToString } from "react-dom/server";
import { Badge, Button } from "@cascivo/react";
import { expect, test } from "vite-plus/test";

test("cascivo renders to static HTML under React", () => {
  const html = renderToString(<Button variant="secondary">Go</Button>);
  expect(html).toContain("<button");
  expect(html).toContain('data-variant="secondary"');
  expect(renderToString(<Badge>pre-release</Badge>)).toContain("pre-release");
});
