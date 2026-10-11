import { useEffect, useRef, useState, type ReactNode } from "react";
import type { Message, Person, PersonDetails } from "../backend";
import type { LoadMedia } from "./Attachments";
import { Avatar } from "./Avatar";
import { BackIcon, ChevronRightIcon, FileIcon, MuteIcon, NewChatIcon, PhoneIcon, StarIcon, VideoIcon } from "./Icons";

interface ContactInfoProps {
  load: () => Promise<Person>;
  /** About, full-size picture and groups in common, which take a moment longer. */
  loadDetails?: () => Promise<PersonDetails>;
  loadMedia?: LoadMedia;
  onClose: () => void;
  /** Opens the chat with them, or one of its messages. Absent for ourselves. */
  onMessage?: (person: Person, messageId?: string) => void;
  onOpenGroup?: (id: string) => void;
  onStarred?: () => void;
  onMute?: (person: Person, muted: boolean) => Promise<void>;
}

/** Someone's contact info, as WhatsApp shows it beside the chat. */
export function ContactInfo({ load, loadDetails, loadMedia, onClose, onMessage, onOpenGroup, onStarred, onMute }: ContactInfoProps) {
  const [person, setPerson] = useState<Person | null>(null);
  const [details, setDetails] = useState<PersonDetails | null>(null);

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
    if (!loadDetails) return;
    let alive = true;
    loadDetails()
      .then((loaded) => alive && setDetails(loaded))
      .catch(console.error);
    return () => {
      alive = false;
    };
  }, [loadDetails]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => event.key === "Escape" && onClose();
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  async function toggleMute() {
    if (!person || !onMute) return;
    const muted = !person.muted;
    setPerson({ ...person, muted });
    await onMute(person, muted).catch(() => setPerson({ ...person, muted: !muted }));
  }

  // What was found last time shows at once; the network's answer replaces it.
  const groups = details?.commonGroups ?? person?.commonGroups ?? [];
  const about = details ? details.about : person?.about;
  return (
    <aside
      className="absolute inset-y-0 right-0 z-40 flex w-[min(420px,100%)] flex-col border-l border-(--divider) bg-(--panel-bg)"
      aria-label="Contact info"
    >
      <header className="flex h-15 shrink-0 items-center gap-4 bg-(--panel-header-bg) px-3">
        <button
          type="button"
          onClick={onClose}
          aria-label="Back"
          className="flex h-10 w-10 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
        >
          <BackIcon size={22} />
        </button>
        <h2 className="text-[16px] text-(--text-primary)">Contact info</h2>
      </header>
      {person ? (
        <div className="flex-1 overflow-y-auto">
          <section className="flex flex-col items-center px-6 pt-7 pb-6 text-center">
            <Avatar
              name={person.name}
              initials={person.initials}
              hue={person.avatarHue}
              src={details?.photoUrl ?? person.avatarUrl}
              size={200}
            />
            <h3 className="mt-4 text-[22px] leading-7 text-(--text-primary)">{person.name}</h3>
            {person.phone && person.phone !== person.name ? (
              <p className="mt-1 text-[16px] text-(--text-secondary)">{person.phone}</p>
            ) : null}
            {person.profileName ? <p className="mt-0.5 text-[14px] text-(--text-secondary)">~{person.profileName}</p> : null}
            {!person.isMe ? (
              <div className="mt-5 flex gap-3">
                <Action icon={<NewChatIcon size={22} />} label="Message" onClick={onMessage && (() => onMessage(person))} />
                <Action icon={<PhoneIcon size={22} />} label="Voice" soon />
                <Action icon={<VideoIcon size={22} />} label="Video" soon />
              </div>
            ) : null}
          </section>

          {about ? (
            <Section>
              <h4 className="px-3 text-[14px] text-(--text-secondary)">About</h4>
              <p className="mt-1 px-3 text-[15px] break-words whitespace-pre-wrap text-(--text-primary)">{about}</p>
            </Section>
          ) : null}

          {person.mediaCount > 0 ? (
            <Section>
              <Row
                icon={<FileIcon size={20} />}
                label="Media, links and docs"
                value={String(person.mediaCount)}
                onClick={onMessage && (() => onMessage(person, person.recentMedia[0]?.id))}
              />
              {person.recentMedia.length > 0 ? (
                <div className="mt-2 grid grid-cols-3 gap-2 px-2">
                  {person.recentMedia.map((message) => (
                    <Thumb key={message.id} message={message} loadMedia={loadMedia} onOpen={onMessage && (() => onMessage(person, message.id))} />
                  ))}
                </div>
              ) : null}
            </Section>
          ) : null}

          {person.hasChat ? (
            <Section>
              {person.starred > 0 ? (
                <Row icon={<StarIcon size={20} />} label="Starred messages" value={String(person.starred)} onClick={onStarred} />
              ) : null}
              {onMute ? (
                <button
                  type="button"
                  role="switch"
                  aria-checked={person.muted}
                  onClick={toggleMute}
                  className="flex w-full items-center gap-5 rounded-lg px-3 py-3 text-left hover:bg-(--hover-bg)"
                >
                  <MuteIcon size={20} className="text-(--text-secondary)" />
                  <span className="flex-1 text-[15px] text-(--text-primary)">Mute notifications</span>
                  <span
                    className={["flex h-6 w-11 shrink-0 items-center rounded-full px-0.5 transition-colors", person.muted ? "bg-(--accent)" : "bg-(--text-secondary)/40"].join(" ")}
                    aria-hidden
                  >
                    <span className={["h-5 w-5 rounded-full bg-white transition-transform", person.muted ? "translate-x-5" : ""].join(" ")} />
                  </span>
                </button>
              ) : null}
            </Section>
          ) : null}

          {groups.length > 0 ? (
            <Section>
              <h4 className="px-3 text-[14px] text-(--text-secondary)">
                {groups.length} {groups.length === 1 ? "group" : "groups"} in common
              </h4>
              <ul className="mt-1">
                {groups.map((group) => (
                  <li key={group.id}>
                    <button
                      type="button"
                      onClick={onOpenGroup && (() => onOpenGroup(group.id))}
                      disabled={!onOpenGroup}
                      className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-(--hover-bg)"
                    >
                      <Avatar name={group.name} initials={group.initials} hue={group.avatarHue} src={group.avatarUrl} size={40} group />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[15px] text-(--text-primary)">{group.name}</span>
                        {group.members ? <span className="block text-[13px] text-(--text-secondary)">{group.members} members</span> : null}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            </Section>
          ) : null}

          {!person.saved && !person.isMe ? (
            <p className="px-8 py-5 text-center text-[13px] text-(--text-secondary)">Not in your contacts</p>
          ) : null}
        </div>
      ) : null}
    </aside>
  );
}

/** One of the buttons under the name. Calls are not there yet: they say so. */
function Action({ icon, label, onClick, soon }: { icon: ReactNode; label: string; onClick?: () => void; soon?: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={soon || !onClick}
      title={soon ? `${label} calls are coming soon` : undefined}
      className="flex w-21 flex-col items-center gap-1.5 rounded-xl border border-(--divider) py-2.5 text-[13px] text-(--text-primary) hover:bg-(--hover-bg) disabled:hover:bg-transparent"
    >
      <span className={soon ? "text-(--text-secondary)" : "text-(--accent)"}>{icon}</span>
      {label}
      {soon ? <span className="-mt-1 text-[10.5px] text-(--text-secondary)">Coming soon</span> : null}
    </button>
  );
}

function Section({ children }: { children: ReactNode }) {
  return <section className="border-t-8 border-(--panel-header-bg) px-3 py-3">{children}</section>;
}

function Row({ icon, label, value, onClick }: { icon: ReactNode; label: string; value?: string; onClick?: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={!onClick}
      className="flex w-full items-center gap-5 rounded-lg px-3 py-3 text-left hover:bg-(--hover-bg) disabled:cursor-default disabled:hover:bg-transparent"
    >
      <span className="text-(--text-secondary)">{icon}</span>
      <span className="flex-1 text-[15px] text-(--text-primary)">{label}</span>
      {value ? <span className="text-[14px] text-(--text-secondary)">{value}</span> : null}
      {onClick ? <ChevronRightIcon size={18} className="text-(--text-secondary)" /> : null}
    </button>
  );
}

/** A photo or video from the chat with them, square. */
function Thumb({ message, loadMedia, onOpen }: { message: Message; loadMedia?: LoadMedia; onOpen?: () => void }) {
  const [url, setUrl] = useState<string | null>(null);
  const video = useRef(message.kind !== "image");
  useEffect(() => {
    if (!loadMedia || !message.hasMedia) return;
    let alive = true;
    loadMedia(message)
      .then((loaded) => alive && setUrl(loaded))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [message, loadMedia]);
  return (
    <button
      type="button"
      onClick={onOpen}
      disabled={!onOpen}
      aria-label={video.current ? "Open video" : "Open photo"}
      className="aspect-square overflow-hidden rounded-lg bg-(--hover-bg)"
    >
      {url ? (
        video.current ? (
          <video src={`${url}#t=0.1`} muted playsInline preload="metadata" className="size-full object-cover" />
        ) : (
          <img src={url} alt="" draggable={false} className="size-full object-cover" />
        )
      ) : null}
    </button>
  );
}
