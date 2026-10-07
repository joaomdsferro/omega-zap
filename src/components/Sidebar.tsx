import { useMemo, useState, type MouseEvent, type ReactNode } from "react";
import type { Chat } from "../backend";
import { ChatList } from "./ChatList";
import { MoreIcon, NewChatIcon, PlusIcon, SearchIcon, SettingsIcon, StarIcon } from "./Icons";
import { PopupMenu, type MenuEntry } from "./PopupMenu";

/** A built-in filter, or `list:` and the name of a custom list. */
type Filter = "all" | "unread" | "favorites" | "private" | "groups" | `list:${string}`;

const FILTERS: { id: Filter; label: string; test: (chat: Chat) => boolean }[] = [
  { id: "all", label: "All", test: () => true },
  { id: "unread", label: "Unread", test: (c) => c.unread > 0 },
  { id: "favorites", label: "Favorites", test: (c) => Boolean(c.favorite) },
  { id: "private", label: "Private", test: (c) => !c.isGroup },
  { id: "groups", label: "Groups", test: (c) => Boolean(c.isGroup) },
];

interface SidebarProps {
  title: string;
  chats: Chat[];
  selected: Chat | null;
  onSelect: (chat: Chat) => void;
  /** Mark each chat with its network: the list mixes accounts. */
  showAccount?: boolean;
  /** Shown above the list: demo notice, connection problems, sync progress. */
  banner?: ReactNode;
  emptyText: string;
  /** The custom lists, by name, in order. */
  lists: string[];
  /** The names of the custom lists a chat is in. */
  listsOf: (chat: Chat) => string[];
  onNewChat: () => void;
  onStarred: () => void;
  onSettings: () => void;
  onToggleFavorite: (chat: Chat) => void;
  onToggleList: (chat: Chat, list: string) => void;
  /** Opens the list editor, with a chat already in the new list. */
  onNewList: (chat?: Chat) => void;
  onEditList: (list: string) => void;
}

interface OpenMenu {
  x: number;
  y: number;
  label: string;
  items: MenuEntry[];
}

function Chip({
  active,
  label,
  count,
  onClick,
  onContextMenu,
}: {
  active: boolean;
  label: string;
  count?: number;
  onClick: () => void;
  onContextMenu?: (event: MouseEvent) => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      onContextMenu={onContextMenu}
      aria-label={count ? `${label}, ${count} unread` : label}
      className={[
        "flex h-8 shrink-0 items-center gap-1.5 rounded-full border px-3 text-[14px] whitespace-nowrap transition-colors duration-100",
        active
          ? "border-transparent bg-(--chip-active-bg) text-(--chip-active-text)"
          : "border-transparent bg-(--chip-bg) text-(--chip-text) hover:brightness-95",
      ].join(" ")}
    >
      {label}
      {count ? <span className="text-[12.5px] tabular-nums opacity-80">{count}</span> : null}
    </button>
  );
}

function HeaderButton({ label, onClick, children }: { label: string; onClick: (event: MouseEvent) => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className="flex h-10 w-10 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
    >
      {children}
    </button>
  );
}

