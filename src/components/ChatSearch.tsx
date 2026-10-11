import { useEffect, useRef, useState } from "react";
import type { Message } from "../backend";
import { matches } from "../lib/search";
import { ChevronDownIcon, CloseIcon, SearchIcon } from "./Icons";

interface ChatSearchProps {
  query: string;
  onQuery: (query: string) => void;
  /** The ids of the messages found, newest first (`searchChat`). */
  found: string[];
  /** Brings a found message into view. */
  onJump: (messageId: string) => void;
  onClose: () => void;
}

/** Searches the open chat, newest match first, stepping back through older ones. */
/** The messages of a chat with every word of `query` in them, newest first. */
export function searchChat(messages: Message[], query: string): string[] {
  const q = query.trim();
  if (!q) return [];
  return messages
    .filter((m) => !m.deleted && matches(`${m.text ?? ""} ${m.attachments?.length || m.hasMedia ? (m.mediaLabel ?? "") : ""}`, q))
    .map((m) => m.id)
    .reverse();
}

export function ChatSearch({ query, onQuery, found, onJump, onClose }: ChatSearchProps) {
  // Counted from the newest match.
  const [at, setAt] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  // A new search starts again from the newest match.
  useEffect(() => setAt(0), [query]);
  const current = found[Math.min(at, found.length - 1)];
  useEffect(() => {
    if (current) onJump(current);
    // Only when the match changes, not when the messages around it do.
  }, [current]);

  const step = (by: number) => found.length && setAt((prev) => (prev + by + found.length) % found.length);
  const button =
    "flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg) disabled:opacity-40";

  return (
    <div className="flex h-13 shrink-0 items-center gap-1 border-b border-l border-(--divider) bg-(--panel-header-bg) px-3" role="search">
      <div className="relative min-w-0 flex-1">
        <SearchIcon size={16} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-(--icon)" />
        <input
          ref={input}
          id="chat-search-in"
          autoFocus
          value={query}
          onChange={(event) => onQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") onClose();
            else if (event.key === "Enter") {
              event.preventDefault();
              // Enter goes back in time, as reading a chat upwards does.
              step(event.shiftKey ? -1 : 1);
            }
          }}
          placeholder="Search in this chat"
          aria-label="Search in this chat"
          className="h-8.5 w-full rounded-lg bg-(--search-bg) pr-3 pl-9 text-[14px] text-(--text-primary) outline-none placeholder:text-(--text-secondary)"
        />
      </div>
      <span className="min-w-16 shrink-0 text-center text-[12.5px] text-(--text-secondary) tabular-nums" aria-live="polite">
        {query.trim() ? (found.length ? `${Math.min(at, found.length - 1) + 1} of ${found.length}` : "No results") : ""}
      </span>
      <button type="button" onClick={() => step(1)} disabled={found.length < 2} aria-label="Older match" title="Older match (Enter)" className={button}>
        <ChevronDownIcon size={20} className="rotate-180" />
      </button>
      <button type="button" onClick={() => step(-1)} disabled={found.length < 2} aria-label="Newer match" title="Newer match (Shift+Enter)" className={button}>
        <ChevronDownIcon size={20} />
      </button>
      <button type="button" onClick={onClose} aria-label="Close search" title="Close search (Esc)" className={button}>
        <CloseIcon size={18} />
      </button>
    </div>
  );
}
