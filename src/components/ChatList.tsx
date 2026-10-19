import { memo, useCallback, useEffect, useMemo, useRef } from "react";
import type { Chat } from "../backend";
import { formatListTime } from "../lib/format";
import { ACCOUNTS } from "../accounts/accounts";
import { AccountBadge } from "./AccountBadge";
import { Avatar } from "./Avatar";
import { FormattedLine } from "../lib/markup";
import { useVirtualRows } from "../lib/useVirtualRows";
import { useSettings } from "../settings/settings";
import { ChevronDownIcon, MuteIcon, PinIcon, Ticks } from "./Icons";

interface ChatListProps {
  chats: Chat[];
  selected: Chat | null;
  onSelect: (chat: Chat) => void;
  /** The pointer rests on a chat that may be opened next. */
  onPeek?: (chat: Chat) => void;
  /** A chat to point out, as `account:id`. */
  highlighted?: string | null;
  /** The pointed-out chat is fading back. */
  highlightFading?: boolean;
  /** Mark each chat with its network: the list mixes accounts. */
  showAccount?: boolean;
  /** Opens a chat's options at a point on screen. */
  onMenu?: (chat: Chat, x: number, y: number) => void;
}

/** Every row's height, in px (`h-18`). */
const ROW_PX = 72;

interface ChatRowProps {
  chat: Chat;
  selected: boolean;
  highlighted: boolean;
  fading: boolean;
  showAccount?: boolean;
  /** A reaction newer than the last message shows in its place. */
  showReactions: boolean;
  onSelect: (chat: Chat) => void;
  onPeek?: (chat: Chat) => void;
  onMenu?: (chat: Chat, x: number, y: number) => void;
}

/** One chat. Redrawn only when it or its selection changes, not with every other row. */
const ChatRow = memo(function ChatRow({
  chat,
  selected,
  highlighted,
  fading,
  showAccount,
  showReactions,
  onSelect,
  onPeek,
  onMenu,
}: ChatRowProps) {
  const unread = chat.unread > 0 || !!chat.markedUnread;
  const reaction = showReactions ? chat.reactionPreview : undefined;
  const row = useRef<HTMLLIElement>(null);
  useEffect(() => {
    if (highlighted) row.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [highlighted]);
  return (
    <li
      ref={row}
      className={["chat-row group/row relative", highlighted ? "chat-highlight" : "", highlighted && fading ? "chat-highlight-out" : ""].join(" ")}
    >
      <button
        type="button"
        role="option"
        aria-selected={selected}
        onClick={() => onSelect(chat)}
        onPointerEnter={onPeek && !selected ? () => onPeek(chat) : undefined}
        onContextMenu={
          onMenu
            ? (event) => {
                event.preventDefault();
                // The context-menu key has no pointer position.
                const rect = event.currentTarget.getBoundingClientRect();
                const byKey = event.clientX === 0 && event.clientY === 0;
                onMenu(chat, byKey ? rect.right - 220 : event.clientX, byKey ? rect.top + 40 : event.clientY);
              }
            : undefined
        }
        className={[
          "group flex h-18 w-full items-center gap-3 border-l-[3px] pl-2.25 text-left transition-colors duration-100",
          selected ? "bg-(--selected-bg)" : chat.typing ? "chat-typing" : "hover:bg-(--hover-bg)",
        ].join(" ")}
        style={{ borderLeftColor: showAccount ? ACCOUNTS[chat.account].color : "transparent" }}
      >
        <div className={["relative shrink-0", chat.typing ? "typing-ring" : ""].join(" ")}>
          <Avatar
            name={chat.name}
            initials={chat.initials}
            hue={chat.avatarHue}
            src={chat.avatarUrl}
            size={49}
            group={chat.isGroup}
            online={showAccount ? undefined : chat.online}
            timer={chat.disappearing ? (showAccount ? "left" : "right") : undefined}
          />
          {showAccount ? (
            <span className="absolute -right-0.5 -bottom-0.5 rounded-full border-2 border-(--panel-bg)">
              <AccountBadge account={chat.account} size={18} />
            </span>
          ) : null}
        </div>
        <div className="flex h-full min-w-0 flex-1 flex-col justify-center border-b border-(--divider) pr-4">
          <div className="flex items-baseline justify-between gap-2">
            <span className="truncate text-[17px] leading-5.25 text-(--text-primary)">
              {chat.name}
            </span>
            <time
              className={[
                "shrink-0 text-[12px] tabular-nums",
                unread ? "text-(--badge-bg)" : "text-(--text-secondary)",
              ].join(" ")}
              dateTime={chat.lastAt}
            >
              {/* A chat named by the first sync without any message may have no date. */}
              {Date.parse(chat.lastAt) > 0 ? formatListTime(chat.lastAt) : ""}
            </time>
          </div>
          <div className="mt-0.5 flex items-center gap-1.5 text-(--text-secondary)">
            {chat.typing ? (
              // Someone typing takes the place of the last message, as on the phone.
              <>
                <span className="typing-bubble" aria-hidden>
                  <i />
                  <i />
                  <i />
                </span>
                <p className="min-w-0 flex-1 truncate text-[14px] leading-5 font-semibold text-(--accent)">{chat.typing}</p>
              </>
            ) : reaction ? (
              <p className="min-w-0 flex-1 truncate text-[14px] leading-5"><FormattedLine text={reaction} emojiSize={18} /></p>
            ) : (
              <>
                {chat.lastFromMe ? (
                  <span className="shrink-0">
                    <Ticks status={chat.lastStatus} account={chat.account} />
                  </span>
                ) : null}
                <p className="min-w-0 flex-1 truncate text-[14px] leading-5"><FormattedLine text={chat.lastMessage ?? ""} emojiSize={18} /></p>
              </>
            )}
            {chat.muted ? <MuteIcon size={16} aria-label="Muted" /> : null}
            {chat.pinned ? <PinIcon size={16} aria-label="Pinned" /> : null}
            {chat.unread > 0 ? (
              <span className="flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full bg-(--badge-bg) px-1.5 text-[12px] font-semibold text-(--badge-text)">
                {chat.unread}
              </span>
            ) : chat.markedUnread ? (
              // Marked unread by hand: a dot with no count, and a word saying
              // so, apart from a chat with messages to read.
              <span className="flex shrink-0 items-center gap-1.5">
                <span className="text-[12px] text-(--badge-bg)">Marked unread</span>
                <span className="mr-1 size-3 rounded-full bg-(--badge-bg)" aria-hidden />
              </span>
            ) : null}
            {/* Room for the options button, which only shows on hover. */}
            {onMenu ? (
              <span className="w-0 shrink-0 transition-[width] duration-100 group-hover/row:w-6" aria-hidden />
            ) : null}
          </div>
        </div>
      </button>
      {onMenu ? (
        <button
          type="button"
          tabIndex={-1}
          aria-label={`Options for ${chat.name}`}
          aria-haspopup="menu"
          onClick={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            onMenu(chat, rect.left, rect.bottom + 4);
          }}
          className="absolute right-3 bottom-3.25 flex h-6 w-6 items-center justify-center rounded-full text-(--icon) opacity-0 transition-opacity group-hover/row:opacity-100"
        >
          <ChevronDownIcon size={20} />
        </button>
      ) : null}
    </li>
  );
});

