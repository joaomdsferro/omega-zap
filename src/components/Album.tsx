import { useEffect, useRef, useState } from "react";
import type { GroupMember, Message } from "../backend";
import { SenderAvatar } from "./Avatar";
import { authorColor } from "../lib/avatar";
import { formatMessageTime } from "../lib/format";
import type { LoadMedia } from "./Attachments";

/** WhatsApp gathers four photos or more; fewer stay as they are. */
export const ALBUM_MIN = 4;

/** A photo that can be part of an album: no caption, no quote, not deleted. */
export function albumPhoto(message: Message): boolean {
  return (
    message.kind === "image" &&
    !message.attachments?.length &&
    !message.text?.trim() &&
    !message.replyTo &&
    !message.deleted
  );
}

/** Whether `next` continues an album that ends with `last`. As on WhatsApp, the
 * photos only have to follow each other, however long apart: any other
 * message, another sender or another day ends the album. */
export function continuesAlbum(last: Message, next: Message): boolean {
  return (
    albumPhoto(next) &&
    next.fromMe === last.fromMe &&
    next.author === last.author
  );
}

interface AlbumProps {
  messages: Message[];
  showAuthor: boolean;
  /** As on a message bubble. */
  sender?: GroupMember | null;
  onOpenPerson?: (id: string) => void;
  tail: boolean;
  loadMedia?: LoadMedia;
  onOpen?: (message: Message) => void;
}

/** Four or more photos in a row, as WhatsApp shows them: a 2×2 grid, the fourth counting the rest. */
export function Album({ messages, showAuthor, sender, onOpenPerson, tail, loadMedia, onOpen }: AlbumProps) {
  const first = messages[0];
  const mine = first.fromMe;
  const tiles = messages.slice(0, 4);
  const more = messages.length - tiles.length;
  return (
    <div
      className={["flex gap-1 px-[5%]", mine ? "flex-row-reverse" : "flex-row", tail ? "mt-3" : "mt-0.5"].join(" ")}
    >
      {sender !== undefined && !mine ? <SenderAvatar member={sender} onOpen={onOpenPerson} /> : null}
      <div
        className={["rounded-[7.5px] p-0.75", tail ? (mine ? "rounded-tr-none" : "rounded-tl-none") : ""].join(" ")}
        style={{ background: mine ? "var(--bubble-out)" : "var(--bubble-in)", boxShadow: "var(--bubble-shadow)" }}
      >
        {showAuthor && first.author ? (
          <p className="px-1.5 pt-0.5 pb-1 text-[12.8px] font-medium" style={{ color: authorColor(first.author) }}>
            {first.author}
          </p>
        ) : null}
        <div className="grid grid-cols-2 gap-0.75">
          {tiles.map((message, index) => (
            <Tile
              key={message.id}
              message={message}
              loadMedia={loadMedia}
              more={index === 3 && more > 0 ? more : 0}
              onOpen={onOpen && (() => onOpen(message))}
            />
          ))}
        </div>
        {/* The photos past the fourth, so a jump to one of them finds the album. */}
        {messages.slice(4).map((message) => (
          <span key={message.id} data-message-id={message.id} hidden />
        ))}
      </div>
    </div>
  );
}

function Tile({ message, loadMedia, more, onOpen }: { message: Message; loadMedia?: LoadMedia; more: number; onOpen?: () => void }) {
  const box = useRef<HTMLButtonElement>(null);
  const [near, setNear] = useState(false);
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    const el = box.current;
    if (!el || near) return;
    const observer = new IntersectionObserver((entries) => entries.some((entry) => entry.isIntersecting) && setNear(true), {
      rootMargin: "600px 0px",
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [near]);

  useEffect(() => {
    if (!near || !message.hasMedia || !loadMedia) return;
    let alive = true;
    loadMedia(message)
      .then((loaded) => alive && setUrl(loaded))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [near, message, loadMedia]);

  const label = more ? `Open photo, and ${more} more` : "Open photo";
  return (
    <button
      ref={box}
      type="button"
      data-message-id={message.id}
      onClick={onOpen}
      disabled={!onOpen}
      aria-label={label}
      className="relative block size-41 overflow-hidden rounded-[5px] bg-(--panel-bg) disabled:cursor-default"
    >
      {url ? <img src={url} alt="" draggable={false} className="size-full object-cover" /> : null}
      {more ? (
        <span className="absolute inset-0 flex items-center justify-center bg-black/45 text-[32px] font-normal text-white">
          +{more}
        </span>
      ) : (
        <time
          dateTime={message.sentAt}
          className="absolute right-0 bottom-0 left-0 bg-linear-to-t from-black/45 to-transparent px-1.5 pt-4 pb-1 text-right text-[11px] text-white"
        >
          {formatMessageTime(message.sentAt)}
        </time>
      )}
    </button>
  );
}
