// Scroll-spy for the docs "On this page" rail: which heading is being read.
export type SpyView = {
  /** A heading counts as read once its top is at or above this many px from the viewport top. */
  offset: number;
  /** innerHeight */
  viewport: number;
  /** scrollY */
  scrollY: number;
  /** document.documentElement.scrollHeight */
  scrollHeight: number;
};

const SLACK = 2;

/** Index of the current heading from each heading's viewport top, or -1 for none. */
export function pickCurrent(tops: readonly number[], v: SpyView): number {
  if (tops.length === 0) return -1;
  // A page that cannot scroll shows every heading at once: the first is the start.
  if (v.scrollHeight - v.viewport <= SLACK) return 0;
  let cur = -1;
  tops.forEach((top, i) => {
    if (top <= v.offset) cur = i;
  });
  // Before any heading passes the offset, the first one is current once on screen.
  if (cur === -1 && tops[0]! < v.viewport) cur = 0;
  // The last heading may never reach the offset; at the bottom it is current once on screen.
  const last = tops.length - 1;
  const atBottom = v.viewport + v.scrollY >= v.scrollHeight - SLACK;
  if (atBottom && tops[last]! < v.viewport) cur = last;
  return cur;
}
