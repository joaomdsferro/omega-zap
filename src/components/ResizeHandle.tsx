import { useRef, type KeyboardEvent, type PointerEvent, type RefObject } from "react";

interface ResizeHandleProps {
  /** The element being resized; its left edge anchors the drag. */
  target: RefObject<HTMLElement | null>;
  width: number;
  min: number;
  max: number;
  onResize: (width: number) => void;
  onReset: () => void;
}

const KEY_STEP = 16;

export function ResizeHandle({ target, width, min, max, onResize, onReset }: ResizeHandleProps) {
  const origin = useRef<number | null>(null);
  const handle = useRef<HTMLDivElement>(null);
  const pending = useRef<number | null>(null);

  // While dragging, the width goes straight to the DOM (nothing here reads
  // layout, so the browser still lays out once per frame); React state only
  // gets the final value, so the chat list and conversation don't re-render
  // on every pointer event.
  function paint(value: number) {
    if (target.current) target.current.style.width = `${value}px`;
    handle.current?.setAttribute("aria-valuenow", String(value));
  }

  function clamp(value: number) {
    return Math.min(max, Math.max(min, value));
  }

  function onPointerDown(e: PointerEvent<HTMLDivElement>) {
    if (e.button !== 0 || !target.current) return;
    // Stops the press from starting a text selection that the drag would
    // then extend across both panes. It also cancels the default focus, so
    // focus by hand to keep the arrow keys working after a click.
    e.preventDefault();
    e.currentTarget.focus();
    origin.current = target.current.getBoundingClientRect().left;
    e.currentTarget.setPointerCapture(e.pointerId);
    document.documentElement.classList.add("resizing");
  }

  function onPointerMove(e: PointerEvent<HTMLDivElement>) {
    if (origin.current === null) return;
    const next = Math.round(clamp(e.clientX - origin.current));
    if (next === pending.current) return;
    pending.current = next;
    paint(next);
  }

  function endDrag(e: PointerEvent<HTMLDivElement>) {
    if (origin.current === null) return;
    origin.current = null;
    e.currentTarget.releasePointerCapture(e.pointerId);
    document.documentElement.classList.remove("resizing");
    if (pending.current === null) return;
    onResize(pending.current);
    pending.current = null;
  }

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    const next =
      e.key === "ArrowLeft"
        ? width - KEY_STEP
        : e.key === "ArrowRight"
          ? width + KEY_STEP
          : e.key === "Home"
            ? min
            : e.key === "End"
              ? max
              : null;
    if (next === null) return;
    e.preventDefault();
    onResize(clamp(next));
  }

  return (
    <div
      ref={handle}
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize chat list"
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={Math.round(width)}
      tabIndex={0}
      title="Drag to resize · double-click to reset"
      onPointerDown={onPointerDown}
      onMouseDown={(e) => e.preventDefault()}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onDoubleClick={onReset}
      onKeyDown={onKeyDown}
      className="group relative z-10 -mx-0.75 ww-1.5shrink-0 cursor-col-resize focus-visible:outline-none"
    >
      <span className="absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-(--divider) transition-colors duration-100 group-hover:w-0.75 group-hover:bg-(--accent) group-focus-visible:w-0.75 group-focus-visible:bg-(--accent) group-active:w-0.75 group-active:bg-(--accent)" />
    </div>
  );
}
