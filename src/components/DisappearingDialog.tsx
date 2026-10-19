import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { timerLabel } from "../lib/format";

const HOUR = 60 * 60;

/** The timers Signal offers, in seconds; 0 is off. */
const OPTIONS = [0, 4 * 7 * 24 * HOUR, 7 * 24 * HOUR, 24 * HOUR, 8 * HOUR, HOUR, 5 * 60, 30];

interface DisappearingDialogProps {
  /** The timer now, in seconds; 0 or absent is off. */
  current?: number;
  onSet: (secs: number) => Promise<void>;
  onClose: () => void;
}

export function DisappearingDialog({ current = 0, onSet, onClose }: DisappearingDialogProps) {
  const options = OPTIONS.includes(current) ? OPTIONS : [...OPTIONS, current].sort((a, b) => (a === 0 ? -1 : b === 0 ? 1 : b - a));
  const [choice, setChoice] = useState(current);
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

  async function save() {
    if (choice === current) return onClose();
    setBusy(true);
    setError(null);
    try {
      await onSet(choice);
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
      className="animate-fade-in fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4"
      onPointerDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="timer-title"
        aria-describedby="timer-note"
        onKeyDown={onKeyDown}
        className="animate-fade-rise w-full max-w-100 rounded-2xl bg-(--panel-bg) px-6 pt-6 pb-4"
        style={{ boxShadow: "0 17px 50px rgba(11,20,26,.19), 0 12px 15px rgba(11,20,26,.24)" }}
      >
        <h2 id="timer-title" className="text-[18px] text-(--text-primary)">
          Disappearing messages
        </h2>
        <p id="timer-note" className="mt-2 text-[14px] text-(--text-secondary)">
          New messages in this chat disappear after the time you pick, for everyone in it. Theirs count from when you see them.
        </p>
        <fieldset className="mt-4 flex flex-col">
          <legend className="sr-only">Disappear after</legend>
          {options.map((secs) => (
            <label key={secs} className="flex cursor-pointer items-center gap-4 rounded-lg px-1 py-2 text-[15px] text-(--text-primary)">
              <input
                type="radio"
                name="timer"
                checked={choice === secs}
                onChange={() => setChoice(secs)}
                onKeyDown={(event) => event.key === "Enter" && void save()}
                className="size-4.5 cursor-pointer accent-(--accent)"
              />
              {secs === 0 ? "Off" : timerLabel(secs)}
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
            onClick={() => void save()}
            className="rounded-full bg-(--accent) px-5 py-2 text-[14px] font-medium text-(--on-accent) hover:brightness-95 disabled:opacity-60"
          >
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
