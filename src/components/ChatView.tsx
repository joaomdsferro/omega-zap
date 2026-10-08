import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Chat, Message } from "../backend";
import { listenForFileDrops } from "../lib/dropped";
import { formatDayLabel, sameDay } from "../lib/format";
import { AccountBadge } from "./AccountBadge";
import { Avatar } from "./Avatar";
import { Composer } from "./Composer";
import { isVisual } from "./Attachments";
import type { LoadMedia } from "./Attachments";
import { MediaViewer } from "./MediaViewer";
import type { ViewerItem } from "./MediaViewer";
import { DeleteDialog } from "./DeleteDialog";
import { MessageBubble } from "./MessageBubble";

/** Messages drawn when a chat opens; more are drawn, this many at a time, on scrolling up. */
const PAGE = 60;
/** How close to the top, in pixels, scrolling has to come for the next page to be drawn. */
const PAGE_MARGIN = 600;

interface ChatViewProps {
  chat: Chat;
  messages: Message[];
  subtitle: string;
  /** Show which network the chat is on: the list mixes accounts. */
  showAccount?: boolean;
  canSend: boolean;
  cannotSendReason?: string;
  /** Sends a text, or files with the text as their caption, quoting the message `replyTo` when given. */
  onSend: (text: string, files: File[], replyTo?: string, onFileSent?: () => void) => Promise<void>;
  /** Files can be sent in this chat. */
  canAttach?: boolean;
  /** Reports something that went wrong, such as a dropped file that can't be read. */
  onError?: (message: string) => void;
  /** Where to load a message's attachment from, when the account can fetch it. */
  loadMedia?: LoadMedia;
  /** Opens a document in the system's app for it. */
  openMedia?: (message: Message, index: number) => Promise<void>;
  /** Where to load the full-size picture of a message's link card from. */
  loadLinkThumbnail?: (message: Message) => Promise<string>;
  /** Reacts to a message, when the account supports it. */
  onReact?: (message: Message, emoji: string) => void;
  onStar?: (message: Message) => void;
  /** Sends a message on to other chats. */
  onForward?: (message: Message) => void;
  /** Deletes a message for us, or for everyone. */
  onDelete?: (message: Message, forEveryone: boolean) => Promise<void>;
  /** A message to scroll to and highlight once it has loaded. */
  focusMessageId?: string | null;
  onFocused?: () => void;
}