export function ChatList({
  chats,
  selected: current,
  onSelect,
  onPeek,
  highlighted,
  highlightFading,
  showAccount,
  onMenu,
}: ChatListProps) {
  const { settings } = useSettings();
  // Only the rows in or near view exist, so thousands of chats cost what a
  // screenful does and switching accounts or filters stays instant.
  const list = useRef<HTMLUListElement>(null);
  const tops = useMemo(() => Array.from({ length: chats.length + 1 }, (_, i) => i * ROW_PX), [chats.length]);
  const { start, end, reveal } = useVirtualRows(list, tops);
  const rows = chats.slice(start, end);
  // A chat pointed out far down the list is scrolled to, so its row exists to glow.
  const highlightedIndex = highlighted ? chats.findIndex((c) => `${c.account}:${c.id}` === highlighted) : -1;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => reveal(highlightedIndex), [highlightedIndex]);

  // The rows get handlers that never change, so a new handler from above
  // does not redraw every row.
  const handlers = useRef({ onSelect, onPeek, onMenu });
  handlers.current = { onSelect, onPeek, onMenu };
  const select = useCallback((chat: Chat) => handlers.current.onSelect(chat), []);
  const peek = useCallback((chat: Chat) => handlers.current.onPeek?.(chat), []);
  const menu = useCallback((chat: Chat, x: number, y: number) => handlers.current.onMenu?.(chat, x, y), []);
  return (
    <ul
      ref={list}
      className="flex flex-col"
      role="listbox"
      aria-label="Chats"
      style={{ paddingTop: start * ROW_PX, paddingBottom: (chats.length - end) * ROW_PX }}
    >
      {rows.map((chat) => (
        <ChatRow
          key={`${chat.account}:${chat.id}`}
          chat={chat}
          selected={chat.id === current?.id && chat.account === current.account}
          highlighted={highlighted === `${chat.account}:${chat.id}`}
          fading={highlighted === `${chat.account}:${chat.id}` && Boolean(highlightFading)}
          showAccount={showAccount}
          showReactions={settings.reactionsInList}
          onSelect={select}
          onPeek={onPeek ? peek : undefined}
          onMenu={onMenu ? menu : undefined}
        />
      ))}
    </ul>
  );
}
