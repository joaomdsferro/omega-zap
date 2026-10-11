import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AccountId } from "../accounts/accounts";
import { COMMAND_PREFIX } from "../accounts/useAccount";
import { hueFor, initialsFor } from "../lib/avatar";
import { fileUrl } from "../lib/files";
import { signalBody } from "../lib/markup";
import { MAX_SEND_BYTES, base64Json, fileDetails } from "../lib/outgoing";
import type {
  Pins,
  Place,
  Poll,
  ContactCard,
  Attachment,
  BackendEvent,
  Chat,
  ChatBackend,
  Contact,
  CustomList,
  DeliveryStatus,
  Mention,
  Message,
  MessageKind,
  OutgoingLink,
  GroupInfo,
  GroupRef,
  Person,
  PersonDetails,
  Notice,
  Privacy,
  PrivacyCategory,
  ReactionCount,
  StarredMessage,
  TypingState,
} from "./types";

interface RawChat {
  id: string;
  name: string;
  lastMessage: string;
  lastAt: number;
  lastFromMe: boolean;
  lastStatus: DeliveryStatus | null;
  unread: number;
  markedUnread: boolean;
  pinned: boolean;
  muted: boolean;
  archived: boolean;
  isGroup: boolean;
  avatar: string | null;
  favorite: boolean;
  lists: string[];
  blocked: boolean;
}

interface RawContact {
  id: string;
  name: string;
  phone: string | null;
  avatar: string | null;
  isMe: boolean;
}

interface RawNotice {
  chatId: string;
  title: string;
  body: string;
  muted: boolean;
  avatar: string | null;
}

interface RawMessage {
  id: string;
  chatId: string;
  kind: MessageKind;
  sentAt: number;
  fromMe: boolean;
  author: string | null;
  text: string | null;
  imageLabel: string | null;
  mediaLabel: string | null;
  voiceSeconds: number | null;
  replyTo: { author: string; fromMe: boolean; id: string | null; text: string } | null;
  mentions: Mention[];
  authorId: string | null;
  status: DeliveryStatus | null;
  hasMedia: boolean;
  attachments: { kind: Attachment["kind"]; mime: string; fileName: string | null; size: number; width: number | null; height: number | null }[];
  mediaWidth: number | null;
  mediaHeight: number | null;
  forwarded: number;
  linkPreview: { url: string; title: string | null; description: string | null; thumbnail: string | null; hasLarge: boolean } | null;
  reactions: ReactionCount[];
  starred: boolean;
  deleted: boolean;
  edited?: boolean;
  place?: Place;
  contacts?: ContactCard[];
  poll?: Poll;
}

interface RawGroupRef {
  id: string;
  name: string;
  avatar: string | null;
  members: number | null;
}

interface RawGroupInfo {
  id: string;
  name: string;
  avatar: string | null;
  members: number | null;
  description: string | null;
  createdMs: number | null;
  createdBy: string | null;
  muted: boolean;
  starred: number;
  mediaCount: number;
  recentMedia: RawMessage[];
  participants: { id: string; name: string; phone: string | null; avatar: string | null; isMe: boolean; admin: boolean; about: string | null }[];
  isAdmin: boolean;
}

function toGroupInfo(raw: RawGroupInfo, account: AccountId): GroupInfo {
  return {
    id: raw.id,
    account,
    name: raw.name,
    avatarUrl: raw.avatar ? fileUrl(raw.avatar, "avatar") : undefined,
    avatarHue: hueFor(raw.id),
    initials: initialsFor(raw.name),
    members: raw.members ?? undefined,
    description: raw.description ?? undefined,
    createdAt: raw.createdMs ? new Date(raw.createdMs).toISOString() : undefined,
    createdBy: raw.createdBy ?? undefined,
    muted: raw.muted,
    starred: raw.starred,
    mediaCount: raw.mediaCount,
    recentMedia: raw.recentMedia.map(toMessage),
    participants: raw.participants.map((m) => ({
      id: m.id,
      name: m.name,
      phone: m.phone ?? undefined,
      avatarUrl: m.avatar ? fileUrl(m.avatar, "avatar") : undefined,
      avatarHue: hueFor(m.id),
      initials: initialsFor(m.name),
      isMe: m.isMe,
      admin: m.admin,
      about: m.about ?? undefined,
    })),
    isAdmin: raw.isAdmin,
  };
}

