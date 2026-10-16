import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import type { Attachment, Chat, Message } from "../backend";
import { hueFor, initialsFor } from "../lib/avatar";
import { formatDayLabel, formatMessageTime } from "../lib/format";
import { GifVideo, plays, useMediaUrl, useNear } from "./Attachments";
import type { LoadMedia } from "./Attachments";
import { Avatar } from "./Avatar";
import { VideoPlayer } from "./VideoPlayer";
import { ChevronLeftIcon, ChevronRightIcon, CloseIcon, PlayIcon } from "./Icons";

/** One photo or video: a message's attachment, or the message itself when it has one picture. */
export interface ViewerItem {
  key: string;
  message: Message;
  index: number;
  attachment?: Attachment;
}

interface MediaViewerProps {
  chat: Chat;
  /** Every photo and video in the chat, oldest first: the arrows and the strip walk through them. */
  items: ViewerItem[];
  startKey: string;
  loadMedia: LoadMedia;
  onClose: () => void;
}

function itemLabel(item: ViewerItem) {
  if (item.attachment?.kind === "video") return "Video";
  if (item.attachment?.kind === "gif") return "GIF";
  return item.message.imageLabel ?? "Photo";
}

/** Fetches an item once `wanted`; null while it loads, "failed" when it can't. */
function usePicture(item: ViewerItem, loadMedia: LoadMedia, wanted = true) {
  const video = item.attachment ? plays(item.attachment) : false;
  const url = useMediaUrl(item.message, item.index, loadMedia, wanted && Boolean(item.message.hasMedia), video);
  return item.message.hasMedia ? url : "failed";
}

function Thumb({ item, loadMedia, current, onPick }: {
  item: ViewerItem;
  loadMedia: LoadMedia;
  current: boolean;
  onPick: () => void;
}) {
  const el = useRef<HTMLButtonElement>(null);
  // A chat can hold hundreds of photos: only the ones scrolled into the strip are fetched.
  const near = useNear(el, "0px 300px");
  // Videos are not fetched just for a thumbnail.
  const video = item.attachment ? plays(item.attachment) : false;
  const url = usePicture(item, loadMedia, near && !video);

  useLayoutEffect(() => {
    if (current) el.current?.scrollIntoView({ block: "nearest", inline: "center" });
  }, [current]);

  return (
    <button
      ref={el}
      type="button"
      onClick={onPick}
      aria-label={itemLabel(item)}
      aria-current={current}
      className={[
        "relative h-15 w-15 shrink-0 overflow-hidden rounded-md border-2 transition-transform",
        current ? "scale-90 border-(--accent)" : "border-transparent opacity-70 hover:opacity-100",
      ].join(" ")}
    >
      {url && url !== "failed" ? (
        <img src={url} alt="" draggable={false} className="h-full w-full object-cover" />
      ) : (
        <span className="flex h-full w-full items-center justify-center bg-[linear-gradient(145deg,color-mix(in_oklab,var(--accent)_35%,#0b141a),#1f3a33)] text-white">
          {video ? <PlayIcon size={22} /> : null}
        </span>
      )}
    </button>
  );
}