export function Sidebar({
  title,
  chats,
  selected,
  onSelect,
  showAccount,
  banner,
  emptyText,
  lists,
  listsOf,
  onNewChat,
  onStarred,
  onSettings,
  onToggleFavorite,
  onToggleList,
  onNewList,
  onEditList,
}: SidebarProps) {
  const [query, setQuery] = useState("");
  const [chosen, setChosen] = useState<Filter>("all");
  const [menu, setMenu] = useState<OpenMenu | null>(null);

  const filters = useMemo(
    () => [
      ...FILTERS,
      ...lists.map((name) => ({
        id: `list:${name}` as Filter,
        label: name,
        test: (chat: Chat) => listsOf(chat).includes(name),
      })),
    ],
    [lists, listsOf],
  );
  // A list that was deleted falls back to every chat.
  const active = filters.find((f) => f.id === chosen) ?? filters[0];

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return chats.filter((c) => {
      if (!active.test(c)) return false;
      if (!q) return true;
      return c.name.toLowerCase().includes(q) || c.lastMessage.toLowerCase().includes(q);
    });
  }, [chats, query, active]);

  function openChatMenu(chat: Chat, x: number, y: number) {
    const inLists = listsOf(chat);
    setMenu({
      x,
      y,
      label: `Options for ${chat.name}`,
      items: [
        {
          label: chat.favorite ? "Remove from favorites" : "Add to favorites",
          icon: <StarIcon size={18} filled={chat.favorite} />,
          onSelect: () => onToggleFavorite(chat),
        },
        "separator",
        ...(lists.length > 0 ? [{ heading: "Lists" }] : []),
        ...lists.map((name) => ({
          label: name,
          checked: inLists.includes(name),
          onSelect: () => onToggleList(chat, name),
        })),
        { label: "New list…", icon: <PlusIcon size={18} />, onSelect: () => onNewList(chat) },
      ],
    });
  }

  function openHeaderMenu(event: MouseEvent) {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    setMenu({
      x: rect.left,
      y: rect.bottom + 4,
      label: "Menu",
      items: [
        { label: "Starred messages", icon: <StarIcon size={18} />, onSelect: onStarred },
        { label: "New list", icon: <PlusIcon size={18} />, onSelect: () => onNewList() },
        { label: "Settings", icon: <SettingsIcon size={18} />, onSelect: onSettings },
      ],
    });
  }

  return (
    <div className="flex h-full flex-col bg-(--panel-bg)">
      <header className="flex h-15 shrink-0 items-center gap-1 pr-2 pl-4">
        <h1 className="min-w-0 flex-1 truncate font-display text-[22px] font-bold text-(--text-primary)">{title}</h1>
        <HeaderButton label="New chat" onClick={onNewChat}>
          <NewChatIcon size={22} />
        </HeaderButton>
        <HeaderButton label="Menu" onClick={openHeaderMenu}>
          <MoreIcon size={22} className="rotate-90" />
        </HeaderButton>
      </header>

      <div className="px-3 pb-2">
        <label className="sr-only" htmlFor="chat-search">
          Search or start a new chat
        </label>
        <div className="relative">
          <SearchIcon
            size={18}
            className="pointer-events-none absolute top-1/2 left-4 -translate-y-1/2 text-(--icon)"
          />
          <input
            id="chat-search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setQuery("");
            }}
            placeholder="Search"
            className="h-9 w-full rounded-lg bg-(--search-bg) pr-3 pl-12 text-[15px] text-(--text-primary) outline-none placeholder:text-(--text-secondary) focus-visible:outline-none"
          />
        </div>
      </div>

      <div className="no-scrollbar flex gap-2 overflow-x-auto px-3 pb-2" role="group" aria-label="Filter chats">
        {filters.map((f) => {
          const list = f.id.startsWith("list:") ? f.label : null;
          return (
            <Chip
              key={f.id}
              active={active.id === f.id}
              label={f.label}
              count={f.id === "all" ? undefined : chats.filter((c) => c.unread > 0 && f.test(c)).length}
              onClick={() => setChosen(f.id)}
              onContextMenu={
                list
                  ? (event) => {
                      event.preventDefault();
                      setMenu({
                        x: event.clientX,
                        y: event.clientY,
                        label: `Options for ${list}`,
                        items: [{ label: "Edit list", onSelect: () => onEditList(list) }],
                      });
                    }
                  : undefined
              }
            />
          );
        })}
        <button
          type="button"
          onClick={() => onNewList()}
          aria-label="New list"
          title="New list"
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-(--chip-bg) text-(--chip-text) hover:brightness-95"
        >
          <PlusIcon size={18} />
        </button>
      </div>

      {banner}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {filtered.length === 0 ? (
          <div className="px-6 py-10 text-center text-[14px] text-(--text-secondary)">
            <p>{chats.length === 0 ? emptyText : query ? "No chats match" : `No chats in ${active.label}`}</p>
            {active.id.startsWith("list:") && !query && chats.length > 0 ? (
              <button
                type="button"
                onClick={() => onEditList(active.label)}
                className="mt-3 font-medium text-(--accent-strong) hover:underline"
              >
                Add chats
              </button>
            ) : null}
          </div>
        ) : (
          <ChatList
            chats={filtered}
            selected={selected}
            onSelect={onSelect}
            showAccount={showAccount}
            onMenu={openChatMenu}
          />
        )}
      </div>

      {menu ? (
        <PopupMenu x={menu.x} y={menu.y} label={menu.label} items={menu.items} onClose={() => setMenu(null)} />
      ) : null}
    </div>
  );
}