function toGroupRef(g: RawGroupRef): GroupRef {
  return {
    id: g.id,
    name: g.name,
    avatarUrl: g.avatar ? fileUrl(g.avatar, "avatar") : undefined,
    avatarHue: hueFor(g.id),
    initials: initialsFor(g.name),
    members: g.members ?? undefined,
  };
}

function toChat(raw: RawChat, account: AccountId): Chat {
  return {
    id: raw.id,
    account,
    name: raw.name,
    initials: initialsFor(raw.name),
    avatarHue: hueFor(raw.id),
    avatarUrl: raw.avatar ? fileUrl(raw.avatar, "avatar") : undefined,
    lastMessage: raw.lastMessage,
    lastAt: new Date(raw.lastAt).toISOString(),
    lastFromMe: raw.lastFromMe,
    lastStatus: raw.lastStatus ?? undefined,
    unread: raw.unread,
    markedUnread: raw.markedUnread || undefined,
    pinned: raw.pinned,
    muted: raw.muted,
    archived: raw.archived || undefined,
    isGroup: raw.isGroup,
    favorite: raw.favorite,
    lists: raw.lists,
    blocked: raw.blocked || undefined,
  };
}

function toMessage(raw: RawMessage): Message {
  return {
    id: raw.id,
    chatId: raw.chatId,
    kind: raw.kind,
    sentAt: new Date(raw.sentAt).toISOString(),
    fromMe: raw.fromMe,
    author: raw.author ?? undefined,
    text: raw.text ?? undefined,
    imageLabel: raw.imageLabel ?? undefined,
    mediaLabel: raw.mediaLabel ?? undefined,
    voiceSeconds: raw.voiceSeconds ?? undefined,
    mentions: raw.mentions?.length ? raw.mentions : undefined,
    authorId: raw.authorId ?? undefined,
    replyTo: raw.replyTo ? { author: raw.replyTo.author, fromMe: raw.replyTo.fromMe, id: raw.replyTo.id ?? undefined, text: raw.replyTo.text } : undefined,
    status: raw.status ?? undefined,
    hasMedia: raw.hasMedia,
    attachments: raw.attachments.length
      ? raw.attachments.map((a) => ({
          kind: a.kind,
          mime: a.mime,
          fileName: a.fileName ?? undefined,
          size: a.size,
          width: a.width ?? undefined,
          height: a.height ?? undefined,
        }))
      : undefined,
    mediaWidth: raw.mediaWidth ?? undefined,
    mediaHeight: raw.mediaHeight ?? undefined,
    linkPreview: raw.linkPreview
      ? {
          url: raw.linkPreview.url,
          title: raw.linkPreview.title ?? undefined,
          description: raw.linkPreview.description ?? undefined,
          thumbnail: raw.linkPreview.thumbnail ?? undefined,
          hasLarge: raw.linkPreview.hasLarge,
        }
      : undefined,
    forwarded: raw.forwarded || undefined,
    reactions: raw.reactions,
    starred: raw.starred,
    deleted: raw.deleted,
    edited: raw.edited,
    place: raw.place,
    contacts: raw.contacts,
    poll: raw.poll,
  };
}

/** Real chats, served by the Rust side from a linked account's session. */
export class TauriBackend implements ChatBackend {
  readonly demo = false;
  private readonly prefix: string;

