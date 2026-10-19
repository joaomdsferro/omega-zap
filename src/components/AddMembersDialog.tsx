import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { Contact } from "../backend";
import { fold } from "../lib/search";
import { Avatar } from "./Avatar";
import { CheckIcon, CloseIcon } from "./Icons";

interface AddMembersDialogProps {
  /** The account's contacts; those already in the group are left out. */
  loadContacts: () => Promise<Contact[]>;
  /** Ids already in the group. */
  members: Set<string>;
  onAdd: (people: Contact[]) => Promise<void>;
  onClose: () => void;
}

/** Picks contacts to add to a group, as WhatsApp's "Add member". */
export function AddMembersDialog({ loadContacts, members, onAdd, onClose }: AddMembersDialogProps) {
  const [contacts, setContacts] = useState<Contact[] | null>(null);
  const [query, setQuery] = useState("");
  const [picked, setPicked] = useState<Contact[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialog = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    loadContacts()
      .then((list) => alive && setContacts(list.filter((c) => !c.isMe && !members.has(c.id))))
      .catch((e) => alive && setError(String(e instanceof Error ? e.message : e)));
    return () => {
      alive = false;
    };
  }, [loadContacts, members]);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  const found = useMemo(() => {
    const q = fold(query.trim());
    const list = contacts ?? [];
    if (!q) return list;
    const digits = q.replace(/\D/g, "");
    return list.filter((c) => fold(c.name).includes(q) || (digits && c.phone?.replace(/\D/g, "").includes(digits)));
  }, [contacts, query]);

  function toggle(contact: Contact) {
    setPicked((prev) => (prev.some((p) => p.id === contact.id) ? prev.filter((p) => p.id !== contact.id) : [...prev, contact]));
  }

  async function add() {
    if (!picked.length) return;
    setBusy(true);
    setError(null);
    try {
      await onAdd(picked);
      onClose();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
      setBusy(false);
    }
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    if (!busy) onClose();
  }

  return (
    <div
      className="animate-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      onPointerDown={(event) => event.target === event.currentTarget && !busy && onClose()}
    >
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-members-title"
        onKeyDown={onKeyDown}
        className="animate-fade-rise flex max-h-[min(640px,90vh)] w-full max-w-110 flex-col overflow-hidden rounded-2xl bg-(--panel-bg)"
        style={{ boxShadow: "0 17px 50px rgba(11,20,26,.19), 0 12px 15px rgba(11,20,26,.24)" }}
      >
        <header className="flex items-center gap-3 px-4 pt-4 pb-2">
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="flex h-9 w-9 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
          >
            <CloseIcon size={20} />
          </button>
          <h2 id="add-members-title" className="text-[17px] text-(--text-primary)">
            Add member
          </h2>
        </header>
        <div className="px-4 pb-2">
          <input
            autoFocus
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search name or number"
            aria-label="Search contacts"
            className="h-9 w-full rounded-lg bg-(--search-bg,var(--hover-bg)) px-3 text-[14px] text-(--text-primary) outline-none placeholder:text-(--text-secondary)"
          />
          {picked.length ? (
            <ul className="mt-2 flex flex-wrap gap-1.5" aria-label="Picked">
              {picked.map((p) => (
                <li key={p.id}>
                  <button
                    type="button"
                    onClick={() => toggle(p)}
                    aria-label={`Remove ${p.name}`}
                    className="flex items-center gap-1.5 rounded-full bg-(--hover-bg) py-0.5 pr-2 pl-0.5"
                  >
                    <Avatar name={p.name} initials={p.initials} hue={p.avatarHue} src={p.avatarUrl} size={22} />
                    <span className="max-w-32 truncate text-[13px] text-(--text-primary)">{p.name}</span>
                    <CloseIcon size={12} className="text-(--text-secondary)" />
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
        </div>
        <ul className="min-h-0 flex-1 overflow-y-auto px-2" aria-label="Contacts">
          {contacts === null && !error ? <li className="px-3 py-3 text-[14px] text-(--text-secondary)">Loading contacts…</li> : null}
          {contacts && found.length === 0 ? (
            <li className="px-3 py-3 text-[14px] text-(--text-secondary)">{query ? "No contacts found" : "Everyone in your contacts is in this group"}</li>
          ) : null}
          {found.map((contact) => {
            const on = picked.some((p) => p.id === contact.id);
            return (
              <li key={contact.id}>
                <button
                  type="button"
                  role="checkbox"
                  aria-checked={on}
                  onClick={() => toggle(contact)}
                  className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-(--hover-bg)"
                >
                  <Avatar name={contact.name} initials={contact.initials} hue={contact.avatarHue} src={contact.avatarUrl} size={40} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[15px] text-(--text-primary)">{contact.name}</span>
                    {contact.phone && contact.phone !== contact.name ? (
                      <span className="block truncate text-[13px] text-(--text-secondary)">{contact.phone}</span>
                    ) : null}
                  </span>
                  <span
                    aria-hidden
                    className={[
                      "flex size-5 shrink-0 items-center justify-center rounded border-2",
                      on ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--text-secondary)/60",
                    ].join(" ")}
                  >
                    {on ? <CheckIcon size={14} /> : null}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
        {error ? (
          <p className="px-5 pt-2 text-[13px] text-(--danger)" role="alert">
            {error}
          </p>
        ) : null}
        <div className="flex justify-end gap-2 px-5 pt-3 pb-4">
          <button type="button" onClick={onClose} disabled={busy} className="rounded-full border border-(--divider) px-5 py-2 hover:bg-(--hover-bg)">
            <span className="text-[14px] font-medium text-(--accent)">Cancel</span>
          </button>
          <button
            type="button"
            onClick={() => void add()}
            disabled={busy || picked.length === 0}
            className="rounded-full bg-(--accent) px-5 py-2 hover:brightness-95 disabled:opacity-60"
          >
            <span className="text-[14px] font-medium text-(--on-accent)">{picked.length ? `Add ${picked.length}` : "Add"}</span>
          </button>
        </div>
      </div>
    </div>
  );
}
