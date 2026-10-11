import { useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import type { Chat, Message } from "../backend";
import { AccountBadge } from "./AccountBadge";
import { Avatar } from "./Avatar";
import { chatKey } from "./ListEditor";
import { CheckIcon, CloseIcon, SearchIcon, SendIcon } from "./Icons";

/** How many chats one message can be forwarded to at once, as on the phone. */
const MAX_TARGETS = 5;

/** Whether a message can be sent on: attachments are uploaded again, texts sent as they are. */
export function canForward(message: Message): boolean {
  return !message.deleted && Boolean(message.hasMedia || message.attachments?.length || (message.text && message.kind === "text"));
}

interface ForwardDialogProps {
  /** The messages to send on, oldest first. */
  messages: Message[];
  chats: Chat[];
  showAccount?: boolean;
  /** Sends the messages to every chosen chat. */
  onForward: (targets: Chat[]) => Promise<void>;
  onClose: () => void;
}

export function ForwardDialog({ messages, chats, showAccount, onForward, onClose }: ForwardDialogProps) {
  const [selected, setSelected] = useState<string[]>([]);
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialog = useRef<HTMLFormElement>(null);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return chats.filter((c) => !q || c.name.toLowerCase().includes(q));
  }, [chats, query]);

  function toggle(key: string) {
    setSelected((prev) =>
      prev.includes(key) ? prev.filter((k) => k !== key) : prev.length < MAX_TARGETS ? [...prev, key] : prev,
    );
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (selected.length === 0 || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onForward(selected.flatMap((key) => chats.filter((c) => chatKey(c) === key)));
      onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  // Tab stays inside the dialog.
  function trapFocus(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== "Tab" || !dialog.current) return;
    const focusable = [
      ...dialog.current.querySelectorAll<HTMLElement>("input, button:not(:disabled), [role='checkbox']"),
    ];
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  }

  const names = selected.flatMap((key) => chats.filter((c) => chatKey(c) === key).map((c) => c.name));

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4"
      onPointerDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <form
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="forward-title"
        onSubmit={submit}
        onKeyDown={trapFocus}
        className="animate-fade-rise flex max-h-[min(640px,100%)] w-full max-w-110 flex-col overflow-hidden rounded-2xl bg-(--panel-bg)"
        style={{ boxShadow: "0 17px 50px rgba(11,20,26,.19), 0 12px 15px rgba(11,20,26,.24)" }}
      >
        <header className="flex items-center gap-3 px-5 pt-5 pb-3">
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="flex h-9 w-9 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
          >
            <CloseIcon size={20} />
          </button>
          <h2 id="forward-title" className="flex-1 text-[19px] font-medium text-(--text-primary)">
            {messages.length === 1 ? "Forward message to" : `Forward ${messages.length} messages to`}
          </h2>
        </header>

        <div className="px-5 pb-2">
          <div className="relative">
            <SearchIcon size={16} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-(--icon)" />
            <input
              type="search"
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search chats"
              aria-label="Search chats"
              className="h-9 w-full rounded-lg bg-(--search-bg) pr-3 pl-9 text-[14px] text-(--text-primary) outline-none placeholder:text-(--text-secondary) focus-visible:outline-none"
            />
          </div>
          {messages.length === 1 && messages[0].text ? (
            <p className="mt-2 line-clamp-2 text-[13px] text-(--text-secondary)">{messages[0].text}</p>
          ) : null}
        </div>

        <ul className="min-h-30 flex-1 overflow-y-auto pb-2" aria-label="Chats">
          {shown.map((chat) => {
            const key = chatKey(chat);
            const checked = selected.includes(key);
            const full = !checked && selected.length >= MAX_TARGETS;
            return (
              <li key={key}>
                <div
                  role="checkbox"
                  aria-checked={checked}
                  aria-disabled={full}
                  tabIndex={0}
                  onClick={() => toggle(key)}
                  onKeyDown={(event) => {
                    if (event.key === " " || event.key === "Enter") {
                      event.preventDefault();
                      toggle(key);
                    }
                  }}
                  className={[
                    "flex items-center gap-3 px-5 py-2 hover:bg-(--hover-bg) focus-visible:bg-(--hover-bg)",
                    full ? "opacity-50" : "",
                  ].join(" ")}
                >
                  <span
                    aria-hidden
                    className={[
                      "flex h-5 w-5 shrink-0 items-center justify-center rounded-sm border-2",
                      checked ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--text-secondary)",
                    ].join(" ")}
                  >
                    {checked ? <CheckIcon size={14} strokeWidth={3} /> : null}
                  </span>
                  <div className="relative shrink-0">
                    <Avatar name={chat.name} initials={chat.initials} hue={chat.avatarHue} src={chat.avatarUrl} size={40} group={chat.isGroup} />
                    {showAccount ? (
                      <span className="absolute -right-0.5 -bottom-0.5 rounded-full border-2 border-(--panel-bg)">
                        <AccountBadge account={chat.account} size={15} />
                      </span>
                    ) : null}
                  </div>
                  <span className="min-w-0 flex-1 truncate text-[15.5px] text-(--text-primary)">{chat.name}</span>
                </div>
              </li>
            );
          })}
          {shown.length === 0 ? (
            <li className="px-5 py-6 text-center text-[14px] text-(--text-secondary)">No chats found</li>
          ) : null}
        </ul>

        {error ? <p className="px-5 pb-2 text-[13px] text-(--danger)" role="alert">{error}</p> : null}

        {selected.length > 0 ? (
          <footer className="flex items-center gap-3 bg-(--panel-header-bg) px-5 py-3">
            <p className="min-w-0 flex-1 truncate text-[14px] text-(--text-primary)">{names.join(", ")}</p>
            <button
              type="submit"
              disabled={busy}
              aria-label="Send"
              className="flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-(--accent) text-(--on-accent) disabled:opacity-60"
            >
              <SendIcon size={22} />
            </button>
          </footer>
        ) : null}
      </form>
    </div>
  );
}
