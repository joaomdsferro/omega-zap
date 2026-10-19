import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { Poll } from "../backend";
import { CheckIcon, CloseIcon } from "./Icons";

interface PollCardProps {
  question: string;
  poll: Poll;
  /** Sends our vote: the options picked, none to take it back. */
  onVote?: (options: number[]) => Promise<void>;
}

/** A poll as WhatsApp Web shows it: each option with its share of the votes, ticked when ours. */
export function PollCard({ question, poll, onVote }: PollCardProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [details, setDetails] = useState(false);
  const single = poll.selectable === 1;
  const most = Math.max(1, ...poll.options.map((o) => o.people.length));
  const canVote = Boolean(onVote) && poll.open;

  async function pick(index: number) {
    if (!onVote || busy) return;
    const mine = poll.options.flatMap((o, i) => (o.mine ? [i] : []));
    let next: number[];
    if (mine.includes(index)) next = mine.filter((i) => i !== index);
    else if (single) next = [index];
    else next = [...mine, index];
    if (poll.selectable > 1 && next.length > poll.selectable) {
      setError(`Pick up to ${poll.selectable}.`);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onVote(next);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="mb-1 w-75 max-w-full px-0.5">
      <p className="text-[15px] font-semibold wrap-break-word text-(--text-primary)">{question}</p>
      <p className="mt-0.5 mb-2 text-[12.5px] text-(--bubble-meta)">
        {single ? "Select one" : poll.selectable > 1 ? `Select up to ${poll.selectable}` : "Select one or more"}
        {!poll.open && onVote ? " · vote from your phone" : null}
      </p>
      <ul className="flex flex-col gap-2.5" role={single ? "radiogroup" : "group"} aria-label={question}>
        {poll.options.map((option, index) => (
          <li key={index}>
            <button
              type="button"
              role={single ? "radio" : "checkbox"}
              aria-checked={option.mine}
              disabled={!canVote || busy}
              onClick={() => void pick(index)}
              className="flex w-full items-start gap-2.5 text-left"
            >
              <span
                className={[
                  "mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full border-2",
                  option.mine ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--bubble-meta)",
                ].join(" ")}
              >
                {option.mine ? <CheckIcon size={13} strokeWidth={3} /> : null}
              </span>
              <span className="min-w-0 flex-1">
                <span className="flex items-baseline gap-2">
                  <span className="min-w-0 flex-1 text-[14.2px] wrap-break-word text-(--text-primary)">{option.name}</span>
                  <span className="text-[13px] tabular-nums text-(--bubble-meta)">{option.people.length}</span>
                </span>
                <span className="mt-1 block h-1.5 overflow-hidden rounded-full bg-[color-mix(in_oklab,var(--bubble-meta)_25%,transparent)]">
                  <span
                    className="block h-full rounded-full bg-(--accent) transition-[width] duration-300"
                    style={{ width: `${(option.people.length / most) * 100}%` }}
                  />
                </span>
              </span>
            </button>
          </li>
        ))}
      </ul>
      {error ? <p className="mt-2 text-[12.5px] text-(--danger)">{error}</p> : null}
      <div className="-mx-2.5 mt-2.5 border-t border-(--divider)">
        <button
          type="button"
          onClick={() => setDetails(true)}
          disabled={poll.voters === 0}
          className="w-full pt-2 text-center"
        >
          {/* On the span: the app's button reset outranks text utilities on a button. */}
          <span className={poll.voters === 0 ? "text-[14px] text-(--bubble-meta)" : "text-[14px] text-(--accent)"}>
            {poll.voters === 0 ? "No votes yet" : "View votes"}
          </span>
        </button>
      </div>
      {details ? <PollVotes question={question} poll={poll} onClose={() => setDetails(false)} /> : null}
    </div>
  );
}

/** Who picked each option. */
function PollVotes({ question, poll, onClose }: { question: string; poll: Poll; onClose: () => void }) {
  const dialog = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    dialog.current?.querySelector<HTMLElement>("button")?.focus();
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, []);
  const ranked = poll.options.map((option, index) => ({ ...option, index })).sort((a, b) => b.people.length - a.people.length);
  // On the body: the message list would clip a fixed layer drawn inside it.
  return createPortal(
    <div
      className="animate-fade-in fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4"
      onPointerDown={(event) => event.target === event.currentTarget && onClose()}
      onClick={(event) => event.stopPropagation()}
    >
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="poll-votes-title"
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          }
        }}
        className="animate-fade-rise flex max-h-[80vh] w-full max-w-110 flex-col overflow-hidden rounded-xl bg-(--panel-bg) text-left"
        style={{ boxShadow: "0 17px 50px rgba(11,20,26,.19), 0 12px 15px rgba(11,20,26,.24)" }}
      >
        <header className="flex h-14 shrink-0 items-center gap-3 bg-(--panel-header-bg) px-3">
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="flex h-10 w-10 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
          >
            <CloseIcon size={22} />
          </button>
          <h2 id="poll-votes-title" className="text-[16px] text-(--text-primary)">
            Poll details
          </h2>
        </header>
        <div className="overflow-y-auto px-5 py-4">
          <p className="mb-4 text-[15px] font-semibold wrap-break-word text-(--text-primary)">{question}</p>
          {ranked.map((option) => (
            <section key={option.index} className="mb-4">
              <h3 className="flex items-baseline gap-2 border-b border-(--divider) pb-1.5 text-[14.5px] text-(--text-primary)">
                <span className="min-w-0 flex-1 wrap-break-word">{option.name}</span>
                <span className="text-[13px] tabular-nums text-(--text-secondary)">
                  {option.people.length} {option.people.length === 1 ? "vote" : "votes"}
                </span>
              </h3>
              <ul className="mt-1.5">
                {option.people.map((name, i) => (
                  <li key={i} className="py-1 text-[14px] text-(--text-primary)">
                    {name}
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      </div>
    </div>,
    document.body,
  );
}
