import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { AccountId } from "../accounts/accounts";
import type { Message } from "../backend";

/** How long after sending a message can still be deleted for everyone. */
const DELETE_FOR_EVERYONE_MS: Record<AccountId, number> = {
  whatsapp: 60 * 60 * 60 * 1000,
  business: 60 * 60 * 60 * 1000,
  signal: 24 * 60 * 60 * 1000,
  // Telegram has no limit on deleting your own messages.
  telegram: Number.POSITIVE_INFINITY,
};

interface DeleteDialogProps {
  messages: Message[];
  account: AccountId;
  onDelete: (forEveryone: boolean) => Promise<void>;
  onClose: () => void;
}

export function DeleteDialog({ messages, account, onDelete, onClose }: DeleteDialogProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialog = useRef<HTMLDivElement>(null);
  // Several go for everyone only when each of them still can.
  const forEveryone = messages.every(
    (message) =>
      message.fromMe &&
      !message.deleted &&
      Date.now() - new Date(message.sentAt).getTime() < DELETE_FOR_EVERYONE_MS[account],
  );

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    dialog.current?.querySelector<HTMLElement>("button")?.focus();
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  async function run(everyone: boolean) {
    setBusy(true);
    setError(null);
    try {
      await onDelete(everyone);
      onClose();
    } catch (e) {
      setError(String(e));
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
    const focusable = [...dialog.current.querySelectorAll<HTMLElement>("button:not(:disabled)")];
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

  const button =
    "rounded-full px-4 py-2 text-[14px] font-medium text-(--accent) hover:bg-(--hover-bg) disabled:opacity-60";

  return (
    <div
      className="animate-fade-in fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4"
      onPointerDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <div
        ref={dialog}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="delete-title"
        onKeyDown={onKeyDown}
        className="animate-fade-rise w-full max-w-100 rounded-2xl bg-(--panel-bg) px-6 pt-6 pb-4"
        style={{ boxShadow: "0 17px 50px rgba(11,20,26,.19), 0 12px 15px rgba(11,20,26,.24)" }}
      >
        <h2 id="delete-title" className="text-[16px] text-(--text-primary)">
          {messages.length === 1 ? "Delete message?" : `Delete ${messages.length} messages?`}
        </h2>
        {error ? <p className="mt-3 text-[13px] text-(--danger)" role="alert">{error}</p> : null}
        <div className="mt-6 flex flex-col items-end gap-1">
          {forEveryone ? (
            <button type="button" disabled={busy} onClick={() => void run(true)} className={button}>
              Delete for everyone
            </button>
          ) : null}
          <button type="button" disabled={busy} onClick={() => void run(false)} className={button}>
            Delete for me
          </button>
          <button type="button" onClick={onClose} className={button}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
