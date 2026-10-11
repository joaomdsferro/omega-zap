import { Fragment, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Chat, Message, OutgoingLink, StarredMessage } from "../backend";
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
import { ChatSearch, searchChat } from "./ChatSearch";
import { searchWords } from "../lib/search";
import { SearchIcon } from "./Icons";

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
  onSend: (text: string, files: File[], replyTo?: string, onFileSent?: () => void, link?: OutgoingLink) => Promise<void>;
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
  /** Loads the messages from before `before` (an ISO time); false when there are none. */
  onLoadOlder?: (before: string) => Promise<boolean>;
  /** Changes when older messages of this chat came from the phone: there may be more to load. */
  historyTick?: number;
  /** Searches the whole history, for the search in this chat. */
  searchMessages?: (query: string, chatId?: string) => Promise<StarredMessage[]>;
}

export function ChatView({ chat, messages, subtitle, showAccount, canSend, cannotSendReason, onSend, canAttach, onError, loadMedia, openMedia, loadLinkThumbnail, onReact, onStar, onForward, onDelete, focusMessageId, onFocused, onLoadOlder, historyTick, searchMessages }: ChatViewProps) {
  const scroller = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);
  const lastChat = useRef<string | null>(null);
  const chatKey = `${chat.account}:${chat.id}`;
  // A chat opened with its messages in hand shows them as they are; one that
  // had to wait for them lets them fade in when they come.
  const openedOn = useRef<string | null>(null);
  const openedEmpty = useRef(false);
  /** Where the unread messages began when the chat was opened; settled once its messages are in. */
  const unread = useRef<UnreadMark>({ count: 0, id: null, settled: true, shown: false });
  if (openedOn.current !== chatKey) {
    openedOn.current = chatKey;
    openedEmpty.current = messages.length === 0;
    unread.current = { count: chat.unread, id: null, settled: chat.unread === 0, shown: false };
  }
  if (!unread.current.settled && messages.length > 0) {
    unread.current.settled = true;
    unread.current.id = firstUnread(messages, unread.current.count);
  }
  /** The unread divider, kept where the chat opened it while pictures above
   *  it load, until the user scrolls. */
  const pinned = useRef<Element | null>(null);
  /** Where the view was put last, to tell the user's scrolling from ours. */
  const placedAt = useRef<number | null>(null);
  if (lastChat.current !== chatKey) pinned.current = null;

  function toDivider() {
    const el = scroller.current;
    const divider = pinned.current;
    if (!el || !divider?.isConnected) return void (pinned.current = null);
    el.scrollTop += divider.getBoundingClientRect().top - el.getBoundingClientRect().top - UNREAD_OFFSET;
    placedAt.current = el.scrollTop;
    stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
  }

  const unreadAt = unread.current.id ? messages.findIndex((m) => m.id === unread.current.id) : -1;

  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const switched = lastChat.current !== chatKey;
    lastChat.current = chatKey;
    // A chat with unread messages opens where they start, as WhatsApp does.
    const mark = unread.current;
    const divider = mark.id && !mark.shown ? content.current?.querySelector("[data-unread-divider]") : null;
    if (divider) {
      mark.shown = true;
      pinned.current = divider;
      toDivider();
      setFlashed(mark.id);
      return;
    }
    if (switched || stickToBottom.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [messages, chatKey]);

  // Pictures arrive after the messages and change their height.
  useEffect(() => {
    const el = scroller.current;
    if (!el || !content.current) return;
    const observer = new ResizeObserver(() => {
      if (pinned.current) toDivider();
      else if (stickToBottom.current) el.scrollTop = el.scrollHeight;
    });
    observer.observe(content.current);
    return () => observer.disconnect();
  }, [chatKey]);

  // How many of the newest messages are drawn, for this chat: a chat opens
  // with one page, so opening it never waits on hundreds of bubbles.
  const [drawn, setDrawn] = useState({ chatKey, count: PAGE });
  // Enough is drawn on opening to reach the first unread message.
  const count = drawn.chatKey === chatKey ? drawn.count : Math.max(PAGE, unreadAt === -1 ? 0 : messages.length - unreadAt + 10);
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

  /** Older history being asked for, so only one page is on its way at a time. */
  const olderLoading = useRef(false);
  /** The chat whose history has nothing older left. */
  const [olderDone, setOlderDone] = useState<string | null>(null);
  useEffect(() => setOlderDone(null), [historyTick]);
  function loadOlder() {
    if (!onLoadOlder || olderLoading.current || olderDone === chatKey || !messages.length) return;
    olderLoading.current = true;
    const key = chatKey;
    onLoadOlder(messages[0].sentAt)
      .then((got) => {
        if (!got) setOlderDone(key);
      })
      .catch(() => setOlderDone(key))
      .finally(() => {
        olderLoading.current = false;
      });
  }

  const [flashed, setFlashed] = useState<string | null>(null);
  /** The search bar over the conversation is open, for this chat. */
  const [searching, setSearching] = useState<string | null>(null);
  const searchOpen = searching === chatKey;
  const [searchQuery, setSearchQuery] = useState("");
  useEffect(() => setSearchQuery(""), [searchOpen]);
  // What is loaded is searched at once; the rest of the history after a pause in typing.
  const [savedFound, setSavedFound] = useState<Message[]>([]);
  useEffect(() => {
    setSavedFound([]);
    const q = searchQuery.trim();
    if (!searchOpen || !searchMessages || !q) return;
    let alive = true;
    const timer = window.setTimeout(() => {
      searchMessages(q, chat.id)
        .then((found) => alive && setSavedFound(found.filter((f) => f.account === chat.account).map((f) => f.message)))
        .catch(() => {});
    }, 200);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [searchOpen, searchQuery, searchMessages, chat.account, chat.id]);
  const searchFound = useMemo(() => {
    if (!searchOpen) return [];
    const loaded = searchChat(messages, searchQuery);
    if (!savedFound.length) return loaded;
    const when = new Map<string, string>();
    for (const m of [...savedFound, ...messages]) when.set(m.id, m.sentAt);
    return [...new Set([...loaded, ...savedFound.map((m) => m.id)])].sort((a, b) => when.get(b)!.localeCompare(when.get(a)!));
  }, [searchOpen, messages, searchQuery, savedFound]);
  const searchHits = useMemo(() => new Set(searchFound), [searchFound]);
  const searchMarks = useMemo(() => searchWords(searchQuery), [searchQuery]);

  // Ctrl+F searches the open chat.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "f" || event.shiftKey || event.altKey) return;
      event.preventDefault();
      setSearching(chatKey);
      document.getElementById("chat-search-in")?.focus();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [chatKey]);
  /** A quoted message that was clicked, to scroll to. */
  const [jumpTo, setJumpTo] = useState<string | null>(null);
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
    setJumpTo(null);
  }, [chatKey]);

  // A quoted message that is deleted can't be replied to any more.
  const replyTarget = replyingTo && messages.find((m) => m.id === replyingTo.id && !m.deleted);

  async function send(text: string, files: File[], link?: OutgoingLink) {
    // Each file leaves the tray as it is sent, so a failure leaves only the
    // ones still to send. What is sent shows at the bottom.
    pinned.current = null;
    stickToBottom.current = true;
    await onSend(text, files, replyTarget?.id, () => setPending((prev) => prev.slice(1)), link);
    setReplyingTo(null);
    setPending([]);
  }

  // Files can be added while the account connects; they wait for Send.
  const attaching = Boolean(canAttach);
  const [dragging, setDragging] = useState(false);

  function addFiles(files: File[]) {
    if (files.length) setPending((prev) => [...prev, ...files]);
  }

  const section = useRef<HTMLElement>(null);
  const reportError = useRef(onError);
  reportError.current = onError;

  // Files dragged in from the desktop are taken when they land on the conversation.
  // Where they can't be sent, the drop says so rather than doing nothing.
  useEffect(() => {
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
        if (!attaching) return reportError.current?.("Files can't be sent to this chat.");
        void read().then(({ files, errors }) => {
          addFiles(files);
          if (errors.length) reportError.current?.(errors.join(" "));
        });
      },
    });
  }, [attaching]);

  // Jumping to a message: it goes to the middle of the view and lights up.
  const target = focusMessageId ?? jumpTo;
  useLayoutEffect(() => {
    if (!target) return;
    const fromQuote = !focusMessageId;
    // An older message is drawn first, with a little of what came before it.
    const at = messages.findIndex((m) => m.id === target);
    if (at === -1) {
      // Further back than what is loaded: older pages come until it does.
      if (onLoadOlder && olderDone !== chatKey) return loadOlder();
      // Older than the whole history here: nowhere to go.
      if (fromQuote) setJumpTo(null);
      return;
    }
    if (at !== -1 && at < hidden) {
      setDrawn({ chatKey, count: messages.length - at + 10 });
      return;
    }
    const el = content.current?.querySelector(`[data-message-id="${CSS.escape(target)}"]`);
    if (!el) return;
    const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    el.scrollIntoView({ block: "center", behavior: fromQuote && !still ? "smooth" : "auto" });
    stickToBottom.current = false;
    pinned.current = null;
    setFlashed(null);
    // Restarts the highlight when the same message is jumped to twice.
    requestAnimationFrame(() => setFlashed(target));
    if (fromQuote) setJumpTo(null);
    else onFocused?.();
  }, [messages, hidden, chatKey, target, focusMessageId, onFocused, olderDone]);

  useEffect(() => {
    if (!flashed) return;
    const id = window.setTimeout(() => setFlashed(null), 1700);
    return () => window.clearTimeout(id);
  }, [flashed]);

  function onScroll() {
    const el = scroller.current;
    if (!el) return;
    if (pinned.current && placedAt.current !== null && Math.abs(el.scrollTop - placedAt.current) > 2) pinned.current = null;
    stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
    if (el.scrollTop < PAGE_MARGIN && grownFrom.current === null) {
      if (hidden > 0) drawMore(count + PAGE);
      else loadOlder();
    }
  }

  // Messages that don't fill the view leave nothing to scroll, so the
  // older ones are drawn until they do.
  useEffect(() => {
    const el = scroller.current;
    if (!el || el.scrollTop >= PAGE_MARGIN || grownFrom.current !== null) return;
    if (hidden > 0) drawMore(count + PAGE);
    else if (el.scrollHeight <= el.clientHeight + PAGE_MARGIN) loadOlder();
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
          {attaching ? `Drop to send to ${chat.name}` : "Files can't be sent to this chat"}
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
        <span className="ml-auto flex items-center gap-2">
          <button
            type="button"
            onClick={() => setSearching(searchOpen ? null : chatKey)}
            aria-label="Search in this chat"
            aria-pressed={searchOpen}
            title="Search in this chat (Ctrl+F)"
            className={[
              "flex h-10 w-10 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)",
              searchOpen ? "bg-(--hover-bg)" : "",
            ].join(" ")}
          >
            <SearchIcon size={20} />
          </button>
          {showAccount ? <AccountBadge account={chat.account} size={26} /> : null}
        </span>
      </header>
      {searchOpen ? <ChatSearch query={searchQuery} onQuery={setSearchQuery} found={searchFound} onJump={setJumpTo} onClose={() => setSearching(null)} /> : null}

      <div
        ref={scroller}
        onScroll={onScroll}
        className="chat-wallpaper min-h-0 flex-1 overflow-y-auto pt-2 pb-4"
      >
        {messages.length === 0 && !chat.lastMessage ? (
          <div className="flex h-full items-center justify-center px-6">
            <p
              className="max-w-90 rounded-[7.5px] bg-(--day-chip-bg) px-4 py-2 text-center text-[13px] text-(--day-chip-text)"
              style={{ boxShadow: "var(--bubble-shadow)" }}
            >
              No messages on this computer yet. They are fetched from your phone in the background.
            </p>
          </div>
        ) : null}
        <div key={chatKey} ref={content} className={openedEmpty.current && messages.length > 0 ? "animate-fade-in" : undefined}>
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
                const bubble = (
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
                    onJumpTo={setJumpTo}
                    flash={flashed === message.id}
                    highlight={searchHits.has(message.id) ? searchMarks : undefined}
                  />
                );
                if (message.id !== unread.current.id) return bubble;
                return (
                  <Fragment key={message.id}>
                    <div data-unread-divider className="my-3 flex justify-center bg-[color-mix(in_oklab,var(--panel-bg)_45%,transparent)] py-1.25">
                      <span className="rounded-full bg-(--panel-bg) px-3 py-1 text-[12.5px] text-(--text-secondary)" style={{ boxShadow: "var(--bubble-shadow)" }}>
                        {unread.current.count === 1 ? "1 unread message" : `${unread.current.count} unread messages`}
                      </span>
                    </div>
                    {bubble}
                  </Fragment>
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
        account={chat.account}
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

/** How far below the top of the view the unread divider lands, clear of the day chip. */
const UNREAD_OFFSET = 52;

interface UnreadMark {
  /** Unread messages when the chat was opened. */
  count: number;
  /** The first of them, which the divider goes above. */
  id: string | null;
  /** The first one has been looked for. */
  settled: boolean;
  /** The view has been scrolled to the divider. */
  shown: boolean;
}

/** The oldest of the last `count` messages from others: where the unread ones start. */
function firstUnread(messages: Message[], count: number): string | null {
  let left = count;
  let first: string | null = null;
  for (let at = messages.length - 1; at >= 0 && left > 0; at--) {
    if (messages[at].fromMe) continue;
    first = messages[at].id;
    left--;
  }
  return first;
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
