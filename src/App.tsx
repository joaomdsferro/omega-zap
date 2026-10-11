import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ACCOUNTS, ACCOUNT_IDS, type AccountId } from "./accounts/accounts";
import { formatLastSeen } from "./lib/format";
import { useAccount, type Account } from "./accounts/useAccount";
import {
  MockBackend,
  TauriBackend,
  type Chat,
  type ChatBackend,
  type Contact,
  type CustomList,
  type Message,
  type OutgoingLink,
  type PrivacyCategory,
  type StarredMessage,
} from "./backend";
import { ChatView } from "./components/ChatView";
import { ConfirmDialog, type ConfirmRequest } from "./components/ConfirmDialog";
import { EmptyState } from "./components/EmptyState";
import { ForwardDialog } from "./components/ForwardDialog";
import { MuteDialog } from "./components/MuteDialog";
import { LinkScreen } from "./components/LinkScreen";
import { ListEditor, chatKey } from "./components/ListEditor";
import { NavRail, type RailAccount, type SidebarView } from "./components/NavRail";
import { NewChatPanel } from "./components/NewChatPanel";
import { ResizeHandle } from "./components/ResizeHandle";
import { SettingsPanel } from "./components/SettingsPanel";
import { ShortcutsDialog } from "./components/ShortcutsDialog";
import { chatNumber, shortcutFor } from "./lib/shortcuts";
import { Sidebar } from "./components/Sidebar";
import { StarredPanel } from "./components/StarredPanel";
import { HistoryProgress } from "./components/HistoryProgress";
import { ZoomToast } from "./components/ZoomToast";
import { hueFor, initialsFor } from "./lib/avatar";
import {
  dismissNotification,
  onNotificationOpen,
  showNotification,
  syncNotificationPrefs,
  syncTrayBadge,
} from "./lib/notify";
import {
  CHAT_MIN,
  SIDEBAR_DEFAULT,
  SIDEBAR_MAX,
  SIDEBAR_MIN,
  SettingsProvider,
  useSettings,
} from "./settings/settings";
import { useZoom } from "./settings/useZoom";

const RAIL_WIDTH = 64;
/** How long a chat stays pointed out after its notification was clicked. */
const HIGHLIGHT_MS = 10_000;
/** Matches the transition on `.chat-highlight::after`. */
const HIGHLIGHT_FADE_MS = 600;

function upsert(messages: Message[], message: Message): Message[] {
  const index = messages.findIndex((m) => m.id === message.id);
  if (index === -1) return [...messages, message];
  const next = [...messages];
  next[index] = message;
  return next;
}

/** Logs, while developing, how long opening a chat took until its messages were on screen. */
function logSwitch(key: string, started: number, how: string) {
  requestAnimationFrame(() =>
    setTimeout(() => console.debug(`[perf] opened ${key} (${how}) in ${Math.round(performance.now() - started)} ms`)),
  );
}

/** How many chats keep their messages in memory, so going back to one shows it at once. */
const CACHED_CHATS = 30;

