import { invoke, type InvokeArgs } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AccountId } from "../accounts/accounts";
import { isWhatsApp } from "../accounts/accounts";
import { COMMAND_PREFIX, EVENT_PREFIX, accountHeaders } from "../accounts/useAccount";
import { hueFor, initialsFor } from "../lib/avatar";
import { fileUrl } from "../lib/files";
import { officialLogo } from "../lib/logos";
import { mentionSpans, signalBody } from "../lib/markup";
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
  MemberChange,
  GroupRef,
  Person,
  PersonDetails,
  Notice,
  Privacy,
  PrivacyCategory,
  ReactionCount,
  StarredMessage,
  StickerShelf,
  SharedStickerPack,
  VoiceRecorder,
  OutgoingMedia,
  TypingState,
  RequestAnswer,
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
  left: boolean;
  cleared: boolean;
  request?: boolean;
  expireSecs?: number;
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
  waveform?: number[];
  replyTo: { author: string; fromMe: boolean; id: string | null; text: string } | null;
  mentions: Mention[];
  authorId: string | null;
  status: DeliveryStatus | null;
  hasMedia: boolean;
  attachments: { kind: Attachment["kind"]; mime: string; fileName: string | null; size: number; width: number | null; height: number | null; stickerId?: string }[];
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
  stickerPack?: SharedStickerPack;
  timer?: number;
  expiresIn?: number;
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
  left: boolean;
  infoLocked: boolean;
}

function toGroupInfo(raw: RawGroupInfo, account: AccountId): GroupInfo {
  return {
    left: raw.left || undefined,
    infoLocked: raw.infoLocked || undefined,
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
    avatarUrl: raw.avatar
      ? fileUrl(raw.avatar, "avatar")
      : officialLogo(account, raw.id),
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
    request: raw.request || undefined,
    disappearing: raw.expireSecs || undefined,
    left: raw.left || undefined,
    cleared: raw.cleared || undefined,
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
    waveform: raw.waveform?.length ? raw.waveform : undefined,
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
          stickerId: a.stickerId,
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
    stickerPack: raw.stickerPack,
    timer: raw.timer,
    expiresIn: raw.expiresIn,
  };
}

/**
 * `text` with each of `mentions` ("@Name") written as the network keeps it:
 * "@<number>" for WhatsApp (the Rust side picks the number the group uses),
 * "@<ACI>" for Signal. Also the ids of those still in the text.
 */
function wireMentions(text: string, mentions: Mention[] = []): { text: string; ids: string[]; everyone: boolean } {
  if (!mentions.length) return { text, ids: [], everyone: false };
  const idOf = new Map(mentions.map((m) => [m.text, m.id]));
  const ids = new Set<string>();
  let out = "";
  let at = 0;
  let everyone = false;
  for (const span of mentionSpans(text, [...idOf.keys()])) {
    const id = idOf.get(span.text)!;
    // Everyone (no id) is sent as written: "@all".
    if (!id) everyone = true;
    else ids.add(id);
    out += text.slice(at, span.start) + (id ? `@${id.split("@")[0]}` : span.text);
    at = span.end;
  }
  return { text: out + text.slice(at), ids: [...ids], everyone };
}

/** Real chats, served by the Rust side from a linked account's session. */
export class TauriBackend implements ChatBackend {
  readonly demo = false;
  private readonly prefix: string;
  /** Prefix of the events this account sends. */
  private readonly events: string;
  /** `invoke`, telling a shared WhatsApp command which account it is for. */
  private readonly invoke: typeof invoke;

