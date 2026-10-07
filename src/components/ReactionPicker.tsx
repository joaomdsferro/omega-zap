import { useRef, useState } from "react";
import type { AccountId } from "../accounts/accounts";
import { EMOJI_SECTIONS } from "../data/emoji";
import { MoreIcon, PlusIcon } from "./Icons";

interface ReactionPickerProps {
  account: AccountId;
  /** The emoji we already reacted with: picking it again removes it. */
  current?: string;
  /** Which edge of the bubble the picker lines up with. */
  align: "left" | "right";
  /** Open under the bubble when there is no room above it. */
  below: boolean;
  /** Start on the full emoji grid instead of the quick row. */
  expanded?: boolean;
  /** Called when the full emoji grid opens, so the bubble can make room for it. */
  onExpand: () => void;
  onPick: (emoji: string) => void;
}

/** The row each phone app offers on a long press, in its own order. */
const QUICK_REACTIONS: Record<AccountId, string[]> = {
  whatsapp: ["👍", "❤️", "😂", "😮", "😢", "🙏"],
  signal: ["❤️", "👍", "👎", "😂", "😮", "😢"],
};

const ALL_SECTIONS = EMOJI_SECTIONS.map((section) => ({
  ...section,
  emoji: section.emoji.map((entry) => {
    const cut = entry.indexOf(" ");
    const [name, keywords = ""] = entry.slice(cut + 1).split("\t");
    // What a search runs against: the name, then the words people use for it.
    return { emoji: entry.slice(0, cut), name, search: `${name} ${keywords}`.toLowerCase() };
  }),
}));

/** Signal Desktop lists Travel & Places before Activity; WhatsApp keeps Unicode's order. */
const SECTION_ORDER: Record<AccountId, string[]> = {
  whatsapp: ["Smileys & People", "Animals & Nature", "Food & Drink", "Activity", "Travel & Places", "Objects", "Symbols", "Flags"],
  signal: ["Smileys & People", "Animals & Nature", "Food & Drink", "Travel & Places", "Activity", "Objects", "Symbols", "Flags"],
};

const SECTIONS_BY_ACCOUNT = Object.fromEntries(
  Object.entries(SECTION_ORDER).map(([account, order]) => [
    account,
    order.flatMap((name) => ALL_SECTIONS.filter((section) => section.name === name)),
  ]),
) as Record<AccountId, typeof ALL_SECTIONS>;

const cellClass =
  "flex h-9 w-9 items-center justify-center rounded-full text-[22px] leading-none hover:bg-(--hover-bg)";

/** Every emoji, searchable and split into the sections the phone keyboards use. */
function EmojiGrid({ account, current, onPick }: Pick<ReactionPickerProps, "account" | "current" | "onPick">) {
  const SECTIONS = SECTIONS_BY_ACCOUNT[account];
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
        autoFocus
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
              {section.icon}
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
                  {emoji}
                </button>
              ))}
            </div>
          </section>
        ))}
      </div>
    </>
  );
}

export function ReactionPicker({ account, current, align, below, expanded: startExpanded = false, onExpand, onPick }: ReactionPickerProps) {
  const [expanded, setExpanded] = useState(startExpanded);
  const whatsapp = account === "whatsapp";

  let quick = QUICK_REACTIONS[account];
  // A reaction from outside the row still has to be visible, to take it back.
  if (current && !quick.includes(current)) quick = [...quick.slice(0, -1), current];

  const position = [
    "reaction-pop absolute z-20 bg-(--panel-bg)",
    below ? "top-full mt-2" : "bottom-full mb-2",
    align === "right" ? "right-0" : "left-0",
    below
      ? align === "right" ? "origin-top-right" : "origin-top-left"
      : align === "right" ? "origin-bottom-right" : "origin-bottom-left",
  ].join(" ");
  const shadow = { boxShadow: "0 2px 5px rgba(11,20,26,.26), 0 2px 10px rgba(11,20,26,.16)" };

  if (expanded) {
    return (
      <div
        className={`${position} flex h-[340px] w-[340px] flex-col overflow-hidden rounded-2xl`}
        style={shadow}
        role="dialog"
        aria-label="All emoji"
      >
        <EmojiGrid account={account} current={current} onPick={onPick} />
      </div>
    );
  }

  return (
    <div
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
            <span className="transition-transform duration-100 group-hover/emoji:scale-125">{emoji}</span>
          </button>
        );
      })}
      <button
        type="button"
        role="menuitem"
        aria-label="More emoji"
        onClick={() => {
          onExpand();
          setExpanded(true);
        }}
        className={[
          "ml-1 flex items-center justify-center rounded-full text-(--icon)",
          whatsapp
            ? "h-8 w-8 bg-(--hover-bg) hover:bg-(--divider)"
            : "h-10 w-10 hover:bg-(--hover-bg)",
        ].join(" ")}
      >
        {whatsapp ? <PlusIcon size={20} /> : <MoreIcon size={24} />}
      </button>
    </div>
  );
}
