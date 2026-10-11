import { matches } from "../lib/search";
import { DEMO_CHATS, DEMO_MESSAGES } from "../data/demo-chats";
import type { BackendEvent, Chat, ChatBackend, Contact, CustomList, Message, OutgoingLink, StarredMessage } from "./types";

function cloneChats(): Chat[] {
  return DEMO_CHATS.map((c) => ({ ...c, account: "whatsapp" }));
}

function cloneMessages(): Map<string, Message[]> {
  const map = new Map<string, Message[]>();
  for (const [id, msgs] of Object.entries(DEMO_MESSAGES)) {
    map.set(
      id,
      msgs.map((m) => ({
        ...m,
        replyTo: m.replyTo ? { ...m.replyTo } : undefined,
      })),
    );
  }
  return map;
}

export class MockBackend implements ChatBackend {
  readonly account = "whatsapp" as const;
  readonly demo = true;
  private chats = cloneChats();
  private messages = cloneMessages();
  private listeners = new Set<(event: BackendEvent) => void>();
  private seq = 1000;
  private favorites = new Set<string>();
  private lists: (CustomList & { chats: Set<string> })[] = [];

  async listChats(): Promise<Chat[]> {
    return [...this.chats]
      .map((c) => ({
        ...c,
        favorite: this.favorites.has(c.id),
        lists: this.lists.filter((l) => l.chats.has(c.id)).map((l) => l.id),
      }))
      .sort((a, b) => {
        if (a.pinned && !b.pinned) return -1;
        if (!a.pinned && b.pinned) return 1;
        return new Date(b.lastAt).getTime() - new Date(a.lastAt).getTime();
      });
  }

  async getMessages(chatId: string): Promise<Message[]> {
    return [...(this.messages.get(chatId) ?? [])];
  }

  async markRead(chatId: string): Promise<void> {
    const chat = this.chats.find((c) => c.id === chatId);
    if (chat && chat.unread > 0) {
      chat.unread = 0;
      this.emit({ type: "chats-changed" });
    }
  }

  async sendText(chatId: string, text: string, replyTo?: string, link?: OutgoingLink, forwarded?: number): Promise<Message> {
    const trimmed = text.trim();
    if (!trimmed) {
      throw new Error("Empty message");
    }
    const quoted = replyTo ? this.messages.get(chatId)?.find((m) => m.id === replyTo) : undefined;

    const message: Message = {
      id: `local-${++this.seq}`,
      chatId,
      kind: "text",
      sentAt: new Date().toISOString(),
      fromMe: true,
      text: trimmed,
      status: "sent",
      forwarded,
      linkPreview: link
        ? {
            url: link.url,
            title: link.title,
            description: link.description,
            thumbnail: link.thumbnail ? `data:image/jpeg;base64,${link.thumbnail}` : undefined,
          }
        : undefined,
      replyTo: quoted
        ? { author: quoted.fromMe ? "You" : (quoted.author ?? this.chats.find((c) => c.id === chatId)?.name ?? ""), fromMe: quoted.fromMe, id: quoted.id, text: quoted.text ?? quoted.mediaLabel ?? "Message" }
        : undefined,
    };

    const thread = this.messages.get(chatId) ?? [];
    thread.push(message);
    this.messages.set(chatId, thread);

    const chat = this.chats.find((c) => c.id === chatId);
    if (chat) {
      chat.lastMessage = trimmed;
      chat.lastFromMe = true;
      chat.lastStatus = "sent";
      chat.lastAt = message.sentAt;
      chat.unread = 0;
    }
    this.emit({ type: "chats-changed" });

    return { ...message };
  }

  async listLists(): Promise<CustomList[]> {
    return this.lists.map(({ id, name }) => ({ id, name }));
  }

  async setFavorite(chatId: string, favorite: boolean): Promise<void> {
    if (favorite) this.favorites.add(chatId);
    else this.favorites.delete(chatId);
    this.emit({ type: "chats-changed" });
  }

