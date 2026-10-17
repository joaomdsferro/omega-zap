import whatsappLogo from "../assets/whatsapp-logo.svg";
import businessLogo from "../assets/whatsapp-business-logo.png";
import signalLogo from "../assets/signal-logo.svg";
import type { AccountId } from "../accounts/accounts";

/**
 * The apps' logos (Wikimedia Commons, public domain), for their own chats:
 * the server has no picture for those, and the phone draws the logo.
 */
const LOGOS: Partial<Record<AccountId, string>> = { whatsapp: whatsappLogo, business: businessLogo, signal: signalLogo };

/** Each app's own chat: WhatsApp's tips and notices, Signal's release notes. */
const OFFICIAL_CHATS: Partial<Record<AccountId, string>> = {
  whatsapp: "0@s.whatsapp.net",
  business: "0@s.whatsapp.net",
  signal: "signal-release-notes",
};

export function isOfficialChat(account: AccountId, chatId: string): boolean {
  return OFFICIAL_CHATS[account] === chatId;
}

/** The logo for an app's own chat. */
export function officialLogo(account: AccountId, chatId: string): string | undefined {
  return isOfficialChat(account, chatId) ? LOGOS[account] : undefined;
}

/** Whether a picture is one of the logos drawn inside the avatar rather than filling it. */
export function isLogo(src: string): boolean {
  return src === whatsappLogo || src === businessLogo;
}

/** Only Signal writes in its chat; WhatsApp's takes replies. */
export function isReadOnlyChat(account: AccountId, chatId: string): boolean {
  return account === "signal" && isOfficialChat(account, chatId);
}