  mediaUrl?: (chatId: string, messageId: string, index?: number) => Promise<string>;
  openMedia?: (chatId: string, messageId: string, index: number) => Promise<void>;
  linkThumbnailUrl?: (chatId: string, messageId: string) => Promise<string>;
  lookupNumber?: (phone: string) => Promise<string | null>;
  pinnedMessages?: (chatId: string) => Promise<Pins>;
  pinMessage?: (chatId: string, messageId: string, secs: number | null) => Promise<void>;
  groupInfo?: (id: string) => Promise<GroupInfo>;
  completeHistory?: (chatId: string) => Promise<boolean>;
  editMessage?: (chatId: string, messageId: string, text: string) => Promise<Message>;
  votePoll?: (chatId: string, messageId: string, options: number[]) => Promise<Message>;
  groupDetails?: (id: string) => Promise<GroupInfo>;
  setTyping?: (chatId: string, state: TypingState) => Promise<void>;
  watchPresence?: (chatId: string) => Promise<void>;
  privacy?: () => Promise<Privacy>;
  setPrivacy?: (category: PrivacyCategory, value: string) => Promise<void>;
  setBlocked?: (id: string, blocked: boolean) => Promise<void>;
  respondRequest?: (chatId: string, action: RequestAnswer) => Promise<void>;
  setDisappearing?: (chatId: string, secs: number) => Promise<Message>;
  leaveGroup?: (id: string) => Promise<void>;
  editGroup?: (id: string, changes: { name?: string; description?: string }) => Promise<GroupInfo>;
  changeMembers?: (id: string, change: MemberChange, members: string[]) => Promise<GroupInfo>;
  sendMedia?: (chatId: string, item: OutgoingMedia, replyTo?: string) => Promise<Message>;
  stickers?: StickerShelf;
  voice?: VoiceRecorder;