  mediaUrl?: (chatId: string, messageId: string, index?: number) => Promise<string>;
  openMedia?: (chatId: string, messageId: string, index: number) => Promise<void>;
  linkThumbnailUrl?: (chatId: string, messageId: string) => Promise<string>;
  lookupNumber?: (phone: string) => Promise<string | null>;
  pinnedMessages?: (chatId: string) => Promise<Pins>;
  pinMessage?: (chatId: string, messageId: string, secs: number | null) => Promise<void>;
  groupInfo?: (id: string) => Promise<GroupInfo>;
  editMessage?: (chatId: string, messageId: string, text: string) => Promise<Message>;
  votePoll?: (chatId: string, messageId: string, options: number[]) => Promise<Message>;
  groupDetails?: (id: string) => Promise<GroupInfo>;
  setTyping?: (chatId: string, state: TypingState) => Promise<void>;
  watchPresence?: (chatId: string) => Promise<void>;
  privacy?: () => Promise<Privacy>;
  setPrivacy?: (category: PrivacyCategory, value: string) => Promise<void>;
  setBlocked?: (id: string, blocked: boolean) => Promise<void>;

  constructor(readonly account: AccountId) {
    this.prefix = COMMAND_PREFIX[account];
    this.mediaUrl = async (chatId, messageId, index = 0) =>
      fileUrl(await invoke<string>(`${this.prefix}_fetch_media`, { chatId, messageId, index }), "media");
    this.openMedia = async (chatId, messageId, index) => {
      await invoke(`${this.prefix}_open_media`, { chatId, messageId, index });
    };
    this.editMessage = async (chatId, messageId, text) => {
      const args: Record<string, unknown> = { chatId, messageId, text };
      // Signal formats with ranges over the text rather than markers in it.
      if (account === "signal") args.styled = signalBody(text.trim());
      return toMessage(await invoke<RawMessage>(`${this.prefix}_edit_message`, args));
    };
    if (account === "whatsapp") {
      this.linkThumbnailUrl = async (chatId, messageId) =>
        fileUrl(await invoke<string>("wa_link_thumbnail", { chatId, messageId }), "media");
      this.lookupNumber = (phone) => invoke<string | null>("wa_lookup_number", { phone });
      this.votePoll = async (chatId, messageId, options) => toMessage(await invoke<RawMessage>("wa_vote_poll", { chatId, messageId, options }));
      this.groupInfo = async (id) => toGroupInfo(await invoke<RawGroupInfo>("wa_group_info", { id }), account);
      this.groupDetails = async (id) => toGroupInfo(await invoke<RawGroupInfo>("wa_group_details", { id }), account);
      this.pinnedMessages = async (chatId) => {
        const raw = await invoke<{ messages: RawMessage[]; notes: { messageId: string; at: number; pinned: boolean; byMe: boolean; by: string | null }[] }>("wa_pinned_messages", { chatId });
        return {
          messages: raw.messages.map(toMessage),
          notes: raw.notes.map((n) => ({ messageId: n.messageId, at: new Date(n.at).toISOString(), pinned: n.pinned, byMe: n.byMe, by: n.by ?? undefined })),
        };
      };
      this.pinMessage = (chatId, messageId, secs) => invoke("wa_pin_message", { chatId, messageId, secs });
      this.setTyping = (chatId, state) => invoke("wa_set_typing", { chatId, state });
      this.watchPresence = (chatId) => invoke("wa_watch_presence", { chatId });
      this.privacy = async () => {
        const raw = await invoke<{ lastSeen: string | null; online: string | null; readReceipts: string | null }>("wa_privacy");
        return {
          lastSeen: raw.lastSeen ?? undefined,
          online: raw.online ?? undefined,
          readReceipts: raw.readReceipts ?? undefined,
        };
      };
      this.setPrivacy = (category, value) => invoke("wa_set_privacy", { category, value });
      this.setBlocked = (id, blocked) => invoke("wa_set_blocked", { id, blocked });
    }
  }

  async react(chatId: string, messageId: string, emoji: string): Promise<Message> {
    return toMessage(await invoke<RawMessage>(`${this.prefix}_react`, { chatId, messageId, emoji }));
  }

  async listChats(): Promise<Chat[]> {
    const chats = await invoke<RawChat[]>(`${this.prefix}_list_chats`);
    return chats.map((raw) => toChat(raw, this.account));
  }

  async getMessages(chatId: string): Promise<Message[]> {
    return (await invoke<RawMessage[]>(`${this.prefix}_get_messages`, { chatId })).map(toMessage);
  }

