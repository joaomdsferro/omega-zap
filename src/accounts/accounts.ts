export type AccountId = "whatsapp" | "business" | "signal" | "telegram";

export interface AccountInfo {
  id: AccountId;
  label: string;
  /** CSS colour that marks this network wherever chats are mixed. */
  color: string;
  linkSteps: string[];
  linkNote: string;
  logoutConfirm: string;
  /** Shown when a linked account has no chats yet. */
  emptyText: string;
}

export const ACCOUNTS: Record<AccountId, AccountInfo> = {
  whatsapp: {
    id: "whatsapp",
    label: "WhatsApp",
    color: "var(--brand-whatsapp)",
    linkSteps: [
      "Open WhatsApp on your phone",
      "Tap Menu (⋮) on Android or Settings on iPhone",
      "Tap Linked devices, then Link a device",
      "Point your phone at this screen to scan the QR code",
    ],
    linkNote:
      "Omega Zap is an unofficial client for everyday chatting. Bulk messaging and automation are what get accounts banned, so don't use it for that.",
    logoutConfirm:
      "Log out and remove this computer from your linked devices? Chats saved here are deleted.",
    emptyText: "Your chats will appear here once they sync.",
  },
  business: {
    id: "business",
    label: "WhatsApp Business",
    color: "var(--brand-business)",
    linkSteps: [
      "Open WhatsApp Business on your phone",
      "Tap Menu (⋮) on Android or Settings on iPhone",
      "Tap Linked devices, then Link a device",
      "Point your phone at this screen to scan the QR code",
    ],
    linkNote:
      "Omega Zap is an unofficial client for everyday chatting. Bulk messaging and automation are what get accounts banned, so don't use it for that.",
    logoutConfirm:
      "Log out and remove this computer from your linked devices? Chats saved here are deleted.",
    emptyText: "Your chats will appear here once they sync.",
  },
  signal: {
    id: "signal",
    label: "Signal",
    color: "var(--brand-signal)",
    linkSteps: [
      "Open Signal on your phone",
      "Tap your profile picture, then Linked devices",
      "Tap Link a new device",
      "Point your phone at this screen to scan the QR code",
      "Choose Transfer message history to bring your chats along",
    ],
    linkNote:
      "Omega Zap is an unofficial client. Your phone only offers the message history while linking: without it, chats appear as new messages arrive.",
    logoutConfirm:
      "Log out of Signal on this computer? Chats saved here are deleted. Signal only lets the phone remove a linked device, so also remove “Omega Zap” under Linked devices there.",
    emptyText:
      "No chats yet. Older messages only come over when you choose Transfer message history while linking; new ones appear as they arrive.",
  },
  telegram: {
    id: "telegram",
    label: "Telegram",
    color: "var(--brand-telegram)",
    linkSteps: [
      "Open Telegram on your phone",
      "Go to Settings, then Devices",
      "Tap Link Desktop Device",
      "Point your phone at this screen to scan the QR code",
    ],
    linkNote:
      "Omega Zap is an unofficial Telegram client. Your chats come from Telegram's servers, so older messages load as you scroll.",
    logoutConfirm:
      "Log out of Telegram on this computer? Telegram ends this session, and chats saved here are deleted.",
    emptyText: "Your chats will appear here once they load.",
  },
};

export const ACCOUNT_IDS = Object.keys(ACCOUNTS) as AccountId[];

/** Both WhatsApp apps speak the same protocol, so what one can do, the other can. */
export function isWhatsApp(id: AccountId): boolean {
  return id === "whatsapp" || id === "business";
}

/** Networks planned next; listed in Settings so the roadmap is visible. */
export const PLANNED_ACCOUNTS: string[] = [];
