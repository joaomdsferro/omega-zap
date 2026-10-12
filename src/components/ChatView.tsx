import { Fragment, type ReactNode, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Chat, Contact, GroupInfo as Group, GroupMember, MemberChange, Message, OutgoingLink, OutgoingMedia, Person, PersonDetails, PinNote, Pins, StarredMessage, StickerLibrary, StickerShelf, TypingState } from "../backend";
import { ContactInfo } from "./ContactInfo";
import { GroupInfo } from "./GroupInfo";
import { listenForFileDrops } from "../lib/dropped";
import { formatDayLabel, formatMessageTime, sameDay } from "../lib/format";
import { AccountBadge } from "./AccountBadge";
import { Avatar } from "./Avatar";
import { Composer } from "./Composer";
import { isVisual } from "./Attachments";
import type { LoadMedia } from "./Attachments";
import { Album, ALBUM_MIN, albumPhoto, continuesAlbum } from "./Album";
import { MediaViewer } from "./MediaViewer";
import type { ViewerItem } from "./MediaViewer";
import { DeleteDialog } from "./DeleteDialog";
import { EditDialog, canEdit } from "./EditDialog";
import { canForward } from "./ForwardDialog";
import { MessageBubble } from "./MessageBubble";
import { PinnedBar } from "./PinnedBar";
import { PinDialog } from "./PinDialog";
import { BlockedIcon, CheckIcon, ChevronDownIcon, ClearIcon, CloseIcon, ExitIcon, ForwardIcon, InfoIcon, MoreIcon, MuteIcon, PinIcon, SelectIcon, StarIcon, TrashIcon } from "./Icons";
import { PopupMenu, type MenuEntry } from "./PopupMenu";
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
  /** Sends a sticker or GIF, answering `replyTo`; absent where they can't be sent. */
  onSendMedia?: (item: OutgoingMedia, replyTo?: string) => Promise<void>;
  /** The account's stickers, for the panel and for stickers' own actions in messages. */
  stickers?: StickerShelf;
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
  /** Sends messages on to other chats. */
  onForward?: (messages: Message[]) => void;
  /** Deletes a message for us, or for everyone. */
  onDelete?: (message: Message, forEveryone: boolean) => Promise<void>;
  /** Changes what one of our texts says, for everyone. */
  onEdit?: (message: Message, text: string) => Promise<void>;
  /** Votes on a poll for the options picked; none takes the vote back. */
  onVote?: (message: Message, options: number[]) => Promise<void>;
  /** Opens a chat with a number from a contact card, digits only. */
  onMessageNumber?: (waId: string, name: string) => void;
  /** A message to scroll to and highlight once it has loaded. */
  focusMessageId?: string | null;
  onFocused?: () => void;
  /** Someone's contact info, opened from a sender's name or a mention. */
  loadPerson?: (id: string) => Promise<Person>;
  /** About, full-size picture and groups in common for someone's contact info. */
  loadPersonDetails?: (id: string) => Promise<PersonDetails>;
  /** Opens a chat with someone from their contact info, or one of its messages. */
  onMessagePerson?: (person: Person, messageId?: string) => void;
  /** Opens another chat of this account, such as a group in common. */
  onOpenChatId?: (id: string) => void;
  onStarred?: () => void;
  onMuteChat?: (chatId: string, muted: boolean) => Promise<void>;
  /** Asks to block or unblock someone; settles once it is done, failing when cancelled. */
  onBlock?: (person: { id: string; name: string }, blocked: boolean) => Promise<void>;
  /** Asks to empty this chat of its messages. */
  onClearChat?: () => void;
  /** Asks to delete a chat of this account: this one, or the one with someone from their contact info. */
  onDeleteChat?: (id: string, isGroup: boolean) => void;
  /** Asks to leave this group; settles once it is done, failing when cancelled. */
  onExitGroup?: () => Promise<void>;
  onEditGroup?: (changes: { name?: string; description?: string }) => Promise<Group>;
  /** Changes this group's members; removing asks first. */
  onChangeMembers?: (change: MemberChange, members: GroupMember[]) => Promise<Group>;
  /** Contacts that can be added to a group. */
  loadContacts?: () => Promise<Contact[]>;
  /** Changes when this group changed elsewhere. */
  groupTick?: number;
  /** Tells a chat, named `account:id`, that we are typing or stopped. */
  onTyping?: (chatKey: string, state: TypingState) => void;
  /** Loads the messages from before `before` (an ISO time); false when there are none. */
  onLoadOlder?: (before: string) => Promise<boolean>;
  /** Changes when older messages of this chat came from the phone: there may be more to load. */
  historyTick?: number;
  /** Searches the whole history, for the search in this chat. */
  searchMessages?: (query: string, chatId?: string) => Promise<StarredMessage[]>;
  /** A group's info, from what this computer knows. */
  loadGroup?: (id: string) => Promise<Group>;
  /** A group's info fresh from the network. */
  loadGroupDetails?: (id: string) => Promise<Group>;
  /** The messages pinned in a chat, the latest pin first, and who pinned what. */
  loadPinned?: (chatId: string) => Promise<Pins>;
  /** Pins a message for everyone for `secs`, or unpins it with `null`. */
  onPin?: (message: Message, secs: number | null) => Promise<void>;
  /** Changes when a message of this chat was pinned or unpinned. */
  pinsTick?: number;
}