  async sendText(chatId: string, text: string, replyTo?: string, link?: OutgoingLink, forwarded?: number): Promise<Message> {
    const args: Record<string, unknown> = { chatId, text, replyTo: replyTo ?? null, link: link ?? null, forwarded: forwarded ?? null };
    // Signal formats with ranges over the text rather than markers in it.
    if (this.account === "signal") args.styled = signalBody(text.trim());
    return toMessage(await invoke<RawMessage>(`${this.prefix}_send_text`, args));
  }

  async sendFile(chatId: string, file: File, caption?: string, replyTo?: string, forwarded?: number): Promise<Message> {
    if (file.size > MAX_SEND_BYTES) throw new Error(`${file.name} is larger than ${MAX_SEND_BYTES / 1024 / 1024} MB.`);
    const details = await fileDetails(file);
    const description = { chatId, name: file.name, mime: file.type, caption, replyTo, forwarded: forwarded ?? 0, ...details };
    const bytes = new Uint8Array(await file.arrayBuffer());
    const raw = await invoke<RawMessage>(`${this.prefix}_send_file`, bytes, {
      headers: { "x-file": base64Json(description) },
    });
    return toMessage(raw);
  }

  async deleteMessage(chatId: string, messageId: string, forEveryone: boolean): Promise<Message | null> {
    const raw = await invoke<RawMessage | null>(`${this.prefix}_delete_message`, { chatId, messageId, forEveryone });
    return raw ? toMessage(raw) : null;
  }

  async setUnread(chatId: string, unread: boolean): Promise<void> {
    await invoke(`${this.prefix}_set_unread`, { chatId, unread });
  }

  async markRead(chatId: string): Promise<void> {
    await invoke(`${this.prefix}_mark_read`, { chatId });
  }

  listLists(): Promise<CustomList[]> {
    return invoke<CustomList[]>(`${this.prefix}_lists`);
  }

  async setFavorite(chatId: string, favorite: boolean): Promise<void> {
    await invoke(`${this.prefix}_set_favorite`, { chatId, favorite });
  }

  saveList(id: string | null, name: string, add: string[], remove: string[]): Promise<string> {
    return invoke<string>(`${this.prefix}_save_list`, { id, name, add, remove });
  }

  async deleteList(id: string): Promise<void> {
    await invoke(`${this.prefix}_delete_list`, { id });
  }

  async setStarred(chatId: string, messageId: string, starred: boolean): Promise<Message> {
    return toMessage(await invoke<RawMessage>(`${this.prefix}_set_starred`, { chatId, messageId, starred }));
  }

  async listStarred(): Promise<StarredMessage[]> {
    const raw = await invoke<{ chatName: string; message: RawMessage }[]>(`${this.prefix}_starred`);
    return raw.map((s) => ({ account: this.account, chatName: s.chatName, message: toMessage(s.message) }));
  }

  async fetchHistoryFirst(chatId: string): Promise<void> {
    // Only WhatsApp can ask the phone for older messages.
    if (this.account === "whatsapp") await invoke("wa_fetch_history_first", { chatId });
  }

  async olderMessages(chatId: string, beforeMs: number): Promise<Message[]> {
    const raw = await invoke<RawMessage[]>(`${this.prefix}_older_messages`, { chatId, beforeMs: Math.floor(beforeMs) });
    return raw.map(toMessage);
  }

  async searchMessages(query: string, chatId?: string): Promise<StarredMessage[]> {
    const raw = await invoke<{ chatName: string; message: RawMessage }[]>(`${this.prefix}_search`, { query, chatId: chatId ?? null });
    return raw.map((s) => ({ account: this.account, chatName: s.chatName, message: toMessage(s.message) }));
  }

