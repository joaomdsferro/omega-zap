import type { Chat, Message } from "../backend/types";

export const DEMO_CHATS: Omit<Chat, "account">[] = [];

export const DEMO_MESSAGES: Record<string, Message[]> = {};