  constructor(readonly account: AccountId) {
    this.prefix = COMMAND_PREFIX[account];
    this.events = EVENT_PREFIX[account];
    const headers = accountHeaders(account);
    this.invoke = (cmd, args, options) => invoke(cmd, args, { ...options, headers: { ...headers, ...(options?.headers as Record<string, string>) } });
    this.mediaUrl = async (chatId, messageId, index = 0) =>
      fileUrl(await this.invoke<string>(`${this.prefix}_fetch_media`, { chatId, messageId, index }), "media");
    this.openMedia = async (chatId, messageId, index) => {
      await this.invoke(`${this.prefix}_open_media`, { chatId, messageId, index });
    };
    this.editMessage = async (chatId, messageId, text) => {
      const args: Record<string, unknown> = { chatId, messageId, text };
      // Signal formats with ranges over the text rather than markers in it.
      if (account === "signal") args.styled = signalBody(text.trim());
      return toMessage(await this.invoke<RawMessage>(`${this.prefix}_edit_message`, args));
    };
    if (isWhatsApp(account)) {
      this.sendMedia = async (chatId, item, replyTo) => {
        const reply = replyTo ?? null;
        switch (item.kind) {
          case "sticker":
            return toMessage(await this.invoke<RawMessage>("wa_send_sticker", { chatId, id: item.id, replyTo: reply }));
          case "new-sticker":
            return toMessage(
              await this.invoke<RawMessage>("wa_send_new_sticker", new Uint8Array(await item.picture.arrayBuffer()), {
                headers: { "x-chat": chatId, "x-reply": replyTo ?? "" },
              }),
            );
          case "found-sticker":
            return toMessage(await this.invoke<RawMessage>("wa_send_klipy_sticker", { chatId, pick: item.pick, replyTo: reply }));
          case "gif":
            return toMessage(await this.invoke<RawMessage>("wa_send_gif", { chatId, pick: item.pick, replyTo: reply }));
          case "recent-gif":
            return toMessage(await this.invoke<RawMessage>("wa_send_recent_gif", { chatId, id: item.id, replyTo: reply }));
          case "voice":
            return toMessage(await this.invoke<RawMessage>("wa_send_voice", { chatId, replyTo: reply }));
        }
      };
      const subscribe = <T,>(event: string, listener: (payload: T) => void) => {
        const pending = listen<T>(event, (e) => listener(e.payload));
        return () => void pending.then((unlisten) => unlisten());
      };
      this.voice = {
        start: (microphone) => this.invoke("voice_start", { microphone: microphone ?? null }),
        pause: (paused) => this.invoke("voice_pause", { paused }),
        cancel: () => this.invoke("voice_cancel"),
        preview: async () => new Blob([await this.invoke<ArrayBuffer>("voice_preview")], { type: "audio/ogg" }),
        microphones: () => this.invoke("voice_microphones"),
        onLevel: (listener) => subscribe("voice://level", listener),
        onError: (listener) => subscribe("voice://error", listener),
      };
      const target = (t: { id: string } | { chatId: string; messageId: string }) =>
        "id" in t ? { id: t.id, chatId: null, messageId: null } : { id: null, chatId: t.chatId, messageId: t.messageId };
      this.stickers = {
        library: () => this.invoke("wa_sticker_library"),
        fileUrl: async (id) => fileUrl(await this.invoke<string>("wa_library_media", { id }), "media"),
        setFavorite: (t, favorite) => this.invoke("wa_set_sticker_favorite", { ...target(t), favorite }),
        store: () => this.invoke("wa_sticker_store"),
        installPack: (id) => this.invoke("wa_install_pack", { id }),
        removePack: (id) => this.invoke("wa_remove_pack", { id }),
        createPack: (name) => this.invoke<string>("wa_create_pack", { name }),
        renamePack: (id, name) => this.invoke("wa_rename_pack", { id, name }),
        addToPack: (packId, t) => this.invoke("wa_add_to_pack", { packId, ...target(t) }),
        removeFromPack: (packId, id) => this.invoke("wa_remove_from_pack", { packId, id }),
        addNewSticker: async (packId, picture) => {
          await this.invoke("wa_add_new_sticker", new Uint8Array(await picture.arrayBuffer()), { headers: { "x-pack": packId } });
        },
        addFoundSticker: (packId, pick) => this.invoke("wa_add_klipy_sticker", { packId, pick }),
        addSharedPack: (chatId, messageId) => this.invoke<string>("wa_add_shared_pack", { chatId, messageId }),
        subscribe: (listener) => {
          // The library is the app's, whichever account changed it.
          const pending = listen("stickers://changed", () => listener());
          return () => void pending.then((unlisten) => unlisten());
        },
      };
      this.linkThumbnailUrl = async (chatId, messageId) =>
        fileUrl(await this.invoke<string>("wa_link_thumbnail", { chatId, messageId }), "media");
      this.lookupNumber = (phone) => this.invoke<string | null>("wa_lookup_number", { phone });
      this.votePoll = async (chatId, messageId, options) => toMessage(await this.invoke<RawMessage>("wa_vote_poll", { chatId, messageId, options }));
      this.groupInfo = async (id) => toGroupInfo(await this.invoke<RawGroupInfo>("wa_group_info", { id }), account);
      this.completeHistory = (chatId) => this.invoke<boolean>("wa_complete_history", { chatId });
      this.groupDetails = async (id) => toGroupInfo(await this.invoke<RawGroupInfo>("wa_group_details", { id }), account);
      this.pinnedMessages = async (chatId) => {
        const raw = await this.invoke<{ messages: RawMessage[]; notes: { messageId: string; at: number; pinned: boolean; byMe: boolean; by: string | null }[] }>("wa_pinned_messages", { chatId });
        return {
          messages: raw.messages.map(toMessage),
          notes: raw.notes.map((n) => ({ messageId: n.messageId, at: new Date(n.at).toISOString(), pinned: n.pinned, byMe: n.byMe, by: n.by ?? undefined })),
        };
      };
      this.pinMessage = (chatId, messageId, secs) => this.invoke("wa_pin_message", { chatId, messageId, secs });
      this.setTyping = (chatId, state) => this.invoke("wa_set_typing", { chatId, state });
      this.watchPresence = (chatId) => this.invoke("wa_watch_presence", { chatId });
      this.privacy = async () => {
        const raw = await this.invoke<{ lastSeen: string | null; online: string | null; readReceipts: string | null }>("wa_privacy");
        return {
          lastSeen: raw.lastSeen ?? undefined,
          online: raw.online ?? undefined,
          readReceipts: raw.readReceipts ?? undefined,
        };
      };
      this.setPrivacy = (category, value) => this.invoke("wa_set_privacy", { category, value });
      this.setBlocked = (id, blocked) => this.invoke("wa_set_blocked", { id, blocked });
      this.leaveGroup = (id) => this.invoke("wa_leave_group", { id });
      this.editGroup = async (id, { name, description }) =>
        toGroupInfo(await this.invoke<RawGroupInfo>("wa_edit_group", { id, name: name ?? null, description: description ?? null }), account);
      this.changeMembers = async (id, change, members) =>
        toGroupInfo(await this.invoke<RawGroupInfo>("wa_change_members", { id, change, members }), account);
    } else {
      this.groupInfo = async (id) => toGroupInfo(await this.invoke<RawGroupInfo>(`${this.prefix}_group_info`, { id }), account);
      if (account === "signal") {
        this.setTyping = (chatId, state) => this.invoke("signal_set_typing", { chatId, state });
        this.setBlocked = (id, blocked) => this.invoke("signal_set_blocked", { id, blocked });
        this.respondRequest = (chatId, action) => this.invoke("signal_respond_request", { chatId, action });
        this.setDisappearing = async (chatId, secs) => toMessage(await this.invoke<RawMessage>("signal_set_disappearing", { chatId, secs }));
        this.sendMedia = async (chatId, item, replyTo) => {
          if (item.kind !== "sticker") throw new Error("Only stickers from your library can be sent on Signal for now.");
          return toMessage(await this.invoke<RawMessage>("signal_send_sticker", { chatId, id: item.id, replyTo: replyTo ?? null }));
        };
        this.stickers = signalStickers((cmd, args) => this.invoke(cmd, args));
      }
    }
  }

