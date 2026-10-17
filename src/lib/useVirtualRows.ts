import { useCallback, useLayoutEffect, useRef, useState, type RefObject } from "react";

/** How far past the edges of the view rows are kept drawn, in px. */
const OVERSCAN_PX = 720;

function scrollParent(node: HTMLElement): HTMLElement | null {
  for (let el = node.parentElement; el; el = el.parentElement) {
    const { overflowY } = getComputedStyle(el);
    if (overflowY === "auto" || overflowY === "scroll") return el;
  }
  return null;
}

/** The first index whose row ends below `y`. */
function indexAt(tops: number[], y: number): number {
  let lo = 0;
  let hi = tops.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (tops[mid + 1] <= y) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/**
 * Draws only the rows of a long list that are in or near view, so a list of
 * thousands costs what a screenful does. `tops` holds where each row starts,
 * in px from the top of the list, plus one entry for where the last ends; the
 * list keeps that full height, so the scroll bar is right from the start.
 *
 * Returns the rows to draw, `[start, end)`, and a way to bring a row into view.
 */
export function useVirtualRows(list: RefObject<HTMLElement | null>, tops: number[]) {
  const count = tops.length - 1;
  const scroller = useRef<HTMLElement | null>(null);
  // Before the first layout, a screenful.
  const [range, setRange] = useState<[number, number]>(() => [0, Math.min(count, 20)]);

  const measure = useCallback(() => {
    const el = list.current;
    const view = scroller.current;
    if (!el || !view) return;
    const offset = el.getBoundingClientRect().top - view.getBoundingClientRect().top + view.scrollTop;
    const from = view.scrollTop - offset - OVERSCAN_PX;
    const to = view.scrollTop - offset + view.clientHeight + OVERSCAN_PX;
    const start = count === 0 ? 0 : indexAt(tops, Math.max(0, from));
    const end = count === 0 ? 0 : Math.min(count, indexAt(tops, Math.max(0, to)) + 1);
    setRange((prev) => (prev[0] === start && prev[1] === end ? prev : [start, end]));
  }, [list, tops, count]);

  useLayoutEffect(() => {
    if (list.current && !scroller.current) scroller.current = scrollParent(list.current);
    const view = scroller.current;
    measure();
    if (!view) return;
    view.addEventListener("scroll", measure, { passive: true });
    const resized = new ResizeObserver(measure);
    resized.observe(view);
    return () => {
      view.removeEventListener("scroll", measure);
      resized.disconnect();
    };
  }, [list, measure]);

  /** Scrolls so that row `index` is drawn, centred when it was far off. */
  const reveal = useCallback(
    (index: number) => {
      const el = list.current;
      const view = scroller.current;
      if (!el || !view || index < 0 || index >= count) return;
      if (index >= range[0] && index < range[1]) return;
      const offset = el.getBoundingClientRect().top - view.getBoundingClientRect().top + view.scrollTop;
      view.scrollTop = offset + tops[index] - view.clientHeight / 2;
    },
    [list, tops, count, range],
  );

  return { start: range[0], end: range[1], reveal };
}
