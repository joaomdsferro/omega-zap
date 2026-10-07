import { MonitorIcon } from "./Icons";

interface EmptyStateProps {
  text: string;
  /** Offers to link an account when there is one to link. */
  action?: { label: string; onClick: () => void };
}

export function EmptyState({ text, action }: EmptyStateProps) {
  return (
    <section className="relative flex h-full flex-1 flex-col items-center justify-center border-l border-(--divider) bg-(--empty-bg) px-8">
      <div className="flex max-w-115 flex-col items-center text-center animate-fade-rise">
        <div className="mb-8 flex h-30 w-30 items-center justify-center rounded-full bg-[color-mix(in_oklab,var(--accent)_14%,transparent)] text-(--accent)">
          <MonitorIcon size={56} strokeWidth={1.4} />
        </div>
        <h2 className="font-display text-[32px] leading-tight font-light text-(--text-primary)">
          Omega Zap
        </h2>
        <p className="mt-4 text-[14px] leading-5 text-(--text-secondary)">{text}</p>
        {action ? (
          <button
            type="button"
            onClick={action.onClick}
            className="mt-7 h-10 rounded-full bg-(--accent) px-6 text-[14px] font-medium text-(--on-accent) transition-[filter] hover:brightness-110"
          >
            {action.label}
          </button>
        ) : null}
      </div>
      <div className="absolute inset-x-0 bottom-0 h-1.5 bg-(--badge-bg)" />
    </section>
  );
}
