// @vitest-environment happy-dom
import { render } from "preact";
import { act } from "preact/test-utils";
import { afterEach, expect, test, vi } from "vite-plus/test";
import { CopyButton } from "./copy-button";

let root: HTMLDivElement;

function deferred() {
  const result = { resolve: () => {}, promise: Promise.resolve() };
  result.promise = new Promise<void>((r) => {
    result.resolve = r;
  });
  return result;
}

function mount(text = "pnpm install") {
  root = document.createElement("div");
  document.body.append(root);
  act(() => render(<CopyButton text={text} label="Copy command" />, root));
  return root;
}

afterEach(() => {
  render(null, root);
  root.remove();
  vi.unstubAllGlobals();
});

test("renders nothing without the Clipboard API", () => {
  vi.stubGlobal("navigator", { clipboard: undefined });
  expect(mount().querySelector("button")).toBeNull();
});

test("copies the text and announces it", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const el = mount("node scripts/render.mjs new");
  const button = el.querySelector("button");
  expect(button?.textContent).toBe("Copy");
  expect(button?.getAttribute("aria-label")).toBe("Copy command");
  await act(async () => {
    button?.click();
  });
  expect(writeText).toHaveBeenCalledWith("node scripts/render.mjs new");
  expect(el.querySelector("[aria-live]")?.textContent).toBe("Copied");
});

test("clears the message on each click so a repeat copy is announced again", async () => {
  const pending = deferred();
  const writeText = vi.fn().mockResolvedValueOnce(undefined).mockReturnValueOnce(pending.promise);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const el = mount();
  const status = () => el.querySelector("[aria-live]")?.textContent;
  await act(async () => {
    el.querySelector("button")?.click();
  });
  expect(status()).toBe("Copied");
  await act(async () => {
    el.querySelector("button")?.click();
  });
  expect(status()).toBe("");
  await act(async () => {
    pending.resolve();
  });
  expect(status()).toBe("Copied");
});

test("says so when copying fails", async () => {
  const writeText = vi.fn().mockRejectedValue(new Error("denied"));
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const el = mount();
  await act(async () => {
    el.querySelector("button")?.click();
  });
  expect(el.querySelector("[aria-live]")?.textContent).toBe("Copy failed; select the text instead");
});
