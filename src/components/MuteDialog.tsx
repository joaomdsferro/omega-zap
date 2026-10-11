import { useEffect, useRef, useState, type KeyboardEvent } from "react";

const HOUR_MS = 60 * 60 * 1000;

/** The lengths WhatsApp offers; `null` is for good. */
const OPTIONS: { label: string; ms: number | null }[] = [
  { label: "8 hours", ms: 8 * HOUR_MS },
  { label: "1 week", ms: 7 * 24 * HOUR_MS },
  { label: "Always", ms: null },
];

interface MuteDialogProps {
  /** Mutes until a time in ms, or for good with `undefined`. */
  onMute: (untilMs: number | undefined) => Promise<void>;
  onClose: () => void;
}

export function MuteDialog({ onMute, onClose }: MuteDialogProps) {
  const [choice, setChoice] = useState(0);
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

  async function mute() {
    const { ms } = OPTIONS[choice];
    setBusy(true);
    setError(null);
    try {
      await onMute(ms === null ? undefined : Date.now() + ms);
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
        aria-labelledby="mute-title"
        aria-describedby="mute-note"
        onKeyDown={onKeyDown}
        className="animate-fade-rise w-full max-w-100 rounded-2xl bg-(--panel-bg) px-6 pt-6 pb-4"
        style={{ boxShadow: "0 17px 50px rgba(11,20,26,.19), 0 12px 15px rgba(11,20,26,.24)" }}
      >
        <h2 id="mute-title" className="text-[18px] text-(--text-primary)">
          Mute notifications
        </h2>
        <p id="mute-note" className="mt-2 text-[14px] text-(--text-secondary)">
          No one else in this chat will see that you muted it.
        </p>
        <fieldset className="mt-4 flex flex-col">
          <legend className="sr-only">Mute for</legend>
          {OPTIONS.map((option, i) => (
            <label key={option.label} className="flex items-center gap-4 rounded-lg px-1 py-2.5 text-[15px] text-(--text-primary)">
              <input
                type="radio"
                name="mute-for"
                checked={choice === i}
                onChange={() => setChoice(i)}
                onKeyDown={(event) => event.key === "Enter" && void mute()}
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
            onClick={() => void mute()}
            className="rounded-full bg-(--accent) px-5 py-2 text-[14px] font-medium text-(--on-accent) hover:brightness-95 disabled:opacity-60"
          >
            Mute
          </button>
        </div>
      </div>
    </div>
  );
}
