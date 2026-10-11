import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import type { AccountId } from "../accounts/accounts";
import { Emoji } from "../lib/emoji";
import { ALL_SECTIONS } from "../lib/emojiSearch";
import { MoreIcon, PlusIcon } from "./Icons";

interface ReactionPickerProps {
  account: AccountId;
  /** The emoji we already reacted with: picking it again removes it. */
  current?: string;
  /** What was clicked to open it, in viewport pixels: the picker opens above it, centred on x. */
  anchor: PickerAnchor;
  /** Start on the full emoji grid instead of the quick row. */
  expanded?: boolean;
  onPick: (emoji: string) => void;
}

/** A point is an anchor whose top and bottom are the same. */
export interface PickerAnchor {
  x: number;
  top: number;
  bottom: number;
}

const EDGE = 8;
const GAP = 8;

/** The row each phone app offers on a long press, in its own order. */
const QUICK_REACTIONS: Record<AccountId, string[]> = {
  whatsapp: ["👍", "❤️", "😂", "😮", "😢", "🙏"],
  signal: ["❤️", "👍", "👎", "😂", "😮", "😢"],
};


/** Signal Desktop lists Travel & Places before Activity; WhatsApp keeps Unicode's order. */
const SECTION_ORDER: Record<AccountId, string[]> = {
  whatsapp: ["Smileys & People", "Animals & Nature", "Food & Drink", "Activity", "Travel & Places", "Objects", "Symbols", "Flags", "Characters"],
  signal: ["Smileys & People", "Animals & Nature", "Food & Drink", "Travel & Places", "Activity", "Objects", "Symbols", "Flags", "Characters"],
};

const SECTIONS_BY_ACCOUNT = Object.fromEntries(
  Object.entries(SECTION_ORDER).map(([account, order]) => [
    account,
    order.flatMap((name) => ALL_SECTIONS.filter((section) => section.name === name)),
  ]),
) as Record<AccountId, typeof ALL_SECTIONS>;

/** Without the characters that are not emoji, which can't be reactions. */
const REACTION_SECTIONS = Object.fromEntries(
  Object.entries(SECTIONS_BY_ACCOUNT).map(([account, sections]) => [account, sections.filter((s) => s.name !== "Characters")]),
) as Record<AccountId, typeof ALL_SECTIONS>;

const cellClass =
  "flex h-9 w-9 items-center justify-center rounded-full text-[22px] leading-none hover:bg-(--hover-bg)";

/** Every emoji, searchable and split into the sections the phone keyboards use. */
export function EmojiGrid({
  account,
  current,
  onPick,
  autoFocus = true,
  characters = false,
}: Pick<ReactionPickerProps, "account" | "current" | "onPick"> & {
  autoFocus?: boolean;
  /** Offer characters that are not emoji (€, Ω…), which can't be reactions. */
  characters?: boolean;
}) {
  const SECTIONS = (characters ? SECTIONS_BY_ACCOUNT : REACTION_SECTIONS)[account];
  const [query, setQuery] = useState("");
  const list = useRef<HTMLDivElement>(null);
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const sections = words.length
    ? [
        {
          name: "Search results",
          icon: "",
          emoji: SECTIONS.flatMap((section) =>
            section.emoji.filter(({ search }) => words.every((word) => search.includes(word))),
          ),
        },
      ]
    : SECTIONS;

  return (
    <>
      <input
        autoFocus={autoFocus}
        type="search"
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        placeholder="Search emoji"
        aria-label="Search emoji"
        className="mx-2 mt-2 h-8 shrink-0 rounded-full bg-(--hover-bg) px-3 text-[14px] outline-none placeholder:text-(--text-secondary)"
      />
      {words.length === 0 ? (
        <div className="flex shrink-0 justify-between border-b border-(--divider) px-2 pt-1" role="tablist">
          {SECTIONS.map((section, index) => (
            <button
              key={section.name}
              type="button"
              role="tab"
              title={section.name}
              aria-label={section.name}
              onClick={() => list.current?.children[index]?.scrollIntoView({ block: "start" })}
              className="flex h-8 w-8 items-center justify-center rounded-md text-[17px] leading-none hover:bg-(--hover-bg)"
            >
              <Emoji emoji={section.icon} size={18} />
            </button>
          ))}
        </div>
      ) : null}
      <div ref={list} className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {sections.map((section) => (
          <section key={section.name} style={{ contentVisibility: "auto", containIntrinsicSize: "auto 400px" }}>
            <h3 className="px-1 pt-2 pb-1 text-[12px] text-(--text-secondary)">
              {section.emoji.length > 0 ? section.name : "No emoji found"}
            </h3>
            <div className="grid grid-cols-9">
              {section.emoji.map(({ emoji, name }) => (
                <button
                  key={emoji}
                  type="button"
                  title={name}
                  aria-label={name}
                  onClick={() => onPick(emoji)}
                  className={emoji === current ? `${cellClass} bg-(--hover-bg)` : cellClass}
                >
                  <Emoji emoji={emoji} size={26} />
                </button>
              ))}
            </div>
          </section>
        ))}
      </div>
    </>
  );
}

