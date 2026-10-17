import { useMemo, useRef, useState } from "react";
import type { Contact } from "../backend";
import { AccountBadge } from "./AccountBadge";
import { Avatar } from "./Avatar";
import { BackIcon, SearchIcon } from "./Icons";
import { useVirtualRows } from "../lib/useVirtualRows";

interface NewChatPanelProps {
  /** `null` while they load. */
  contacts: Contact[] | null;
  /** Mark each contact with its network: they come from several accounts. */
  showAccount?: boolean;
  /** Finds who has a phone number, on the networks that can look one up. */
  lookupNumber?: (phone: string) => Promise<Contact | null>;
  /** Named in the hint, e.g. "WhatsApp". */
  lookupLabel?: string;
  onBack: () => void;
  onOpen: (contact: Contact) => void;
}

/** Heights of a contact row (`h-18`) and of a letter heading, in px. */
const ROW_PX = 72;
const LETTER_PX = 58;

const byName = new Intl.Collator(undefined, { sensitivity: "base" });

/** One line of the list: ourselves, a letter heading or a contact under it. */
type Item = { me: Contact } | { letter: string } | { contact: Contact };

/** The letter a contact is filed under; anything that isn't one goes under "#". */
function sectionOf(name: string) {
  const first = name.normalize("NFD").replace(/[̀-ͯ]/g, "").charAt(0).toUpperCase();
  return /[A-Z]/.test(first) ? first : "#";
}

function Row({ contact, showAccount, title, subtitle, onClick }: { contact: Contact; showAccount?: boolean; title: string; subtitle?: string; onClick: () => void }) {
  return (
    <li>
      <button
        type="button"
        onClick={onClick}
        className="flex h-18 w-full items-center gap-4 px-4 text-left hover:bg-(--hover-bg)"
      >
        <div className="relative shrink-0">
          <Avatar name={contact.name} initials={contact.initials} hue={contact.avatarHue} src={contact.avatarUrl} size={49} />
          {showAccount ? (
            <span className="absolute -right-0.5 -bottom-0.5 rounded-full border-2 border-(--panel-bg)">
              <AccountBadge account={contact.account} size={18} />
            </span>
          ) : null}
        </div>
        <span className="flex min-w-0 flex-1 flex-col border-b border-(--divider) py-3.5">
          <span className="truncate text-[17px] leading-5.25 text-(--text-primary)">{title}</span>
          {subtitle ? <span className="truncate text-[14px] leading-5 text-(--text-secondary)">{subtitle}</span> : null}
        </span>
      </button>
    </li>
  );
}

