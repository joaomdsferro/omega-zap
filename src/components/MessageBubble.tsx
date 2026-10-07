import { useEffect, useMemo, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import type { AccountId } from "../accounts/accounts";
import type { DeliveryStatus, Message } from "../backend";
import { authorColor } from "../lib/avatar";
import { formatMessageTime, formatVoiceDuration } from "../lib/format";
import { URL_PATTERN, isPlayable } from "../lib/links";
import { useSettings } from "../settings/settings";
import { Attachments } from "./Attachments";
import type { LoadMedia } from "./Attachments";
import { LinkCard, openLink } from "./LinkCard";
import {
  BlockedIcon,
  ChevronDownIcon,
  CopyIcon,
  FileIcon,
  ForwardIcon,
  MoreIcon,
  PlayIcon,
  ReplyIcon,
  SmileIcon,
  StarIcon,
  Ticks,
  TrashIcon,
} from "./Icons";
import { PopupMenu } from "./PopupMenu";
import type { MenuAction, MenuEntry } from "./PopupMenu";
import { ReactionPicker } from "./ReactionPicker";

interface MessageBubbleProps {
  message: Message;
  /** The network decides how reactions look and which ones are offered. */
  account: AccountId;
  showAuthor: boolean;
  /** First bubble of a run from the same sender: gets the tail. */
  tail: boolean;
  loadMedia?: LoadMedia;
  /** Opens a document in the system's app for it. */
  openMedia?: (message: Message, index: number) => Promise<void>;
  loadLinkThumbnail?: (message: Message) => Promise<string>;
  /** Opens a photo or video full-window; `index` picks the attachment. */
  onOpenPhoto?: (message: Message, index: number) => void;
  onReact?: (message: Message, emoji: string) => void;
  /** Stars the message, or takes the star away. */
  onStar?: (message: Message) => void;
  /** Quotes the message in the next one sent. */
  onReply?: (message: Message) => void;
  /** Sends the message on to other chats. */
  onForward?: (message: Message) => void;
  /** Asks how the message should be deleted. */
  onDelete?: (message: Message) => void;
  /** Briefly highlighted: the message was jumped to. */
  flash?: boolean;
}

function Tail({ mine }: { mine: boolean }) {
  return (
    <svg
      viewBox="0 0 8 13"
      width="8"
      height="13"
      aria-hidden
      className={["absolute top-0", mine ? "-right-2" : "-left-2 -scale-x-100"].join(" ")}
      style={{ color: mine ? "var(--bubble-out)" : "var(--bubble-in)" }}
    >
      <path fill="currentColor" d="M5.2 1H0v11.2L6.9 3.3C8 1.9 7.2 1 5.2 1z" />
    </svg>
  );
}

const STATUS_LABEL: Record<DeliveryStatus, string> = {
  sent: "Sent",
  delivered: "Delivered",
  read: "Read",
};

function Meta({ message, account }: { message: Message; account: AccountId }) {
  return (
    <span className="flex items-center gap-1 text-[11px] leading-3.75 whitespace-nowrap text-(--bubble-meta)">
      {message.starred ? <StarIcon size={11} filled aria-label="Starred" aria-hidden={false} role="img" /> : null}
      <time dateTime={message.sentAt}>{formatMessageTime(message.sentAt)}</time>
      {message.fromMe && message.status ? (
        <>
          <span className="text-[10px] opacity-80" aria-hidden>
            {STATUS_LABEL[message.status]}
          </span>
          <Ticks status={message.status} account={account} />
        </>
      ) : null}
    </span>
  );
}

function Waveform() {
  const bars = [4, 10, 7, 14, 9, 16, 8, 12, 6, 15, 9, 11, 5, 13, 7, 10, 6, 12, 8, 5];
  return (
    <div className="flex h-7 flex-1 items-center gap-0.5" aria-hidden>
      {bars.map((h, i) => (
        <span
          key={i}
          className="w-0.75 rounded-full bg-(--text-secondary) opacity-60"
          style={{ height: `${h}px` }}
        />
      ))}
    </div>
  );
}

/** The largest a photo is drawn, in pixels. */
const PHOTO_MAX_WIDTH = 330;
const PHOTO_MAX_HEIGHT = 340;

function Photo({ message, loadMedia, onOpen }: Pick<MessageBubbleProps, "message" | "loadMedia"> & { onOpen?: () => void }) {
  const label = message.imageLabel ?? "Photo";
  const wanted = Boolean(message.hasMedia && loadMedia);
  const box = useRef<HTMLDivElement>(null);
  const [near, setNear] = useState(false);
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  // A long chat holds hundreds of photos: each is fetched as it comes close to the screen.
  useEffect(() => {
    const el = box.current;
    if (!el || near) return;
    const observer = new IntersectionObserver(
      (entries) => entries.some((entry) => entry.isIntersecting) && setNear(true),
      { rootMargin: "600px 0px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [near]);

  useEffect(() => {
    if (!near || !message.hasMedia || !loadMedia) return;
    let alive = true;
    setFailed(false);
    loadMedia(message)
      .then((loaded) => alive && setUrl(loaded))
      .catch(() => alive && setFailed(true));
    return () => {
      alive = false;
    };
  }, [near, message.id, message.hasMedia, loadMedia]);

  // With the size known up front the picture takes its place before it loads,
  // so the messages around it stay put.
  const { mediaWidth: w, mediaHeight: h } = message;
  const sized =
    w && h
      ? { width: Math.round(w * Math.min(PHOTO_MAX_WIDTH / w, PHOTO_MAX_HEIGHT / h, 1)), aspectRatio: `${w} / ${h}` }
      : undefined;

  if (url && !failed) {
    const picture = (
      <img
        src={url}
        alt={label}
        draggable={false}
        onError={() => setFailed(true)}
        style={sized}
        className={[
          "block max-w-full rounded-md",
          onOpen ? "" : "mb-1",
          sized ? "object-cover" : "max-h-85 object-contain",
        ].join(" ")}
      />
    );
    return onOpen ? (
      <button type="button" onClick={onOpen} aria-label={`Open ${label.toLowerCase()}`} className="mb-1 block max-w-full">
        {picture}
      </button>
    ) : (
      picture
    );
  }
  return (
    <div
      ref={box}
      style={sized}
      className={[
        "mb-1 flex max-w-full items-end overflow-hidden rounded-md bg-[linear-gradient(145deg,color-mix(in_oklab,var(--accent)_35%,#0b141a),#1f3a33)] p-2",
        sized ? "" : "h-55 w-70",
      ].join(" ")}
      role="img"
      aria-label={label}
    >
      <span className="rounded bg-black/40 px-2 py-0.5 text-[12px] text-white/90">
        {failed ? `${label} unavailable` : wanted ? `Loading ${label.toLowerCase()}…` : label}
      </span>
    </div>
  );
}

function Linkified({ text }: { text: string }) {
  const parts = [];
  let end = 0;
  for (const match of text.matchAll(URL_PATTERN)) {
    const url = match[0];
    parts.push(text.slice(end, match.index));
    parts.push(
      <a
        key={match.index}
        href={url}
        target="_blank"
        rel="noreferrer"
        onClick={(event) => openLink(event, url)}
        className="break-all text-(--accent) underline"
      >
        {url}
      </a>,
    );
    end = match.index + url.length;
  }
  parts.push(text.slice(end));
  return <>{parts}</>;
}

/** How long a press on a bubble lasts before it opens the reactions, as on the phone. */
const LONG_PRESS_MS = 450;

export function MessageBubble({ message, account, showAuthor, tail, loadMedia, openMedia, loadLinkThumbnail, onOpenPhoto, onReact, onStar, onReply, onForward, onDelete, flash }: MessageBubbleProps) {
  const { settings } = useSettings();
  const [picking, setPicking] = useState(false);
  /** The full emoji grid instead of the quick row. */
  const [allEmoji, setAllEmoji] = useState(false);
  /** Where the message's menu is open, in viewport pixels. */
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [below, setBelow] = useState(false);
  const row = useRef<HTMLDivElement>(null);
  const bubble = useRef<HTMLDivElement>(null);
  const press = useRef<{ timer: number; x: number; y: number } | null>(null);
  const reactions = message.reactions ?? [];
  const myReaction = reactions.find((reaction) => reaction.mine)?.emoji;
  const mine = message.fromMe;
  const hasText = Boolean(message.text);
  // A video link gets a card to play it from even when the sender's phone made none.
  const linkPreview = useMemo(() => {
    if (message.linkPreview || !settings.inlineVideos) return message.linkPreview;
    const url = message.text?.match(URL_PATTERN)?.find(isPlayable);
    return url ? { url } : undefined;
  }, [message.linkPreview, message.text, settings.inlineVideos]);
  // Inline meta: an invisible spacer reserves room so the time sits on the
  // last text line when it fits, as in WhatsApp.
  // The spacer is an invisible copy of the meta, so it always matches its width.
  const metaSpacer = (
    <span className="invisible inline-block pl-1 align-top select-none" aria-hidden>
      <Meta message={message} account={account} />
    </span>
  );

  useEffect(() => {
    if (!picking) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!row.current?.contains(event.target as Node)) setPicking(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setPicking(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [picking]);

  useEffect(() => cancelPress, []);

  // The picker opens above the bubble, unless the top of the chat would cut it off.
  function place(height: number) {
    const el = bubble.current;
    if (!el) return;
    const top = el.closest(".chat-wallpaper")?.getBoundingClientRect().top ?? 0;
    setBelow(el.getBoundingClientRect().top - top < height);
  }

  function openPicker() {
    place(72);
    setAllEmoji(false);
    setPicking(true);
  }

  const deleted = Boolean(message.deleted);
  const whatsapp = account === "whatsapp";
  const react = deleted ? undefined : onReact;

  // The same actions, in the order each network's own app lists them.
  function menuItems(): MenuEntry[] {
    const text = message.text;
    const copy: MenuAction | null = text
      ? { label: whatsapp ? "Copy" : "Copy text", icon: <CopyIcon size={20} />, onSelect: () => void navigator.clipboard.writeText(text) }
      : null;
    const actions: (MenuAction | null)[] = [
      !deleted && onReply ? { label: "Reply", icon: <ReplyIcon size={20} />, onSelect: () => onReply(message) } : null,
      whatsapp ? copy : null,
      react
        ? {
            label: "React",
            icon: <SmileIcon size={20} />,
            onSelect: () => {
              place(356);
              setAllEmoji(true);
              setPicking(true);
            },
          }
        : null,
      whatsapp ? null : copy,
      // Only text can be sent on: attachments would have to be uploaded again.
      onForward && text && message.kind === "text" && !message.attachments?.length
        ? { label: "Forward", icon: <ForwardIcon size={20} />, onSelect: () => onForward(message) }
        : null,
      !deleted && onStar
        ? {
            label: message.starred ? "Unstar" : "Star",
            icon: <StarIcon size={20} filled={message.starred} />,
            onSelect: () => onStar(message),
          }
        : null,
    ];
    const items: MenuEntry[] = actions.filter((item) => item !== null);
    if (onDelete) {
      if (items.length > 0) items.push("separator");
      items.push({ label: "Delete", icon: <TrashIcon size={20} />, onSelect: () => onDelete(message) });
    }
    return items;
  }

  function openMenu(x: number, y: number) {
    if (menuItems().length === 0) return;
    setMenu({ x, y });
    // WhatsApp offers the reactions over the bubble along with the menu.
    if (react && whatsapp) openPicker();
  }

  function cancelPress() {
    if (press.current) window.clearTimeout(press.current.timer);
    press.current = null;
  }

  function startPress(event: ReactPointerEvent) {
    if (!react || event.button !== 0) return;
    if ((event.target as Element).closest("button, a, [role='menu'], [role='dialog']")) return;
    cancelPress();
    press.current = {
      timer: window.setTimeout(() => {
        press.current = null;
        openPicker();
      }, LONG_PRESS_MS),
      x: event.clientX,
      y: event.clientY,
    };
  }

  // Dragging is selecting text or scrolling, not pressing.
  function movePress(event: ReactPointerEvent) {
    const start = press.current;
    if (start && Math.hypot(event.clientX - start.x, event.clientY - start.y) > 8) cancelPress();
  }

  const pillClass =
    "flex h-[22px] items-center gap-1 rounded-full border px-1.5 text-[12px] leading-none text-(--text-secondary)";
  const plainPill = "border-(--divider) bg-(--panel-bg)";
  // Signal shows the three most picked and folds the rest into a "+N".
  const ranked = [...reactions].sort((a, b) => b.count - a.count);
  const total = reactions.reduce((sum, reaction) => sum + reaction.count, 0);

  return (
    <div
      ref={row}
      data-message-id={message.id}
      className={[
        "group/message flex items-center gap-1 px-[5%]",
        mine ? "flex-row-reverse" : "flex-row",
        tail ? "mt-3" : "mt-0.5",
        reactions.length > 0 ? "mb-5" : "",
        flash ? "message-flash" : "",
      ].join(" ")}
    >
      <div
        ref={bubble}
        onPointerDown={startPress}
        onPointerMove={movePress}
        onPointerUp={cancelPress}
        onPointerLeave={cancelPress}
        onPointerCancel={cancelPress}
        onContextMenu={(event) => {
          event.preventDefault();
          cancelPress();
          openMenu(event.clientX, event.clientY);
        }}
        className={[
          "relative max-w-[min(65%,560px)] min-w-22 rounded-[7.5px] px-1.75 pt-1.5 pb-2 text-[14.2px] leading-4.75 text-(--text-primary)",
          tail ? (mine ? "rounded-tr-none" : "rounded-tl-none") : "",
        ].join(" ")}
        style={{
          background: mine ? "var(--bubble-out)" : "var(--bubble-in)",
          boxShadow: "var(--bubble-shadow)",
        }}
      >
        {tail ? <Tail mine={mine} /> : null}

        {showAuthor && message.author ? (
          <p className="mb-0.5 px-0.5 text-[12.8px] font-medium" style={{ color: authorColor(message.author) }}>
            {message.author}
          </p>
        ) : null}

        {whatsapp && !deleted ? (
          // WhatsApp's menu button: a chevron that fades in over the corner of the bubble.
          <button
            type="button"
            onClick={(event) => {
              const box = event.currentTarget.getBoundingClientRect();
              openMenu(box.left, box.bottom + 4);
            }}
            aria-label="Message options"
            aria-haspopup="menu"
            aria-expanded={menu !== null}
            className={[
              "absolute top-0.5 right-0.5 z-[1] flex h-6 w-9 items-start justify-end rounded-tr-[7.5px] pt-0.5 pr-1 text-(--bubble-meta) transition-opacity group-hover/message:opacity-100 focus-visible:opacity-100",
              menu ? "opacity-100" : "opacity-0",
            ].join(" ")}
            style={{
              background: `radial-gradient(at top right, ${mine ? "var(--bubble-out)" : "var(--bubble-in)"} 60%, transparent 80%)`,
            }}
          >
            <ChevronDownIcon size={18} strokeWidth={2.2} />
          </button>
        ) : null}

        {deleted ? (
          <p className="flex items-center gap-1.5 px-0.5 text-(--text-secondary) italic">
            <BlockedIcon size={16} className="shrink-0" />
            <span>
              {mine ? "You deleted this message" : "This message was deleted"}
              {metaSpacer}
            </span>
          </p>
        ) : null}

        {message.replyTo ? (
          <div className="mb-1 overflow-hidden rounded-[7.5px] border-l-4 border-(--accent) bg-(--quote-bg) px-2 py-1.5">
            <p className="text-[12.8px] font-medium text-(--accent)">{message.replyTo.author}</p>
            <p className="line-clamp-2 text-[13.2px] text-(--text-secondary)">{message.replyTo.text}</p>
          </div>
        ) : null}

        {message.attachments?.length ? (
          <Attachments
            message={message}
            attachments={message.attachments}
            loadMedia={loadMedia}
            openMedia={openMedia}
            onOpen={onOpenPhoto}
          />
        ) : null}

        {message.kind === "image" && !message.attachments?.length ? (
          <Photo message={message} loadMedia={loadMedia} onOpen={onOpenPhoto && (() => onOpenPhoto(message, 0))} />
        ) : null}

        {message.kind === "media" && !message.attachments?.length ? (
          <div className="mb-1 flex items-center gap-3 rounded-md bg-(--quote-bg) px-3 py-2.5">
            <FileIcon size={26} className="shrink-0 text-(--icon)" />
            <span className="min-w-0 truncate text-[14px]">{message.mediaLabel ?? "Attachment"}</span>
          </div>
        ) : null}

        {message.kind === "voice" && !message.attachments?.length ? (
          <div className="flex w-65 max-w-full items-center gap-3 py-1 pr-1">
            <button
              type="button"
              disabled
              title="Voice playback is not available yet"
              className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-(--icon) disabled:cursor-default"
              aria-label="Play voice message"
            >
              <PlayIcon size={22} />
            </button>
            <Waveform />
            <span className="text-[11px] tabular-nums text-(--bubble-meta)">
              {formatVoiceDuration(message.voiceSeconds ?? 0)}
            </span>
          </div>
        ) : null}

        {linkPreview ? (
          <LinkCard
            key={linkPreview.url}
            preview={linkPreview}
            loadLarge={loadLinkThumbnail ? () => loadLinkThumbnail(message) : undefined}
          />
        ) : null}

        {hasText && !deleted ? (
          <p className="px-0.5 wrap-break-word whitespace-pre-wrap">
            <Linkified text={message.text ?? ""} />
            {metaSpacer}
          </p>
        ) : null}

        <span className={hasText || deleted ? "absolute right-1.75 bottom-1.25" : "flex justify-end"}>
          <Meta message={message} account={account} />
        </span>

        {reactions.length > 0 ? (
          <div className={["absolute -bottom-4.5 flex gap-1", mine ? "right-2" : "left-2"].join(" ")}>
            {account === "whatsapp" ? (
              // WhatsApp gathers every reaction into one pill; tapping it opens the picker.
              <button
                type="button"
                disabled={!onReact}
                onClick={() => (picking ? setPicking(false) : openPicker())}
                aria-label={`${total} ${total === 1 ? "reaction" : "reactions"}: ${ranked.map((r) => r.emoji).join(" ")}`}
                className={`${pillClass} ${plainPill} disabled:cursor-default`}
                style={{ boxShadow: "var(--bubble-shadow)" }}
              >
                <span className="text-[13px]">
                  {ranked
                    .slice(0, 3)
                    .map((reaction) => reaction.emoji)
                    .join("")}
                </span>
                {total > 1 ? <span className="tabular-nums">{total}</span> : null}
              </button>
            ) : (
              <>
                {ranked.slice(0, 3).map((reaction) => (
                  <button
                    key={reaction.emoji}
                    type="button"
                    disabled={!onReact}
                    onClick={() => onReact?.(message, reaction.emoji)}
                    aria-label={`${reaction.emoji} ${reaction.count}${reaction.mine ? ", including you" : ""}`}
                    aria-pressed={reaction.mine}
                    className={[
                      pillClass,
                      "disabled:cursor-default",
                      reaction.mine
                        ? "border-(--accent) bg-[color-mix(in_oklab,var(--accent)_18%,var(--panel-bg))]"
                        : plainPill,
                    ].join(" ")}
                    style={{ boxShadow: "var(--bubble-shadow)" }}
                  >
                    <span className="text-[13px]">{reaction.emoji}</span>
                    {reaction.count > 1 ? <span className="tabular-nums">{reaction.count}</span> : null}
                  </button>
                ))}
                {ranked.length > 3 ? (
                  <span className={`${pillClass} ${plainPill}`} style={{ boxShadow: "var(--bubble-shadow)" }}>
                    +{ranked.slice(3).reduce((sum, reaction) => sum + reaction.count, 0)}
                  </span>
                ) : null}
              </>
            )}
          </div>
        ) : null}

        {picking && react ? (
          <ReactionPicker
            key={allEmoji ? "all" : "quick"}
            account={account}
            current={myReaction}
            align={mine ? "right" : "left"}
            below={below}
            expanded={allEmoji}
            onExpand={() => place(356)}
            onPick={(emoji) => {
              setPicking(false);
              setMenu(null);
              react(message, emoji);
            }}
          />
        ) : null}
      </div>

      {react ? (
        <button
          type="button"
          onClick={() => (picking ? setPicking(false) : openPicker())}
          aria-label="React"
          aria-expanded={picking}
          className={[
            "flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[rgba(11,20,26,.35)] text-white transition-opacity group-hover/message:opacity-100 focus-visible:opacity-100",
            picking ? "opacity-100" : "opacity-0",
          ].join(" ")}
        >
          <SmileIcon size={18} />
        </button>
      ) : null}

      {!whatsapp && !deleted ? (
        // Signal's menu button sits beside the bubble, after the reaction button.
        <button
          type="button"
          onClick={(event) => {
            const box = event.currentTarget.getBoundingClientRect();
            openMenu(box.left, box.bottom + 4);
          }}
          aria-label="More actions"
          aria-haspopup="menu"
          aria-expanded={menu !== null}
          className={[
            "flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[rgba(11,20,26,.35)] text-white transition-opacity group-hover/message:opacity-100 focus-visible:opacity-100",
            picking || menu ? "opacity-100" : "opacity-0",
          ].join(" ")}
        >
          <MoreIcon size={18} />
        </button>
      ) : null}

      {menu ? (
        <PopupMenu x={menu.x} y={menu.y} label="Message options" items={menuItems()} onClose={() => setMenu(null)} />
      ) : null}

      {onStar && !deleted ? (
        <button
          type="button"
          onClick={() => onStar(message)}
          aria-label={message.starred ? "Unstar" : "Star"}
          aria-pressed={Boolean(message.starred)}
          title={message.starred ? "Unstar" : "Star"}
          className={[
            "flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[rgba(11,20,26,.35)] text-white transition-opacity group-hover/message:opacity-100 focus-visible:opacity-100",
            picking ? "opacity-100" : "opacity-0",
          ].join(" ")}
        >
          <StarIcon size={16} filled={message.starred} />
        </button>
      ) : null}
    </div>
  );
}