  async react(chatId: string, messageId: string, emoji: string): Promise<Message> {
    return toMessage(await this.invoke<RawMessage>(`${this.prefix}_react`, { chatId, messageId, emoji }));
  }

  async listChats(): Promise<Chat[]> {
    const chats = await this.invoke<RawChat[]>(`${this.prefix}_list_chats`);
    return chats.map((raw) => toChat(raw, this.account));
  }

  async getMessages(chatId: string): Promise<Message[]> {
    return (await this.invoke<RawMessage[]>(`${this.prefix}_get_messages`, { chatId })).map(toMessage);
  }

  async sendText(chatId: string, text: string, replyTo?: string, link?: OutgoingLink, forwarded?: number, mentions?: Mention[]): Promise<Message> {
    const wire = wireMentions(text.trim(), mentions);
    const args: Record<string, unknown> = { chatId, text: wire.text, replyTo: replyTo ?? null, link: link ?? null, forwarded: forwarded ?? null };
    if (isWhatsApp(this.account)) Object.assign(args, { mentions: wire.ids, mentionAll: wire.everyone });
    // Signal formats with ranges over the text rather than markers in it.
    if (this.account === "signal") args.styled = signalBody(wire.text);
    return toMessage(await this.invoke<RawMessage>(`${this.prefix}_send_text`, args));
  }

  async sendFile(chatId: string, file: File, caption?: string, replyTo?: string, forwarded?: number, mentions?: Mention[]): Promise<Message> {
    if (file.size > MAX_SEND_BYTES) throw new Error(`${file.name} is larger than ${MAX_SEND_BYTES / 1024 / 1024} MB.`);
    const details = await fileDetails(file);
    // Signal's captions carry no mention ranges here, so there they stay "@Name".
    const wire = isWhatsApp(this.account) && caption ? wireMentions(caption, mentions) : { text: caption, ids: [], everyone: false };
    const description = { chatId, name: file.name, mime: file.type, caption: wire.text, mentions: wire.ids, mentionAll: wire.everyone, replyTo, forwarded: forwarded ?? 0, ...details };
    const bytes = new Uint8Array(await file.arrayBuffer());
    const raw = await this.invoke<RawMessage>(`${this.prefix}_send_file`, bytes, {
      headers: { "x-file": base64Json(description) },
    });
    return toMessage(raw);
  }

