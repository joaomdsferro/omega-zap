import { memo, useCallback, useRef } from "react";
import type { Chat } from "../backend";
import { formatListTime } from "../lib/format";
import { ACCOUNTS } from "../accounts/accounts";
import { AccountBadge } from "./AccountBadge";
import { Avatar } from "./Avatar";
import { ChevronDownIcon, MuteIcon, PinIcon, Ticks } from "./Icons";

interface ChatListProps {
  chats: Chat[];
  selected: Chat | null;
  onSelect: (chat: Chat) => void;
  /** Mark each chat with its network: the list mixes accounts. */
  showAccount?: boolean;
  /** Opens a chat's options at a point on screen. */
  onMenu?: (chat: Chat, x: number, y: number) => void;
}

interface ChatRowProps {
  chat: Chat;
  selected: boolean;
  showAccount?: boolean;
  onSelect: (chat: Chat) => void;
  onMenu?: (chat: Chat, x: number, y: number) => void;
}

/** One chat. Redrawn only when it or its selection changes, not with every other row. */
const ChatRow = memo(function ChatRow({ chat, selected, showAccount, onSelect, onMenu }: ChatRowProps) {
  const unread = chat.unread > 0;
  return (
    <li className="group/row relative">
      <button
        type="button"
        role="option"
        aria-selected={selected}
        onClick={() => onSelect(chat)}
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
          selected ? "bg-(--selected-bg)" : "hover:bg-(--hover-bg)",
        ].join(" ")}
        style={{ borderLeftColor: showAccount ? ACCOUNTS[chat.account].color : "transparent" }}
      >
        <div className="relative shrink-0">
          <Avatar
            name={chat.name}
            initials={chat.initials}
            hue={chat.avatarHue}
            src={chat.avatarUrl}
            size={49}
            group={chat.isGroup}
            online={showAccount ? undefined : chat.online}
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
              {formatListTime(chat.lastAt)}
            </time>
          </div>
          <div className="mt-0.5 flex items-center gap-1.5 text-(--text-secondary)">
            {chat.lastFromMe ? (
              <span className="shrink-0">
                <Ticks status={chat.lastStatus} account={chat.account} />
              </span>
            ) : null}
            <p className="min-w-0 flex-1 truncate text-[14px] leading-5">{chat.lastMessage}</p>
            {chat.muted ? <MuteIcon size={16} aria-label="Muted" /> : null}
            {chat.pinned ? <PinIcon size={16} aria-label="Pinned" /> : null}
            {unread ? (
              <span className="flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full bg-(--badge-bg) px-1.5 text-[12px] font-semibold text-(--badge-text)">
                {chat.unread}
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

export function ChatList({ chats, selected: current, onSelect, showAccount, onMenu }: ChatListProps) {
  // The rows get handlers that never change, so a new handler from above
  // does not redraw every row.
  const handlers = useRef({ onSelect, onMenu });
  handlers.current = { onSelect, onMenu };
  const select = useCallback((chat: Chat) => handlers.current.onSelect(chat), []);
  const menu = useCallback((chat: Chat, x: number, y: number) => handlers.current.onMenu?.(chat, x, y), []);
  return (
    <ul className="flex flex-col" role="listbox" aria-label="Chats">
      {chats.map((chat) => (
        <ChatRow
          key={`${chat.account}:${chat.id}`}
          chat={chat}
          selected={chat.id === current?.id && chat.account === current.account}
          showAccount={showAccount}
          onSelect={select}
          onMenu={onMenu ? menu : undefined}
        />
      ))}
    </ul>
  );
}
