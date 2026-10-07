import type { ReactNode } from "react";
import { ACCOUNTS, type AccountId } from "../accounts/accounts";
import type { Account } from "../accounts/useAccount";
import { ACCOUNT_ICONS } from "./AccountBadge";
import { ChatsIcon, SettingsIcon } from "./Icons";
import { OwnAvatar } from "./OwnAvatar";

export type SidebarView = "chats" | "settings";
export type Connection = "demo" | "online" | "offline" | "unlinked";

export interface RailAccount {
  id: AccountId;
  unread: number;
  connection: Connection;
}

interface NavRailProps {
  view: SidebarView;
  onChange: (view: SidebarView) => void;
  accounts: RailAccount[];
  /** The account whose chats are open, or `null` when all share one list. */
  active: AccountId | null;
  onSelectAccount: (account: AccountId) => void;
  /** The accounts whose own profile picture is shown, above Settings. */
  profiles: Account[];
}

const CONNECTION: Record<Connection, { color: string; label: string }> = {
  online: { color: "var(--accent)", label: "Connected" },
  offline: { color: "var(--danger)", label: "Not connected" },
  demo: { color: "var(--text-secondary)", label: "Demo mode" },
  unlinked: { color: "var(--text-secondary)", label: "Not linked" },
};

function RailButton({
  active,
  label,
  onClick,
  children,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      aria-current={active ? "page" : undefined}
      title={label}
      className={[
        "relative flex h-10 w-10 items-center justify-center rounded-full text-(--icon) transition-colors duration-150",
        active ? "bg-(--rail-active)" : "hover:bg-(--rail-active)/60",
      ].join(" ")}
    >
      {children}
    </button>
  );
}

function UnreadBadge({ count }: { count: number }) {
  if (count <= 0) return null;
  return (
    <span className="absolute -top-0.5 -right-0.5 flex h-4.5 min-w-4.5 items-center justify-center rounded-full bg-(--badge-bg) px-1 text-[11px] font-semibold text-(--badge-text)">
      {count > 99 ? "99+" : count}
    </span>
  );
}

export function NavRail({ view, onChange, accounts, active, onSelectAccount, profiles }: NavRailProps) {
  const linked = accounts.filter((a) => a.connection !== "unlinked");
  // One dot for the shared list: red as soon as any linked account is down.
  const overall: Connection =
    linked.find((a) => a.connection === "offline")?.connection ?? linked[0]?.connection ?? "demo";

  return (
    <nav
      className="flex h-full w-16 shrink-0 flex-col items-center justify-between border-r border-(--divider) bg-(--rail-bg) py-3"
      aria-label="Main"
    >
      <div className="flex flex-col items-center gap-2">
        {active === null ? (
          <RailButton active={view === "chats"} label="Chats" onClick={() => onChange("chats")}>
            <ChatsIcon size={22} />
            <UnreadBadge count={accounts.reduce((sum, a) => sum + a.unread, 0)} />
          </RailButton>
        ) : (
          accounts.map((account) => {
            const info = ACCOUNTS[account.id];
            const Icon = ACCOUNT_ICONS[account.id];
            const selected = view === "chats" && active === account.id;
            const status = CONNECTION[account.connection];
            return (
              <RailButton
                key={account.id}
                active={selected}
                label={`${info.label} (${status.label.toLowerCase()})`}
                onClick={() => onSelectAccount(account.id)}
              >
                <span style={{ color: selected ? info.color : undefined }}>
                  <Icon size={22} />
                </span>
                <UnreadBadge count={account.unread} />
                {account.connection !== "unlinked" ? (
                  <span
                    className="absolute right-0.5 bottom-0.5 h-2.5 w-2.5 rounded-full border-2 border-(--rail-bg)"
                    style={{ background: status.color }}
                  />
                ) : null}
              </RailButton>
            );
          })
        )}
      </div>
      <div className="flex flex-col items-center gap-3">
        {active === null ? (
          <span
            className="h-2.5 w-2.5 rounded-full"
            style={{ background: CONNECTION[overall].color }}
            title={CONNECTION[overall].label}
            role="status"
            aria-label={CONNECTION[overall].label}
          />
        ) : null}
        {profiles.map((account) => {
          const { label } = ACCOUNTS[account.id];
          const name = account.status.me?.name;
          return (
            <RailButton
              key={account.id}
              active={false}
              label={name ? `${name} on ${label}` : `Your ${label} profile`}
              onClick={() => onChange("settings")}
            >
              <OwnAvatar account={account} size={30} badge={profiles.length > 1 ? "rail" : undefined} />
            </RailButton>
          );
        })}
        <RailButton active={view === "settings"} label="Settings" onClick={() => onChange("settings")}>
          <SettingsIcon size={22} />
        </RailButton>
      </div>
    </nav>
  );
}
