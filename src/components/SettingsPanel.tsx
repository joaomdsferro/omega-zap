import { useEffect, useState, type ReactNode } from "react";
import { ACCOUNTS, PLANNED_ACCOUNTS, type AccountId } from "../accounts/accounts";
import type { Account } from "../accounts/useAccount";
import type { Microphone, Privacy, PrivacyCategory } from "../backend";
import { version } from "../../package.json";
import { modKey } from "../lib/platform";
import {
  SIDEBAR_DEFAULT,
  THEMES,
  TRAY_BADGES,
  ZOOM_STEPS,
  useSettings,
  useSidebarWidth,
} from "../settings/settings";
import { AccountBadge } from "./AccountBadge";
import { BackIcon } from "./Icons";
import { OwnAvatar } from "./OwnAvatar";

interface SettingsPanelProps {
  accounts: Account[];
  onBack: () => void;
  onLink: (account: AccountId) => void;
  /** WhatsApp's privacy settings, while it is connected. */
  privacy?: {
    load: () => Promise<Privacy>;
    save: (category: PrivacyCategory, value: string) => Promise<void>;
  };
  /** Lists the microphones voice messages can be recorded with; absent where none can be. */
  microphones?: () => Promise<Microphone[]>;
}

const LAST_SEEN_OPTIONS = [
  { value: "all", label: "Everyone" },
  { value: "contacts", label: "My contacts" },
  { value: "none", label: "Nobody" },
];

const ONLINE_OPTIONS = [
  { value: "all", label: "Everyone" },
  { value: "match_last_seen", label: "Same as last seen" },
];

function Choice({
  label,
  name,
  options,
  value,
  disabled,
  stacked,
  onChange,
}: {
  label: string;
  name: string;
  options: { value: string; label: string }[];
  value?: string;
  disabled?: boolean;
  /** One option per line, for long labels. */
  stacked?: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <fieldset className="px-3 py-2.5" disabled={disabled}>
      <legend className="text-[15px] text-(--text-primary)">{label}</legend>
      <div className={`mt-1.5 flex gap-x-5 gap-y-1 ${stacked ? "flex-col" : "flex-wrap"}`}>
        {options.map((option) => (
          <label key={option.value} className="flex items-center gap-2 py-1 text-[14px] text-(--text-primary)">
            <input
              type="radio"
              name={name}
              checked={value === option.value}
              onChange={() => onChange(option.value)}
              className="size-4 accent-(--accent)"
            />
            {option.label}
          </label>
        ))}
      </div>
    </fieldset>
  );
}