  async deleteMessage(chatId: string, messageId: string, forEveryone: boolean): Promise<Message | null> {
    const raw = await this.invoke<RawMessage | null>(`${this.prefix}_delete_message`, { chatId, messageId, forEveryone });
    return raw ? toMessage(raw) : null;
  }

  async setUnread(chatId: string, unread: boolean): Promise<void> {
    await this.invoke(`${this.prefix}_set_unread`, { chatId, unread });
  }

  async markRead(chatId: string): Promise<void> {
    await this.invoke(`${this.prefix}_mark_read`, { chatId });
  }

  listLists(): Promise<CustomList[]> {
    return this.invoke<CustomList[]>(`${this.prefix}_lists`);
  }

  async setFavorite(chatId: string, favorite: boolean): Promise<void> {
    await this.invoke(`${this.prefix}_set_favorite`, { chatId, favorite });
  }

  saveList(id: string | null, name: string, add: string[], remove: string[]): Promise<string> {
    return this.invoke<string>(`${this.prefix}_save_list`, { id, name, add, remove });
  }

  async deleteList(id: string): Promise<void> {
    await this.invoke(`${this.prefix}_delete_list`, { id });
  }

  async setStarred(chatId: string, messageId: string, starred: boolean): Promise<Message> {
    return toMessage(await this.invoke<RawMessage>(`${this.prefix}_set_starred`, { chatId, messageId, starred }));
  }

  async listStarred(): Promise<StarredMessage[]> {
    const raw = await this.invoke<{ chatName: string; message: RawMessage }[]>(`${this.prefix}_starred`);
    return raw.map((s) => ({ account: this.account, chatName: s.chatName, message: toMessage(s.message) }));
  }

  async fetchHistoryFirst(chatId: string): Promise<void> {
    // Only WhatsApp can ask the phone for older messages.
    if (isWhatsApp(this.account)) await this.invoke("wa_fetch_history_first", { chatId });
  }

  async olderMessages(chatId: string, beforeMs: number): Promise<Message[]> {
    const raw = await this.invoke<RawMessage[]>(`${this.prefix}_older_messages`, { chatId, beforeMs: Math.floor(beforeMs) });
    return raw.map(toMessage);
  }

  async searchMessages(query: string, chatId?: string): Promise<StarredMessage[]> {
    const raw = await this.invoke<{ chatName: string; message: RawMessage }[]>(`${this.prefix}_search`, { query, chatId: chatId ?? null });
    return raw.map((s) => ({ account: this.account, chatName: s.chatName, message: toMessage(s.message) }));
  }

