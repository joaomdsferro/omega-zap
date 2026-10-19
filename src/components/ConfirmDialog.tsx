import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

/** What a confirm dialog asks, without how it closes. */
export type ConfirmRequest = Omit<ConfirmDialogProps, "onClose">;

interface ConfirmDialogProps {
  title: string;
  body?: ReactNode;
  /** A choice that comes with it, such as keeping starred messages. */
  option?: { label: string; initial: boolean };
  confirmLabel: string;
  /** Deletes, blocks or leaves: the button says so in red. */
  danger?: boolean;
  /** Runs the action, with the option's state; the dialog closes once it is done and shows why when it fails. */
  onConfirm: (option: boolean) => Promise<void>;
  onClose: () => void;
}

/** Asks before doing something that can't be taken back, as WhatsApp does. */
export function ConfirmDialog({ title, body, option, confirmLabel, danger, onConfirm, onClose }: ConfirmDialogProps) {
  const [checked, setChecked] = useState(option?.initial ?? false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialog = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    dialog.current?.querySelector<HTMLElement>("[data-confirm]")?.focus();
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  async function confirm() {
    setBusy(true);
    setError(null);
    try {
      await onConfirm(checked);
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
      event.stopPropagation();
      if (!busy) onClose();
      return;
    }
    if (event.key !== "Tab" || !dialog.current) return;
    const focusable = [...dialog.current.querySelectorAll<HTMLElement>("input, button:not(:disabled)")];
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
      className="animate-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      onPointerDown={(event) => event.target === event.currentTarget && !busy && onClose()}
    >
      <div
        ref={dialog}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        aria-describedby={body ? "confirm-body" : undefined}
        onKeyDown={onKeyDown}
        className="animate-fade-rise w-full max-w-110 rounded-2xl bg-(--panel-bg) px-6 pt-6 pb-4"
        style={{ boxShadow: "0 17px 50px rgba(11,20,26,.19), 0 12px 15px rgba(11,20,26,.24)" }}
      >
        <h2 id="confirm-title" className="text-[18px] text-(--text-primary)">
          {title}
        </h2>
        {body ? (
          <div id="confirm-body" className="mt-2 text-[14px] text-(--text-secondary)">
            {body}
          </div>
        ) : null}
        {option ? (
          <label className="mt-4 flex items-center gap-3 text-[15px] text-(--text-primary)">
            <input
              type="checkbox"
              checked={checked}
              onChange={(event) => setChecked(event.target.checked)}
              className="size-4.5 accent-(--accent)"
            />
            {option.label}
          </label>
        ) : null}
        {error ? (
          <p className="mt-3 text-[13px] text-(--danger)" role="alert">
            {error}
          </p>
        ) : null}
        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className="rounded-full border border-(--divider) px-5 py-2 hover:bg-(--hover-bg)"
          >
            <span className="text-[14px] font-medium text-(--accent)">Cancel</span>
          </button>
          <button
            type="button"
            data-confirm
            disabled={busy}
            onClick={() => void confirm()}
            className={[
              "rounded-full px-5 py-2 hover:brightness-95 disabled:opacity-60",
              danger ? "bg-(--danger)" : "bg-(--accent)",
            ].join(" ")}
          >
            <span className={["text-[14px] font-medium", danger ? "text-white" : "text-(--on-accent)"].join(" ")}>{confirmLabel}</span>
          </button>
        </div>
      </div>
    </div>
  );
}