/** Who sees our last seen, online and read receipts: the account's own settings, changed on WhatsApp itself. */
function WhatsAppPrivacy({ load, save }: NonNullable<SettingsPanelProps["privacy"]>) {
  const [privacy, setPrivacy] = useState<Privacy | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    load()
      .then((p) => alive && setPrivacy(p))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [load]);

  async function change(category: PrivacyCategory, key: keyof Privacy, value: string) {
    if (!privacy) return;
    const before = privacy;
    setPrivacy({ ...privacy, [key]: value });
    setBusy(true);
    setError(null);
    try {
      await save(category, value);
    } catch (e) {
      setPrivacy(before);
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  if (!privacy) {
    return (
      <p className="px-3 py-2.5 text-[14px] text-(--text-secondary)">
        {error ?? "Loading WhatsApp's privacy settings…"}
      </p>
    );
  }
  // "My contacts except…" is set on the phone, with its list of people.
  const lastSeen =
    privacy.lastSeen === "contact_blacklist"
      ? [...LAST_SEEN_OPTIONS.slice(0, 2), { value: "contact_blacklist", label: "My contacts except…" }, LAST_SEEN_OPTIONS[2]]
      : LAST_SEEN_OPTIONS;
  return (
    <>
      <Choice
        label="Last seen"
        name="privacy-last-seen"
        options={lastSeen}
        value={privacy.lastSeen}
        disabled={busy}
        onChange={(value) => void change("last", "lastSeen", value)}
      />
      <Choice
        label="Online"
        name="privacy-online"
        options={ONLINE_OPTIONS}
        value={privacy.online}
        disabled={busy}
        onChange={(value) => void change("online", "online", value)}
      />
      <Switch
        checked={privacy.readReceipts !== "none"}
        onChange={(on) => void change("readreceipts", "readReceipts", on ? "all" : "none")}
        label="Read receipts"
        description="Others see blue ticks when you read their messages. When off, you don't see theirs either; groups always send them."
      />
      <p className="px-3 pt-1 pb-2 text-[13px] text-(--text-secondary)">
        These are your WhatsApp account's own settings, the same as on your phone. If you don't share your last seen or
        online, you can't see other people's. You show as online only while the Omega Zap window is in front.
      </p>
      {error ? <p className="px-3 text-[13px] text-(--danger)" role="alert">{error}</p> : null}
    </>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="border-b border-(--divider) px-6 py-5">
      <h3 className="mb-4 text-[14px] font-medium text-(--accent-strong)">{title}</h3>
      {children}
    </section>
  );
}

const STATE_LABEL: Record<string, string> = {
  unsupported: "Only available in the desktop app",
  idle: "Not linked",
  starting: "Preparing…",
  qr: "Waiting for the QR scan",
  qr_expired: "QR code expired",
  connecting: "Connecting…",
  connected: "Connected",
  logged_out: "Unlinked from your phone",
  error: "Problem connecting",
};

function StepButton({ label, onClick, disabled, children }: { label: string; onClick: () => void; disabled?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      disabled={disabled}
      className="flex h-9 w-9 items-center justify-center rounded-full border border-(--divider) text-[18px] text-(--text-primary) hover:bg-(--hover-bg) disabled:opacity-40"
    >
      {children}
    </button>
  );
}

function AccountCard({ account, onLink }: { account: Account; onLink: () => void }) {
  const [confirmLogout, setConfirmLogout] = useState(false);
  const [busy, setBusy] = useState(false);
  const { status } = account;
  const info = ACCOUNTS[account.id];

  async function run(action: () => Promise<void>) {
    setBusy(true);
    try {
      await action();
    } catch (e) {
      console.error(e);
    } finally {
      setBusy(false);
      setConfirmLogout(false);
    }
  }

  return (
    <div className="flex items-start gap-4">
      {status.linked && status.me ? (
        <OwnAvatar account={account} size={48} badge="panel" />
      ) : (
        <AccountBadge account={account.id} size={48} />
      )}
      <div className="min-w-0 flex-1">
        <p className="text-[16px] text-(--text-primary)">
          {info.label}
          {status.linked && status.me?.name ? (
            <span className="text-(--text-secondary)"> · {status.me.name}</span>
          ) : null}
        </p>
        <p className="text-[14px] text-(--text-secondary)">
          {status.linked
            ? [status.me?.phone, STATE_LABEL[status.state]].filter(Boolean).join(" · ")
            : STATE_LABEL[status.state]}
        </p>
        {status.error && status.state !== "qr_expired" ? (
          <p className="mt-1 text-[13px] text-(--danger)">{status.error}</p>
        ) : null}

        <div className="mt-3 flex flex-wrap gap-2">
          {!status.linked && status.state !== "unsupported" ? (
            <button
              type="button"
              onClick={onLink}
              className="h-9 rounded-full bg-(--accent) px-4 text-[14px] font-medium text-(--on-accent) hover:brightness-110"
            >
              Link {info.label}
            </button>
          ) : null}
          {status.linked && status.state !== "connected" ? (
            <button
              type="button"
              disabled={busy}
              onClick={() => run(account.reconnect)}
              className="h-9 rounded-full border border-(--divider) px-4 text-[14px] text-(--text-primary) hover:bg-(--hover-bg) disabled:opacity-50"
            >
              Reconnect
            </button>
          ) : null}
          {status.linked && !confirmLogout ? (
            <button
              type="button"
              disabled={busy}
              onClick={() => setConfirmLogout(true)}
              className="h-9 rounded-full border border-(--divider) px-4 text-[14px] text-(--danger) hover:bg-(--hover-bg) disabled:opacity-50"
            >
              Log out
            </button>
          ) : null}
        </div>

        {confirmLogout ? (
          <div className="mt-3 rounded-lg bg-(--search-bg) p-3" role="alertdialog" aria-label="Confirm log out">
            <p className="text-[14px] text-(--text-primary)">{info.logoutConfirm}</p>
            <div className="mt-3 flex gap-2">
              <button
                type="button"
                disabled={busy}
                onClick={() => run(account.logout)}
                className="h-8 rounded-full bg-(--danger) px-4 text-[13px] font-medium text-white disabled:opacity-50"
              >
                {busy ? "Logging out…" : "Log out"}
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => setConfirmLogout(false)}
                className="h-8 rounded-full px-4 text-[13px] text-(--text-primary) hover:bg-(--hover-bg)"
              >
                Cancel
              </button>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description: string;
}

function Switch({ checked, onChange, label, description }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className="flex w-full items-center gap-4 rounded-lg px-3 py-2.5 text-left hover:bg-(--hover-bg)"
    >
      <span className="min-w-0 flex-1">
        <span className="block text-[15px] text-(--text-primary)">{label}</span>
        <span className="block text-[13px] text-(--text-secondary)">{description}</span>
      </span>
      <span
        className={[
          "flex h-6 w-11 shrink-0 items-center rounded-full px-0.5 transition-colors",
          checked ? "bg-(--accent)" : "bg-(--text-secondary)/40",
        ].join(" ")}
        aria-hidden
      >
        <span className={["h-5 w-5 rounded-full bg-white transition-transform", checked ? "translate-x-5" : ""].join(" ")} />
      </span>
    </button>
  );
}

/** Which microphone records voice messages. */
function MicrophoneChoice({ list }: { list: () => Promise<Microphone[]> }) {
  const { settings, setMicrophone } = useSettings();
  const [microphones, setMicrophones] = useState<Microphone[] | null>(null);
  useEffect(() => {
    let alive = true;
    list().then(
      (found) => alive && setMicrophones(found),
      () => alive && setMicrophones([]),
    );
    return () => {
      alive = false;
    };
  }, [list]);
  if (!microphones) return <p className="px-3 py-2.5 text-[14px] text-(--text-secondary)">Looking for microphones…</p>;
  if (!microphones.length) {
    return <p className="px-3 py-2.5 text-[14px] text-(--text-secondary)">Voice messages are recorded with the system's microphone.</p>;
  }
  const chosen = settings.microphone;
  const options = [
    { value: "", label: "System default" },
    ...microphones.map((m) => ({ value: m.id, label: m.name })),
    // One picked before and unplugged since: recording falls back to the system's meanwhile.
    ...(chosen && !microphones.some((m) => m.id === chosen) ? [{ value: chosen, label: "The one picked before (not connected)" }] : []),
  ];
  return (
    <Choice label="Microphone" name="microphone" stacked options={options} value={chosen ?? ""} onChange={(id) => setMicrophone(id || null)} />
  );
}

export function SettingsPanel({ accounts, onBack, onLink, privacy, microphones }: SettingsPanelProps) {
  const {
    settings,
    setTheme,
    zoomIn,
    zoomOut,
    resetZoom,
    setUnifiedInbox,
    setLargeLinkPreviews,
    setFetchLinkPreviews,
    setInlineVideos,
    setNotifications,
    setNotificationPreview,
    setTrayBadge,
    setNotificationOpensChat,
    setSendTyping,
  } = useSettings();
  const { width: sidebarWidth, setWidth: setSidebarWidth } = useSidebarWidth();

  const zoomIndex = ZOOM_STEPS.indexOf(settings.zoom);

  return (
    <div className="flex h-full flex-col bg-(--panel-bg)">
      <header className="flex h-15 shrink-0 items-center gap-4 px-4">
        <button
          type="button"
          onClick={onBack}
          aria-label="Back to chats"
          className="flex h-9 w-9 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
        >
          <BackIcon size={22} />
        </button>
        <h1 className="font-display text-[22px] font-bold text-(--text-primary)">Settings</h1>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <Section title="Accounts">
          <div className="flex flex-col gap-6">
            {accounts.map((account) => (
              <AccountCard key={account.id} account={account} onLink={() => onLink(account.id)} />
            ))}
          </div>
          <p className="mt-5 text-[13px] text-(--text-secondary)">
            Coming later: {PLANNED_ACCOUNTS.join(", ")}.
          </p>

          <div className="mt-5">
            <Switch
              checked={settings.unifiedInbox}
              onChange={setUnifiedInbox}
              label="Show all chats together"
              description="One list for every linked account, each chat marked with its network. When off, you switch between accounts from the side rail."
            />
          </div>
        </Section>

        <Section title="Privacy">
          {privacy ? <WhatsAppPrivacy load={privacy.load} save={privacy.save} /> : null}
          <Switch
            checked={settings.sendTyping}
            onChange={setSendTyping}
            label="Show when I'm typing"
            description="Others see “typing…” while you write here. This one is only Omega Zap's: your phone still shows it."
          />
        </Section>

        <Section title="Notifications">
          <Switch
            checked={settings.notifications}
            onChange={setNotifications}
            label="Message notifications"
            description="New messages, replies and reactions to your messages, except in muted chats and the chat you have open."
          />
          {settings.notifications ? (
            <Switch
              checked={settings.notificationPreview}
              onChange={setNotificationPreview}
              label="Show previews"
              description="Show what the message says. When off, notifications only name the chat."
            />
          ) : null}
          {settings.notifications ? (
            <Switch
              checked={settings.notificationOpensChat}
              onChange={setNotificationOpensChat}
              label="Open the chat from a notification"
              description="Clicking a notification opens its chat, which marks it as read. When off, the chat is pointed out in the list instead."
            />
          ) : null}
          <div className="flex items-center gap-4 rounded-lg px-3 py-2.5">
            <span className="min-w-0 flex-1">
              <span className="block text-[15px] text-(--text-primary)">Tray badge</span>
              <span className="block text-[13px] text-(--text-secondary)">
                Colour of the unread count on the tray icon.
              </span>
            </span>
            <div className="flex shrink-0 gap-2" role="radiogroup" aria-label="Tray badge colour">
              {TRAY_BADGES.map((badge) => {
                const active = settings.trayBadge === badge.id;
                return (
                  <button
                    key={badge.id}
                    type="button"
                    role="radio"
                    aria-checked={active}
                    aria-label={badge.label}
                    title={badge.label}
                    onClick={() => setTrayBadge(badge.id)}
                    className={[
                      "flex h-8 min-w-8 items-center justify-center rounded-full px-2 text-[13px] font-bold text-white",
                      "ring-offset-2 ring-offset-(--panel-bg) transition-shadow",
                      active ? "ring-2 ring-(--accent)" : "hover:ring-2 hover:ring-(--divider)",
                    ].join(" ")}
                    style={{ background: `linear-gradient(${badge.gradient[0]}, ${badge.gradient[1]})` }}
                  >
                    3
                  </button>
                );
              })}
            </div>
          </div>
        </Section>

        <Section title="Links">
          <Switch
            checked={settings.largeLinkPreviews}
            onChange={setLargeLinkPreviews}
            label="Large link pictures"
            description="Download the full-size picture of a link card from WhatsApp. When off, cards keep the small picture that comes with the message."
          />
          <Switch
            checked={settings.inlineVideos}
            onChange={setInlineVideos}
            label="Play videos in the chat"
            description="TikTok, Instagram, YouTube Shorts and X videos play here, loaded from their site when you press play (for X, the post is looked up when its card shows). When off, they open in your browser."
          />
          <Switch
            checked={settings.fetchLinkPreviews}
            onChange={setFetchLinkPreviews}
            label="Cards for links without one"
            description="When a link arrives with no card, read its title and picture from the site, once, and keep them on this computer. The site sees your IP address, as it would if you opened the link."
          />
        </Section>

        {microphones ? (
          <Section title="Voice messages">
            <MicrophoneChoice list={microphones} />
          </Section>
        ) : null}

        <Section title="Theme">
          <div className="flex flex-col gap-1" role="radiogroup" aria-label="Theme">
            {THEMES.map((theme) => {
              const active = settings.theme === theme.id;
              return (
                <button
                  key={theme.id}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  onClick={() => setTheme(theme.id)}
                  className={[
                    "flex items-center gap-4 rounded-lg px-3 py-2.5 text-left transition-colors",
                    active ? "bg-(--selected-bg)" : "hover:bg-(--hover-bg)",
                  ].join(" ")}
                >
                  <span
                    className="flex h-9 w-12 shrink-0 overflow-hidden rounded-md border border-(--divider)"
                    aria-hidden
                  >
                    {theme.swatch.map((color) => (
                      <span key={color} className="flex-1" style={{ background: color }} />
                    ))}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block text-[15px] text-(--text-primary)">{theme.label}</span>
                    <span className="block text-[13px] text-(--text-secondary)">{theme.description}</span>
                  </span>
                  <span
                    className={[
                      "flex h-5 w-5 shrink-0 items-center justify-center rounded-full border-2",
                      active ? "border-(--accent)" : "border-(--text-secondary)",
                    ].join(" ")}
                    aria-hidden
                  >
                    {active ? <span className="h-2.5 w-2.5 rounded-full bg-(--accent)" /> : null}
                  </span>
                </button>
              );
            })}
          </div>
        </Section>

        <Section title="Zoom">
          <div className="flex items-center gap-3">
            <StepButton label="Zoom out" onClick={zoomOut} disabled={zoomIndex === 0}>
              −
            </StepButton>
            <output className="w-14 text-center text-[15px] tabular-nums text-(--text-primary)" aria-live="polite">
              {Math.round(settings.zoom * 100)}%
            </output>
            <StepButton label="Zoom in" onClick={zoomIn} disabled={zoomIndex === ZOOM_STEPS.length - 1}>
              +
            </StepButton>
            <button
              type="button"
              onClick={resetZoom}
              disabled={settings.zoom === 1}
              className="ml-2 h-9 rounded-full px-3 text-[14px] text-(--accent-strong) hover:bg-(--hover-bg) disabled:opacity-40"
            >
              Reset
            </button>
          </div>
          <p className="mt-3 text-[13px] text-(--text-secondary)">
            {modKey} + and {modKey} − change the zoom, {modKey} 0 resets it. {modKey} + scroll works too.
          </p>
        </Section>

        <Section title="Chat list">
          <div className="flex items-center justify-between gap-3">
            <p className="text-[14px] text-(--text-primary)">
              Width <span className="text-(--text-secondary)">{sidebarWidth}px</span>
            </p>
            <button
              type="button"
              onClick={() => setSidebarWidth(SIDEBAR_DEFAULT)}
              disabled={sidebarWidth === SIDEBAR_DEFAULT}
              className="h-9 rounded-full px-3 text-[14px] text-(--accent-strong) hover:bg-(--hover-bg) disabled:opacity-40"
            >
              Reset width
            </button>
          </div>
          <p className="mt-2 text-[13px] text-(--text-secondary)">
            Drag the edge of the chat list to resize it. Double-click the edge to reset.
          </p>
        </Section>

        <Section title="About">
          <p className="text-[14px] text-(--text-primary)">Omega Zap {version}</p>
          <p className="mt-1 text-[13px] leading-5 text-(--text-secondary)">
            An unofficial WhatsApp and Signal client. Not affiliated with WhatsApp,
            Meta or Signal.
          </p>
        </Section>
      </div>
    </div>
  );
}