  async person(id: string): Promise<Person> {
    const raw = await this.invoke<{
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
    const raw = await this.invoke<{
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
    await this.invoke(`${this.prefix}_set_muted`, { chatId, muted, untilMs: untilMs ?? null });
  }

  async setArchived(chatId: string, archived: boolean): Promise<void> {
    await this.invoke(`${this.prefix}_set_archived`, { chatId, archived });
  }

  async setPinned(chatId: string, pinned: boolean): Promise<void> {
    await this.invoke(`${this.prefix}_set_pinned`, { chatId, pinned });
  }

  async deleteChat(chatId: string): Promise<void> {
    await this.invoke(`${this.prefix}_delete_chat`, { chatId });
  }

  async clearChat(chatId: string, keepStarred: boolean): Promise<void> {
    await this.invoke(`${this.prefix}_clear_chat`, { chatId, keepStarred });
  }

  async listContacts(): Promise<Contact[]> {
    const raw = await this.invoke<RawContact[]>(`${this.prefix}_contacts`);
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
      listen(`${this.events}://chats`, () => listener({ type: "chats-changed" })),
      listen<string>(`${this.events}://history`, (e) => listener({ type: "history", chatId: e.payload })),
      listen<string>(`${this.events}://pins`, (e) => listener({ type: "pins", chatId: e.payload })),
      listen<string>(`${this.events}://cleared`, (e) => listener({ type: "cleared", chatId: e.payload })),
      listen<string>(`${this.events}://group`, (e) => listener({ type: "group", chatId: e.payload })),
      listen<RawMessage>(`${this.events}://message`, (e) =>
        listener({ type: "message", message: toMessage(e.payload) }),
      ),
      listen<{ chatId: string; who: string | null; recording: boolean; active: boolean }>(`${this.events}://typing`, (e) =>
        listener({ type: "typing", ...e.payload, who: e.payload.who ?? undefined }),
      ),
      listen<{ chatId: string; online: boolean; lastSeenMs: number | null }>(`${this.events}://presence`, (e) =>
        listener({
          type: "presence",
          chatId: e.payload.chatId,
          online: e.payload.online,
          lastSeen: e.payload.lastSeenMs ? new Date(e.payload.lastSeenMs).toISOString() : undefined,
        }),
      ),
      listen<RawNotice>(`${this.events}://notice`, (e) => {
        const notice: Notice = { ...e.payload, avatar: e.payload.avatar ?? undefined };
        listener({ type: "notice", notice });
      }),
    ];
    return () => {
      for (const p of pending) void p.then((unlisten) => unlisten());
    };
  }
}

/**
 * Signal's stickers: WhatsApp's library (the account the host picks for it),
 * whose favorites and packs are made into Signal packs to send from.
 */
function signalStickers(signal: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>): StickerShelf {
  const library = <T,>(cmd: string, args?: InvokeArgs, options?: { headers: Record<string, string> }) =>
    invoke<T>(cmd, args, { headers: { "x-wa-account": "stickers", ...options?.headers } });
  const target = (t: { id: string } | { chatId: string; messageId: string }) =>
    "id" in t ? { id: t.id, chatId: null, messageId: null } : { id: null, chatId: t.chatId, messageId: t.messageId };
  const on = (events: string[], listener: () => void) => {
    const pending = events.map((event) => listen(event, () => listener()));
    return () => pending.forEach((p) => void p.then((unlisten) => unlisten()));
  };
  return {
    library: () => library("wa_sticker_library"),
    fileUrl: async (id) => fileUrl(await library<string>("wa_library_media", { id }), "media"),
    setFavorite: (t, favorite) => library("wa_set_sticker_favorite", { ...target(t), favorite }),
    store: () => library("wa_sticker_store"),
    installPack: (id) => library("wa_install_pack", { id }),
    removePack: (id) => library("wa_remove_pack", { id }),
    createPack: (name) => library<string>("wa_create_pack", { name }),
    renamePack: (id, name) => library("wa_rename_pack", { id, name }),
    addToPack: (packId, t) => library("wa_add_to_pack", { packId, ...target(t) }),
    removeFromPack: (packId, id) => library("wa_remove_from_pack", { packId, id }),
    addNewSticker: async (packId, picture) => {
      await library("wa_add_new_sticker", new Uint8Array(await picture.arrayBuffer()), { headers: { "x-pack": packId } });
    },
    addFoundSticker: (packId, pick) => library("wa_add_klipy_sticker", { packId, pick }),
    subscribe: (listener) => on(["stickers://changed", "signal://stickers"], listener),
    libraryOnly: true,
    network: {
      name: "Signal",
      packs: async () =>
        (await signal<{ source: string; title: string; count: number; onSignal: number; url: string | null; uploading: boolean }[]>("signal_sticker_packs")).map(
          (p) => ({ source: p.source, title: p.title, count: p.count, onNetwork: p.onSignal, url: p.url ?? undefined, uploading: p.uploading }),
        ),
      upload: (source) => signal("signal_upload_stickers", { source }),
      subscribe: (listener) => on(["signal://stickers", "stickers://changed"], listener),
    },
  };
}