  async setMuted(chatId: string, muted: boolean): Promise<void> {
    this.update(chatId, { muted });
  }

  async setArchived(chatId: string, archived: boolean): Promise<void> {
    this.update(chatId, archived ? { archived, pinned: false } : { archived });
  }

  async setPinned(chatId: string, pinned: boolean): Promise<void> {
    if (pinned && this.chats.filter((c) => c.pinned && !c.archived).length >= 3) {
      throw new Error("You can only pin up to 3 chats.");
    }
    this.update(chatId, { pinned });
  }

  private update(chatId: string, patch: Partial<Chat>) {
    const chat = this.chats.find((c) => c.id === chatId);
    if (!chat) return;
    Object.assign(chat, patch);
    this.emit({ type: "chats-changed" });
  }

  async saveList(id: string | null, name: string, add: string[], remove: string[]): Promise<string> {
    let list = id ? this.lists.find((l) => l.id === id) : undefined;
    if (!list) {
      list = { id: `list-${++this.seq}`, name, chats: new Set() };
      this.lists.push(list);
    }
    list.name = name.trim();
    for (const chatId of remove) list.chats.delete(chatId);
    for (const chatId of add) list.chats.add(chatId);
    this.emit({ type: "chats-changed" });
    return list.id;
  }

  async deleteList(id: string): Promise<void> {
    this.lists = this.lists.filter((l) => l.id !== id);
    this.emit({ type: "chats-changed" });
  }

  async deleteMessage(chatId: string, messageId: string, forEveryone: boolean): Promise<Message | null> {
    const thread = this.messages.get(chatId) ?? [];
    const index = thread.findIndex((m) => m.id === messageId);
    if (index < 0) throw new Error("That message is gone.");
    this.emit({ type: "chats-changed" });
    if (!forEveryone) {
      thread.splice(index, 1);
      return null;
    }
    const { id, sentAt, fromMe, author, status } = thread[index];
    thread[index] = { id, chatId, kind: "text", sentAt, fromMe, author, status, deleted: true };
    return { ...thread[index] };
  }

  async setStarred(chatId: string, messageId: string, starred: boolean): Promise<Message> {
    const message = this.messages.get(chatId)?.find((m) => m.id === messageId);
    if (!message) throw new Error("That message is gone.");
    message.starred = starred;
    return { ...message };
  }

  async searchMessages(query: string, only?: string): Promise<StarredMessage[]> {
    return [...this.messages.entries()]
      .filter(([chatId]) => !only || chatId === only)
      .flatMap(([chatId, thread]) => {
        const chatName = this.chats.find((c) => c.id === chatId)?.name ?? "Chat";
        return thread
          .filter((m) => !m.deleted && matches(m.text ?? m.mediaLabel ?? "", query))
          .map((m) => ({ account: this.account, chatName, message: { ...m } }));
      })
      .sort((a, b) => b.message.sentAt.localeCompare(a.message.sentAt));
  }

  async listStarred(): Promise<StarredMessage[]> {
    return [...this.messages.entries()]
      .flatMap(([chatId, thread]) => {
        const chatName = this.chats.find((c) => c.id === chatId)?.name ?? "Chat";
        return thread.filter((m) => m.starred).map((m) => ({ account: this.account, chatName, message: { ...m } }));
      })
      .sort((a, b) => b.message.sentAt.localeCompare(a.message.sentAt));
  }

  async listContacts(): Promise<Contact[]> {
    const people: Contact[] = this.chats
      .filter((c) => !c.isGroup)
      .map((c) => ({
        id: c.id,
        account: this.account,
        name: c.name,
        initials: c.initials,
        avatarHue: c.avatarHue,
        avatarUrl: c.avatarUrl,
      }));
    return [...people, { id: "me", account: this.account, name: "You", initials: "ME", avatarHue: 160, isMe: true }];
  }

  subscribe(listener: (event: BackendEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(event: BackendEvent) {
    for (const listener of this.listeners) listener(event);
  }
}
