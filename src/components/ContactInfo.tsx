import { useEffect, useState } from "react";
import type { Person } from "../backend";
import { Avatar } from "./Avatar";
import { CloseIcon } from "./Icons";

interface ContactInfoProps {
  load: () => Promise<Person>;
  onClose: () => void;
  /** Opens a chat with them; absent for ourselves. */
  onMessage?: (person: Person) => void;
}

/** Someone's contact info, as WhatsApp shows it beside the chat. */
export function ContactInfo({ load, onClose, onMessage }: ContactInfoProps) {
  const [person, setPerson] = useState<Person | null>(null);

  useEffect(() => {
    let alive = true;
    load()
      .then((loaded) => alive && setPerson(loaded))
      .catch(console.error);
    return () => {
      alive = false;
    };
  }, [load]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => event.key === "Escape" && onClose();
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return (
    <aside
      className="absolute inset-y-0 right-0 z-40 flex w-[min(400px,100%)] flex-col border-l border-(--divider) bg-(--panel-bg)"
      aria-label="Contact info"
    >
      <header className="flex h-15 shrink-0 items-center gap-6 bg-(--panel-header-bg) px-4">
        <button
          type="button"
          onClick={onClose}
          aria-label="Close contact info"
          className="flex h-10 w-10 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
        >
          <CloseIcon size={22} />
        </button>
        <h2 className="text-[16px] text-(--text-primary)">Contact info</h2>
      </header>
      {person ? (
        <div className="flex-1 overflow-y-auto">
          <div className="flex flex-col items-center px-6 pt-7 pb-5 text-center">
            <Avatar name={person.name} initials={person.initials} hue={person.avatarHue} src={person.avatarUrl} size={200} />
            <h3 className="mt-4 text-[22px] text-(--text-primary)">{person.name}</h3>
            {person.phone && person.phone !== person.name ? (
              <p className="mt-1 text-[16px] text-(--text-secondary)">{person.phone}</p>
            ) : null}
            {person.profileName ? <p className="mt-1 text-[14px] text-(--text-secondary)">~{person.profileName}</p> : null}
            {!person.saved && !person.isMe ? <p className="mt-1 text-[13px] text-(--text-secondary)">Not in your contacts</p> : null}
          </div>
          {onMessage && !person.isMe ? (
            <div className="flex justify-center border-t border-(--divider) py-4">
              <button
                type="button"
                onClick={() => onMessage(person)}
                className="rounded-full border border-(--divider) px-6 py-2 text-[14px] text-(--accent) hover:bg-(--hover-bg)"
              >
                Message
              </button>
            </div>
          ) : null}
        </div>
      ) : null}
    </aside>
  );
}
