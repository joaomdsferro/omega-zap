import { useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import type { Chat } from "../backend";
import { AccountBadge } from "./AccountBadge";
import { Avatar } from "./Avatar";
import { CheckIcon, CloseIcon, SearchIcon } from "./Icons";

export function chatKey(chat: { account: string; id: string }) {
  return `${chat.account}:${chat.id}`;
}

interface ListEditorProps {
  /** The list's current name, when it already exists. */
  name?: string;
  /** Chats that start out ticked. */
  selected: Set<string>;
  chats: Chat[];
  showAccount?: boolean;
  onSave: (name: string, selected: Set<string>) => Promise<void>;
  onDelete?: () => Promise<void>;
  onClose: () => void;
}

export function ListEditor({ name: initialName, selected: initialSelected, chats, showAccount, onSave, onDelete, onClose }: ListEditorProps) {
  const [name, setName] = useState(initialName ?? "");
  const [selected, setSelected] = useState(() => new Set(initialSelected));
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const dialog = useRef<HTMLFormElement>(null);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  // Ticked chats first, so the list's members are easy to review.
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const matching = chats.filter((c) => !q || c.name.toLowerCase().includes(q));
    return [
      ...matching.filter((c) => initialSelected.has(chatKey(c))),
      ...matching.filter((c) => !initialSelected.has(chatKey(c))),
    ];
  }, [chats, query, initialSelected]);

  function toggle(key: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!name.trim() || busy) return;
    void run(() => onSave(name.trim(), selected));
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

  const title = initialName ? "Edit list" : "New list";

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4"
      onPointerDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <form
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="list-editor-title"
        onSubmit={submit}
        onKeyDown={trapFocus}
        className="animate-fade-rise flex max-h-[min(640px,100%)] w-full max-w-110 flex-col overflow-hidden rounded-2xl bg-(--panel-bg)"
        style={{ boxShadow: "0 17px 50px rgba(11,20,26,.19), 0 12px 15px rgba(11,20,26,.24)" }}
      >
        <header className="flex items-center gap-3 px-5 pt-5 pb-3">
          <h2 id="list-editor-title" className="flex-1 text-[19px] font-medium text-(--text-primary)">
            {title}
          </h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="flex h-9 w-9 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
          >
            <CloseIcon size={20} />
          </button>
        </header>

        <div className="px-5">
          <label htmlFor="list-name" className="text-[13px] text-(--text-secondary)">
            List name
          </label>
          <input
            id="list-name"
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            maxLength={40}
            placeholder="Example: Work, Friends"
            className="mt-1 h-10 w-full border-b-2 border-(--accent) bg-transparent text-[16px] text-(--text-primary) outline-none placeholder:text-(--text-secondary) focus-visible:outline-none"
          />
        </div>

        <div className="px-5 pt-4 pb-2">
          <p className="mb-2 text-[13px] text-(--text-secondary)">
            {selected.size === 0 ? "Choose the chats in this list" : `${selected.size} ${selected.size === 1 ? "chat" : "chats"} in this list`}
          </p>
          <div className="relative">
            <SearchIcon size={16} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-(--icon)" />
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search chats"
              aria-label="Search chats"
              className="h-9 w-full rounded-lg bg-(--search-bg) pr-3 pl-9 text-[14px] text-(--text-primary) outline-none placeholder:text-(--text-secondary) focus-visible:outline-none"
            />
          </div>
        </div>

        <ul className="min-h-30 flex-1 overflow-y-auto pb-2" aria-label="Chats">
          {shown.map((chat) => {
            const key = chatKey(chat);
            const checked = selected.has(key);
            return (
              <li key={key}>
                <div
                  role="checkbox"
                  aria-checked={checked}
                  tabIndex={0}
                  onClick={() => toggle(key)}
                  onKeyDown={(event) => {
                    if (event.key === " " || event.key === "Enter") {
                      event.preventDefault();
                      toggle(key);
                    }
                  }}
                  className="flex items-center gap-3 px-5 py-2 hover:bg-(--hover-bg) focus-visible:bg-(--hover-bg)"
                >
                  <div className="relative shrink-0">
                    <Avatar name={chat.name} initials={chat.initials} hue={chat.avatarHue} src={chat.avatarUrl} size={40} group={chat.isGroup} />
                    {showAccount ? (
                      <span className="absolute -right-0.5 -bottom-0.5 rounded-full border-2 border-(--panel-bg)">
                        <AccountBadge account={chat.account} size={15} />
                      </span>
                    ) : null}
                  </div>
                  <span className="min-w-0 flex-1 truncate text-[15.5px] text-(--text-primary)">{chat.name}</span>
                  <span
                    aria-hidden
                    className={[
                      "flex h-5 w-5 shrink-0 items-center justify-center rounded-sm border-2",
                      checked ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--text-secondary)",
                    ].join(" ")}
                  >
                    {checked ? <CheckIcon size={14} strokeWidth={3} /> : null}
                  </span>
                </div>
              </li>
            );
          })}
          {shown.length === 0 ? (
            <li className="px-5 py-8 text-center text-[14px] text-(--text-secondary)">No chats match</li>
          ) : null}
        </ul>

        {error ? (
          <p role="alert" className="px-5 pb-2 text-[13.5px] text-(--danger)">
            {error}
          </p>
        ) : null}

        <footer className="flex items-center gap-2 border-t border-(--divider) px-5 py-3">
          {onDelete ? (
            confirmDelete ? (
              <button
                type="button"
                disabled={busy}
                onClick={() => void run(onDelete)}
                className="h-9 rounded-full px-3 text-[14px] font-medium text-(--danger) hover:bg-(--hover-bg) disabled:opacity-50"
              >
                Delete for good?
              </button>
            ) : (
              <button
                type="button"
                disabled={busy}
                onClick={() => setConfirmDelete(true)}
                className="h-9 rounded-full px-3 text-[14px] text-(--danger) hover:bg-(--hover-bg) disabled:opacity-50"
              >
                Delete list
              </button>
            )
          ) : null}
          <span className="flex-1" />
          <button
            type="button"
            onClick={onClose}
            className="h-9 rounded-full px-4 text-[14px] text-(--accent-strong) hover:bg-(--hover-bg)"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!name.trim() || busy}
            className="h-9 rounded-full bg-(--accent) px-5 text-[14px] font-medium text-(--on-accent) hover:brightness-105 disabled:opacity-50"
          >
            {busy ? "Saving…" : "Save"}
          </button>
        </footer>
      </form>
    </div>
  );
}