/** Full-window photo viewer, as WhatsApp Web opens a photo from the chat. */
export function MediaViewer({ chat, items, startKey, loadMedia, onClose }: MediaViewerProps) {
  // Followed by key: more history can load while the viewer is open.
  const [currentId, setCurrentId] = useState(startKey);
  const index = Math.max(0, items.findIndex((item) => item.key === currentId));
  const [zoomed, setZoomed] = useState(false);
  const dialog = useRef<HTMLDivElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const item = items[index];
  const message = item.message;
  const url = usePicture(item, loadMedia);
  const label = itemLabel(item);
  const video = item.attachment ? plays(item.attachment) : false;
  const sender = message.fromMe ? "You" : (message.author ?? chat.name);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    closeButton.current?.focus();
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  useEffect(() => setZoomed(false), [currentId]);

  function go(step: number) {
    const next = items[index + step];
    if (next) setCurrentId(next.key);
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      if (zoomed) setZoomed(false);
      else onClose();
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      go(-1);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      go(1);
    } else if (event.key === "Tab" && dialog.current) {
      // Tab stays inside the viewer.
      const focusable = [...dialog.current.querySelectorAll<HTMLElement>("button:not(:disabled)")];
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
  }

  const arrowClass =
    "absolute top-1/2 z-10 flex h-11 w-11 -translate-y-1/2 items-center justify-center rounded-full bg-(--panel-bg) text-(--icon) disabled:opacity-0";
  const arrowShadow = { boxShadow: "0 1px 3px rgba(11,20,26,.3)" };

  // On the body: the panels use `contain`, which would clip a fixed overlay.
  return createPortal(
    <div
      ref={dialog}
      role="dialog"
      aria-modal="true"
      aria-label={`${label} from ${sender}`}
      onKeyDown={onKeyDown}
      className="animate-fade-rise fixed inset-0 z-50 flex flex-col"
      style={{ background: "color-mix(in oklab, var(--panel-bg) 96%, transparent)" }}
    >
      <header className="flex h-15 shrink-0 items-center gap-3 px-4">
        {message.fromMe || chat.isGroup ? (
          <Avatar name={sender} initials={initialsFor(sender)} hue={hueFor(sender)} size={40} />
        ) : (
          <Avatar name={chat.name} initials={chat.initials} hue={chat.avatarHue} src={chat.avatarUrl} size={40} />
        )}
        <div className="min-w-0 flex-1">
          <p className="truncate text-[16px] leading-5.25 text-(--text-primary)">{sender}</p>
          <p className="truncate text-[13px] leading-5 text-(--text-secondary)">
            {formatDayLabel(message.sentAt)} at {formatMessageTime(message.sentAt)}
          </p>
        </div>
        <button
          ref={closeButton}
          type="button"
          onClick={onClose}
          aria-label="Close"
          title="Close"
          className="flex h-10 w-10 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
        >
          <CloseIcon size={24} />
        </button>
      </header>

      <div className="relative min-h-0 flex-1">
        <button type="button" onClick={() => go(-1)} disabled={index === 0} aria-label="Previous photo" className={`${arrowClass} left-4`} style={arrowShadow}>
          <ChevronLeftIcon size={26} />
        </button>
        <button type="button" onClick={() => go(1)} disabled={index === items.length - 1} aria-label="Next photo" className={`${arrowClass} right-4`} style={arrowShadow}>
          <ChevronRightIcon size={26} />
        </button>

        <div
          className={["absolute inset-0 flex px-20 py-4", zoomed ? "overflow-auto" : "items-center justify-center"].join(" ")}
          onClick={(event) => event.target === event.currentTarget && onClose()}
        >
          {url && url !== "failed" && item.attachment?.kind === "gif" ? (
            <GifVideo
              key={url}
              src={url}
              // The copies share a grid cell, which has no height to take a share of.
              wrapperClassName="animate-fade-rise"
              className="max-h-[calc(100dvh-13rem)] max-w-[calc(100vw-10rem)] bg-black"
              style={{ boxShadow: "0 2px 12px rgba(11,20,26,.18)" }}
            />
          ) : url && url !== "failed" && video ? (
            <VideoPlayer
              key={url}
              src={url}
              onError={() => {}}
              className="animate-fade-rise"
              videoClassName="max-h-[calc(100dvh-13rem)] max-w-[calc(100vw-10rem)]"
              videoStyle={{ boxShadow: "0 2px 12px rgba(11,20,26,.18)" }}
            />
          ) : url && url !== "failed" ? (
            <img
              key={url}
              src={url}
              alt={label}
              draggable={false}
              onClick={() => setZoomed((z) => !z)}
              title={zoomed ? "Zoom out" : "Zoom in"}
              className={[
                "animate-fade-rise cursor-pointer",
                zoomed ? "m-auto max-w-none" : "max-h-full max-w-full object-contain",
              ].join(" ")}
              style={{ boxShadow: "0 2px 12px rgba(11,20,26,.18)" }}
            />
          ) : (
            <p className="rounded bg-black/40 px-3 py-1 text-[13px] text-white/90">
              {url === "failed" ? `${label} unavailable` : `Loading ${label.toLowerCase()}…`}
            </p>
          )}
        </div>
      </div>

      {message.text ? (
        <p className="mx-auto max-w-[min(80%,640px)] shrink-0 px-4 pt-1 pb-2 text-center text-[14.2px] whitespace-pre-wrap text-(--text-primary)">
          {message.text}
        </p>
      ) : null}

      {items.length > 1 ? (
        <div className="no-scrollbar flex h-21 shrink-0 items-center gap-1.5 overflow-x-auto border-t border-(--divider) px-4">
          <div className="mx-auto flex gap-1.5">
            {items.map((entry, at) => (
              <Thumb key={entry.key} item={entry} loadMedia={loadMedia} current={at === index} onPick={() => setCurrentId(entry.key)} />
            ))}
          </div>
        </div>
      ) : null}
    </div>,
    document.body,
  );
}