function useWindowWidth() {
  const [width, setWidth] = useState(window.innerWidth);
  useEffect(() => {
    const onResize = () => setWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return width;
}

function Banner({ tone = "info", children }: { tone?: "info" | "warn"; children: ReactNode }) {
  return (
    <div
      className={[
        "mx-3 mb-2 flex items-center gap-3 rounded-lg px-3 py-2.5 text-[13.5px] leading-5",
        tone === "warn"
          ? "bg-(--banner-bg) text-(--banner-text)"
          : "bg-(--search-bg) text-(--text-secondary)",
      ].join(" ")}
      role="status"
    >
      {children}
    </div>
  );
}

function BannerAction({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="ml-auto shrink-0 font-medium text-(--accent-strong) hover:underline"
    >
      {children}
    </button>
  );
}

function accountBanner(account: Account, onLink: () => void, onReconnect: () => void) {
  const { status } = account;
  const { label } = ACCOUNTS[account.id];
  if (!status.linked) {
    if (status.state === "logged_out") {
      return (
        <Banner key={account.id} tone="warn">
          <span>{status.error}</span>
          <BannerAction onClick={onLink}>Link again</BannerAction>
        </Banner>
      );
    }
    const demo = account.id === "whatsapp";
    return (
      <Banner key={account.id}>
        <span>
          {demo
            ? status.state === "unsupported"
              ? "Demo chats (browser preview)."
              : "These are demo chats."
            : status.state === "unsupported"
              ? `${label} needs the desktop app.`
              : `${label} isn't linked yet.`}
        </span>
        {status.state !== "unsupported" ? <BannerAction onClick={onLink}>Link {label}</BannerAction> : null}
      </Banner>
    );
  }
  if (status.state === "error") {
    return (
      <Banner key={account.id} tone="warn">
        <span>{status.error ?? `Can't reach ${label}.`}</span>
        <BannerAction onClick={onReconnect}>Reconnect</BannerAction>
      </Banner>
    );
  }
  if (status.state === "connecting" || status.state === "starting") {
    return (
      <Banner key={account.id} tone="warn">
        <span>Connecting to {label}…</span>
      </Banner>
    );
  }
  if (status.syncProgress !== null) {
    return (
      <Banner key={account.id}>
        <span>Syncing chat history… {status.syncProgress}%</span>
      </Banner>
    );
  }
  if (status.notice) {
    return (
      <Banner key={account.id} tone="warn">
        <span>{status.notice}</span>
      </Banner>
    );
  }
  return null;
}

/** Pinned chats first, then the most recent: the order each account already uses. */
/** Someone typing is forgotten after this long without hearing from them. */
const TYPING_FORGET_MS = 25_000;

function byRecency(a: Chat, b: Chat) {
  if (Boolean(a.pinned) !== Boolean(b.pinned)) return a.pinned ? -1 : 1;
  return b.lastAt.localeCompare(a.lastAt);
}

function Shell() {
  const { settings, setSidebarWidth, setActiveAccount } = useSettings();
  const zoomAnnounce = useZoom();
  const whatsapp = useAccount("whatsapp");
  const signal = useAccount("signal");
  const accounts: Record<AccountId, Account> = useMemo(() => ({ whatsapp, signal }), [whatsapp, signal]);

  const demo = useMemo(() => new MockBackend(), []);
  const real = useMemo(
    () => ({ whatsapp: new TauriBackend("whatsapp"), signal: new TauriBackend("signal") }),
    [],
  );
  const waLinked = whatsapp.status.linked;
  const signalLinked = signal.status.linked;
  const anyLinked = waLinked || signalLinked;
  // Every list worth loading: the linked accounts, and the demo chats that
  // stand in for WhatsApp until it is linked.
  const sources = useMemo(() => {
    const list: ChatBackend[] = [waLinked ? real.whatsapp : demo];
    if (signalLinked) list.push(real.signal);
    return list;
  }, [demo, real, waLinked, signalLinked]);

  const unified = settings.unifiedInbox;
  const active = settings.activeAccount;

  const [view, setView] = useState<SidebarView>("chats");
  /** What the chat list side shows while on the chats view. */
  const [panel, setPanel] = useState<"chats" | "new-chat" | "starred">("chats");
  const [listsBy, setListsBy] = useState<Map<ChatBackend, CustomList[]>>(new Map());
  /** A chat opened from the contacts that has no messages yet, so isn't listed. */
  const [draft, setDraft] = useState<Chat | null>(null);
  /** A message to jump to once its chat has loaded. */
  const [focusMessage, setFocusMessage] = useState<string | null>(null);
  const [editor, setEditor] = useState<{ name?: string; selected: Set<string> } | null>(null);
  const [contacts, setContacts] = useState<Contact[] | null>(null);
  const [starred, setStarred] = useState<StarredMessage[] | null>(null);
  const [linking, setLinking] = useState<AccountId | null>(null);
  const [chatsBy, setChatsBy] = useState<Map<ChatBackend, Chat[]>>(new Map());
  const [selected, setSelected] = useState<{ account: AccountId; id: string } | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  /** Counts older messages arriving from the phone for the open chat. */
  const [historyTick, setHistoryTick] = useState(0);
  const [pinsTick, setPinsTick] = useState(0);
  /** The messages of recently opened chats, most recent last, by `account:id`. */
  const messageCache = useRef(new Map<string, Message[]>());
  /** The chat whose messages `messages` holds. */
  const messagesOf = useRef<string | null>(null);
  const [forwarding, setForwarding] = useState<Message[] | null>(null);
  /** Who is typing where, by `account:id`, as the list words it. */
  const [typing, setTyping] = useState<Map<string, string>>(new Map());
  const typingTimers = useRef(new Map<string, number>());
  /** Who is online, or when they last were, by `account:id`. */
  const [presence, setPresence] = useState<Map<string, { online: boolean; lastSeen?: string }>>(new Map());
  /** A chat asking how long to be muted for, and who waits on the answer. */
  const [muting, setMuting] = useState<{
    chat: Pick<Chat, "account" | "id">;
    resolve: () => void;
    reject: (reason: unknown) => void;
  } | null>(null);
  /** Something that can't be taken back, waiting to be confirmed. */
  const [confirming, setConfirming] = useState<ConfirmRequest | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);

  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  // When the chat on screen last changed: the render that follows a click
  // is the first moment this side knows of it.
  const switchStarted = useRef(0);
  const lastSelected = useRef<string | null>(null);
  const selectedKey = selected ? `${selected.account}:${selected.id}` : null;
  if (lastSelected.current !== selectedKey) {
    lastSelected.current = selectedKey;
    switchStarted.current = performance.now();
  }
  // The chat's messages as last seen are put in place during this same
  // render, so the first frame of the new chat already shows them: no frame
  // of the previous chat, and no empty one, between the click and them.
  const [messagesFor, setMessagesFor] = useState<string | null>(null);
  const openedFromCache = useRef(false);
  if (messagesFor !== selectedKey) {
    setMessagesFor(selectedKey);
    const cached = selectedKey ? messageCache.current.get(selectedKey) : undefined;
    openedFromCache.current = Boolean(cached);
    messagesOf.current = selectedKey;
    setMessages(cached ?? []);
  }
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  /** A chat to open once the account it is on has been switched to. */
  const pendingOpen = useRef<{ account: AccountId; id: string } | null>(null);
  /** A chat a clicked notification asked for, until its account's list is in. */
  const [noticeTarget, setNoticeTarget] = useState<{ account: AccountId; id: string; open: boolean } | null>(null);
  /** A chat pointed out in the list, as `account:id`, and whether it is fading out. */
  const [highlight, setHighlight] = useState<string | null>(null);
  const [highlightFading, setHighlightFading] = useState(false);
  const sidebarRef = useRef<HTMLElement>(null);

  const windowWidth = useWindowWidth();
  const maxSidebar = Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, windowWidth - RAIL_WIDTH - CHAT_MIN));
  const sidebarWidth = Math.min(settings.sidebarWidth, maxSidebar);

  // Chat ids mean nothing once the lists behind them change, unless the
  // change was made to open one.
  useEffect(() => {
    setSelected(pendingOpen.current);
    pendingOpen.current = null;
  }, [sources, unified, active]);

  useEffect(() => {
    let alive = true;
    const unsubscribe = sources.map((backend) => {
      const refresh = () =>
        Promise.all([backend.listChats(), backend.listLists()])
          .then(([list, lists]) => {
            if (!alive) return;
            setChatsBy((prev) => new Map(prev).set(backend, list));
            setListsBy((prev) => new Map(prev).set(backend, lists));
          })
          .catch(console.error);
      void refresh();
      return backend.subscribe((event) => {
        const open = selectedRef.current;
        if (event.type === "chats-changed") {
          void refresh();
        } else if (event.type === "notice") {
          const { notifications, notificationPreview } = settingsRef.current;
          const looking = document.hasFocus() && open?.account === backend.account && open.id === event.notice.chatId;
          if (notifications && !event.notice.muted && !looking) {
            showNotification(backend.account, event.notice, notificationPreview);
          }
        } else if (event.type === "typing") {
          const key = `${backend.account}:${event.chatId}`;
          const label = event.recording ? "recording audio…" : "typing…";
          showTyping(key, event.active ? (event.who ? `${event.who} is ${label}` : label) : null);
        } else if (event.type === "presence") {
          const key = `${backend.account}:${event.chatId}`;
          setPresence((prev) => new Map(prev).set(key, { online: event.online, lastSeen: event.lastSeen ?? prev.get(key)?.lastSeen }));
        } else if (event.type === "cleared") {
          messageCache.current.delete(`${backend.account}:${event.chatId}`);
          if (open?.account !== backend.account || open.id !== event.chatId) return;
          backend
            .getMessages(event.chatId)
            .then((list) => {
              if (messagesOf.current === `${backend.account}:${event.chatId}`) setMessages(list);
            })
            .catch(console.error);
        } else if (event.type === "pins") {
          if (open?.account === backend.account && open.id === event.chatId) setPinsTick((tick) => tick + 1);
        } else if (event.type === "history") {
          if (open?.account !== backend.account || open.id !== event.chatId) return;
          setHistoryTick((tick) => tick + 1);
          // A chat opened empty shows what came at once.
          backend
            .getMessages(event.chatId)
            .then((list) => {
              if (messagesOf.current === `${backend.account}:${event.chatId}`) setMessages((prev) => (prev.length ? prev : list));
            })
            .catch(console.error);
        } else if (event.type !== "message") {
          return;
        } else if (!event.message.fromMe && typingTimers.current.has(`${backend.account}:${event.message.chatId}`)) {
          // Their message is what the typing was for.
          showTyping(`${backend.account}:${event.message.chatId}`, null);
        }
        if (event.type === "message" && open?.account === backend.account && open.id === event.message.chatId) {
          setMessages((prev) => upsert(prev, event.message));
          if (!event.message.fromMe && document.hasFocus()) {
            void backend.markRead(event.message.chatId);
          }
        }
      });
    });
    return () => {
      alive = false;
      for (const stop of unsubscribe) stop();
    };
  }, [sources]);

  // Kept the same between renders, so the settings don't load them again each time.
  const whatsappPrivacy = useMemo(
    () => ({
      load: () => real.whatsapp.privacy!(),
      save: (category: PrivacyCategory, value: string) => real.whatsapp.setPrivacy!(category, value),
    }),
    [real],
  );

  /** Shows someone typing in a chat, or stops; it stops by itself if no word follows, as WhatsApp does. */
  const showTyping = useCallback((key: string, label: string | null) => {
    window.clearTimeout(typingTimers.current.get(key));
    typingTimers.current.delete(key);
    if (label) typingTimers.current.set(key, window.setTimeout(() => showTyping(key, null), TYPING_FORGET_MS));
    setTyping((prev) => {
      if ((prev.get(key) ?? null) === label) return prev;
      const next = new Map(prev);
      if (label) next.set(key, label);
      else next.delete(key);
      return next;
    });
  }, []);

  const backendFor = useCallback(
    (account: AccountId) => sources.find((b) => b.account === account) ?? null,
    [sources],
  );
  const selectedBackend = selected ? backendFor(selected.account) : null;

  // Resting the pointer on a chat fetches its messages ahead of the click,
  // so a chat not opened yet opens as fast as one that was.
  const peekTimer = useRef<number | undefined>(undefined);
  const peek = useCallback(
    (chat: Chat) => {
      window.clearTimeout(peekTimer.current);
      const key = `${chat.account}:${chat.id}`;
      if (messageCache.current.has(key)) return;
      peekTimer.current = window.setTimeout(() => {
        backendFor(chat.account)
          ?.getMessages(chat.id)
          .then((list) => {
            const cache = messageCache.current;
            if (cache.has(key) || list.length === 0) return;
            cache.set(key, list);
            if (cache.size > CACHED_CHATS) cache.delete(cache.keys().next().value!);
          })
          .catch(() => {});
      }, 60);
    },
    [backendFor],
  );
  const selectedId = selected?.id ?? null;

  // Keeps the cache up to date with what arrives in the open chat.
  useEffect(() => {
    const key = messagesOf.current;
    if (!key) return;
    const cache = messageCache.current;
    // A chat left before its messages arrived has nothing worth keeping.
    if (messages.length === 0 && !cache.has(key)) return;
    cache.delete(key);
    cache.set(key, messages);
    if (cache.size > CACHED_CHATS) cache.delete(cache.keys().next().value!);
  }, [messages]);

  useEffect(() => {
    if (!selectedBackend || !selectedId) return;
    const key = `${selectedBackend.account}:${selectedId}`;
    const started = switchStarted.current;
    const cached = openedFromCache.current;
    if (import.meta.env.DEV && cached) logSwitch(key, started, "cached");
    let alive = true;
    selectedBackend
      .getMessages(selectedId)
      .then((list) => {
        if (!alive) return;
        setMessages(list);
        // Nothing here yet: its history is asked for next.
        if (!list.length) void selectedBackend.fetchHistoryFirst?.(selectedId);
        if (import.meta.env.DEV && !cached) logSwitch(key, started, "loaded");
      })
      .catch(console.error);
    void selectedBackend.markRead(selectedId);
    // Online and last seen come for the chat on screen, as on WhatsApp Web.
    selectedBackend.watchPresence?.(selectedId).catch(() => {});
    setChatsBy((prev) => {
      const list = prev.get(selectedBackend);
      if (!list?.some((c) => c.id === selectedId && (c.unread > 0 || c.markedUnread))) return prev;
      return new Map(prev).set(
        selectedBackend,
        list.map((c) => (c.id === selectedId ? { ...c, unread: 0, markedUnread: false } : c)),
      );
    });
    return () => {
      alive = false;
    };
  }, [selectedBackend, selectedId]);

  // Messages that arrived while the window was in the background are read now.
  useEffect(() => {
    if (!selectedBackend || !selectedId) return;
    const onFocus = () => void selectedBackend.markRead(selectedId);
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [selectedBackend, selectedId]);

  const linkingDone = linking !== null && accounts[linking].status.linked;
  useEffect(() => {
    if (linkingDone) setLinking(null);
  }, [linkingDone]);

  useEffect(() => {
    if (!error) return;
    const id = window.setTimeout(() => setError(null), 5000);
    return () => window.clearTimeout(id);
  }, [error]);

  const openLinking = useCallback(
    (account: AccountId) => {
      setLinking(account);
      setView("chats");
      setSelected(null);
      if (!unified) setActiveAccount(account);
      accounts[account].startLinking().catch((e) => setError(String(e)));
    },
    [accounts, unified, setActiveAccount],
  );

  const closeLinking = useCallback(() => {
    if (linking && !accounts[linking].status.linked) void accounts[linking].cancelLinking();
    setLinking(null);
  }, [accounts, linking]);

  const reconnect = useCallback(
    (account: AccountId) => {
      accounts[account].reconnect().catch((e) => setError(String(e)));
    },
    [accounts],
  );

  /** Opens a chat, switching to its account first when accounts are kept apart. */
  const openChat = useCallback(
    (account: AccountId, id: string, messageId?: string) => {
      closeLinking();
      setView("chats");
      setPanel("chats");
      setFocusMessage(messageId ?? null);
      if (!unified && active !== account) {
        pendingOpen.current = { account, id };
        setActiveAccount(account);
      } else {
        setSelected({ account, id });
      }
    },
    [closeLinking, unified, active, setActiveAccount],
  );

  useEffect(
    () =>
      onNotificationOpen((account, id) =>
        setNoticeTarget({ account, id, open: settingsRef.current.notificationOpensChat }),
      ),
    [],
  );

  // Waits for the chat's list: a window made by the click is still loading
  // it, and the lists changing under it would drop an earlier selection.
  useEffect(() => {
    if (!noticeTarget) return;
    const { account, id, open } = noticeTarget;
    if (!unified && active !== account) {
      setActiveAccount(account);
      return;
    }
    const backend = sources.find((b) => !b.demo && b.account === account);
    if (!backend || !chatsBy.has(backend)) return;
    setNoticeTarget(null);
    closeLinking();
    setView("chats");
    setPanel("chats");
    if (open) {
      setFocusMessage(null);
      setSelected({ account, id });
    } else {
      setHighlight(`${account}:${id}`);
    }
  }, [noticeTarget, unified, active, sources, chatsBy, closeLinking, setActiveAccount]);

  // Pointed out for a while, then faded out.
  useEffect(() => {
    setHighlightFading(false);
    if (!highlight) return;
    const fade = window.setTimeout(() => setHighlightFading(true), HIGHLIGHT_MS - HIGHLIGHT_FADE_MS);
    const end = window.setTimeout(() => setHighlight(null), HIGHLIGHT_MS);
    return () => {
      window.clearTimeout(fade);
      window.clearTimeout(end);
    };
  }, [highlight]);

  // And no longer once the chat is opened.
  useEffect(() => {
    if (selectedKey) setHighlight((current) => (current === selectedKey ? null : current));
  }, [selectedKey]);

  // An opened chat's notification has been answered.
  useEffect(() => {
    if (selected) dismissNotification(selected.account, selected.id);
  }, [selected]);

  useEffect(
    () => syncNotificationPrefs(settings.notifications, settings.notificationPreview),
    [settings.notifications, settings.notificationPreview],
  );

  useEffect(() => syncTrayBadge(settings.trayBadge), [settings.trayBadge]);

  const clearFocus = useCallback(() => setFocusMessage(null), []);

  // The chat's history past what was loaded with it, a page at a time.
  const loadOlder = useCallback(
    async (before: string) => {
      const backend = selectedBackend;
      const chatId = selectedId;
      if (!backend?.olderMessages || !chatId) return false;
      const key = `${backend.account}:${chatId}`;
      const older = await backend.olderMessages(chatId, Date.parse(before));
      if (messagesOf.current !== key || !older.length) return false;
      setMessages((prev) => {
        const known = new Set(prev.map((m) => m.id));
        return [...older.filter((m) => !known.has(m.id)), ...prev];
      });
      return true;
    },
    [selectedBackend, selectedId],
  );

  async function handleSend(text: string, files: File[], replyTo?: string, onFileSent?: () => void, link?: OutgoingLink) {
    const backend = selectedBackend;
    const chatId = selectedId;
    if (!backend || !chatId) return;
    try {
      if (!files.length || !backend.sendFile) {
        const message = await backend.sendText(chatId, text, replyTo, link);
        setMessages((prev) => upsert(prev, message));
        return;
      }
      // The text goes under the first file, like a caption on the phone; the
      // reply quotes from that one too. WhatsApp's audio has no caption, so
      // there it is sent as a message of its own.
      const captionless = backend.account === "whatsapp" && files[0].type.startsWith("audio/");
      for (const [index, file] of files.entries()) {
        const first = index === 0;
        const message = await backend.sendFile(
          chatId,
          file,
          first && !captionless ? text || undefined : undefined,
          first ? replyTo : undefined,
        );
        setMessages((prev) => upsert(prev, message));
        onFileSent?.();
      }
      if (captionless && text) {
        const message = await backend.sendText(chatId, text);
        setMessages((prev) => upsert(prev, message));
      }
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  const loadMedia = useMemo(() => {
    const load = selectedBackend?.mediaUrl;
    return load ? (message: Message, index = 0) => load(message.chatId, message.id, index) : undefined;
  }, [selectedBackend]);

  const openMedia = useMemo(() => {
    const open = selectedBackend?.openMedia;
    return open ? (message: Message, index: number) => open(message.chatId, message.id, index) : undefined;
  }, [selectedBackend]);

  const loadLinkThumbnail = useMemo(() => {
    const load = selectedBackend?.linkThumbnailUrl;
    return load ? (message: Message) => load(message.chatId, message.id) : undefined;
  }, [selectedBackend]);

  const loadGroup = useMemo(() => selectedBackend?.groupInfo?.bind(selectedBackend), [selectedBackend]);
  const loadGroupDetails = useMemo(() => selectedBackend?.groupDetails?.bind(selectedBackend), [selectedBackend]);
  const loadPinned = useMemo(() => selectedBackend?.pinnedMessages?.bind(selectedBackend), [selectedBackend]);
  const pinMessage = useMemo(() => {
    const backend = selectedBackend;
    if (!backend?.pinMessage) return undefined;
    return async (message: Message, secs: number | null) => {
      await backend.pinMessage!(message.chatId, message.id, secs);
      setPinsTick((tick) => tick + 1);
    };
  }, [selectedBackend]);

  const star = useMemo(() => {
    const backend = selectedBackend;
    if (!backend) return undefined;
    return (message: Message) => {
      backend
        .setStarred(message.chatId, message.id, !message.starred)
        .then((updated) => setMessages((prev) => upsert(prev, updated)))
        .catch((e) => setError(String(e)));
    };
  }, [selectedBackend]);

  const deleteMessage = useMemo(() => {
    const backend = selectedBackend;
    if (!backend?.deleteMessage) return undefined;
    return async (message: Message, forEveryone: boolean) => {
      const left = await backend.deleteMessage!(message.chatId, message.id, forEveryone);
      setMessages((prev) => (left ? upsert(prev, left) : prev.filter((m) => m.id !== message.id)));
    };
  }, [selectedBackend]);

  const votePoll = useMemo(() => {
    const backend = selectedBackend;
    if (!backend?.votePoll) return undefined;
    return async (message: Message, options: number[]) => {
      const voted = await backend.votePoll!(message.chatId, message.id, options);
      setMessages((prev) => upsert(prev, voted));
    };
  }, [selectedBackend]);

  const editMessage = useMemo(() => {
    const backend = selectedBackend;
    if (!backend?.editMessage) return undefined;
    return async (message: Message, text: string) => {
      const edited = await backend.editMessage!(message.chatId, message.id, text);
      setMessages((prev) => upsert(prev, edited));
    };
  }, [selectedBackend]);

  const react = useMemo(() => {
    const backend = selectedBackend;
    if (!backend?.react) return undefined;
    return (message: Message, emoji: string) => {
      backend.react!(message.chatId, message.id, emoji)
        .then((updated) => setMessages((prev) => upsert(prev, updated)))
        .catch((e) => setError(String(e)));
    };
  }, [selectedBackend]);

  // What the chat list shows: one account's chats, or every linked account's.
  // The demo chats only fill in while nothing real could be shown instead.
  const shown = useMemo(
    () => sources.filter((backend) => (unified ? !backend.demo || !anyLinked : backend.account === active)),
    [sources, unified, anyLinked, active],
  );
  const chats = shown.flatMap((backend) => {
    const list = chatsBy.get(backend) ?? [];
    if (!typing.size && !presence.size) return list;
    // Only the chats with something to add are new objects, so the other rows stay as drawn.
    return list.map((chat) => {
      const key = `${backend.account}:${chat.id}`;
      const typed = typing.get(key);
      const seen = presence.get(key);
      if (!typed && !seen) return chat;
      return { ...chat, typing: typed, online: seen?.online, lastSeen: seen?.lastSeen };
    });
  });
  if (shown.length > 1) chats.sort(byRecency);
  const showAccount = unified && anyLinked;

  const searchMessages = useCallback(
    (query: string, chatId?: string) =>
      Promise.all(shown.map((backend) => backend.searchMessages(query, chatId))).then((lists) =>
        lists.flat().sort((a, b) => b.message.sentAt.localeCompare(a.message.sentAt)),
      ),
    [shown],
  );

  const selectedChat =
    chats.find((c) => c.account === selected?.account && c.id === selected.id) ??
    (draft && draft.account === selected?.account && draft.id === selected.id ? draft : null);

  // Custom lists are matched by name, so one list can hold chats from every account.
  const shownLists = useMemo(
    () => shown.flatMap((backend) => (listsBy.get(backend) ?? []).map((list) => ({ ...list, backend }))),
    [shown, listsBy],
  );
  const listNames = useMemo(() => [...new Set(shownLists.map((list) => list.name))], [shownLists]);
  const listsOf = useMemo(() => {
    const names = new Map(shownLists.map((list) => [`${list.backend.account}:${list.id}`, list.name]));
    return (chat: Chat) => (chat.lists ?? []).flatMap((id) => names.get(`${chat.account}:${id}`) ?? []);
  }, [shownLists]);

  function toggleUnread(chat: Chat) {
    const backend = backendFor(chat.account);
    if (!backend) return;
    const unread = !(chat.unread > 0 || chat.markedUnread);
    // Shown at once; the list that comes back after says the same.
    setChatsBy((prev) => {
      const list = prev.get(backend);
      if (!list) return prev;
      return new Map(prev).set(
        backend,
        list.map((c) => (c.id === chat.id ? { ...c, markedUnread: unread, unread: unread ? c.unread : 0 } : c)),
      );
    });
    const done = unread ? backend.setUnread?.(chat.id, true) : backend.markRead(chat.id);
    done?.catch((e) => setError(String(e)));
  }

  /** Changes a chat in the list at once; the list that comes back after says the same. */
  function patchChat(backend: ChatBackend, chatId: string, patch: Partial<Chat>) {
    setChatsBy((prev) => {
      const list = prev.get(backend);
      if (!list) return prev;
      return new Map(prev).set(backend, list.map((c) => (c.id === chatId ? { ...c, ...patch } : c)));
    });
  }

  /** Runs a chat change, putting the list back as it was when it fails. */
  function changeChat(chat: Chat, patch: Partial<Chat>, run: (backend: ChatBackend) => Promise<void> | undefined) {
    const backend = backendFor(chat.account);
    const done = backend && run(backend);
    if (!backend || !done) return;
    patchChat(backend, chat.id, patch);
    done.catch((e) => {
      setError(String(e instanceof Error ? e.message : e));
      backend.listChats().then((list) => setChatsBy((prev) => new Map(prev).set(backend, list)), console.error);
    });
  }

  function toggleArchived(chat: Chat) {
    const archived = !chat.archived;
    changeChat(chat, archived ? { archived, pinned: false } : { archived }, (b) => b.setArchived?.(chat.id, archived));
  }

  function togglePinned(chat: Chat) {
    changeChat(chat, { pinned: !chat.pinned }, (b) => b.setPinned?.(chat.id, !chat.pinned));
  }

  /** Unmutes a chat, or asks for how long to mute it. Settles once it is done, failing when cancelled. */
  function toggleMuted(chat: Pick<Chat, "account" | "id" | "muted">): Promise<void> {
    if (chat.muted) {
      return new Promise((resolve, reject) => {
        changeChat(chat as Chat, { muted: false }, (b) => {
          const done = b.setMuted?.(chat.id, false);
          done?.then(resolve, reject);
          return done;
        });
      });
    }
    return new Promise((resolve, reject) => setMuting({ chat, resolve, reject }));
  }

  /** Where a chat's messages go when it is cleared or deleted, as the dialog says it. */
  function reachOf(account: AccountId) {
    return account === "whatsapp" ? "this computer and your phone" : "this computer only; your phone keeps them";
  }

  function askClearChat(chat: Pick<Chat, "account" | "id">) {
    const backend = backendFor(chat.account);
    if (!backend?.clearChat) return;
    setConfirming({
      title: "Clear this chat?",
      body: `The chat stays in your list, empty. Its messages are removed from ${reachOf(chat.account)}.`,
      option: { label: "Keep starred messages", initial: true },
      confirmLabel: "Clear chat",
      danger: true,
      onConfirm: async (keepStarred) => {
        await backend.clearChat!(chat.id, keepStarred);
        patchChat(backend, chat.id, { lastMessage: "", unread: 0, markedUnread: false });
      },
    });
  }

  function askDeleteChat(chat: Pick<Chat, "account" | "id" | "isGroup">) {
    const backend = backendFor(chat.account);
    if (!backend?.deleteChat) return;
    setConfirming({
      title: "Delete this chat?",
      body: `Its messages are deleted from ${reachOf(chat.account)}.${chat.isGroup ? " You stay in the group." : ""}`,
      confirmLabel: "Delete chat",
      danger: true,
      onConfirm: async () => {
        await backend.deleteChat!(chat.id);
        setChatsBy((prev) => {
          const list = prev.get(backend);
          return list ? new Map(prev).set(backend, list.filter((c) => c.id !== chat.id)) : prev;
        });
        if (selectedRef.current?.account === chat.account && selectedRef.current.id === chat.id) setSelected(null);
      },
    });
  }

  function toggleFavorite(chat: Chat) {
    backendFor(chat.account)
      ?.setFavorite(chat.id, !chat.favorite)
      .catch((e) => setError(String(e)));
  }

  /** Saves a list under `name`, on every account that has chats in it. */
  async function saveList(oldName: string | undefined, name: string, before: Set<string>, after: Set<string>) {
    const plan = shown.map((backend) => {
      const mine = (keys: Set<string>) =>
        [...keys].filter((k) => k.startsWith(`${backend.account}:`)).map((k) => k.slice(backend.account.length + 1));
      const [was, now] = [mine(before), mine(after)];
      return {
        backend,
        existing: (listsBy.get(backend) ?? []).find((l) => l.name === (oldName ?? name)),
        add: now.filter((id) => !was.includes(id)),
        remove: was.filter((id) => !now.includes(id)),
      };
    });
    let work = plan.filter((p) => p.existing || p.add.length > 0);
    // An empty new list still has to live somewhere.
    if (work.length === 0) work = plan.slice(0, 1);
    for (const { backend, existing, add, remove } of work) {
      await backend.saveList(existing?.id ?? null, name, add, remove);
    }
  }

  async function deleteList(name: string) {
    for (const { backend, id } of shownLists.filter((l) => l.name === name)) {
      await backend.deleteList(id);
    }
  }

  function toggleList(chat: Chat, name: string) {
    const key = new Set([chatKey(chat)]);
    const inside = listsOf(chat).includes(name);
    saveList(name, name, inside ? key : new Set(), inside ? new Set() : key).catch((e) => setError(String(e)));
  }

  function editList(name?: string, withChat?: Chat) {
    const selected = new Set(chats.filter((c) => name !== undefined && listsOf(c).includes(name)).map(chatKey));
    if (withChat) selected.add(chatKey(withChat));
    setEditor({ name, selected });
  }

  // Contacts and starred messages load each time their panel opens, and
  // starred ones again when anything changes.
  useEffect(() => {
    if (panel !== "new-chat") return;
    let alive = true;
    setContacts(null);
    Promise.all(shown.map((backend) => backend.listContacts()))
      .then((lists) => alive && setContacts(lists.flat()))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [panel, shown]);

  // Our own entry goes by the name on our profile.
  const namedContacts = contacts?.map((contact) => {
    const me = contact.isMe ? accounts[contact.account].status.me?.name : undefined;
    return me ? { ...contact, name: me, initials: initialsFor(me) } : contact;
  }) ?? null;

  useEffect(() => {
    if (panel !== "starred") return;
    let alive = true;
    Promise.all(shown.map((backend) => backend.listStarred()))
      .then((lists) => {
        if (!alive) return;
        setStarred(lists.flat().sort((a, b) => b.message.sentAt.localeCompare(a.message.sentAt)));
      })
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [panel, shown, chatsBy, messages]);

  const loadPerson = useMemo(
    () => (selectedBackend?.person ? (id: string) => selectedBackend.person!(id) : undefined),
    [selectedBackend],
  );
  const loadPersonDetails = useMemo(
    () => (selectedBackend?.personDetails ? (id: string) => selectedBackend.personDetails!(id) : undefined),
    [selectedBackend],
  );

  function openContact(contact: Contact, messageId?: string) {
    const known = chats.find((c) => c.account === contact.account && c.id === contact.id);
    if (!known) {
      setDraft({
        id: contact.id,
        account: contact.account,
        name: contact.isMe && contact.name !== "You" ? `${contact.name} (You)` : contact.name,
        initials: contact.initials,
        avatarHue: contact.avatarHue,
        avatarUrl: contact.avatarUrl,
        lastMessage: "",
        lastAt: new Date().toISOString(),
        unread: 0,
      });
    }
    openChat(contact.account, contact.id, messageId);
  }

  // Phone numbers can be looked up on WhatsApp, once it is linked.
  const numberBackend = shown.find((b) => b.lookupNumber && !b.demo);
  const lookupNumber = numberBackend?.lookupNumber
    ? async (phone: string): Promise<Contact | null> => {
        const id = await numberBackend.lookupNumber!(phone);
        if (!id) return null;
        const saved = namedContacts?.find((c) => c.account === numberBackend.account && c.id === id);
        const name = saved?.name ?? `+${phone}`;
        return saved ?? { id, account: numberBackend.account, name, initials: initialsFor(name), avatarHue: hueFor(id) };
      }
    : undefined;
  /** Opens a chat with a number from a contact card, named as the card names it unless it is saved. */
  async function messageNumber(waId: string, name: string) {
    try {
      const found = await lookupNumber!(waId);
      if (!found) return setError(`${name} is not on WhatsApp.`);
      const saved = namedContacts?.some((c) => c.account === found.account && c.id === found.id);
      openContact(saved ? found : { ...found, name, initials: initialsFor(name) });
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  }

  const unreadOf = (backend: ChatBackend | null) =>
    (backend ? (chatsBy.get(backend) ?? []) : []).reduce((sum, c) => sum + (c.muted || c.archived ? 0 : Math.max(c.unread, c.markedUnread ? 1 : 0)), 0);
  const rail: RailAccount[] = ACCOUNT_IDS.map((id) => {
    const { status } = accounts[id];
    const backend = backendFor(id);
    return {
      id,
      unread: unified && backend?.demo && anyLinked ? 0 : unreadOf(backend),
      connection: status.linked
        ? status.state === "connected"
          ? "online"
          : "offline"
        : backend?.demo
          ? "demo"
          : "unlinked",
    };
  });
  const canSend =
    Boolean(selectedBackend?.demo) ||
    (selected !== null && accounts[selected.account].status.state === "connected");

  // A message can go on to any shown chat whose account can send right now.
  const forwardTargets = chats.filter(
    (chat) => backendFor(chat.account)?.demo || accounts[chat.account].status.state === "connected",
  );

  /** Sends messages, oldest first, on to each of `targets`, then `note` as a message of our own. */
  async function forward(messages: Message[], targets: Chat[], note: string) {
    // Attachments are fetched once and uploaded again to each chat.
    const load = selectedBackend?.mediaUrl;
    const parts = await Promise.all(
      messages.map(async (message) => {
        let files: File[] = [];
        if (message.hasMedia || message.attachments?.length) {
          if (!load) throw new Error("The attachment can't be read here.");
          const count = message.attachments?.length || 1;
          files = await Promise.all(
            Array.from({ length: count }, async (_, index) => {
              const response = await fetch(await load(message.chatId, message.id, index));
              if (!response.ok) throw new Error("The attachment could not be read.");
              const blob = await response.blob();
              const attachment = message.attachments?.[index];
              const type = attachment?.mime || blob.type || "application/octet-stream";
              const name = attachment?.fileName || message.mediaLabel?.match(/[^/\\]+\.\w{1,8}$/)?.[0] || `forwarded.${type.split("/")[1]?.split(/[;+]/)[0] || "bin"}`;
              return new File([blob], name, { type });
            }),
          );
        }
        return { text: message.text, files, score: (message.forwarded ?? 0) + 1 };
      }),
    );
    const failed: string[] = [];
    for (const target of targets) {
      const backend = backendFor(target.account);
      try {
        if (!backend) throw new Error("not linked");
        const sent: Message[] = [];
        for (const { text, files, score } of parts) {
          if (files.length) {
            if (!backend.sendFile) throw new Error("no files here");
            for (const [index, file] of files.entries()) {
              sent.push(await backend.sendFile(target.id, file, index === 0 ? text || undefined : undefined, undefined, score));
            }
          } else if (text) {
            sent.push(await backend.sendText(target.id, text, undefined, undefined, score));
          }
        }
        if (note) sent.push(await backend.sendText(target.id, note));
        const open = selectedRef.current;
        if (open?.account === target.account && open.id === target.id) {
          setMessages((prev) => sent.reduce((list, one) => upsert(list, one), prev));
        }
      } catch {
        failed.push(target.name);
      }
    }
    const what = messages.length === 1 ? "The message was" : "The messages were";
    if (failed.length) throw new Error(`${what} not sent to ${failed.join(", ")}.`);
  }

  function subtitleFor(chat: Chat) {
    const phone = chat.id.match(/^(\d+)@s\.whatsapp\.net$/)?.[1];
    const detail = chat.typing
      ? chat.typing
      : chat.isGroup
        ? "Group"
        : chat.online
          ? "online"
          : chat.lastSeen
            ? formatLastSeen(chat.lastSeen)
            : phone
              ? `+${phone}`
              : "";
    return [showAccount ? ACCOUNTS[chat.account].label : "", detail].filter(Boolean).join(" · ");
  }

  // Accounts whose state the chat list should explain.
  const bannerAccounts = unified
    ? ACCOUNT_IDS.filter((id) => (anyLinked ? accounts[id].status.linked : id === "whatsapp"))
    : [active];
  const banners = bannerAccounts.map((id) =>
    accountBanner(accounts[id], () => openLinking(id), () => reconnect(id)),
  );
  // In its own environment an unlinked account has nothing else to show.
  const focus = accounts[unified ? "whatsapp" : active];
  const canLink = focus.status.state !== "unsupported";

  // Keyboard shortcuts. The handler is swapped each render so it sees the
  // chat open now; the listener stays.
  const onShortcut = useRef<(event: KeyboardEvent) => void>(() => {});
  onShortcut.current = (event: KeyboardEvent) => {
    if (event.defaultPrevented || event.isComposing) return;
    const chat = selectedChat;
    const nth = chatNumber(event);
    if (nth !== null) {
      event.preventDefault();
      return openListed((rows) => rows[nth]);
    }
    const id = shortcutFor(event);
    if (!id) return;
    // Something open over the app answers its own keys.
    const covered = document.querySelector('[aria-modal="true"], [role="menu"], [role="listbox"][aria-label="Emoji suggestions"], [role="dialog"][aria-label="Emoji"]');
    if (covered && id !== "help") return;
    switch (id) {
      case "newChat":
        closeLinking();
        setView("chats");
        setPanel("new-chat");
        break;
      case "search":
        closeLinking();
        setView("chats");
        setPanel("chats");
        requestAnimationFrame(() => {
          const box = document.getElementById("chat-search") as HTMLInputElement | null;
          box?.focus();
          box?.select();
        });
        break;
      case "nextChat":
      case "previousChat": {
        const step = id === "nextChat" ? 1 : -1;
        openListed((rows, at) => rows[at === -1 ? (step === 1 ? 0 : rows.length - 1) : at + step]);
        break;
      }
      case "closeChat": {
        // Escape still clears a box with something typed in it.
        const target = event.target as HTMLElement;
        const typed = target instanceof HTMLInputElement ? target.value : target.isContentEditable ? target.textContent : "";
        if (!selected || typed) return;
        setSelected(null);
        break;
      }
      case "markUnread":
        if (!chat) return;
        toggleUnread(chat);
        break;
      case "mute":
        if (!chat) return;
        void toggleMuted(chat).catch(() => {});
        break;
      case "archive":
        if (!chat) return;
        toggleArchived(chat);
        break;
      case "pin":
        if (!chat) return;
        togglePinned(chat);
        break;
      case "settings":
        closeLinking();
        setView("settings");
        break;
      case "help":
        setShortcutsOpen((open) => !open);
        break;
      case "emoji":
        // The message box opens its own panel.
        return;
    }
    event.preventDefault();
  };
  useEffect(() => {
    const listener = (event: KeyboardEvent) => onShortcut.current(event);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);

  /** Opens a chat from the list as it shows now (filtered, searched), picked from its rows and the open one's place. */
  function openListed(pick: (rows: HTMLElement[], at: number) => HTMLElement | undefined) {
    const rows = [...document.querySelectorAll<HTMLElement>('[aria-label="Chats"] [role="option"]')];
    const row = pick(rows, rows.findIndex((r) => r.getAttribute("aria-selected") === "true"));
    if (!row) return;
    row.click();
    row.scrollIntoView({ block: "nearest" });
  }

  let main: ReactNode;
  if (linking && !accounts[linking].status.linked) {
    main = (
      <LinkScreen
        account={ACCOUNTS[linking]}
        status={accounts[linking].status}
        onRetry={() => reconnect(linking)}
        onClose={closeLinking}
      />
    );
  } else if (selectedChat) {
    main = (
      <ChatView
        chat={selectedChat}
        messages={messages}
        subtitle={subtitleFor(selectedChat)}
        showAccount={showAccount}
        canSend={canSend}
        cannotSendReason={`Waiting for ${ACCOUNTS[selectedChat.account].label} to connect…`}
        onSend={handleSend}
        canAttach={Boolean(selectedBackend?.sendFile)}
        onError={setError}
        loadMedia={loadMedia}
        openMedia={openMedia}
        loadLinkThumbnail={loadLinkThumbnail}
        onReact={canSend ? react : undefined}
        // Signal's stars stay on this computer; WhatsApp's go to the phone.
        onStar={canSend || selectedChat.account === "signal" ? star : undefined}
        onForward={forwardTargets.length > 0 ? setForwarding : undefined}
        // Signal deletes for us on this computer only; WhatsApp tells the phone.
        onDelete={canSend || selectedChat.account === "signal" ? deleteMessage : undefined}
        focusMessageId={focusMessage}
        onLoadOlder={selectedBackend?.olderMessages ? loadOlder : undefined}
        historyTick={historyTick}
        searchMessages={searchMessages}
        onEdit={canSend ? editMessage : undefined}
        onVote={canSend ? votePoll : undefined}
        onMessageNumber={lookupNumber ? (waId, name) => void messageNumber(waId, name) : undefined}
        loadPinned={loadPinned}
        onPin={canSend ? pinMessage : undefined}
        pinsTick={pinsTick}
        loadGroup={loadGroup}
        loadGroupDetails={loadGroupDetails}
        onFocused={clearFocus}
        loadPerson={loadPerson}
        loadPersonDetails={loadPersonDetails}
        onMessagePerson={(person, messageId) => openContact(person, messageId)}
        onOpenChatId={(id) => openChat(selectedChat.account, id)}
        onStarred={() => setPanel("starred")}
        onTyping={(key, state) => {
          if (!settings.sendTyping) return;
          const at = key.indexOf(":");
          backendFor(key.slice(0, at) as AccountId)
            ?.setTyping?.(key.slice(at + 1), state)
            .catch(() => {});
        }}
        onMuteChat={
          selectedBackend?.setMuted
            ? (chatId, muted) => toggleMuted({ account: selectedChat.account, id: chatId, muted: !muted })
            : undefined
        }
      />
    );
  } else if (unified ? anyLinked : focus.status.linked) {
    main = <EmptyState text="Send and receive messages from this computer. Pick a chat on the left to start." />;
  } else {
    const { label } = ACCOUNTS[focus.id];
    main = (
      <EmptyState
        text={
          focus.id === "whatsapp"
            ? "You're looking at demo chats. Link your phone to read and send your real WhatsApp messages from this computer."
            : `Link your phone to read and send your ${label} messages from this computer.`
        }
        action={canLink ? { label: `Link ${label}`, onClick: () => openLinking(focus.id) } : undefined}
      />
    );
  }

  return (
    <div className="flex h-full overflow-hidden">
      <NavRail
        view={view}
        onChange={setView}
        accounts={rail}
        active={unified ? null : active}
        profiles={(unified ? ACCOUNT_IDS : [active])
          .map((id) => accounts[id])
          .filter((account) => account.status.linked && account.status.me !== null)}
        onSelectAccount={(account) => {
          setView("chats");
          closeLinking();
          setActiveAccount(account);
        }}
      />

      <aside
        ref={sidebarRef}
        className="h-full shrink-0 overflow-hidden contain-strict"
        style={{ width: sidebarWidth }}
        aria-label={view === "chats" ? "Chat list" : "Settings"}
      >
        {view === "chats" && panel === "new-chat" ? (
          <NewChatPanel
            contacts={namedContacts}
            showAccount={showAccount}
            lookupNumber={lookupNumber}
            lookupLabel={numberBackend ? ACCOUNTS[numberBackend.account].label : undefined}
            onBack={() => setPanel("chats")}
            onOpen={openContact}
          />
        ) : view === "chats" && panel === "starred" ? (
          <StarredPanel
            items={starred}
            showAccount={showAccount}
            onBack={() => setPanel("chats")}
            onOpen={(item) => openChat(item.account, item.message.chatId, item.message.id)}
            onUnstar={(item) =>
              backendFor(item.account)
                ?.setStarred(item.message.chatId, item.message.id, false)
                .then((updated) => {
                  setStarred((prev) => prev?.filter((s) => !(s.account === item.account && s.message.id === updated.id)) ?? null);
                  setMessages((prev) => (prev.some((m) => m.id === updated.id) ? upsert(prev, updated) : prev));
                })
                .catch((e) => setError(String(e)))
            }
          />
        ) : view === "chats" ? (
          <Sidebar
            title={unified ? "Chats" : ACCOUNTS[active].label}
            chats={chats}
            selected={selectedChat}
            onSelect={(chat) => {
              closeLinking();
              setFocusMessage(null);
              setSelected({ account: chat.account, id: chat.id });
            }}
            onPeek={peek}
            highlighted={highlight}
            highlightFading={highlightFading}
            showAccount={showAccount}
            banner={banners}
            footer={bannerAccounts.map((id) => {
              const backfill = accounts[id].status.backfill;
              return backfill ? <HistoryProgress key={id} label={ACCOUNTS[id].label} backfill={backfill} /> : null;
            })}
            emptyText={
              (unified ? anyLinked : focus.status.linked)
                ? unified
                  ? "Your chats will appear here once they sync."
                  : ACCOUNTS[active].emptyText
                : "No chats yet"
            }
            lists={listNames}
            listsOf={listsOf}
            onNewChat={() => setPanel("new-chat")}
            onStarred={() => {
              setStarred(null);
              setPanel("starred");
            }}
            onSettings={() => setView("settings")}
            onShortcuts={() => setShortcutsOpen(true)}
            onToggleFavorite={toggleFavorite}
            onToggleUnread={toggleUnread}
            onToggleList={toggleList}
            onToggleArchived={toggleArchived}
            onTogglePinned={togglePinned}
            onToggleMuted={(chat) => void toggleMuted(chat).catch(() => {})}
            onClearChat={askClearChat}
            onDeleteChat={askDeleteChat}
            onNewList={(chat) => editList(undefined, chat)}
            onEditList={(name) => editList(name)}
            searchMessages={searchMessages}
            onOpenMessage={(item) => openChat(item.account, item.message.chatId, item.message.id)}
          />
        ) : (
          <SettingsPanel
            accounts={ACCOUNT_IDS.map((id) => accounts[id])}
            onBack={() => setView("chats")}
            onLink={openLinking}
            privacy={
              accounts.whatsapp.status.state === "connected" && real.whatsapp.privacy && real.whatsapp.setPrivacy
                ? whatsappPrivacy
                : undefined
            }
          />
        )}
      </aside>

      <ResizeHandle
        target={sidebarRef}
        width={sidebarWidth}
        min={SIDEBAR_MIN}
        max={maxSidebar}
        onResize={setSidebarWidth}
        onReset={() => setSidebarWidth(SIDEBAR_DEFAULT)}
      />

      <main className="flex h-full min-w-0 flex-1 contain-strict">{main}</main>

      <ZoomToast zoom={zoomAnnounce} />

      {editor ? (
        <ListEditor
          name={editor.name}
          selected={editor.selected}
          chats={chats}
          showAccount={showAccount}
          onSave={(name, after) => saveList(editor.name, name, editor.selected, after)}
          onDelete={editor.name !== undefined ? () => deleteList(editor.name!) : undefined}
          onClose={() => setEditor(null)}
        />
      ) : null}

      {muting ? (
        <MuteDialog
          onMute={async (untilMs) => {
            const backend = backendFor(muting.chat.account);
            if (!backend?.setMuted) throw new Error("This chat can't be muted.");
            await backend.setMuted(muting.chat.id, true, untilMs);
            patchChat(backend, muting.chat.id, { muted: true });
            muting.resolve();
          }}
          onClose={() => {
            // Closed without muting: whoever asked puts its switch back.
            muting.reject(new Error("cancelled"));
            setMuting(null);
          }}
        />
      ) : null}
      {confirming ? <ConfirmDialog {...confirming} onClose={() => setConfirming(null)} /> : null}
      {shortcutsOpen ? <ShortcutsDialog onClose={() => setShortcutsOpen(false)} /> : null}
      {forwarding ? (
        <ForwardDialog
          messages={forwarding}
          chats={forwardTargets}
          showAccount={showAccount}
          onForward={(targets, note) => forward(forwarding, targets, note)}
          onClose={() => setForwarding(null)}
        />
      ) : null}

      {error ? (
        <div
          role="alert"
          className="fixed bottom-24 left-1/2 z-50 max-w-120 -translate-x-1/2 rounded-lg bg-(--overlay) px-4 py-2.5 text-[14px] text-white animate-fade-rise"
        >
          {error}
        </div>
      ) : null}
    </div>
  );
}

export default function App() {
  return (
    <SettingsProvider>
      <Shell />
    </SettingsProvider>
  );
}