export function ChatView({ chat, messages, subtitle, showAccount, canSend, cannotSendReason, onSend, canAttach, onSendMedia, stickers, onError, loadMedia, openMedia, loadLinkThumbnail, onReact, onStar, onForward, onDelete, onEdit, onVote, onMessageNumber, focusMessageId, onFocused, onLoadOlder, historyTick, searchMessages, loadPerson, loadPersonDetails, onMessagePerson, onOpenChatId, onStarred, onMuteChat, onBlock, onClearChat, onDeleteChat, onExitGroup, onEditGroup, onChangeMembers, loadContacts, groupTick, onTyping, loadPinned, onPin, pinsTick, loadGroup, loadGroupDetails }: ChatViewProps) {
  const [personId, setPersonId] = useState<string | null>(null);
  const closePerson = useCallback(() => setPersonId(null), []);
  const loadShownPerson = useCallback(() => loadPerson!(personId!), [loadPerson, personId]);
  const loadShownDetails = useCallback(() => loadPersonDetails!(personId!), [loadPersonDetails, personId]);
  /** Group info is open, for this chat. */
  const [groupOpen, setGroupOpen] = useState<string | null>(null);
  const closeGroup = useCallback(() => setGroupOpen(null), []);
  const loadShownGroup = useCallback(() => loadGroup!(chat.id), [loadGroup, chat.id]);
  const loadShownGroupDetails = useCallback(() => loadGroupDetails!(chat.id), [loadGroupDetails, chat.id]);
  // Someone's contact info belongs to the chat it was opened from.
  useEffect(() => setPersonId(null), [chat.id, chat.account]);
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

  // Scrolled up from the newest messages: the newest one then, so what came
  // in since can be counted on the jump-to-present button.
  const lastId = messages[messages.length - 1]?.id ?? "";
  const [awayFrom, setAwayFrom] = useState<string | null>(null);
  useEffect(() => setAwayFrom(null), [chatKey]);
  const arrived = useMemo(() => {
    if (awayFrom === null) return 0;
    const at = messages.findIndex((m) => m.id === awayFrom);
    return at === -1 ? 0 : messages.slice(at + 1).filter((m) => !m.fromMe).length;
  }, [awayFrom, messages]);

  function jumpToPresent() {
    const el = scroller.current;
    if (!el) return;
    pinned.current = null;
    stickToBottom.current = true;
    const far = el.scrollHeight - el.scrollTop - el.clientHeight > el.clientHeight * 3;
    el.scrollTo({ top: el.scrollHeight, behavior: far ? "instant" : "smooth" });
    setAwayFrom(null);
  }

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

  // Pinned messages, asked for again when pins change or older history comes.
  const [pinsOf, setPinsOf] = useState<{ chatKey: string; list: Message[]; notes: PinNote[] }>({ chatKey, list: [], notes: [] });
  useEffect(() => {
    if (!loadPinned) return setPinsOf({ chatKey, list: [], notes: [] });
    let alive = true;
    loadPinned(chat.id)
      .then(({ messages: list, notes }) => alive && setPinsOf({ chatKey, list, notes }))
      .catch(console.error);
    return () => {
      alive = false;
    };
  }, [loadPinned, chat.id, chatKey, pinsTick, historyTick]);
  // A pinned message that is loaded shows as it is now, edited or deleted.
  const pins = useMemo(() => {
    if (pinsOf.chatKey !== chatKey) return [];
    const loaded = new Map(messages.map((m) => [m.id, m]));
    return pinsOf.list.map((pin) => loaded.get(pin.id) ?? pin).filter((pin) => !pin.deleted);
  }, [pinsOf, chatKey, messages]);
  const pinnedIds = useMemo(() => new Set(pins.map((pin) => pin.id)), [pins]);
  const pinNotes = pinsOf.chatKey === chatKey ? pinsOf.notes : NO_NOTES;
  /** The message whose pin length is being chosen. */
  const [pinning, setPinning] = useState<Message | null>(null);
  const pinsRef = useRef(pinnedIds);
  pinsRef.current = pinnedIds;
  const togglePin = useMemo(() => {
    if (!onPin) return undefined;
    return (message: Message) => {
      if (!pinsRef.current.has(message.id)) return setPinning(message);
      onPin(message, null).catch((e) => reportError.current?.(String(e)));
    };
  }, [onPin]);
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
  const [deleting, setDeleting] = useState<Message[] | null>(null);
  const [editing, setEditing] = useState<Message | null>(null);
  /** Files picked, dropped or pasted, waiting for Send. */
  const [pending, setPending] = useState<File[]>([]);

  useEffect(() => {
    setViewing(null);
    setReplyingTo(null);
    setDeleting(null);
    setEditing(null);
    setPending([]);
    setJumpTo(null);
  }, [chatKey]);

  // Messages picked to act on together, as WhatsApp's "Select messages".
  const [picked, setPicked] = useState<{ chatKey: string; ids: Set<string> } | null>(null);
  const selection = picked?.chatKey === chatKey ? picked.ids : null;
  const chosen = useMemo(() => (selection ? messages.filter((m) => selection.has(m.id)) : []), [selection, messages]);
  /** The message picked last, where a Shift+click range starts. */
  const pickedLast = useRef<string | null>(null);
  const messagesRef = useRef(messages);
  messagesRef.current = messages;
  const startSelecting = useMemo(
    () =>
      onForward || onDelete || onStar
        ? (message: Message) => {
            pickedLast.current = message.id;
            setPicked({ chatKey, ids: new Set([message.id]) });
          }
        : undefined,
    [onForward, onDelete, onStar, chatKey],
  );
  const stopSelecting = useCallback(() => setPicked(null), []);

  /** The chat's menu, open at a point. */
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);
  useEffect(() => setMenuAt(null), [chatKey]);
  const blockPerson = onBlock && !chat.isGroup ? (blocked: boolean) => void onBlock({ id: chat.id, name: chat.name }, blocked).catch(() => {}) : undefined;
  const menuItems: MenuEntry[] = [
    ...((chat.isGroup ? loadGroup : loadPerson)
      ? [{ label: chat.isGroup ? "Group info" : "Contact info", icon: <InfoIcon size={18} />, onSelect: () => (chat.isGroup ? setGroupOpen(chatKey) : setPersonId(chat.id)) }]
      : []),
    ...(startSelecting ? [{ label: "Select messages", icon: <SelectIcon size={18} />, onSelect: () => setPicked({ chatKey, ids: new Set() }) }] : []),
    ...(onMuteChat
      ? [{ label: chat.muted ? "Unmute notifications" : "Mute notifications", icon: <MuteIcon size={18} />, onSelect: () => void onMuteChat(chat.id, !chat.muted).catch(() => {}) }]
      : []),
    "separator",
    ...(onClearChat ? [{ label: "Clear chat", icon: <ClearIcon size={18} />, onSelect: onClearChat }] : []),
    ...(blockPerson ? [{ label: chat.blocked ? "Unblock" : "Block", icon: <BlockedIcon size={18} />, danger: !chat.blocked, onSelect: () => blockPerson(!chat.blocked) }] : []),
    // A group is left before it is deleted, as on the phone.
    ...(chat.isGroup && !chat.left
      ? onExitGroup
        ? [{ label: "Exit group", icon: <ExitIcon size={18} />, danger: true, onSelect: () => void onExitGroup().catch(() => {}) }]
        : []
      : onDeleteChat
        ? [{ label: chat.isGroup ? "Delete group" : "Delete chat", icon: <TrashIcon size={18} />, danger: true, onSelect: () => onDeleteChat(chat.id, Boolean(chat.isGroup)) }]
        : []),
  ];
  // No separator at the end.
  if (menuItems[menuItems.length - 1] === "separator") menuItems.pop();

  /** Picks or unpicks `ids` (an album's photos go together); with Shift, picks everything from the last one picked. */
  function togglePicked(ids: string[], range: boolean) {
    setPicked((prev) => {
      if (!prev || prev.chatKey !== chatKey) return prev;
      const next = new Set(prev.ids);
      const list = messagesRef.current;
      const from = range && pickedLast.current ? list.findIndex((m) => m.id === pickedLast.current) : -1;
      if (from !== -1) {
        const to = list.findIndex((m) => m.id === ids[0]);
        const [lo, hi] = from < to ? [from, to + ids.length - 1] : [to, from];
        for (const m of list.slice(lo, hi + 1)) next.add(m.id);
      } else if (ids.every((id) => next.has(id))) {
        for (const id of ids) next.delete(id);
      } else {
        for (const id of ids) next.add(id);
      }
      pickedLast.current = ids[ids.length - 1];
      return { chatKey, ids: next };
    });
  }

  // Escape stops selecting, unless something over the chat takes it first.
  useEffect(() => {
    if (!selection) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented || document.querySelector('[aria-modal="true"], [role="menu"]')) return;
      event.preventDefault();
      stopSelecting();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [selection, stopSelecting]);

  const forwardOne = useMemo(() => (onForward ? (message: Message) => onForward([message]) : undefined), [onForward]);
  const deleteOne = useMemo(() => (onDelete ? (message: Message) => setDeleting([message]) : undefined), [onDelete]);

  /** Stars all of the picked messages, or unstars them when all are starred already. */
  function starPicked() {
    if (!onStar) return;
    const live = chosen.filter((m) => !m.deleted);
    const unstar = live.every((m) => m.starred);
    for (const message of live) if (Boolean(message.starred) === unstar) onStar(message);
    stopSelecting();
  }

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

  async function sendMedia(item: OutgoingMedia) {
    pinned.current = null;
    stickToBottom.current = true;
    await onSendMedia!(item, replyTarget?.id);
    setReplyingTo(null);
  }

  // Which stickers are favorites and which packs are ours, for a sticker's
  // menu in a message. Kept in a ref and read when a menu opens, so a change
  // to the library doesn't redraw every message.
  const library = useRef<StickerLibrary | null>(null);
  useEffect(() => {
    library.current = null;
    if (!stickers) return;
    let alive = true;
    const load = () =>
      void stickers
        .library()
        .then((l) => {
          if (alive) library.current = l;
        })
        .catch(() => {});
    load();
    const stop = stickers.subscribe(load);
    return () => {
      alive = false;
      stop();
    };
  }, [stickers]);
  const stickerActions = useMemo(() => {
    if (!stickers) return undefined;
    const fail = (e: unknown) => reportError.current?.(String(e));
    return (message: Message): MenuEntry[] => {
      const id = message.attachments?.find((a) => a.kind === "sticker")?.stickerId;
      const known = library.current;
      if (!id || message.deleted || !known) return [];
      const target = { chatId: message.chatId, messageId: message.id };
      const favorite = known.favorites.some((s) => s.id === id);
      const own = known.packs.filter((p) => !p.store);
      const items: MenuEntry[] = [
        {
          label: favorite ? "Remove from favorites" : "Add to favorites",
          icon: <StarIcon size={20} filled={favorite} />,
          onSelect: () => stickers.setFavorite(target, !favorite).catch(fail),
        },
      ];
      if (own.length) {
        items.push({ heading: "Add to pack" });
        for (const pack of own) items.push({ label: pack.name, onSelect: () => stickers.addToPack(pack.id, target).catch(fail) });
      }
      return items;
    };
  }, [stickers]);

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
    setAwayFrom(el.scrollHeight - el.scrollTop - el.clientHeight > el.clientHeight / 2 ? (prev) => prev ?? lastId : null);
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

  /** Wraps a bubble or album so that, while selecting, a click picks it. */
  function selectable(key: string, group: Message[], node: ReactNode, mine: boolean) {
    return (
      <Selectable
        key={key}
        selecting={selection !== null}
        checked={selection !== null && group.every((m) => selection.has(m.id))}
        label={mine ? "Your message" : `Message from ${group[0].author ?? chat.name}`}
        onToggle={(range) => togglePicked(group.map((m) => m.id), range)}
      >
        {node}
      </Selectable>
    );
  }

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
        {/* The header opens contact info, or group info for a group, as on WhatsApp. */}
        <button
          type="button"
          onClick={() => (chat.isGroup ? setGroupOpen(chatKey) : setPersonId(chat.id))}
          disabled={chat.isGroup ? !loadGroup : !loadPerson}
          aria-label={chat.isGroup ? (loadGroup ? `Group info for ${chat.name}` : undefined) : loadPerson ? `Contact info for ${chat.name}` : undefined}
          className="flex min-w-0 items-center gap-3 text-left disabled:cursor-default"
        >
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
        </button>
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
          <button
            type="button"
            onClick={(event) => {
              const rect = event.currentTarget.getBoundingClientRect();
              setMenuAt(menuAt ? null : { x: rect.right - 220, y: rect.bottom + 4 });
            }}
            aria-label="Menu"
            aria-haspopup="menu"
            aria-expanded={menuAt !== null}
            className={["flex h-10 w-10 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)", menuAt ? "bg-(--hover-bg)" : ""].join(" ")}
          >
            <MoreIcon size={20} />
          </button>
          {showAccount ? <AccountBadge account={chat.account} size={26} /> : null}
        </span>
      </header>
      {menuAt ? <PopupMenu x={menuAt.x} y={menuAt.y} label={`Options for ${chat.name}`} items={menuItems} onClose={() => setMenuAt(null)} /> : null}
      <PinnedBar pins={pins} chatKey={chatKey} onJump={setJumpTo} />
      {searchOpen ? <ChatSearch query={searchQuery} onQuery={setSearchQuery} found={searchFound} onJump={setJumpTo} onClose={() => setSearching(null)} /> : null}

      <div className="relative flex min-h-0 flex-1 flex-col">
      <div
        ref={scroller}
        onScroll={onScroll}
        className="chat-wallpaper min-h-0 flex-1 overflow-y-auto pt-2 pb-4"
      >
        {messages.length === 0 && !chat.lastMessage && !chat.cleared ? (
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
          {withPinNotes(visible, pinNotes).map(({ key, at, messages: day, notes }) => (
            // Each day is its own block so its sticky chip scrolls away when
            // the next day's chip arrives, instead of stacking under it.
            <section key={key}>
              <div className="sticky top-2 z-10 my-3 flex justify-center">
                <span
                  className="rounded-[7.5px] bg-(--day-chip-bg) px-3 py-1.25 text-[12.5px] text-(--day-chip-text) uppercase"
                  style={{ boxShadow: "var(--bubble-shadow)" }}
                >
                  {formatDayLabel(at)}
                </span>
              </div>
              {albums(day).map(({ message, index, album }) => (
                <Fragment key={message.id}>
                  {pinLines(notes, day[index - 1], album ? album[album.length - 1] : message, setJumpTo)}
                  {(() => {
                const prev = day[index - 1];
                if (album) {
                  const tail = !prev || prev.fromMe !== message.fromMe || (!message.fromMe && prev.author !== message.author);
                  const grid = selectable(
                    message.id,
                    album,
                    <Album
                      messages={album}
                      tail={tail}
                      showAuthor={Boolean(chat.isGroup) && tail && !message.fromMe}
                      loadMedia={loadMedia}
                      onOpen={loadMedia ? (photo) => openPhoto(photo, 0) : undefined}
                    />,
                    message.fromMe,
                  );
                  if (!album.some((photo) => photo.id === unread.current.id)) return grid;
                  return (
                    <Fragment key={message.id}>
                      <div data-unread-divider className="my-3 flex justify-center bg-[color-mix(in_oklab,var(--panel-bg)_45%,transparent)] py-1.25">
                        <span className="rounded-full bg-(--panel-bg) px-3 py-1 text-[12.5px] text-(--text-secondary)" style={{ boxShadow: "var(--bubble-shadow)" }}>
                          {unread.current.count === 1 ? "1 unread message" : `${unread.current.count} unread messages`}
                        </span>
                      </div>
                      {grid}
                    </Fragment>
                  );
                }
                const newRun =
                  !prev ||
                  prev.fromMe !== message.fromMe ||
                  (!message.fromMe && prev.author !== message.author);
                const bubble = selectable(
                  message.id,
                  [message],
                  <MessageBubble
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
                    onForward={forwardOne}
                    onDelete={deleteOne}
                    onEdit={onEdit ? setEditing : undefined}
                    onVote={onVote}
                    onMessageContact={onMessageNumber}
                    onJumpTo={setJumpTo}
                    onOpenPerson={loadPerson ? setPersonId : undefined}
                    flash={flashed === message.id}
                    highlight={searchHits.has(message.id) ? searchMarks : undefined}
                    pinned={pinnedIds.has(message.id)}
                    onPin={togglePin}
                    onSelect={startSelecting}
                    moreActions={stickerActions}
                  />,
                  message.fromMe,
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
                  })()}
                </Fragment>
              ))}
              {pinLines(notes, day[day.length - 1], undefined, setJumpTo)}
            </section>
          ))}
        </div>
      </div>
      {awayFrom !== null ? (
        <button
          type="button"
          onClick={jumpToPresent}
          aria-label={arrived ? `Jump to present, ${arrived} new ${arrived === 1 ? "message" : "messages"}` : "Jump to present"}
          title="Jump to present"
          className="animate-fade-rise absolute right-5 bottom-4 z-20 flex h-10.5 w-10.5 items-center justify-center rounded-full bg-(--panel-bg) text-(--icon) hover:brightness-110"
          style={{ boxShadow: "0 1px 3px rgba(11,20,26,.2), 0 2px 8px rgba(11,20,26,.18)" }}
        >
          <ChevronDownIcon size={24} />
          {arrived ? (
            <span className="absolute -top-1.5 -right-1 flex h-5 min-w-5 items-center justify-center rounded-full bg-(--accent) px-1.5 text-[11px] font-semibold text-(--on-accent) tabular-nums">
              {arrived > 99 ? "99+" : arrived}
            </span>
          ) : null}
        </button>
      ) : null}
      </div>

      {groupOpen === chatKey && loadGroup ? (
        <GroupInfo
          key={chatKey}
          load={loadShownGroup}
          loadDetails={loadGroupDetails ? loadShownGroupDetails : undefined}
          loadMedia={loadMedia}
          onClose={closeGroup}
          onJump={(id) => {
            setGroupOpen(null);
            setJumpTo(id);
          }}
          onOpenPerson={loadPerson ? setPersonId : undefined}
          onStarred={onStarred}
          onMute={onMuteChat ? (muted) => onMuteChat(chat.id, muted) : undefined}
          onEdit={onEditGroup}
          onChangeMembers={onChangeMembers}
          loadContacts={loadContacts}
          onExit={onExitGroup}
          onDelete={
            onDeleteChat
              ? () => {
                  setGroupOpen(null);
                  onDeleteChat(chat.id, true);
                }
              : undefined
          }
          onError={onError}
          version={groupTick}
        />
      ) : null}

      {personId && loadPerson ? (
        <ContactInfo
          key={personId}
          load={loadShownPerson}
          loadDetails={loadPersonDetails ? loadShownDetails : undefined}
          loadMedia={loadMedia}
          onClose={closePerson}
          onMessage={
            onMessagePerson
              ? (person, messageId) => {
                  setPersonId(null);
                  onMessagePerson(person, messageId);
                }
              : undefined
          }
          onOpenGroup={
            onOpenChatId
              ? (id) => {
                  setPersonId(null);
                  onOpenChatId(id);
                }
              : undefined
          }
          onStarred={onStarred}
          onMute={onMuteChat ? (person, muted) => onMuteChat(person.id, muted) : undefined}
          onBlock={onBlock}
          onDeleteChat={
            onDeleteChat
              ? (person) => {
                  setPersonId(null);
                  onDeleteChat(person.id, false);
                }
              : undefined
          }
        />
      ) : null}

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
          messages={deleting}
          account={chat.account}
          onDelete={async (forEveryone) => {
            for (const message of deleting) await onDelete(message, forEveryone);
            stopSelecting();
          }}
          onClose={() => setDeleting(null)}
        />
      ) : null}

      {editing && onEdit ? (
        <EditDialog message={editing} onSave={(text) => onEdit(editing, text)} onClose={() => setEditing(null)} />
      ) : null}
      {pinning && onPin ? (
        <PinDialog full={pins.length >= 3} onPin={(secs) => onPin(pinning, secs)} onClose={() => setPinning(null)} />
      ) : null}

      {selection ? (
        <SelectionBar
          count={chosen.length}
          starred={chosen.length > 0 && chosen.every((m) => m.starred || m.deleted)}
          onStar={onStar && chosen.some((m) => !m.deleted) ? starPicked : undefined}
          onDelete={onDelete && chosen.length ? () => setDeleting(chosen) : undefined}
          onForward={
            onForward && chosen.length && chosen.every(canForward)
              ? () => {
                  onForward(chosen);
                  stopSelecting();
                }
              : undefined
          }
          forwardNote={chosen.some((m) => !canForward(m)) ? "Some of these messages can't be forwarded" : undefined}
          onClose={stopSelecting}
        />
      ) : null}
      {/* Kept while selecting, so what was typed stays. */}
      <div className={selection ? "hidden" : "contents"}>
      {chat.left ? (
        <div className="flex min-h-15.5 shrink-0 items-center justify-center border-l border-(--divider) bg-(--panel-header-bg) px-4 py-3 text-center">
          <span className="text-[14px] text-(--text-secondary)">You can't send messages to this group because you're no longer a member.</span>
        </div>
      ) : chat.blocked ? (
        <div className="flex min-h-15.5 shrink-0 items-center justify-center gap-1 border-l border-(--divider) bg-(--panel-header-bg) px-4 py-3 text-center">
          <span className="text-[14px] text-(--text-secondary)">You blocked this contact.</span>
          {blockPerson ? (
            <button type="button" onClick={() => blockPerson(false)} className="rounded px-1 hover:underline">
              <span className="text-[14px] text-(--accent)">Unblock</span>
            </button>
          ) : null}
        </div>
      ) : (
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
        onEditLast={
          onEdit
            ? () => {
                const last = [...messages].reverse().find((m) => m.fromMe && !m.deleted);
                if (last && canEdit(last, chat.account)) setEditing(last);
              }
            : undefined
        }
        onSend={send}
        onSendMedia={onSendMedia ? sendMedia : undefined}
        stickers={stickers}
        onError={onError}
        onTyping={onTyping}
      />
      )}
      </div>
    </section>
  );
}

