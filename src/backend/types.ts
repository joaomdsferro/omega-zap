import type { AccountId } from "../accounts/accounts";

export type MessageKind = "text" | "image" | "voice" | "media";

export type DeliveryStatus = "sent" | "delivered" | "read";

export interface ReplyQuote {
  author: string;
  /** The quoted message is ours: `author` is then "You". */
  fromMe?: boolean;
  /** The quoted message, when the network named it. */
  id?: string;
  text: string;
}

/** The card sent with a text for its link. Pictures are base64 JPEGs. */
export interface OutgoingLink {
  url: string;
  title?: string;
  description?: string;
  /** Small, carried in the message. */
  thumbnail?: string;
  /** Full size, uploaded beside it. */
  image?: string;
  width?: number;
  height?: number;
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
  /** Who picked it, "You" first. */
  people?: string[];
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
  /** People `text` mentions, written there as "@Name". */
  mentions?: Mention[];
  /** Who sent it, when it is someone else. */
  authorId?: string;
  status?: DeliveryStatus;
  /** The attachment can be fetched with `mediaUrl`. */
  hasMedia?: boolean;
  /** Every attachment, when the network sends several (Signal). */
  attachments?: Attachment[];
  /** Forwarded this many times on the way here (WhatsApp); absent when written by its sender. */
  forwarded?: number;
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
  /** Marked unread by hand: a dot, not a count. */
  markedUnread?: boolean;
  pinned?: boolean;
  muted?: boolean;
  /** Kept out of the chat list, under Archived. */
  archived?: boolean;
  isGroup?: boolean;
  online?: boolean;
  /** When they were last online, as an ISO time, if they share it. */
  lastSeen?: string;
  /** Who is typing or recording right now, as the list says it ("typing…"). */
  typing?: string;
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
export interface Mention {
  /** "@Name", as it appears in the text. */
  text: string;
  id: string;
}

/** Someone's contact info. */
export interface Person {
  id: string;
  account: AccountId;
  name: string;
  /** The name they gave themselves, when it differs from the saved one. */
  profileName?: string;
  phone?: string;
  avatarUrl?: string;
  avatarHue: number;
  initials: string;
  isMe: boolean;
  saved: boolean;
  /** The chat with them, when there is one, and what it holds. */
  hasChat: boolean;
  muted: boolean;
  starred: number;
  /** Photos, videos, files and links in the chat with them. */
  mediaCount: number;
  /** The latest photos and videos there, newest first. */
  recentMedia: Message[];
  /** As found the last time contact info opened; refreshed by `personDetails`. */
  about?: string;
  commonGroups: GroupRef[];
}

/** What someone's contact info adds from the network. */
export interface PersonDetails {
  about?: string;
  /** Their picture at full size. */
  photoUrl?: string;
  commonGroups: GroupRef[];
}

/** Someone in a group, as group info lists them. */
export interface GroupMember {
  id: string;
  name: string;
  /** Their number, when they are not a saved contact. */
  phone?: string;
  avatarUrl?: string;
  avatarHue: number;
  initials: string;
  isMe: boolean;
  admin: boolean;
  about?: string;
}

/** A group's info. */
export interface GroupInfo {
  id: string;
  account: AccountId;
  name: string;
  avatarUrl?: string;
  avatarHue: number;
  initials: string;
  /** How many people are in it, us included, once looked up. */
  members?: number;
  description?: string;
  /** ISO time the group was made. */
  createdAt?: string;
  createdBy?: string;
  muted: boolean;
  starred: number;
  mediaCount: number;
  recentMedia: Message[];
  /** Us first, then admins, then saved contacts and the rest by name. */
  participants: GroupMember[];
  /** We are an admin of it. */
  isAdmin: boolean;
}

export interface GroupRef {
  id: string;
  name: string;
  avatarUrl?: string;
  avatarHue: number;
  initials: string;
  members?: number;
}

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
  | { type: "notice"; notice: Notice }
  /** Older messages of a chat came from the phone. */
  | { type: "history"; chatId: string }
  /** A message of a chat was pinned or unpinned. */
  | { type: "pins"; chatId: string }
  /** Someone started or stopped typing or recording; `who` names them in a group. */
  | { type: "typing"; chatId: string; who?: string; recording: boolean; active: boolean }
  /** Someone came online or went away. */
  | { type: "presence"; chatId: string; online: boolean; lastSeen?: string };

/** What we are doing in the message box, for the other side to see. */
export type TypingState = "typing" | "recording" | "paused";

/** Who sees our last seen, online and read receipts, as WhatsApp names the values. */
export interface Privacy {
  /** "all", "contacts", "contact_blacklist" (contacts except some) or "none". */
  lastSeen?: string;
  /** "all" or "match_last_seen". */
  online?: string;
  /** "all" or "none". */
  readReceipts?: string;
}

export type PrivacyCategory = "last" | "online" | "readreceipts";

export interface ChatBackend {
  readonly account: AccountId;
  /** Sample chats instead of a linked account. */
  readonly demo: boolean;
  listChats(): Promise<Chat[]>;
  getMessages(chatId: string): Promise<Message[]>;
  /** Messages from before `beforeMs`, oldest first: the history past what `getMessages` gives. */
  olderMessages?(chatId: string, beforeMs: number): Promise<Message[]>;
  /** The messages pinned in a chat for everyone, the latest pin first. */
  pinnedMessages?(chatId: string): Promise<Message[]>;
  /** Puts a chat first in line for its history to be fetched from the phone. */
  fetchHistoryFirst?(chatId: string): Promise<void>;
  /**
   * Sends a text, quoting the message `replyTo` when given, with a card for
   * its link when given. `forwarded` is the forwarding score of a message
   * sent on from another chat.
   */
  sendText(chatId: string, text: string, replyTo?: string, link?: OutgoingLink, forwarded?: number): Promise<Message>;
  /** Uploads a file and sends it, with `caption` under it. */
  sendFile?(chatId: string, file: File, caption?: string, replyTo?: string, forwarded?: number): Promise<Message>;
  /**
   * Deletes a message for us, or for everyone when it is ours. Returns the
   * placeholder left behind, or `null` when nothing is shown any more.
   */
  deleteMessage?(chatId: string, messageId: string, forEveryone: boolean): Promise<Message | null>;
  markRead(chatId: string): Promise<void>;
  /** Marks a chat unread by hand: a dot in the list until it is read. */
  setUnread?(chatId: string, unread: boolean): Promise<void>;
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
  /** Messages with every word of `query` in them, ignoring case and accents, newest first; only `chatId`'s when given. */
  searchMessages(query: string, chatId?: string): Promise<StarredMessage[]>;
  listContacts(): Promise<Contact[]>;
  /** Someone's contact info, from what this computer knows. */
  person?(id: string): Promise<Person>;
  /** About, full-size picture and groups in common: slower, from the network. */
  personDetails?(id: string): Promise<PersonDetails>;
  /** A group's info, from what this computer knows. */
  groupInfo?(id: string): Promise<GroupInfo>;
  /** A group's info fresh from the network: members, roles, description, full-size picture. */
  groupDetails?(id: string): Promise<GroupInfo>;
  /** Mutes a chat until `untilMs`, or for good without it, or unmutes it. */
  setMuted?(chatId: string, muted: boolean, untilMs?: number): Promise<void>;
  /** Moves a chat to Archived, which unpins it, or back to the list. */
  setArchived?(chatId: string, archived: boolean): Promise<void>;
  /** Pins a chat to the top of the list, or unpins it. */
  setPinned?(chatId: string, pinned: boolean): Promise<void>;
  /** Tells the chat we are typing, recording, or stopped. */
  setTyping?(chatId: string, state: TypingState): Promise<void>;
  /** Asks to hear when the other person comes online or goes away. */
  watchPresence?(chatId: string): Promise<void>;
  privacy?(): Promise<Privacy>;
  setPrivacy?(category: PrivacyCategory, value: string): Promise<void>;
  /** The chat id for a phone number, or `null` when the number isn't on the network. */
  lookupNumber?(phone: string): Promise<string | null>;
  subscribe(listener: (event: BackendEvent) => void): () => void;
}
