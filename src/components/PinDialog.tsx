import { useEffect, useRef, useState, type KeyboardEvent } from "react";

const DAY_S = 24 * 60 * 60;

/** The lengths WhatsApp offers, in seconds. */
const OPTIONS: { label: string; secs: number }[] = [
  { label: "24 hours", secs: DAY_S },
  { label: "7 days", secs: 7 * DAY_S },
  { label: "30 days", secs: 30 * DAY_S },
];

interface PinDialogProps {
  /** Whether the chat already shows as many pins as it can. */
  full: boolean;
  onPin: (secs: number) => Promise<void>;
  onClose: () => void;
}

/** Asks how long a message stays pinned, as WhatsApp Web does. */
export function PinDialog({ full, onPin, onClose }: PinDialogProps) {
  const [choice, setChoice] = useState(1);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialog = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    dialog.current?.querySelector<HTMLElement>("input:checked")?.focus();
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  async function pin() {
    setBusy(true);
    setError(null);
    try {
      await onPin(OPTIONS[choice].secs);
      onClose();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
      setBusy(false);
    }
  }

  // Tab stays inside the dialog.
  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== "Tab" || !dialog.current) return;
    const focusable = [...dialog.current.querySelectorAll<HTMLElement>("input:checked, button:not(:disabled)")];
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  }

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4"
      onPointerDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="pin-title"
        aria-describedby="pin-note"
        onKeyDown={onKeyDown}
        className="animate-fade-rise w-full max-w-100 rounded-2xl bg-(--panel-bg) px-6 pt-6 pb-4"
        style={{ boxShadow: "0 17px 50px rgba(11,20,26,.19), 0 12px 15px rgba(11,20,26,.24)" }}
      >
        <h2 id="pin-title" className="text-[18px] text-(--text-primary)">
          Choose how long your pin lasts
        </h2>
        <p id="pin-note" className="mt-2 text-[14px] text-(--text-secondary)">
          {full ? "Your new pin will replace the oldest one. " : ""}You can unpin at any time.
        </p>
        <fieldset className="mt-4 flex flex-col">
          <legend className="sr-only">Pin for</legend>
          {OPTIONS.map((option, i) => (
            <label key={option.label} className="flex items-center gap-4 rounded-lg px-1 py-2.5 text-[15px] text-(--text-primary)">
              <input
                type="radio"
                name="pin-for"
                checked={choice === i}
                onChange={() => setChoice(i)}
                onKeyDown={(event) => event.key === "Enter" && void pin()}
                className="size-4.5 accent-(--accent)"
              />
              {option.label}
            </label>
          ))}
        </fieldset>
        {error ? <p className="mt-3 text-[13px] text-(--danger)" role="alert">{error}</p> : null}
        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-full border border-(--divider) px-5 py-2 text-[14px] font-medium text-(--accent) hover:bg-(--hover-bg)"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => void pin()}
            className="rounded-full bg-(--accent) px-5 py-2 text-[14px] font-medium text-(--on-accent) hover:brightness-95 disabled:opacity-60"
          >
            Pin
          </button>
        </div>
      </div>
    </div>
  );
}