interface SelectableProps {
  selecting: boolean;
  checked: boolean;
  /** Who the message is from, for screen readers. */
  label: string;
  onToggle: (range: boolean) => void;
  children: ReactNode;
}

/**
 * A message row that, while selecting, has a checkbox and is picked by a
 * click anywhere on it; what is inside takes no clicks then. Outside of
 * selecting it adds nothing to the layout, and the bubble inside keeps its
 * place in the tree either way so it isn't drawn again.
 */
function Selectable({ selecting, checked, label, onToggle, children }: SelectableProps) {
  return (
    <div
      role={selecting ? "checkbox" : undefined}
      aria-checked={selecting ? checked : undefined}
      aria-label={selecting ? label : undefined}
      tabIndex={selecting ? 0 : undefined}
      onClick={selecting ? (event) => onToggle(event.shiftKey) : undefined}
      onKeyDown={
        selecting
          ? (event) => {
              if (event.key !== " " && event.key !== "Enter") return;
              event.preventDefault();
              onToggle(event.shiftKey);
            }
          : undefined
      }
      className={
        selecting
          ? [
              "flex items-center pl-4 outline-none select-none focus-visible:bg-(--hover-bg)/60",
              checked ? "bg-[color-mix(in_oklab,var(--accent)_16%,transparent)]" : "hover:bg-(--hover-bg)/40",
            ].join(" ")
          : "contents"
      }
    >
      {selecting ? (
        <span
          aria-hidden
          className={[
            "flex h-4.5 w-4.5 shrink-0 items-center justify-center rounded-sm border-2",
            checked ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--text-secondary) bg-(--panel-bg)",
          ].join(" ")}
        >
          {checked ? <CheckIcon size={13} strokeWidth={3} /> : null}
        </span>
      ) : null}
      <div className={selecting ? "pointer-events-none min-w-0 flex-1 pb-0.5" : "contents"}>{children}</div>
    </div>
  );
}

