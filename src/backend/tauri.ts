import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AccountId } from "../accounts/accounts";
import { COMMAND_PREFIX } from "../accounts/useAccount";
import { hueFor, initialsFor } from "../lib/avatar";
import { fileUrl } from "../lib/files";
import type {
  Attachment,
  BackendEvent,
  Chat,
  ChatBackend,
  Contact,
  CustomList,
  DeliveryStatus,
  Message,
  MessageKind,
  Notice,
  ReactionCount,
  StarredMessage,
} from "./types";

interface RawChat {
  id: string;
  name: string;
  lastMessage: string;
  lastAt: number;
  lastFromMe: boolean;
  lastStatus: DeliveryStatus | null;
  unread: number;
  pinned: boolean;
  muted: boolean;
  isGroup: boolean;
  avatar: string | null;
  favorite: boolean;
  lists: string[];
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
  replyTo: { author: string; text: string } | null;
  status: DeliveryStatus | null;
  hasMedia: boolean;
  attachments: { kind: Attachment["kind"]; mime: string; fileName: string | null; size: number; width: number | null; height: number | null }[];
  mediaWidth: number | null;
  mediaHeight: number | null;
  linkPreview: { url: string; title: string | null; description: string | null; thumbnail: string | null; hasLarge: boolean } | null;
  reactions: ReactionCount[];
  starred: boolean;
  deleted: boolean;
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
    pinned: raw.pinned,
    muted: raw.muted,
    isGroup: raw.isGroup,
    favorite: raw.favorite,
    lists: raw.lists,
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
    replyTo: raw.replyTo ?? undefined,
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
    reactions: raw.reactions,
    starred: raw.starred,
    deleted: raw.deleted,
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

  constructor(readonly account: AccountId) {
    this.prefix = COMMAND_PREFIX[account];
    this.mediaUrl = async (chatId, messageId, index = 0) =>
      fileUrl(await invoke<string>(`${this.prefix}_fetch_media`, { chatId, messageId, index }), "media");
    this.openMedia = async (chatId, messageId, index) => {
      await invoke(`${this.prefix}_open_media`, { chatId, messageId, index });
    };
    if (account === "whatsapp") {
      this.linkThumbnailUrl = async (chatId, messageId) =>
        fileUrl(await invoke<string>("wa_link_thumbnail", { chatId, messageId }), "media");
      this.lookupNumber = (phone) => invoke<string | null>("wa_lookup_number", { phone });
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

  async sendText(chatId: string, text: string, replyTo?: string): Promise<Message> {
    return toMessage(await invoke<RawMessage>(`${this.prefix}_send_text`, { chatId, text, replyTo: replyTo ?? null }));
  }

  async deleteMessage(chatId: string, messageId: string, forEveryone: boolean): Promise<Message | null> {
    const raw = await invoke<RawMessage | null>(`${this.prefix}_delete_message`, { chatId, messageId, forEveryone });
    return raw ? toMessage(raw) : null;
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
      listen<RawMessage>(`${this.prefix}://message`, (e) =>
        listener({ type: "message", message: toMessage(e.payload) }),
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
