import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import type { ReactionCount } from "../backend/types";
import { Emoji } from "../lib/emoji";
import type { PickerAnchor } from "./ReactionPicker";

const EDGE = 8;
const GAP = 6;

interface ReactionDetailsProps {
  reactions: ReactionCount[];
  /** The pills that were clicked, in viewport pixels: it opens under them, or above near the bottom. */
  anchor: PickerAnchor;
  /** Takes our reaction back; absent when the chat can't be reacted in. */
  onRemove?: (emoji: string) => void;
  onClose: () => void;
}

/** Every reaction on a message: the total, each emoji with its count, and who picked it. */
export function ReactionDetails({ reactions, anchor, onRemove, onClose }: ReactionDetailsProps) {
  const box = useRef<HTMLDivElement>(null);
  const [tab, setTab] = useState<string | null>(null);
  const [place, setPlace] = useState<CSSProperties>({ left: anchor.x, top: anchor.bottom, visibility: "hidden" });
  const total = reactions.reduce((sum, reaction) => sum + reaction.count, 0);
  const shown = tab ? reactions.filter((reaction) => reaction.emoji === tab) : reactions;
  // Ours first, then by who picked the most.
  const rows = shown.flatMap((reaction) =>
    (reaction.people ?? []).map((name, index) => ({ emoji: reaction.emoji, name, mine: reaction.mine && index === 0 })),
  );
  rows.sort((a, b) => Number(b.mine) - Number(a.mine));

  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const below = anchor.bottom + GAP + height <= window.innerHeight - EDGE;
    const left = Math.max(EDGE, Math.min(anchor.x - width / 2, window.innerWidth - width - EDGE));
    const top = below ? anchor.bottom + GAP : Math.max(EDGE, anchor.top - GAP - height);
    setPlace({ left, top, transformOrigin: `${anchor.x - left}px ${below ? "0" : "100%"}` });
  }, [anchor, tab]);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Element;
      if (!box.current?.contains(target) && !target.closest?.("[data-reaction-pills]")) onClose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    // It stays where it opened, so scrolling the chat closes it.
    const onWheel = (event: WheelEvent) => {
      if (!box.current?.contains(event.target as Node)) onClose();
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("wheel", onWheel, true);
    window.addEventListener("resize", onClose);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("wheel", onWheel, true);
      window.removeEventListener("resize", onClose);
    };
  }, [onClose]);

  const tabClass = (selected: boolean) =>
    [
      "flex h-8 shrink-0 items-center gap-1.5 border-b-2 px-2.5 text-[13px] tabular-nums",
      selected ? "border-(--accent) text-(--text-primary)" : "border-transparent text-(--text-secondary) hover:text-(--text-primary)",
    ].join(" ");

  return createPortal(
    <div
      ref={box}
      className="reaction-pop fixed z-50 flex max-h-[320px] w-[300px] flex-col overflow-hidden rounded-xl bg-(--panel-bg)"
      style={{ ...place, boxShadow: "0 2px 5px rgba(11,20,26,.26), 0 2px 10px rgba(11,20,26,.16)" }}
      role="dialog"
      aria-label={`${total} ${total === 1 ? "reaction" : "reactions"}`}
    >
      <div className="flex overflow-x-auto border-b border-(--divider) px-1" role="tablist">
        <button type="button" role="tab" aria-selected={tab === null} onClick={() => setTab(null)} className={tabClass(tab === null)}>
          All {total}
        </button>
        {reactions.map((reaction) => (
          <button
            key={reaction.emoji}
            type="button"
            role="tab"
            aria-selected={tab === reaction.emoji}
            aria-label={`${reaction.emoji} ${reaction.count}`}
            onClick={() => setTab(reaction.emoji)}
            className={tabClass(tab === reaction.emoji)}
          >
            <Emoji emoji={reaction.emoji} size={16} />
            {reaction.count}
          </button>
        ))}
      </div>
      <ul className="overflow-y-auto py-1">
        {rows.map((row, index) => {
          const content = (
            <>
              <span className="min-w-0 flex-1 truncate">
                <span className="block truncate text-[14.5px] text-(--text-primary)">{row.name}</span>
                {row.mine && onRemove ? <span className="block text-[12.5px] text-(--text-secondary)">Click to remove</span> : null}
              </span>
              <Emoji emoji={row.emoji} size={20} />
            </>
          );
          return (
            <li key={`${row.emoji}-${index}`}>
              {row.mine && onRemove ? (
                <button
                  type="button"
                  onClick={() => onRemove(row.emoji)}
                  className="flex w-full items-center gap-3 px-4 py-2 text-left hover:bg-(--hover-bg)"
                >
                  {content}
                </button>
              ) : (
                <div className="flex items-center gap-3 px-4 py-2">{content}</div>
              )}
            </li>
          );
        })}
      </ul>
    </div>,
    document.body,
  );
}
