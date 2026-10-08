import { useEffect, useRef, useState, type ClipboardEvent, type FormEvent, type KeyboardEvent } from "react";
import type { ReplyQuote } from "../backend";
import { formatFileSize } from "../lib/format";
import { CloseIcon, FileIcon, MicIcon, PlusIcon, SendIcon } from "./Icons";

interface ComposerProps {
  chatId: string;
  disabled?: boolean;
  disabledReason?: string;
  /** The message being replied to, shown above the text box. */
  replyTo?: ReplyQuote;
  onCancelReply?: () => void;
  /** Sends the text, or the files with the text as their caption. */
  onSend: (text: string, files: File[]) => Promise<void>;
  /** Files waiting to be sent. */
  files?: File[];
  /** Adds files to send; absent where files can't be sent. */
  onAddFiles?: (files: File[]) => void;
  onRemoveFile?: (index: number) => void;
}

function Spinner({ size }: { size: number }) {
  return (
    <span
      aria-hidden
      className="block animate-spin rounded-full border-2 border-current border-t-transparent"
      style={{ width: size, height: size }}
    />
  );
}

/** A file waiting to be sent: its picture when it is one, otherwise its name. */
function PendingFile({ file, state, onRemove }: { file: File; state?: "sending" | "waiting"; onRemove?: () => void }) {
  const [preview, setPreview] = useState<string | null>(null);
  useEffect(() => {
    if (!file.type.startsWith("image/")) return;
    const url = URL.createObjectURL(file);
    setPreview(url);
    return () => URL.revokeObjectURL(url);
  }, [file]);
  return (
    <li
      aria-busy={state === "sending"}
      className={[
        "relative flex h-16 max-w-56 shrink-0 items-center gap-2 overflow-hidden rounded-md bg-(--composer-input-bg) pr-8",
        state === "waiting" ? "opacity-50" : "",
      ].join(" ")}
    >
      {preview ? (
        <img src={preview} alt="" className="h-16 w-16 shrink-0 object-cover" />
      ) : (
        <FileIcon size={26} className="ml-3 shrink-0 text-(--icon)" />
      )}
      <span className="min-w-0">
        <span className="block truncate text-[13px] text-(--text-primary)">{file.name}</span>
        <span className="block text-[12px] text-(--text-secondary)">
          {state === "sending" ? "Sending…" : state === "waiting" ? "Waiting…" : formatFileSize(file.size)}
        </span>
      </span>
      {state === "sending" ? (
        <span className="absolute top-1.5 right-1.5 text-(--accent)">
          <Spinner size={16} />
        </span>
      ) : null}
      {onRemove ? (
        <button
          type="button"
          onClick={onRemove}
          aria-label={`Remove ${file.name}`}
          className="absolute top-1 right-1 flex h-6 w-6 items-center justify-center rounded-full bg-black/40 text-white hover:bg-black/60"
        >
          <CloseIcon size={14} />
        </button>
      ) : null}
    </li>
  );
}

export function Composer({ chatId, disabled, disabledReason, replyTo, onCancelReply, onSend, files = [], onAddFiles, onRemoveFile }: ComposerProps) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);
  const picker = useRef<HTMLInputElement>(null);

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
    if ((!value && !files.length) || disabled || sending) return;
    setSending(true);
    try {
      await onSend(value, files);
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

  // A pasted picture or file is added to the ones to send.
  function onPaste(e: ClipboardEvent<HTMLTextAreaElement>) {
    if (!onAddFiles || !e.clipboardData.files.length) return;
    e.preventDefault();
    onAddFiles(Array.from(e.clipboardData.files));
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
  const canSubmit = hasText || files.length > 0;

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
      {files.length ? (
        <ul className="flex gap-2 overflow-x-auto px-4 pt-2" aria-label="Files to send">
          {files.map((file, index) => (
            <PendingFile
              key={`${file.name}-${file.size}-${file.lastModified}-${index}`}
              file={file}
              state={sending ? (index === 0 ? "sending" : "waiting") : undefined}
              onRemove={sending ? undefined : () => onRemoveFile?.(index)}
            />
          ))}
        </ul>
      ) : null}
      <form
        onSubmit={onSubmit}
        className="flex min-h-15.5 items-end gap-2 px-4 py-2.5"
      >
        <button
          type="button"
          onClick={() => picker.current?.click()}
          disabled={disabled || sending || !onAddFiles}
          className="mb-0.75 flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg) disabled:opacity-50"
          aria-label="Attach"
          title={onAddFiles ? "Attach files" : "Files can't be sent here"}
        >
          <PlusIcon size={24} />
        </button>
        <input
          ref={picker}
          type="file"
          multiple
          hidden
          onChange={(e) => {
            onAddFiles?.(Array.from(e.target.files ?? []));
            // The same file can be picked again after it was removed.
            e.target.value = "";
            ref.current?.focus();
          }}
        />

        <textarea
          ref={ref}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
          onPaste={onPaste}
          rows={1}
          placeholder={disabled ? (disabledReason ?? "Can't send messages right now") : files.length ? "Add a caption" : "Type a message"}
          disabled={disabled}
          aria-label="Message"
          className="min-h-10.5 flex-1 resize-none rounded-lg bg-(--composer-input-bg) px-3 py-2.5 text-[15px] leading-5.5 text-(--text-primary) outline-none placeholder:text-(--text-secondary) focus-visible:outline-none disabled:opacity-60"
        />

        {canSubmit ? (
          <button
            type="submit"
            disabled={disabled || sending}
            className="mb-0.75 flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-(--icon) transition-colors hover:text-(--accent) disabled:opacity-50"
            aria-label={sending ? "Sending" : "Send"}
          >
            {sending ? <Spinner size={20} /> : <SendIcon size={24} />}
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
