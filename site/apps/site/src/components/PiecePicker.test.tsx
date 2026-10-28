// @vitest-environment happy-dom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { renderToString } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { buildCommand, CAPTION, DEFAULT_ON } from "../lib/command";
import PiecePicker, { COPIED_MS } from "./PiecePicker";

function stubClipboard(writeText: () => Promise<void>) {
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
}

const code = () => document.querySelector("pre")!.textContent;
const click = async (name: string) => {
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name }));
  });
};
const chip = (name: string) => screen.getByRole("button", { name: new RegExp(`:${name}$`) });

beforeEach(() => {
  vi.useFakeTimers();
  stubClipboard(() => Promise.resolve());
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
});

describe("PiecePicker", () => {
  it("renders web and api pressed, others not, and the default command", () => {
    render(<PiecePicker />);
    expect(chip("web").getAttribute("aria-pressed")).toBe("true");
    expect(chip("api").getAttribute("aria-pressed")).toBe("true");
    for (const n of ["ui", "site", "cli", "desktop", "docs"]) {
      expect(chip(n).getAttribute("aria-pressed")).toBe("false");
    }
    expect(code()).toBe(buildCommand(new Set(DEFAULT_ON)));
  });

  it("server render contains the default command and pressed states", () => {
    const html = renderToString(<PiecePicker />);
    expect(html).toContain("--piece preact-app:web --piece hono-worker:api");
    expect(html.match(/aria-pressed="true"/g)).toHaveLength(2);
    expect(html).not.toMatch(/>Copy</);
  });

  it("shows one splice command and its caption, no mode switch", () => {
    render(<PiecePicker />);
    expect(screen.queryByRole("radiogroup")).toBeNull();
    expect(code()!.startsWith("pnpm dlx @keelstackhq/splice@0.3.1 new")).toBe(true);
    expect(screen.getByText(CAPTION)).toBeTruthy();
  });

  it("toggling a piece updates aria-pressed and the command", () => {
    render(<PiecePicker />);
    fireEvent.click(chip("site"));
    expect(chip("site").getAttribute("aria-pressed")).toBe("true");
    expect(code()).toContain("--piece astro-site:site");
    fireEvent.click(chip("site"));
    expect(chip("site").getAttribute("aria-pressed")).toBe("false");
    expect(code()).not.toContain("--piece astro-site:site");
  });

  it("copy writes the current command and shows Copied for 1400 ms", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    stubClipboard(writeText);
    render(<PiecePicker />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    });
    expect(writeText).toHaveBeenCalledWith(buildCommand(new Set(DEFAULT_ON)));
    expect(screen.getAllByText("Copied").length).toBeGreaterThan(0);
    act(() => {
      vi.advanceTimersByTime(COPIED_MS - 1);
    });
    expect(screen.getByRole("button", { name: "Copied" })).toBeTruthy();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.getByRole("button", { name: "Copy" })).toBeTruthy();
  });

  it("a second click restarts the 1400 ms", async () => {
    render(<PiecePicker />);
    await click("Copy");
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    await click("Copied");
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.getByRole("button", { name: "Copied" })).toBeTruthy();
  });

  it("copy failure shows Copy failed", async () => {
    stubClipboard(() => Promise.reject(new Error("denied")));
    render(<PiecePicker />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    });
    expect(screen.getByRole("button", { name: "Copy failed" })).toBeTruthy();
  });

  it("no copy button without the Clipboard API", () => {
    Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
    render(<PiecePicker />);
    expect(screen.queryByRole("button", { name: /^Copy/ })).toBeNull();
  });
});