export function ReactionPicker({ account, current, anchor, expanded: startExpanded = false, onPick }: ReactionPickerProps) {
  const [expanded, setExpanded] = useState(startExpanded);
  const whatsapp = account === "whatsapp";
  const box = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<CSSProperties>({ left: anchor.x, top: anchor.top, visibility: "hidden" });

  // Above what was clicked, as WhatsApp Web does, or under it when the top of the window is in the way.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const above = anchor.top - GAP - height >= EDGE;
    const left = Math.max(EDGE, Math.min(anchor.x - width / 2, window.innerWidth - width - EDGE));
    const top = above
      ? anchor.top - GAP - height
      : Math.max(EDGE, Math.min(anchor.bottom + GAP, window.innerHeight - height - EDGE));
    setPlace({ left, top, transformOrigin: `${anchor.x - left}px ${above ? "100%" : "0"}` });
  }, [anchor, expanded]);

  let quick = QUICK_REACTIONS[account];
  // A reaction from outside the row still has to be visible, to take it back.
  if (current && !quick.includes(current)) quick = [...quick.slice(0, -1), current];

  const position = "reaction-pop fixed z-50 bg-(--panel-bg)";
  const shadow = { ...place, boxShadow: "0 2px 5px rgba(11,20,26,.26), 0 2px 10px rgba(11,20,26,.16)" };

  if (expanded) {
    return createPortal(
      <div
        ref={box}
        data-reaction-picker
        className={`${position} flex h-[340px] w-[340px] flex-col overflow-hidden rounded-2xl`}
        style={shadow}
        role="dialog"
        aria-label="All emoji"
      >
        <EmojiGrid account={account} current={current} onPick={onPick} />
      </div>,
      document.body,
    );
  }

  return createPortal(
    <div
      ref={box}
      data-reaction-picker
      className={`${position} flex items-center rounded-full ${whatsapp ? "gap-1 px-2 py-1.5" : "gap-0.5 px-2.5 py-2"}`}
      style={shadow}
      role="menu"
      aria-label="Pick a reaction"
    >
      {quick.map((emoji) => {
        const selected = emoji === current;
        return (
          <button
            key={emoji}
            type="button"
            role="menuitem"
            aria-label={selected ? `Remove ${emoji}` : emoji}
            onClick={() => onPick(emoji)}
            className={[
              "group/emoji flex h-10 w-10 items-center justify-center rounded-full leading-none",
              whatsapp ? "text-[26px]" : "text-[28px]",
              selected
                ? whatsapp
                  ? "bg-(--hover-bg) ring-2 ring-(--accent)"
                  : "bg-[color-mix(in_oklab,var(--text-primary)_16%,transparent)]"
                : "",
            ].join(" ")}
          >
            <span className="flex transition-transform duration-100 group-hover/emoji:scale-125">
              <Emoji emoji={emoji} size={whatsapp ? 30 : 32} />
            </span>
          </button>
        );
      })}
      <button
        type="button"
        role="menuitem"
        aria-label="More emoji"
        onClick={() => setExpanded(true)}
        className={[
          "ml-1 flex items-center justify-center rounded-full text-(--icon)",
          whatsapp
            ? "h-8 w-8 bg-(--hover-bg) hover:bg-(--divider)"
            : "h-10 w-10 hover:bg-(--hover-bg)",
        ].join(" ")}
      >
        {whatsapp ? <PlusIcon size={20} /> : <MoreIcon size={24} />}
      </button>
    </div>,
    document.body,
  );
}
