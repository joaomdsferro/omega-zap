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
  /** A WhatsApp sticker's id in the sticker library. */
  stickerId?: string;
}

/** One emoji on a message, with everyone who picked it counted together. */
export interface ReactionCount {
  emoji: string;
  count: number;
  mine: boolean;
  /** Who picked it, "You" first. */
  people?: string[];
}

/** A place shared in a message. */
export interface Place {
  lat: number;
  lng: number;
  name?: string;
  address?: string;
  /** A page about the place, when the sender picked one. */
  url?: string;
  /** A small map of it, as a data URL. */
  thumbnail?: string;
  /** Shared live: this is where it was when it was sent. */
  live?: boolean;
}

/** A contact card shared in a message. */
export interface ContactCard {
  name: string;
  phones?: { number: string; /** Their WhatsApp number, digits only, when the card names one. */ waId?: string }[];
}

/** A poll; its question is the message's text. */
export interface Poll {
  options: { name: string; mine: boolean; /** Who picked it, "You" first. */ people: string[] }[];
  /** How many options one person can pick; 0 for any number. */
  selectable: number;
  /** Votes can be cast from here. */
  open: boolean;
  /** How many people voted. */
  voters: number;
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
  /** A voice message's loudness over its length, 0–100 (WhatsApp). */
  waveform?: number[];
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
  /** Changed by its sender after it was sent. */
  edited?: boolean;
  place?: Place;
  contacts?: ContactCard[];
  poll?: Poll;
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
  /** We blocked them. */
  blocked?: boolean;
  /** A group we are no longer in. */
  left?: boolean;
  /** Its messages were cleared: none are coming from the phone. */
  cleared?: boolean;
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
  /** We blocked them. */
  blocked?: boolean;
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
  /** We are no longer in it. */
  left?: boolean;
  /** Only admins may change its name, description and picture. */
  infoLocked?: boolean;
}

/** What admins can do to members of a group. */
export type MemberChange = "add" | "remove" | "promote" | "demote";

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

/** A line in a conversation saying someone pinned or unpinned a message. */
export interface PinNote {
  messageId: string;
  /** ISO time of the pin or unpin. */
  at: string;
  /** Pinned; unpinned when false. */
  pinned: boolean;
  byMe: boolean;
  /** Who pinned it, by name, when it was someone else. */
  by?: string;
}

export interface Pins {
  /** The latest pin first. */
  messages: Message[];
  /** Oldest first. */
  notes: PinNote[];
}

export type BackendEvent =
  | { type: "chats-changed" }
  | { type: "message"; message: Message }
  | { type: "notice"; notice: Notice }
  /** Older messages of a chat came from the phone. */
  | { type: "history"; chatId: string }
  /** A group's name, description or members changed. */
  | { type: "group"; chatId: string }
  /** A chat was cleared or deleted, here or on another device. */
  | { type: "cleared"; chatId: string }
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

/** A sticker kept to send: recent, favorite or in a pack. */
export interface LibrarySticker {
  id: string;
  width: number;
  height: number;
  /** Unknown until its file has been looked at. */
  animated?: boolean;
  favorite: boolean;
  emojis: string[];
}

export interface StickerPack {
  id: string;
  name: string;
  publisher: string;
  /** One of WhatsApp's own packs; otherwise made here, and can be changed. */
  store: boolean;
  /** The pack's tray picture, for a store pack. */
  tray?: string;
  stickers: LibrarySticker[];
}

export interface StickerLibrary {
  recent: LibrarySticker[];
  favorites: LibrarySticker[];
  packs: StickerPack[];
  /** GIFs we sent, the latest first. */
  gifs: { id: string; width: number; height: number }[];
}

/** A pack in WhatsApp's sticker store. */
export interface StorePack {
  id: string;
  name: string;
  publisher: string;
  description: string;
  animated: boolean;
  tray?: string;
  previews: string[];
  /** Bytes. */
  size: number;
  installed: boolean;
}

/** A GIF found by search, as it is sent. */
export interface GifPick {
  slug: string;
  mp4: string;
  width: number;
  height: number;
  still?: string;
}

export interface GifResult {
  slug: string;
  title: string;
  /** A small animated picture for the grid. */
  preview: string;
  width: number;
  height: number;
  send: GifPick;
}

