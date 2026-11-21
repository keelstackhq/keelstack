import { describe, expect, it } from "vite-plus/test";
import { pickCurrent } from "./scroll-spy";

// tops: each heading's getBoundingClientRect().top; offset 120 is where a heading counts as "read".
const view = { offset: 120, viewport: 800, scrollY: 0, scrollHeight: 3000 };

describe("pickCurrent", () => {
  it("picks the first heading on a page that cannot scroll", () => {
    expect(pickCurrent([300, 450, 600], { ...view, scrollHeight: 800 })).toBe(0);
  });

  it("picks the first heading when the page scrolls a few pixels only", () => {
    expect(pickCurrent([300, 450, 600], { ...view, scrollHeight: 801 })).toBe(0);
  });

  it("picks the first heading at the top of a long page once it is on screen", () => {
    expect(pickCurrent([300, 1200, 2000], view)).toBe(0);
  });

  it("picks the first heading at the top of a page that scrolls a little", () => {
    expect(pickCurrent([300, 450, 600], { ...view, scrollHeight: 900 })).toBe(0);
  });

  it("picks none while the first heading is below the screen", () => {
    expect(pickCurrent([900, 1200, 2000], view)).toBe(-1);
  });

  it("picks the last heading that passed the offset", () => {
    expect(pickCurrent([-900, 40, 700], { ...view, scrollY: 1200 })).toBe(1);
  });

  it("picks the last heading at the bottom when it is visible", () => {
    expect(pickCurrent([-900, 40, 500], { ...view, scrollY: 2200 })).toBe(2);
  });

  it("keeps the nearest heading at the bottom when the last one is off screen", () => {
    expect(pickCurrent([-1900, 60, 900], { ...view, scrollY: 2200 })).toBe(1);
  });

  it("picks none when there are no headings", () => {
    expect(pickCurrent([], view)).toBe(-1);
  });
});
