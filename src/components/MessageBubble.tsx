import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import type { AccountId } from "../accounts/accounts";
import type { DeliveryStatus, LinkPreview, Message, ReplyQuote } from "../backend";
import { authorColor } from "../lib/avatar";
import { formatMessageTime, formatVoiceDuration } from "../lib/format";
import { URL_PATTERN, fetchLinkPreview, isPlayable, readyLinkPreview } from "../lib/links";
import { useSettings } from "../settings/settings";
import { Attachments } from "./Attachments";
import { Waveform } from "./AudioPlayer";
import type { LoadMedia } from "./Attachments";
import { LinkCard, openLink } from "./LinkCard";
import { canEdit } from "./EditDialog";
import { PollCard } from "./PollCard";
import { ContactCards, PlaceCard } from "./SharedCards";
import { canForward } from "./ForwardDialog";
import {
  BlockedIcon,
  ChevronDownIcon,
  CopyIcon,
  EditIcon,
  FileIcon,
  ForwardIcon,
  PersonIcon,
  PinIcon,
  MoreIcon,
  PlayIcon,
  ReplyIcon,
  SelectIcon,
  SmileIcon,
  StarIcon,
  Ticks,
  TrashIcon,
} from "./Icons";
import { PopupMenu } from "./PopupMenu";
import type { MenuAction, MenuEntry } from "./PopupMenu";
import { ReactionDetails } from "./ReactionDetails";
import { ReactionPicker, type PickerAnchor } from "./ReactionPicker";
import { Emoji, emojiOnlyCount, withEmoji } from "../lib/emoji";
import { Formatted, FormattedLine } from "../lib/markup";

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
  /** Words of a search to mark in the text, folded. */
  highlight?: string[];
  /** Asks how the message should be deleted. */
  onDelete?: (message: Message) => void;
  /** Scrolls to a quoted message. */
  onJumpTo?: (messageId: string) => void;
  /** Opens someone's contact info: the sender's name and mentions lead there. */
  onOpenPerson?: (id: string) => void;
  /** Briefly highlighted: the message was jumped to. */
  flash?: boolean;
  /** Pinned in the chat for everyone. */
  pinned?: boolean;
  /** Changes what one of our texts says, while it still can be. */
  onEdit?: (message: Message) => void;
  /** Votes on a poll for the options picked; none takes the vote back. */
  onVote?: (message: Message, options: number[]) => Promise<void>;
  /** Opens a chat with a number from a contact card, digits only. */
  onMessageContact?: (waId: string, name: string) => void;
  /** Pins the message for everyone, or unpins it when `pinned`. */
  onPin?: (message: Message) => void;
  /** Starts selecting messages, with this one picked. */
  onSelect?: (message: Message) => void;
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

