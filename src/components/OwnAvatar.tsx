import { ACCOUNTS } from "../accounts/accounts";
import type { Account } from "../accounts/useAccount";
import { hueFor, initialsFor } from "../lib/avatar";
import { fileUrl } from "../lib/files";
import { AccountBadge } from "./AccountBadge";
import { Avatar } from "./Avatar";

interface OwnAvatarProps {
  account: Account;
  size: number;
  /** Mark the picture with its network, naming the surface it sits on. */
  badge?: "panel" | "rail";
}

/** Our own profile picture on an account. */
export function OwnAvatar({ account, size, badge }: OwnAvatarProps) {
  const me = account.status.me;
  const name = me?.name ?? ACCOUNTS[account.id].label;
  return (
    <span className="relative block shrink-0" style={{ width: size, height: size }}>
      <Avatar
        name={name}
        initials={initialsFor(name)}
        hue={hueFor(account.id)}
        src={me?.avatar ? fileUrl(me.avatar, "avatar") : undefined}
        size={size}
      />
      {badge ? (
        <span
          className="absolute -right-1 -bottom-1 rounded-full border-2"
          style={{ borderColor: `var(--${badge}-bg)` }}
        >
          <AccountBadge account={account.id} size={Math.round(size * 0.4)} />
        </span>
      ) : null}
    </span>
  );
}
