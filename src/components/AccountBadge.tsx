import { ACCOUNTS, type AccountId } from "../accounts/accounts";
import { SignalIcon, TelegramIcon, WhatsAppBusinessIcon, WhatsAppIcon } from "./Icons";

export const ACCOUNT_ICONS = { whatsapp: WhatsAppIcon, business: WhatsAppBusinessIcon, signal: SignalIcon, telegram: TelegramIcon } as const;

/** The network's mark on its colour, for places where accounts are mixed. */
export function AccountBadge({ account, size = 18 }: { account: AccountId; size?: number }) {
  const info = ACCOUNTS[account];
  const Icon = ACCOUNT_ICONS[account];
  return (
    <span
      className="flex shrink-0 items-center justify-center rounded-full text-white"
      style={{ width: size, height: size, background: info.color }}
      role="img"
      aria-label={info.label}
      title={info.label}
    >
      <Icon size={size * 0.72} strokeWidth={2} />
    </span>
  );
}