function Meta({ message, account, pinned }: { message: Message; account: AccountId; pinned?: boolean }) {
  return (
    <span className="flex items-center gap-1 text-[11px] leading-3.75 whitespace-nowrap text-(--bubble-meta)">
      {pinned ? <PinIcon size={11} aria-label="Pinned" aria-hidden={false} role="img" /> : null}
      {message.edited ? <span>Edited</span> : null}
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

/** WhatsApp draws a message of one to three emoji and nothing else this big. */
const BIG_EMOJI = [0, 48, 40, 32];


function Quote({ quote, onJumpTo }: { quote: ReplyQuote; onJumpTo?: (messageId: string) => void }) {
  // Ours stands out with a tag; anyone else's is in the colour of their name.
  const color = quote.fromMe ? "var(--accent)" : authorColor(quote.author);
  const body = (
    <>
      {quote.fromMe ? (
        <span className="mb-0.5 inline-flex h-4.5 items-center gap-0.75 rounded-full bg-[color-mix(in_oklab,var(--accent)_22%,transparent)] px-1.75 text-[11.5px] leading-none font-semibold text-(--accent)">
          <PersonIcon size={11} />
          You
        </span>
      ) : (
        <p className="text-[12.8px] font-medium" style={{ color }}>
          {quote.author}
        </p>
      )}
      <p className="line-clamp-2 text-[13.2px] text-(--text-secondary)">
        <FormattedLine text={quote.text ?? ""} emojiSize={17} />
      </p>
    </>
  );
  const box = "mb-1 block w-full overflow-hidden rounded-[7.5px] border-l-4 bg-(--quote-bg) px-2 py-1.5 text-left";
  const id = quote.id;
  if (!id || !onJumpTo) return <div className={box} style={{ borderColor: color }}>{body}</div>;
  return (
    <button type="button" onClick={() => onJumpTo(id)} aria-label={`Go to the message from ${quote.author}`} style={{ borderColor: color }} className={`${box} transition-colors hover:bg-[color-mix(in_oklab,var(--quote-bg),var(--text-primary)_6%)]`}>
      {body}
    </button>
  );
}

/** The card read from a link's page, once it has come. */
function useFetchedPreview(url: string | undefined): LinkPreview | null {
  const [preview, setPreview] = useState(() => (url ? (readyLinkPreview(url) ?? null) : null));
  useEffect(() => {
    if (!url) return setPreview(null);
    const known = readyLinkPreview(url);
    if (known !== undefined) return setPreview(known);
    let alive = true;
    void fetchLinkPreview(url).then((found) => alive && setPreview(found));
    return () => {
      alive = false;
    };
  }, [url]);
  return preview;
}

/** How long a press on a bubble lasts before it opens the reactions, as on the phone. */
const LONG_PRESS_MS = 450;

/**
 * Memoized: a chat draws hundreds of these, and a change elsewhere in the app
 * should not redraw every one.
 */
/** WhatsApp's forwarding score from which a message counts as forwarded many times. */
const FORWARDED_MANY = 5;

export const MessageBubble = memo(function MessageBubble({ message, account, showAuthor, tail, loadMedia, openMedia, loadLinkThumbnail, onOpenPhoto, onReact, onStar, onReply, onForward, onDelete, onJumpTo, onOpenPerson, flash, highlight, pinned, onEdit, onPin, onSelect, onVote, onMessageContact }: MessageBubbleProps) {
  const { settings } = useSettings();
  const [picking, setPicking] = useState(false);
  // The reactions' details, opened from their pill.
  const [details, setDetails] = useState<PickerAnchor | null>(null);
  const closeDetails = useCallback(() => setDetails(null), []);
  /** The full emoji grid instead of the quick row. */
  const [allEmoji, setAllEmoji] = useState(false);
  /** Where the message's menu is open, in viewport pixels. */
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  /** What the picker opens next to. */
  const [anchor, setAnchor] = useState<PickerAnchor>({ x: 0, top: 0, bottom: 0 });
  const row = useRef<HTMLDivElement>(null);
  const bubble = useRef<HTMLDivElement>(null);
  const press = useRef<{ timer: number; x: number; y: number } | null>(null);
  const reactions = message.reactions ?? [];
  const myReaction = reactions.find((reaction) => reaction.mine)?.emoji;
  const mine = message.fromMe;
  // A poll's text is its question, shown in the poll.
  const hasText = Boolean(message.text) && !message.poll;
  /** Drawn as itself rather than as a file placeholder. */
  const shared = Boolean(message.poll || message.place || message.contacts?.length);
  const bigEmoji = useMemo(
    () => (message.kind === "text" && !message.attachments?.length ? emojiOnlyCount(message.text ?? "") : 0),
    [message.kind, message.attachments, message.text],
  );
  // A link that came without a card gets one made from the page itself.
  const bareLink = useMemo(
    () => (message.linkPreview || message.deleted ? undefined : message.text?.match(URL_PATTERN)?.[0]),
    [message.linkPreview, message.deleted, message.text],
  );
  const fetched = useFetchedPreview(settings.fetchLinkPreviews ? bareLink : undefined);
  // A video link gets a card to play it from even when there is none to show.
  const linkPreview = useMemo(() => {
    if (message.linkPreview || fetched) return message.linkPreview ?? fetched ?? undefined;
    const url = settings.inlineVideos ? message.text?.match(URL_PATTERN)?.find(isPlayable) : undefined;
    return url ? { url } : undefined;
  }, [message.linkPreview, fetched, message.text, settings.inlineVideos]);
  // Inline meta: an invisible spacer reserves room so the time sits on the
  // last text line when it fits, as in WhatsApp.
  // The spacer is an invisible copy of the meta, so it always matches its width.
  const metaSpacer = (
    <span className="invisible inline-block pl-1 align-top select-none" aria-hidden>
      <Meta message={message} account={account} pinned={pinned} />
    </span>
  );

  useEffect(() => {
    if (!picking) return;
    const outside = (event: Event) =>
      !row.current?.contains(event.target as Node) && !(event.target as Element).closest?.("[data-reaction-picker]");
    const onPointerDown = (event: PointerEvent) => {
      if (outside(event)) setPicking(false);
    };
    // It stays where it opened, so scrolling the chat away from it closes it.
    const onWheel = (event: WheelEvent) => {
      if (outside(event)) setPicking(false);
    };
    const close = () => setPicking(false);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setPicking(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("wheel", onWheel, true);
    window.addEventListener("resize", close);
    return () => {
      document.removeEventListener("wheel", onWheel, true);
      window.removeEventListener("resize", close);
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [picking]);

  useEffect(() => cancelPress, []);

  function openPicker(at: PickerAnchor, all = false) {
    setAnchor(at);
    setAllEmoji(all);
    setPicking(true);
  }

  /** Opens the picker next to the button or pill that was clicked. */
  function openPickerAt(el: Element) {
    const box = el.getBoundingClientRect();
    openPicker({ x: box.left + box.width / 2, top: box.top, bottom: box.bottom });
  }

  const deleted = Boolean(message.deleted);
  /** Only voice or audio: the time sits on the line of the player's clock. */
  const soundOnly =
    !hasText && !message.linkPreview && Boolean(message.attachments?.length) &&
    message.attachments!.every((attachment) => attachment.kind === "voice" || attachment.kind === "audio");
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
            onSelect: () => openPicker(menu ? { x: menu.x, top: menu.y, bottom: menu.y } : anchor, true),
          }
        : null,
      whatsapp ? null : copy,
      // Attachments are uploaded again; ones not on this computer yet are fetched first.
      onForward && canForward(message)
        ? { label: "Forward", icon: <ForwardIcon size={20} />, onSelect: () => onForward(message) }
        : null,
      !deleted && onPin ? { label: pinned ? "Unpin" : "Pin", icon: <PinIcon size={20} />, onSelect: () => onPin(message) } : null,
      !deleted && onStar
        ? {
            label: message.starred ? "Unstar" : "Star",
            icon: <StarIcon size={20} filled={message.starred} />,
            onSelect: () => onStar(message),
          }
        : null,
      onEdit && canEdit(message, account) ? { label: "Edit", icon: <EditIcon size={20} />, onSelect: () => onEdit(message) } : null,
      onSelect ? { label: "Select", icon: <SelectIcon size={20} />, onSelect: () => onSelect(message) } : null,
    ];
    const items: MenuEntry[] = actions.filter((item) => item !== null);
    if (onDelete) {
      if (items.length > 0) items.push("separator");
      items.push({ label: "Delete", icon: <TrashIcon size={20} />, onSelect: () => onDelete(message) });
    }
    return items;
  }

  function openMenu(x: number, y: number, at: PickerAnchor = { x, top: y, bottom: y }) {
    if (menuItems().length === 0) return;
    setMenu({ x, y });
    // WhatsApp offers the reactions above where the menu opened.
    if (react && whatsapp) openPicker(at);
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
        const at = press.current;
        press.current = null;
        if (at) openPicker({ x: at.x, top: at.y, bottom: at.y });
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
  // Each emoji with its own count, ours always among them, then the total.
  const ranked = [...reactions].sort((a, b) => b.count - a.count);
  const total = reactions.reduce((sum, reaction) => sum + reaction.count, 0);
  const shown = ranked.slice(0, 3);
  const mineRanked = ranked.find((reaction) => reaction.mine);
  if (mineRanked && !shown.includes(mineRanked)) shown[shown.length - 1] = mineRanked;

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
        data-bubble
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
            {onOpenPerson && message.authorId ? (
              <button type="button" onClick={() => onOpenPerson(message.authorId!)} className="text-left hover:underline">
                {message.author}
              </button>
            ) : (
              message.author
            )}
          </p>
        ) : null}

        {whatsapp && !deleted ? (
          // WhatsApp's menu button: a chevron that fades in over the corner of the bubble.
          <button
            type="button"
            onClick={(event) => {
              const box = event.currentTarget.getBoundingClientRect();
              openMenu(box.left, box.bottom + 4, { x: box.left + box.width / 2, top: box.top, bottom: box.bottom });
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

        {message.forwarded && !deleted ? (
          <p className="mb-0.5 flex items-center gap-1 px-0.5 text-[12.8px] text-(--bubble-meta) italic">
            <ForwardIcon size={14} className="shrink-0" />
            {message.forwarded >= FORWARDED_MANY ? "Forwarded many times" : "Forwarded"}
          </p>
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
          <Quote quote={message.replyTo} onJumpTo={onJumpTo} />
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

        {message.poll && !deleted ? (
          <PollCard
            question={message.text ?? ""}
            poll={message.poll}
            onVote={onVote ? (options) => onVote(message, options) : undefined}
          />
        ) : null}

        {message.place && !deleted ? <PlaceCard place={message.place} /> : null}

        {message.contacts?.length && !deleted ? <ContactCards cards={message.contacts} onMessage={onMessageContact} /> : null}

        {message.kind === "media" && !message.attachments?.length && !shared ? (
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
            <Waveform levels={message.waveform} />
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
          bigEmoji ? (
            <p className="flex gap-0.5 px-0.5 pt-0.5">
              {withEmoji(message.text ?? "", BIG_EMOJI[bigEmoji])}
            </p>
          ) : (
            <p className="px-0.5 wrap-break-word whitespace-pre-wrap">
              <Formatted text={message.text ?? ""} onLink={openLink} highlight={highlight} mentions={message.mentions} onMention={onOpenPerson} />
              {metaSpacer}
            </p>
          )
        ) : null}

        {/* Under a big emoji the time takes its own line, so the bubble is wide enough for it. */}
        <span className={(hasText && !bigEmoji) || deleted || soundOnly ? "absolute right-1.75 bottom-1.25" : "flex justify-end"}>
          <Meta message={message} account={account} pinned={pinned} />
        </span>

        {reactions.length > 0 ? (
          <div className={["absolute -bottom-4.5 flex", mine ? "right-2" : "left-2"].join(" ")}>
            <button
              type="button"
              data-reaction-pills
              onClick={(event) => {
                if (details) return setDetails(null);
                const box = event.currentTarget.getBoundingClientRect();
                setDetails({ x: box.left + box.width / 2, top: box.top, bottom: box.bottom });
              }}
              aria-label={`${total} ${total === 1 ? "reaction" : "reactions"}: ${ranked.map((r) => `${r.emoji} ${r.count}`).join(", ")}${myReaction ? `, yours ${myReaction}` : ""}`}
              aria-expanded={Boolean(details)}
              className={`${pillClass} ${plainPill} gap-1.5`}
              style={{ boxShadow: "var(--bubble-shadow)" }}
            >
              {shown.map((reaction) => (
                <span
                  key={reaction.emoji}
                  className={[
                    "flex items-center gap-0.5 tabular-nums",
                    // Ours, tinted, so it is never lost among the others.
                    reaction.mine ? "-mx-1 rounded-full bg-[color-mix(in_oklab,var(--accent)_28%,transparent)] px-1 py-px text-(--text-primary)" : "",
                  ].join(" ")}
                >
                  <Emoji emoji={reaction.emoji} size={15} />
                  {reaction.count > 1 ? reaction.count : null}
                </span>
              ))}
              {ranked.length > 1 ? (
                <span className="border-l border-(--divider) pl-1.5 font-semibold tabular-nums">{total}</span>
              ) : null}
            </button>
          </div>
        ) : null}

        {details ? (
          <ReactionDetails
            reactions={ranked}
            anchor={details}
            onRemove={
              react
                ? (emoji) => {
                    setDetails(null);
                    react(message, emoji);
                  }
                : undefined
            }
            onClose={closeDetails}
          />
        ) : null}

        {picking && react ? (
          <ReactionPicker
            key={allEmoji ? "all" : "quick"}
            account={account}
            current={myReaction}
            anchor={anchor}
            expanded={allEmoji}
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
          onClick={(event) => (picking ? setPicking(false) : openPickerAt(event.currentTarget))}
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
});
