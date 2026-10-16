import whatsappLogo from "../assets/whatsapp-logo.svg";
import businessLogo from "../assets/whatsapp-business-logo.png";
import type { AccountId } from "../accounts/accounts";

/**
 * The apps' logos (Wikimedia Commons, public domain), for WhatsApp's own
 * chat: the server has no picture for it, and the phone draws the logo.
 */
const LOGOS: Partial<Record<AccountId, string>> = { whatsapp: whatsappLogo, business: businessLogo };

export function officialLogo(account: AccountId): string | undefined {
  return LOGOS[account];
}

/** Whether a picture is one of these logos, drawn inside the avatar rather than filling it. */
export function isLogo(src: string): boolean {
  return src === whatsappLogo || src === businessLogo;
}
