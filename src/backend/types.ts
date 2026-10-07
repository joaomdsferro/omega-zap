import type { AccountId } from "../accounts/accounts";

export type MessageKind = "text" | "image" | "voice" | "media";

export type DeliveryStatus = "sent" | "delivered" | "read";

export interface ReplyQuote {
  author: string;
  text: string;
}

/** The card shown for the link in a message. */
export interface LinkPreview {
  url: string;
  title?: string;
  description?: string;
  /** A `data:` URL */
  thumbnail?: string;
  /** A full-size picture can be fetched with `linkThumbnailUrl`. */
  hasLarge?: boolean;
}

export type AttachmentKind = "image" | "gif" | "video" | "voice" | "audio" | "sticker" | "file";

/** One file sent with a message. It is fetched by its index with `mediaUrl`. */
export interface Attachment {
  kind: AttachmentKind;
  mime: string;
  fileName?: string;
  /** Bytes */
  size: number;
  width?: number;
  height?: number;
}

/** One emoji on a message, with everyone who picked it counted together. */
export interface ReactionCount {
  emoji: string;
  count: number;
  mine: boolean;
}

export interface Message {
  id: string;
  chatId: string;
  kind: MessageKind;
  /** ISO timestamp */
  sentAt: string;
  fromMe: boolean;
  author?: string;
  text?: string;
  /** For image placeholders */
  imageLabel?: string;
  /** Documents, videos, stickers, locations, polls… */
  mediaLabel?: string;
  /** Voice duration in seconds */
  voiceSeconds?: number;
  replyTo?: ReplyQuote;
  status?: DeliveryStatus;
  /** The attachment can be fetched with `mediaUrl`. */
  hasMedia?: boolean;
  /** Every attachment, when the network sends several (Signal). */
  attachments?: Attachment[];
  /** The attachment's size in pixels, when known before it is fetched. */
  mediaWidth?: number;
  mediaHeight?: number;
  linkPreview?: LinkPreview;
  reactions?: ReactionCount[];
  starred?: boolean;
  /** Taken back by its sender: only a placeholder is shown. */
  deleted?: boolean;
}

export interface Chat {
  id: string;
  /** The network this chat lives on. Ids are only unique within it. */
  account: AccountId;
  name: string;
  initials: string;
  avatarHue: number;
  /** Profile picture, when one was downloaded */
  avatarUrl?: string;
  lastMessage: string;
  lastAt: string;
  lastFromMe?: boolean;
  lastStatus?: DeliveryStatus;
  unread: number;
  pinned?: boolean;
  muted?: boolean;
  isGroup?: boolean;
  online?: boolean;
  favorite?: boolean;
  /** Ids of the custom lists the chat is in. */
  lists?: string[];
}

/** A chat list the user made, such as "Family" or "Work". */
export interface CustomList {
  id: string;
  name: string;
}

/** Someone a new chat can be started with. */
export interface Contact {
  id: string;
  account: AccountId;
  name: string;
  initials: string;
  avatarHue: number;
  avatarUrl?: string;
  phone?: string;
  /** Our own account: a chat with ourselves. */
  isMe?: boolean;
}

export interface StarredMessage {
  account: AccountId;
  chatName: string;
  message: Message;
}

/** What a desktop notification about a chat says. */
export interface Notice {
  chatId: string;
  title: string;
  body: string;
  muted: boolean;
  /** File name of the chat's saved picture. */
  avatar?: string;
}

export type BackendEvent =
  | { type: "chats-changed" }
  | { type: "message"; message: Message }
  | { type: "notice"; notice: Notice };

export interface ChatBackend {
  readonly account: AccountId;
  /** Sample chats instead of a linked account. */
  readonly demo: boolean;
  listChats(): Promise<Chat[]>;
  getMessages(chatId: string): Promise<Message[]>;
  /** Sends a text, quoting the message `replyTo` when given. */
  sendText(chatId: string, text: string, replyTo?: string): Promise<Message>;
  /**
   * Deletes a message for us, or for everyone when it is ours. Returns the
   * placeholder left behind, or `null` when nothing is shown any more.
   */
  deleteMessage?(chatId: string, messageId: string, forEveryone: boolean): Promise<Message | null>;
  markRead(chatId: string): Promise<void>;
  /** Downloads one of a message's attachments if needed and returns where to load it from. */
  mediaUrl?(chatId: string, messageId: string, index?: number): Promise<string>;
  /** Downloads an attachment if needed and opens it in the system's app for its type. */
  openMedia?(chatId: string, messageId: string, index: number): Promise<void>;
  /** Downloads the full-size picture of a message's link card if needed and returns where to load it from. */
  linkThumbnailUrl?(chatId: string, messageId: string): Promise<string>;
  /** Adds our reaction, or removes it when it is already that emoji. */
  react?(chatId: string, messageId: string, emoji: string): Promise<Message>;
  listLists(): Promise<CustomList[]>;
  setFavorite(chatId: string, favorite: boolean): Promise<void>;
  /** Creates a list, or renames one, and adds and removes chats. Returns its id. */
  saveList(id: string | null, name: string, add: string[], remove: string[]): Promise<string>;
  deleteList(id: string): Promise<void>;
  setStarred(chatId: string, messageId: string, starred: boolean): Promise<Message>;
  listStarred(): Promise<StarredMessage[]>;
  listContacts(): Promise<Contact[]>;
  /** The chat id for a phone number, or `null` when the number isn't on the network. */
  lookupNumber?(phone: string): Promise<string | null>;
  subscribe(listener: (event: BackendEvent) => void): () => void;
}