  async person(id: string): Promise<Person> {
    const raw = await invoke<{
      id: string;
      name: string;
      profileName: string | null;
      phone: string | null;
      avatar: string | null;
      isMe: boolean;
      saved: boolean;
      hasChat: boolean;
      muted: boolean;
      starred: number;
      mediaCount: number;
      recentMedia: RawMessage[];
      about: string | null;
      commonGroups: RawGroupRef[];
      blocked: boolean;
    }>(`${this.prefix}_person`, { id });
    return {
      id: raw.id,
      account: this.account,
      name: raw.name,
      profileName: raw.profileName ?? undefined,
      phone: raw.phone ?? undefined,
      avatarUrl: raw.avatar ? fileUrl(raw.avatar, "avatar") : undefined,
      avatarHue: hueFor(raw.id),
      initials: initialsFor(raw.name),
      isMe: raw.isMe,
      saved: raw.saved,
      hasChat: raw.hasChat,
      muted: raw.muted,
      starred: raw.starred,
      mediaCount: raw.mediaCount,
      recentMedia: raw.recentMedia.map(toMessage),
      about: raw.about ?? undefined,
      commonGroups: raw.commonGroups.map(toGroupRef),
      blocked: raw.blocked || undefined,
    };
  }

  async personDetails(id: string): Promise<PersonDetails> {
    const raw = await invoke<{
      about: string | null;
      photo: string | null;
      commonGroups: RawGroupRef[];
    }>(`${this.prefix}_person_details`, { id });
    return {
      about: raw.about ?? undefined,
      photoUrl: raw.photo ? fileUrl(raw.photo, "avatar") : undefined,
      commonGroups: raw.commonGroups.map(toGroupRef),
    };
  }

  async setMuted(chatId: string, muted: boolean, untilMs?: number): Promise<void> {
    await invoke(`${this.prefix}_set_muted`, { chatId, muted, untilMs: untilMs ?? null });
  }

  async setArchived(chatId: string, archived: boolean): Promise<void> {
    await invoke(`${this.prefix}_set_archived`, { chatId, archived });
  }

  async setPinned(chatId: string, pinned: boolean): Promise<void> {
    await invoke(`${this.prefix}_set_pinned`, { chatId, pinned });
  }

  async deleteChat(chatId: string): Promise<void> {
    await invoke(`${this.prefix}_delete_chat`, { chatId });
  }

  async clearChat(chatId: string, keepStarred: boolean): Promise<void> {
    await invoke(`${this.prefix}_clear_chat`, { chatId, keepStarred });
  }

  async listContacts(): Promise<Contact[]> {
    const raw = await invoke<RawContact[]>(`${this.prefix}_contacts`);
    return raw.map((c) => ({
      id: c.id,
      account: this.account,
      name: c.name,
      initials: initialsFor(c.name),
      avatarHue: hueFor(c.id),
      avatarUrl: c.avatar ? fileUrl(c.avatar, "avatar") : undefined,
      phone: c.phone ?? undefined,
      isMe: c.isMe,
    }));
  }

  subscribe(listener: (event: BackendEvent) => void): () => void {
    const pending: Promise<UnlistenFn>[] = [
      listen(`${this.prefix}://chats`, () => listener({ type: "chats-changed" })),
      listen<string>(`${this.prefix}://history`, (e) => listener({ type: "history", chatId: e.payload })),
      listen<string>(`${this.prefix}://pins`, (e) => listener({ type: "pins", chatId: e.payload })),
      listen<string>(`${this.prefix}://cleared`, (e) => listener({ type: "cleared", chatId: e.payload })),
      listen<RawMessage>(`${this.prefix}://message`, (e) =>
        listener({ type: "message", message: toMessage(e.payload) }),
      ),
      listen<{ chatId: string; who: string | null; recording: boolean; active: boolean }>(`${this.prefix}://typing`, (e) =>
        listener({ type: "typing", ...e.payload, who: e.payload.who ?? undefined }),
      ),
      listen<{ chatId: string; online: boolean; lastSeenMs: number | null }>(`${this.prefix}://presence`, (e) =>
        listener({
          type: "presence",
          chatId: e.payload.chatId,
          online: e.payload.online,
          lastSeen: e.payload.lastSeenMs ? new Date(e.payload.lastSeenMs).toISOString() : undefined,
        }),
      ),
      listen<RawNotice>(`${this.prefix}://notice`, (e) => {
        const notice: Notice = { ...e.payload, avatar: e.payload.avatar ?? undefined };
        listener({ type: "notice", notice });
      }),
    ];
    return () => {
      for (const p of pending) void p.then((unlisten) => unlisten());
    };
  }
}
