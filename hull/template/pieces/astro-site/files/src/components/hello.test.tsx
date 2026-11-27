// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToString } from "react-dom/server";
import { expect, test } from "vite-plus/test";
import { Hello } from "./hello";

test("renders a cascivo button to static HTML", () => {
  const html = renderToString(<Hello />).replaceAll("<!-- -->", "");
  expect(html).toContain("<button");
  expect(html).toContain("Clicked 0 times");
});

test("the hydrated island counts clicks", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const root = document.createElement("div");
  document.body.append(root);
  await act(async () => createRoot(root).render(<Hello />));
  const button = root.querySelector("button")!;
  expect(button.textContent).toBe("Clicked 0 times");
  await act(async () => button.click());
  expect(button.textContent).toBe("Clicked 1 times");
});
