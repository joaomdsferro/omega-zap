import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ACCOUNTS, ACCOUNT_IDS, type AccountId } from "./accounts/accounts";
import { useAccount, type Account } from "./accounts/useAccount";
import {
  MockBackend,
  TauriBackend,
  type Chat,
  type ChatBackend,
  type Contact,
  type CustomList,
  type Message,
  type StarredMessage,
} from "./backend";
import { ChatView } from "./components/ChatView";
import { EmptyState } from "./components/EmptyState";
import { ForwardDialog } from "./components/ForwardDialog";
import { LinkScreen } from "./components/LinkScreen";
import { ListEditor, chatKey } from "./components/ListEditor";
import { NavRail, type RailAccount, type SidebarView } from "./components/NavRail";
import { NewChatPanel } from "./components/NewChatPanel";
import { ResizeHandle } from "./components/ResizeHandle";
import { SettingsPanel } from "./components/SettingsPanel";
import { Sidebar } from "./components/Sidebar";
import { StarredPanel } from "./components/StarredPanel";
import { ZoomToast } from "./components/ZoomToast";
import { hueFor, initialsFor } from "./lib/avatar";
import { onNotificationOpen, showNotification } from "./lib/notify";
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
  /** The messages of recently opened chats, most recent last, by `account:id`. */
  const messageCache = useRef(new Map<string, Message[]>());
  /** The chat whose messages `messages` holds. */
  const messagesOf = useRef<string | null>(null);
  const [forwarding, setForwarding] = useState<Message | null>(null);
  const [error, setError] = useState<string | null>(null);

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
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  /** A chat to open once the account it is on has been switched to. */
  const pendingOpen = useRef<{ account: AccountId; id: string } | null>(null);
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
        } else if (open?.account === backend.account && open.id === event.message.chatId) {
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

  const backendFor = useCallback(
    (account: AccountId) => sources.find((b) => b.account === account) ?? null,
    [sources],
  );
  const selectedBackend = selected ? backendFor(selected.account) : null;
  const selectedId = selected?.id ?? null;

  // Keeps the cache up to date with what arrives in the open chat.
  useEffect(() => {
    const key = messagesOf.current;
    if (!key) return;
    const cache = messageCache.current;
    cache.delete(key);
    cache.set(key, messages);
    if (cache.size > CACHED_CHATS) cache.delete(cache.keys().next().value!);
  }, [messages]);

  useEffect(() => {
    if (!selectedBackend || !selectedId) {
      messagesOf.current = null;
      setMessages([]);
      return;
    }
    const key = `${selectedBackend.account}:${selectedId}`;
    const started = switchStarted.current;
    // The chat as it was last seen, at once, or nothing: never the chat
    // that was open before, drawn under this one's name.
    const cached = messageCache.current.get(key);
    messagesOf.current = key;
    setMessages(cached ?? []);
    if (import.meta.env.DEV && cached) logSwitch(key, started, "cached");
    let alive = true;
    selectedBackend
      .getMessages(selectedId)
      .then((list) => {
        if (!alive) return;
        setMessages(list);
        if (import.meta.env.DEV && !cached) logSwitch(key, started, "loaded");
      })
      .catch(console.error);
    void selectedBackend.markRead(selectedId);
    setChatsBy((prev) => {
      const list = prev.get(selectedBackend);
      if (!list?.some((c) => c.id === selectedId && c.unread > 0)) return prev;
      return new Map(prev).set(
        selectedBackend,
        list.map((c) => (c.id === selectedId ? { ...c, unread: 0 } : c)),
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

  useEffect(() => onNotificationOpen((account, chatId) => openChat(account, chatId)), [openChat]);

  const clearFocus = useCallback(() => setFocusMessage(null), []);

  async function handleSend(text: string, files: File[], replyTo?: string, onFileSent?: () => void) {
    const backend = selectedBackend;
    const chatId = selectedId;
    if (!backend || !chatId) return;
    try {
      if (!files.length || !backend.sendFile) {
        const message = await backend.sendText(chatId, text, replyTo);
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
  const chats = shown.flatMap((backend) => chatsBy.get(backend) ?? []);
  if (shown.length > 1) chats.sort(byRecency);
  const showAccount = unified && anyLinked;

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

  function openContact(contact: Contact) {
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
    openChat(contact.account, contact.id);
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
  const unreadOf = (backend: ChatBackend | null) =>
    (backend ? (chatsBy.get(backend) ?? []) : []).reduce((sum, c) => sum + (c.muted ? 0 : c.unread), 0);
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

  async function forward(message: Message, targets: Chat[]) {
    const text = message.text;
    if (!text) return;
    const failed: string[] = [];
    for (const target of targets) {
      const backend = backendFor(target.account);
      try {
        if (!backend) throw new Error("not linked");
        const sent = await backend.sendText(target.id, text);
        const open = selectedRef.current;
        if (open?.account === target.account && open.id === target.id) setMessages((prev) => upsert(prev, sent));
      } catch {
        failed.push(target.name);
      }
    }
    if (failed.length) throw new Error(`The message was not sent to ${failed.join(", ")}.`);
  }

  function subtitleFor(chat: Chat) {
    const phone = chat.id.match(/^(\d+)@s\.whatsapp\.net$/)?.[1];
    const detail = chat.isGroup ? "Group" : chat.online ? "online" : phone ? `+${phone}` : "";
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
        onFocused={clearFocus}
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
            showAccount={showAccount}
            banner={banners}
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
            onToggleFavorite={toggleFavorite}
            onToggleList={toggleList}
            onNewList={(chat) => editList(undefined, chat)}
            onEditList={(name) => editList(name)}
          />
        ) : (
          <SettingsPanel
            accounts={ACCOUNT_IDS.map((id) => accounts[id])}
            onBack={() => setView("chats")}
            onLink={openLinking}
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

      {forwarding ? (
        <ForwardDialog
          message={forwarding}
          chats={forwardTargets}
          showAccount={showAccount}
          onForward={(targets) => forward(forwarding, targets)}
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