interface SelectionBarProps {
  count: number;
  /** All of the picked messages are starred: the star button unstars them. */
  starred: boolean;
  onStar?: () => void;
  onDelete?: () => void;
  onForward?: () => void;
  /** Why Forward is off, when it is off for some of the messages. */
  forwardNote?: string;
  onClose: () => void;
}

/** Takes the composer's place while messages are being selected. */
function SelectionBar({ count, starred, onStar, onDelete, onForward, forwardNote, onClose }: SelectionBarProps) {
  const button =
    "flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg) disabled:opacity-40 disabled:hover:bg-transparent";
  return (
    <div role="toolbar" aria-label="Selected messages" className="flex h-15.5 shrink-0 items-center gap-2 bg-(--composer-bg) px-4">
      <button type="button" onClick={onClose} aria-label="Cancel selection" title="Cancel (Esc)" className={button}>
        <CloseIcon size={22} />
      </button>
      <p className="min-w-0 flex-1 truncate text-[15px] text-(--text-primary)" aria-live="polite">
        {count} selected
      </p>
      <button type="button" onClick={onStar} disabled={!onStar} aria-label={starred ? "Unstar" : "Star"} title={starred ? "Unstar" : "Star"} className={button}>
        <StarIcon size={22} filled={starred} />
      </button>
      <button type="button" onClick={onDelete} disabled={!onDelete} aria-label="Delete" title="Delete" className={button}>
        <TrashIcon size={22} />
      </button>
      <button type="button" onClick={onForward} disabled={!onForward} aria-label="Forward" title={forwardNote ?? "Forward"} className={button}>
        <ForwardIcon size={22} />
      </button>
    </div>
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

/** A day's messages, with each run of photos long enough to be an album gathered into one. */
function albums(day: Message[]): { message: Message; index: number; album?: Message[] }[] {
  const out: { message: Message; index: number; album?: Message[] }[] = [];
  for (let index = 0; index < day.length; ) {
    const message = day[index];
    let end = index + 1;
    if (albumPhoto(message)) {
      while (end < day.length && continuesAlbum(day[end - 1], day[end])) end++;
    }
    if (end - index >= ALBUM_MIN) {
      out.push({ message, index, album: day.slice(index, end) });
      index = end;
    } else {
      out.push({ message, index });
      index++;
    }
  }
  return out;
}

const NO_NOTES: PinNote[] = [];

/**
 * The days of a chronological message list, each with the "pinned a
 * message" lines that fall in it. A day can hold only lines. Lines from
 * before the first message shown are left out: where they go isn't known yet.
 */
function withPinNotes(messages: Message[], notes: PinNote[]): { key: string; at: string; messages: Message[]; notes: PinNote[] }[] {
  const days = groupByDay(messages).map((day) => ({ key: day[0].id, at: day[0].sentAt, messages: day, notes: [] as PinNote[] }));
  if (!messages.length) return days;
  const first = Date.parse(messages[0].sentAt);
  for (const note of notes) {
    if (Date.parse(note.at) < first) continue;
    let day = days.find((d) => sameDay(d.at, note.at));
    if (!day) {
      day = { key: `pins-${note.at}`, at: note.at, messages: [], notes: [] };
      days.push(day);
      days.sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
    }
    day.notes.push(note);
  }
  return days;
}

/** The lines for pins made after `after` and up to `upTo` (to the end of the day without it). */
function pinLines(notes: PinNote[], after: Message | undefined, upTo: Message | undefined, onJump: (id: string) => void) {
  if (!notes.length) return null;
  const from = after ? Date.parse(after.sentAt) : -Infinity;
  const to = upTo ? Date.parse(upTo.sentAt) : Infinity;
  return notes
    .filter((note) => {
      const at = Date.parse(note.at);
      return at > from && at <= to;
    })
    .map((note) => (
      <div key={`${note.messageId}-${note.at}`} className="my-2 flex justify-center px-4">
        <button
          type="button"
          onClick={() => onJump(note.messageId)}
          className="flex max-w-full items-center gap-1.5 rounded-[7.5px] bg-(--day-chip-bg) px-3 py-1.25 text-[12.5px] text-(--day-chip-text) hover:brightness-95"
          style={{ boxShadow: "var(--bubble-shadow)" }}
        >
          <PinIcon size={14} className="shrink-0" />
          <span className="truncate">
            {note.byMe ? "You" : (note.by ?? "Someone")} {note.pinned ? "pinned" : "unpinned"} a message
          </span>
          <span className="shrink-0 tabular-nums opacity-75">{formatMessageTime(note.at)}</span>
        </button>
      </div>
    ));
}
