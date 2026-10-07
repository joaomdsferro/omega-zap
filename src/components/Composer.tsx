import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import type { ReplyQuote } from "../backend";
import { CloseIcon, MicIcon, PlusIcon, SendIcon } from "./Icons";

interface ComposerProps {
  chatId: string;
  disabled?: boolean;
  disabledReason?: string;
  /** The message being replied to, shown above the text box. */
  replyTo?: ReplyQuote;
  onCancelReply?: () => void;
  onSend: (text: string) => Promise<void>;
}

export function Composer({ chatId, disabled, disabledReason, replyTo, onCancelReply, onSend }: ComposerProps) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    setText("");
    ref.current?.focus();
  }, [chatId]);

  // Picking Reply puts the cursor where the answer goes.
  useEffect(() => {
    if (replyTo) ref.current?.focus();
  }, [replyTo?.author, replyTo?.text]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 140)}px`;
  }, [text]);

  async function submit() {
    const value = text.trim();
    if (!value || disabled || sending) return;
    setSending(true);
    try {
      await onSend(value);
      setText("");
    } finally {
      setSending(false);
      requestAnimationFrame(() => ref.current?.focus());
    }
  }

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    void submit();
  }

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Escape" && replyTo) {
      e.preventDefault();
      onCancelReply?.();
      return;
    }
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void submit();
    }
  }

  const hasText = text.trim().length > 0;

  return (
    <div className="shrink-0 bg-(--composer-bg)">
      {replyTo ? (
        <div className="flex items-center gap-2 px-4 pt-2">
          <div className="min-w-0 flex-1 overflow-hidden rounded-[7.5px] border-l-4 border-(--accent) bg-(--quote-bg) px-3 py-2">
            <p className="text-[12.8px] font-medium text-(--accent)">{replyTo.author}</p>
            <p className="truncate text-[13.2px] text-(--text-secondary)">{replyTo.text}</p>
          </div>
          <button
            type="button"
            onClick={onCancelReply}
            aria-label="Cancel reply"
            className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
          >
            <CloseIcon size={20} />
          </button>
        </div>
      ) : null}
      <form
        onSubmit={onSubmit}
        className="flex min-h-15.5 items-end gap-2 px-4 py-2.5"
      >
        <button
          type="button"
          disabled
          className="mb-0.75 flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-(--icon) disabled:opacity-50"
          aria-label="Attach"
          title="Attachments are not available yet"
        >
          <PlusIcon size={24} />
        </button>

        <textarea
          ref={ref}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
          rows={1}
          placeholder={disabled ? (disabledReason ?? "Can't send messages right now") : "Type a message"}
          disabled={disabled}
          aria-label="Message"
          className="min-h-10.5 flex-1 resize-none rounded-lg bg-(--composer-input-bg) px-3 py-2.5 text-[15px] leading-5.5 text-(--text-primary) outline-none placeholder:text-(--text-secondary) focus-visible:outline-none disabled:opacity-60"
        />

        {hasText ? (
          <button
            type="submit"
            disabled={disabled || sending}
            className="mb-0.75 flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-(--icon) transition-colors hover:text-(--accent) disabled:opacity-50"
            aria-label="Send"
          >
            <SendIcon size={24} />
          </button>
        ) : (
          <button
            type="button"
            disabled
            className="mb-0.75 flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-(--icon) disabled:opacity-50"
            aria-label="Record voice message"
            title="Voice messages are not available yet"
          >
            <MicIcon size={24} />
          </button>
        )}
      </form>
    </div>
  );
}