export function NewChatPanel({ contacts, showAccount, lookupNumber, lookupLabel, onBack, onOpen }: NewChatPanelProps) {
  const [query, setQuery] = useState("");
  const [checking, setChecking] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const q = query.trim().toLowerCase();
  const digits = query.replace(/\D/g, "");
  const looksLikeNumber = /^\+?[\d\s().-]+$/.test(query.trim()) && digits.length >= 7;

  const items = useMemo(() => {
    const matching = (contacts ?? []).filter(
      (c) => !q || c.name.toLowerCase().includes(q) || (digits.length >= 3 && c.phone?.replace(/\D/g, "").includes(digits)),
    );
    const people = matching.filter((c) => !c.isMe).sort((a, b) => byName.compare(a.name, b.name));
    const grouped = new Map<string, Contact[]>();
    for (const contact of people) {
      const letter = sectionOf(contact.name);
      const group = grouped.get(letter);
      if (group) group.push(contact);
      else grouped.set(letter, [contact]);
    }
    // "#" first, as on the phone.
    const order = [...grouped.keys()].sort((a, b) => (a === "#" ? -1 : b === "#" ? 1 : a.localeCompare(b)));
    const list: Item[] = matching.filter((c) => c.isMe).map((me) => ({ me }));
    for (const letter of order) {
      list.push({ letter });
      for (const contact of grouped.get(letter)!) list.push({ contact });
    }
    return list;
  }, [contacts, q, digits]);

  // Only the lines in view exist: an address book of thousands opens at once.
  const listRef = useRef<HTMLUListElement>(null);
  const tops = useMemo(() => {
    const at = [0];
    for (const item of items) at.push(at[at.length - 1] + ("letter" in item ? LETTER_PX : ROW_PX));
    return at;
  }, [items]);
  const { start, end } = useVirtualRows(listRef, tops);

  async function openNumber() {
    if (!lookupNumber || checking) return;
    setChecking(true);
    setProblem(null);
    try {
      const found = await lookupNumber(digits);
      if (found) onOpen(found);
      else setProblem(`+${digits} isn't on ${lookupLabel ?? "this network"}.`);
    } catch (e) {
      setProblem(String(e));
    } finally {
      setChecking(false);
    }
  }

  const empty = contacts !== null && items.length === 0;

  return (
    <div className="flex h-full flex-col bg-(--panel-bg)">
      <header className="flex h-15 shrink-0 items-center gap-4 px-4">
        <button
          type="button"
          onClick={onBack}
          aria-label="Back to chats"
          className="flex h-9 w-9 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
        >
          <BackIcon size={22} />
        </button>
        <h1 className="text-[17px] font-medium text-(--text-primary)">New chat</h1>
      </header>

      <div className="px-3 pb-2">
        <div className="relative">
          <SearchIcon size={18} className="pointer-events-none absolute top-1/2 left-4 -translate-y-1/2 text-(--icon)" />
          <input
            autoFocus
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setProblem(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                if (query) setQuery("");
                else onBack();
              } else if (e.key === "Enter" && looksLikeNumber && lookupNumber) {
                void openNumber();
              }
            }}
            placeholder={lookupNumber ? "Search name or number" : "Search name"}
            aria-label={lookupNumber ? "Search name or number" : "Search name"}
            className="h-10 w-full rounded-full border-2 border-transparent bg-(--search-bg) pr-3 pl-12 text-[15px] text-(--text-primary) outline-none placeholder:text-(--text-secondary) focus:border-(--accent) focus-visible:outline-none"
          />
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {looksLikeNumber && lookupNumber ? (
          <div className="border-b border-(--divider) pb-1">
            <button
              type="button"
              onClick={() => void openNumber()}
              disabled={checking}
              className="flex h-18 w-full items-center gap-4 px-4 text-left hover:bg-(--hover-bg) disabled:opacity-70"
            >
              <span className="flex h-12.25 w-12.25 shrink-0 items-center justify-center rounded-full bg-(--accent) text-[20px] font-semibold text-(--on-accent)" aria-hidden>
                #
              </span>
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-[17px] text-(--text-primary)">Chat with +{digits}</span>
                <span className="truncate text-[14px] text-(--text-secondary)">
                  {checking ? `Checking ${lookupLabel ?? ""}…` : `Look the number up on ${lookupLabel ?? "the network"}`}
                </span>
              </span>
            </button>
            {problem ? (
              <p role="alert" className="px-4 pb-2 text-[13.5px] text-(--danger)">
                {problem}
              </p>
            ) : null}
          </div>
        ) : null}

        {contacts === null ? (
          <p className="px-6 py-10 text-center text-[14px] text-(--text-secondary)">Loading contacts…</p>
        ) : null}

        <ul
          ref={listRef}
          aria-label="Contacts"
          style={{ paddingTop: tops[start], paddingBottom: tops[items.length] - tops[end] }}
        >
          {items.slice(start, end).map((item) =>
            "me" in item ? (
              <Row
                key={`${item.me.account}:${item.me.id}`}
                contact={item.me}
                showAccount={showAccount}
                title={item.me.name === "You" ? "You" : `${item.me.name} (You)`}
                subtitle="Message yourself"
                onClick={() => onOpen(item.me)}
              />
            ) : "letter" in item ? (
              <li key={`letter:${item.letter}`} className="px-6 pt-6 pb-3 text-[15px] leading-5.5 text-(--text-secondary)" aria-hidden>
                {item.letter}
              </li>
            ) : (
              <Row
                key={`${item.contact.account}:${item.contact.id}`}
                contact={item.contact}
                showAccount={showAccount}
                title={item.contact.name}
                subtitle={item.contact.phone && item.contact.phone !== item.contact.name ? item.contact.phone : undefined}
                onClick={() => onOpen(item.contact)}
              />
            ),
          )}
        </ul>

        {empty && !looksLikeNumber ? (
          <p className="px-6 py-10 text-center text-[14px] text-(--text-secondary)">
            {q
              ? lookupNumber
                ? "No contacts match. Type a full phone number, with its country code, to start a chat with it."
                : "No contacts match."
              : "Your contacts will appear here once they sync."}
          </p>
        ) : null}
      </div>
    </div>
  );
}
