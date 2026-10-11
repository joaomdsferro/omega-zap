import type { Backfill } from "../accounts/useAccount";

/**
 * Where fetching an account's whole history from the phone has got to: the
 * chat it is on, how many chats are complete and how many messages came.
 * Gone once every chat is complete.
 */
export function HistoryProgress({ label, backfill }: { label: string; backfill: Backfill }) {
  if (backfill.state === "done") return null;
  const waiting = backfill.state === "waitingForPhone";
  const share = backfill.total ? backfill.done / backfill.total : 0;
  const counts = `${backfill.done.toLocaleString()} of ${backfill.total.toLocaleString()} chats · ${backfill.messages.toLocaleString()} messages`;
  return (
    <div className="shrink-0 border-t border-(--divider) px-4 pt-2 pb-2.5" role="status" aria-live="polite">
      <div className="flex items-center gap-2 text-[13px] text-(--text-primary)">
        <span
          className={["h-2 w-2 shrink-0 rounded-full", waiting ? "bg-(--text-secondary)" : "animate-pulse bg-(--accent)"].join(" ")}
          aria-hidden
        />
        <span className="min-w-0 truncate">
          {waiting ? `${label} history: waiting for your phone` : `Loading all ${label} history`}
        </span>
      </div>
      <p className="mt-0.5 truncate pl-4 text-[12px] text-(--text-secondary)">
        {backfill.chat && !waiting ? `${backfill.chat} · ${counts}` : counts}
      </p>
      <div className="mt-1.5 ml-4 h-0.75 overflow-hidden rounded-full bg-(--hover-bg)">
        <div className="h-full rounded-full bg-(--accent) transition-[width] duration-500" style={{ width: `${Math.round(share * 100)}%` }} />
      </div>
    </div>
  );
}