export function ChatView({ chat, messages, subtitle, showAccount, canSend, cannotSendReason, onSend, canAttach, onError, loadMedia, openMedia, loadLinkThumbnail, onReact, onStar, onForward, onDelete, focusMessageId, onFocused }: ChatViewProps) {
  const scroller = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);
  const lastChat = useRef<string | null>(null);
  const chatKey = `${chat.account}:${chat.id}`;

  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const switched = lastChat.current !== chatKey;
    lastChat.current = chatKey;
    if (switched || stickToBottom.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [messages, chatKey]);

  // Pictures arrive after the messages and change their height.
  useEffect(() => {
    const el = scroller.current;
    if (!el || !content.current) return;
    const observer = new ResizeObserver(() => {
      if (stickToBottom.current) el.scrollTop = el.scrollHeight;
    });
    observer.observe(content.current);
    return () => observer.disconnect();
  }, [chatKey]);

  // How many of the newest messages are drawn, for this chat: a chat opens
  // with one page, so opening it never waits on hundreds of bubbles.
  const [drawn, setDrawn] = useState({ chatKey, count: PAGE });
  const count = drawn.chatKey === chatKey ? drawn.count : PAGE;
  const visible = useMemo(() => messages.slice(-count), [messages, count]);
  const hidden = messages.length - visible.length;
  /** The scroll height before a page was added above, to keep the view still. */
  const grownFrom = useRef<number | null>(null);

  function drawMore(total: number) {
    const el = scroller.current;
    if (el) grownFrom.current = el.scrollHeight - el.scrollTop;
    setDrawn({ chatKey, count: total });
  }

  // A page added above would push everything down; this keeps the messages
  // on screen where they were.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el || grownFrom.current === null) return;
    el.scrollTop = el.scrollHeight - grownFrom.current;
    grownFrom.current = null;
  }, [count]);

  const [flashed, setFlashed] = useState<string | null>(null);
  const [viewing, setViewing] = useState<string | null>(null);
  const viewable = useMemo(
    () =>
      messages.flatMap((message): ViewerItem[] =>
        message.attachments?.length
          ? message.attachments.flatMap((attachment, index) =>
              isVisual(attachment) ? [{ key: `${message.id}#${index}`, message, index, attachment }] : [],
            )
          : message.kind === "image"
            ? [{ key: `${message.id}#0`, message, index: 0 }]
            : [],
      ),
    [messages],
  );

  const [replyingTo, setReplyingTo] = useState<Message | null>(null);
  const [deleting, setDeleting] = useState<Message | null>(null);
  /** Files picked, dropped or pasted, waiting for Send. */
  const [pending, setPending] = useState<File[]>([]);

  useEffect(() => {
    setViewing(null);
    setReplyingTo(null);
    setDeleting(null);
    setPending([]);
  }, [chatKey]);

  // A quoted message that is deleted can't be replied to any more.
  const replyTarget = replyingTo && messages.find((m) => m.id === replyingTo.id && !m.deleted);

  async function send(text: string, files: File[]) {
    // Each file leaves the tray as it is sent, so a failure leaves only the
    // ones still to send.
    await onSend(text, files, replyTarget?.id, () => setPending((prev) => prev.slice(1)));
    setReplyingTo(null);
    setPending([]);
  }

  const attaching = Boolean(canAttach && canSend);
  const [dragging, setDragging] = useState(false);

  function addFiles(files: File[]) {
    if (files.length) setPending((prev) => [...prev, ...files]);
  }

  const section = useRef<HTMLElement>(null);
  const reportError = useRef(onError);
  reportError.current = onError;

  // Files dragged in from the desktop are taken when they land on the conversation.
  useEffect(() => {
    if (!attaching) return;
    const inside = (x: number, y: number) => {
      const box = section.current?.getBoundingClientRect();
      return Boolean(box && x >= box.left && x <= box.right && y >= box.top && y <= box.bottom);
    };
    return listenForFileDrops({
      over: (x, y) => setDragging(inside(x, y)),
      leave: () => setDragging(false),
      drop: (x, y, read) => {
        setDragging(false);
        if (!inside(x, y)) return;
        void read().then(({ files, errors }) => {
          addFiles(files);
          if (errors.length) reportError.current?.(errors.join(" "));
        });
      },
    });
  }, [attaching]);

  // Jumping to a message: it goes to the middle of the view and lights up.
  useLayoutEffect(() => {
    if (!focusMessageId) return;
    // An older message is drawn first, with a little of what came before it.
    const at = messages.findIndex((m) => m.id === focusMessageId);
    if (at !== -1 && at < hidden) {
      setDrawn({ chatKey, count: messages.length - at + 10 });
      return;
    }
    const el = content.current?.querySelector(`[data-message-id="${CSS.escape(focusMessageId)}"]`);
    if (!el) return;
    el.scrollIntoView({ block: "center" });
    stickToBottom.current = false;
    setFlashed(focusMessageId);
    onFocused?.();
  }, [messages, hidden, chatKey, focusMessageId, onFocused]);

  useEffect(() => {
    if (!flashed) return;
    const id = window.setTimeout(() => setFlashed(null), 1700);
    return () => window.clearTimeout(id);
  }, [flashed]);

  function onScroll() {
    const el = scroller.current;
    if (!el) return;
    stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
    if (hidden > 0 && el.scrollTop < PAGE_MARGIN && grownFrom.current === null) drawMore(count + PAGE);
  }

  // Messages that don't fill the view leave nothing to scroll, so the
  // older ones are drawn until they do.
  useEffect(() => {
    const el = scroller.current;
    if (el && hidden > 0 && el.scrollTop < PAGE_MARGIN && grownFrom.current === null) drawMore(count + PAGE);
  }, [visible]);

  const openPhoto = useCallback((photo: Message, index: number) => setViewing(`${photo.id}#${index}`), []);

  return (
    <section
      ref={section}
      className="relative flex h-full min-w-0 flex-1 flex-col"
      aria-label={`Conversation with ${chat.name}`}
    >
      {dragging ? (
        <div className="pointer-events-none absolute inset-0 z-30 flex items-center justify-center bg-(--panel-header-bg)/85 text-[17px] text-(--text-primary)">
          Drop to send to {chat.name}
        </div>
      ) : null}
      <header className="flex h-15 shrink-0 items-center gap-3 border-l border-(--divider) bg-(--panel-header-bg) px-4">
        <Avatar
          name={chat.name}
          initials={chat.initials}
          hue={chat.avatarHue}
          src={chat.avatarUrl}
          size={40}
          group={chat.isGroup}
        />
        <div className="min-w-0">
          <h2 className="truncate text-[16px] leading-5.25 text-(--text-primary)">{chat.name}</h2>
          <p className="truncate text-[13px] leading-5 text-(--text-secondary)">{subtitle}</p>
        </div>
        {showAccount ? (
          <span className="ml-auto">
            <AccountBadge account={chat.account} size={26} />
          </span>
        ) : null}
      </header>

      <div
        ref={scroller}
        onScroll={onScroll}
        className="chat-wallpaper min-h-0 flex-1 overflow-y-auto pt-2 pb-4"
      >
        <div key={chatKey} ref={content} className="animate-fade-rise">
          {groupByDay(visible).map((day) => (
            // Each day is its own block so its sticky chip scrolls away when
            // the next day's chip arrives, instead of stacking under it.
            <section key={day[0].id}>
              <div className="sticky top-2 z-10 my-3 flex justify-center">
                <span
                  className="rounded-[7.5px] bg-(--day-chip-bg) px-3 py-1.25 text-[12.5px] text-(--day-chip-text) uppercase"
                  style={{ boxShadow: "var(--bubble-shadow)" }}
                >
                  {formatDayLabel(day[0].sentAt)}
                </span>
              </div>
              {day.map((message, index) => {
                const prev = day[index - 1];
                const newRun =
                  !prev ||
                  prev.fromMe !== message.fromMe ||
                  (!message.fromMe && prev.author !== message.author);
                return (
                  <MessageBubble
                    key={message.id}
                    message={message}
                    account={chat.account}
                    tail={newRun}
                    showAuthor={Boolean(chat.isGroup) && newRun && !message.fromMe}
                    loadMedia={loadMedia}
                    openMedia={openMedia}
                    loadLinkThumbnail={loadLinkThumbnail}
                    onOpenPhoto={loadMedia ? openPhoto : undefined}
                    onReact={onReact}
                    onStar={onStar}
                    onReply={canSend ? setReplyingTo : undefined}
                    onForward={onForward}
                    onDelete={onDelete ? setDeleting : undefined}
                    flash={flashed === message.id}
                  />
                );
              })}
            </section>
          ))}
        </div>
      </div>

      {viewing && loadMedia && viewable.some((item) => item.key === viewing) ? (
        <MediaViewer
          chat={chat}
          items={viewable}
          startKey={viewing}
          loadMedia={loadMedia}
          onClose={() => setViewing(null)}
        />
      ) : null}

      {deleting && onDelete ? (
        <DeleteDialog
          message={deleting}
          account={chat.account}
          onDelete={(forEveryone) => onDelete(deleting, forEveryone)}
          onClose={() => setDeleting(null)}
        />
      ) : null}

      <Composer
        chatId={chatKey}
        files={pending}
        onAddFiles={attaching ? addFiles : undefined}
        onRemoveFile={(index) => setPending((prev) => prev.filter((_, at) => at !== index))}
        disabled={!canSend}
        disabledReason={cannotSendReason}
        replyTo={
          replyTarget
            ? {
                author: replyTarget.fromMe ? "You" : (replyTarget.author ?? chat.name),
                text: replyTarget.text ?? replyTarget.mediaLabel ?? replyTarget.imageLabel ?? "Message",
              }
            : undefined
        }
        onCancelReply={() => setReplyingTo(null)}
        onSend={send}
      />
    </section>
  );
}

/** Splits a chronological message list into runs of the same calendar day. */
function groupByDay(messages: Message[]): Message[][] {
  const days: Message[][] = [];
  for (const message of messages) {
    const last = days[days.length - 1];
    if (last && sameDay(last[last.length - 1].sentAt, message.sentAt)) last.push(message);
    else days.push([message]);
  }
  return days;
}
