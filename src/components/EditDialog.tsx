import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { Message } from "../backend";
import { Formatted } from "../lib/markup";
import { CheckIcon, CloseIcon } from "./Icons";
import { MessageInput, type MessageInputHandle } from "./MessageInput";

/** How long after sending WhatsApp lets a message be edited. */
export const EDIT_WINDOW_MS = 15 * 60 * 1000;

/** Whether one of our messages can still be edited: a text, inside the window. */
export function canEdit(message: Message, now = Date.now()): boolean {
  return (
    message.fromMe &&
    message.kind === "text" &&
    !message.deleted &&
    Boolean(message.text) &&
    now - new Date(message.sentAt).getTime() < EDIT_WINDOW_MS
  );
}

interface EditDialogProps {
  message: Message;
  onSave: (text: string) => Promise<void>;
  onClose: () => void;
}

/** Changes what one of our messages says, as WhatsApp Web does: the message above, the new text below. */
export function EditDialog({ message, onSave, onClose }: EditDialogProps) {
  const original = message.text ?? "";
  const [text, setText] = useState(original);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<MessageInputHandle>(null);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    input.current?.setText(original);
    input.current?.focus();
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, [original]);

  const changed = text.trim().length > 0 && text.trim() !== original.trim();

  async function save() {
    if (!changed || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onSave(text.trim());
      onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void save();
    } else if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4"
      onPointerDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="edit-title"
        className="flex w-[min(560px,100%)] flex-col overflow-hidden rounded-xl bg-(--panel-bg) shadow-2xl"
      >
        <header className="flex h-14 shrink-0 items-center gap-3 bg-(--panel-header-bg) px-3">
          <button
            type="button"
            onClick={onClose}
            aria-label="Cancel"
            className="flex h-10 w-10 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
          >
            <CloseIcon size={22} />
          </button>
          <h2 id="edit-title" className="text-[16px] text-(--text-primary)">
            Edit message
          </h2>
        </header>
        <div className="chat-wallpaper flex max-h-[40vh] justify-end overflow-y-auto px-6 py-6">
          <p
            className="max-w-[85%] rounded-lg rounded-tr-none bg-(--bubble-out) px-2.5 py-1.5 text-[14.2px] leading-4.75 break-words whitespace-pre-wrap text-(--text-primary)"
            style={{ boxShadow: "var(--bubble-shadow)" }}
          >
            <Formatted text={original} />
          </p>
        </div>
        <div className="flex items-end gap-2 bg-(--panel-header-bg) px-3 py-2.5">
          <div className="min-w-0 flex-1 rounded-lg bg-(--composer-input-bg)">
            <MessageInput ref={input} onChange={(value) => setText(value)} onKeyDown={onKeyDown} placeholder="Type a message" label="New text" disabled={busy} />
          </div>
          <button
            type="button"
            onClick={() => void save()}
            disabled={!changed || busy}
            aria-label="Save"
            className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-(--accent) text-white disabled:cursor-not-allowed disabled:opacity-50"
          >
            <CheckIcon size={22} />
          </button>
        </div>
        {error ? <p className="bg-(--panel-header-bg) px-4 pb-3 text-[13px] text-(--danger)">{error}</p> : null}
      </div>
    </div>
  );
}