/** A sticker or GIF to send. */
export type OutgoingMedia =
  | { kind: "sticker"; id: string }
  /** A picture made into a sticker as it is sent. */
  | { kind: "new-sticker"; picture: Blob }
  | { kind: "gif"; pick: GifPick }
  | { kind: "recent-gif"; id: string };

/** A sticker in the library, or the one in a message. */
export type StickerTarget = { id: string } | { chatId: string; messageId: string };

/** An account's stickers: recent, favorites (synced with the phone where it can) and packs. */
export interface StickerShelf {
  library(): Promise<StickerLibrary>;
  /** Downloads a library sticker or GIF if needed and returns where to load it from. */
  fileUrl(id: string): Promise<string>;
  setFavorite(target: StickerTarget, favorite: boolean): Promise<void>;
  /** The packs in the network's sticker store. */
  store(): Promise<StorePack[]>;
  installPack(id: string): Promise<void>;
  removePack(id: string): Promise<void>;
  /** Makes an empty pack of our own; returns its id. */
  createPack(name: string): Promise<string>;
  renamePack(id: string, name: string): Promise<void>;
  addToPack(packId: string, target: StickerTarget): Promise<void>;
  removeFromPack(packId: string, id: string): Promise<void>;
  /** Makes a sticker from a picture and puts it in one of our packs. */
  addNewSticker(packId: string, picture: Blob): Promise<void>;
  /** Called whenever the library changes, here or on the phone. */
  subscribe(listener: () => void): () => void;
}

export interface ChatBackend {
  readonly account: AccountId;
  /** Sample chats instead of a linked account. */
  readonly demo: boolean;
  listChats(): Promise<Chat[]>;
  getMessages(chatId: string): Promise<Message[]>;
  /** Messages from before `beforeMs`, oldest first: the history past what `getMessages` gives. */
  olderMessages?(chatId: string, beforeMs: number): Promise<Message[]>;
  /** The messages pinned in a chat for everyone, the latest pin first, and who pinned what. */
  pinnedMessages?(chatId: string): Promise<Pins>;
  /** Pins a message for everyone for `secs`, or unpins it with `null`. */
  pinMessage?(chatId: string, messageId: string, secs: number | null): Promise<void>;
  /** Puts a chat first in line for its history to be fetched from the phone. */
  fetchHistoryFirst?(chatId: string): Promise<void>;
  /**
   * Sends a text, quoting the message `replyTo` when given, with a card for
   * its link when given. `forwarded` is the forwarding score of a message
   * sent on from another chat.
   */
  sendText(chatId: string, text: string, replyTo?: string, link?: OutgoingLink, forwarded?: number): Promise<Message>;
  /** Sends a sticker or GIF. */
  sendMedia?(chatId: string, item: OutgoingMedia, replyTo?: string): Promise<Message>;
  /** Stickers kept to send; absent where stickers can't be sent. */
  readonly stickers?: StickerShelf;
  /** Uploads a file and sends it, with `caption` under it. */
  sendFile?(chatId: string, file: File, caption?: string, replyTo?: string, forwarded?: number): Promise<Message>;
  /**
   * Deletes a message for us, or for everyone when it is ours. Returns the
   * placeholder left behind, or `null` when nothing is shown any more.
   */
  deleteMessage?(chatId: string, messageId: string, forEveryone: boolean): Promise<Message | null>;
  /** Changes what one of our texts says, for everyone. */
  editMessage?(chatId: string, messageId: string, text: string): Promise<Message>;
  /** Votes on a poll for the options at `options`; none takes the vote back. */
  votePoll?(chatId: string, messageId: string, options: number[]): Promise<Message>;
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
  /** Leaves a group; its chat stays, read-only. */
  leaveGroup?(id: string): Promise<void>;
  /** Renames a group or changes its description (empty removes it). */
  editGroup?(id: string, changes: { name?: string; description?: string }): Promise<GroupInfo>;
  /** Adds, removes, promotes or demotes members; fails naming who could not be changed. */
  changeMembers?(id: string, change: MemberChange, members: string[]): Promise<GroupInfo>;
  /** Blocks someone, or unblocks them. */
  setBlocked?(id: string, blocked: boolean): Promise<void>;
  /** Deletes a chat and its messages. */
  deleteChat?(chatId: string): Promise<void>;
  /** Empties a chat of its messages, keeping it in the list; starred ones stay when `keepStarred`. */
  clearChat?(chatId: string, keepStarred: boolean): Promise<void>;
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
