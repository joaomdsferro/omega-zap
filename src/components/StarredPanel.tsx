import type { StarredMessage } from "../backend";
import { formatListTime, formatMessageTime } from "../lib/format";
import { AccountBadge } from "./AccountBadge";
import { BackIcon, StarIcon } from "./Icons";

interface StarredPanelProps {
  /** Newest first; `null` while they load. */
  items: StarredMessage[] | null;
  /** Mark each message with its network: they come from several accounts. */
  showAccount?: boolean;
  onBack: () => void;
  onOpen: (item: StarredMessage) => void;
  onUnstar?: (item: StarredMessage) => void;
}

function summary({ message }: StarredMessage) {
  if (message.kind === "voice") return "Voice message";
  const label = message.kind === "image" ? (message.imageLabel ?? "Photo") : message.mediaLabel;
  if (label && message.text) return `${label}: ${message.text}`;
  return label ?? message.text ?? "";
}

export function StarredPanel({ items, showAccount, onBack, onOpen, onUnstar }: StarredPanelProps) {
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
        <h1 className="text-[17px] font-medium text-(--text-primary)">Starred messages</h1>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {items === null ? (
          <p className="px-6 py-10 text-center text-[14px] text-(--text-secondary)">Loading…</p>
        ) : items.length === 0 ? (
          <div className="flex flex-col items-center px-8 py-16 text-center">
            <span className="mb-5 flex h-20 w-20 items-center justify-center rounded-full bg-(--search-bg) text-(--icon)">
              <StarIcon size={36} />
            </span>
            <p className="text-[15px] text-(--text-primary)">No starred messages</p>
            <p className="mt-2 text-[14px] leading-5 text-(--text-secondary)">
              Hover a message and press the star to find it here later.
            </p>
          </div>
        ) : (
          <ul aria-label="Starred messages">
            {items.map((item) => {
              const { message } = item;
              const from = message.fromMe ? "You" : (message.author ?? item.chatName);
              const where = message.fromMe || message.author ? item.chatName : null;
              return (
                <li key={`${item.account}:${message.chatId}:${message.id}`} className="group/starred relative border-b border-(--divider)">
                  <button
                    type="button"
                    onClick={() => onOpen(item)}
                    aria-label={`${from}${where ? ` in ${where}` : ""}: ${summary(item)}`}
                    className="flex w-full flex-col gap-2 px-4 py-3 text-left hover:bg-(--hover-bg)"
                  >
                    <span className="flex w-full items-center gap-2 text-[13.5px]">
                      {showAccount ? <AccountBadge account={item.account} size={16} /> : null}
                      <span className="min-w-0 truncate text-(--text-primary)">
                        {from}
                        {where ? <span className="text-(--text-secondary)"> ▸ {where}</span> : null}
                      </span>
                      <time className="ml-auto shrink-0 text-[12px] text-(--text-secondary)" dateTime={message.sentAt}>
                        {formatListTime(message.sentAt)}
                      </time>
                    </span>
                    <span
                      className={[
                        "max-w-[92%] rounded-[7.5px] px-2.5 pt-1.5 pb-1 text-[14.2px] leading-4.75 text-(--text-primary)",
                        message.fromMe ? "self-end" : "self-start",
                      ].join(" ")}
                      style={{
                        background: message.fromMe ? "var(--bubble-out)" : "var(--bubble-in)",
                        boxShadow: "var(--bubble-shadow)",
                      }}
                    >
                      <span className="line-clamp-4 wrap-break-word whitespace-pre-wrap">{summary(item)}</span>
                      <span className="mt-0.5 flex items-center justify-end gap-1 text-[11px] text-(--bubble-meta)">
                        <StarIcon size={11} filled />
                        {formatMessageTime(message.sentAt)}
                      </span>
                    </span>
                  </button>
                  {onUnstar ? (
                    <button
                      type="button"
                      onClick={() => onUnstar(item)}
                      aria-label="Unstar message"
                      title="Unstar"
                      className="absolute top-2.5 right-24 flex h-7 w-7 items-center justify-center rounded-full bg-(--panel-bg) text-(--icon) opacity-0 transition-opacity group-hover/starred:opacity-100 hover:bg-(--hover-bg) focus-visible:opacity-100"
                    >
                      <StarIcon size={16} filled />
                    </button>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}
