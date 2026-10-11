import type { StarredMessage } from "../backend";
import { ACCOUNTS } from "../accounts/accounts";
import { formatListTime } from "../lib/format";
import { Highlighted, snippet } from "../lib/search";
import { Ticks } from "./Icons";

interface MessageResultsProps {
  found: StarredMessage[];
  query: string;
  showAccount?: boolean;
  onOpen: (found: StarredMessage) => void;
}

/** Messages a search found, newest first: each opens its chat on it. */
export function MessageResults({ found, query, showAccount, onOpen }: MessageResultsProps) {
  return (
    <ul className="flex flex-col" aria-label="Messages">
      {found.map((item) => {
        const { message } = item;
        const text = snippet(message.text ?? message.mediaLabel ?? "", query);
        return (
          <li key={`${item.account}:${message.chatId}:${message.id}`}>
            <button
              type="button"
              onClick={() => onOpen(item)}
              className="flex h-18 w-full flex-col justify-center border-l-[3px] pr-4 pl-5.25 text-left transition-colors duration-100 hover:bg-(--hover-bg)"
              style={{ borderLeftColor: showAccount ? ACCOUNTS[item.account].color : "transparent" }}
            >
              <span className="flex items-baseline justify-between gap-2">
                <span className="truncate text-[16px] leading-5.25 text-(--text-primary)">{item.chatName}</span>
                <time className="shrink-0 text-[12px] text-(--text-secondary) tabular-nums" dateTime={message.sentAt}>
                  {formatListTime(message.sentAt)}
                </time>
              </span>
              <span className="mt-0.5 flex min-w-0 items-center gap-1.5 text-[14px] leading-5 text-(--text-secondary)">
                {message.fromMe ? (
                  <span className="shrink-0">
                    <Ticks status={message.status} account={item.account} />
                  </span>
                ) : null}
                <span className="min-w-0 truncate">
                  {!message.fromMe && message.author ? `${message.author}: ` : ""}
                  <Highlighted text={text} query={query} />
                </span>
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
