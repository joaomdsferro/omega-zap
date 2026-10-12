import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import type { AccountId } from "../accounts/accounts";
import type { LinkPreview, OutgoingLink, OutgoingMedia, ReplyQuote, StickerShelf, TypingState, VoiceRecorder } from "../backend";
import { Emoji } from "../lib/emoji";
import { emojiForShortcode, searchEmoji } from "../lib/emojiSearch";
import { SHORTCUTS, matches } from "../lib/shortcuts";
import { formatFileSize, formatVoiceDuration } from "../lib/format";
import { URL_PATTERN, fetchLinkPreview } from "../lib/links";
import { FormattedLine } from "../lib/markup";
import { outgoingLink } from "../lib/outgoing";
import { useSettings } from "../settings/settings";
import { CloseIcon, FileIcon, MicIcon, PauseIcon, PlusIcon, SendIcon, SmileIcon, TrashIcon } from "./Icons";
import { MediaPanel } from "./MediaPanel";
import { MessageInput, type MessageInputHandle } from "./MessageInput";

interface ComposerProps {
  chatId: string;
  /** Which network's emoji panel to show. */
  account: AccountId;
  disabled?: boolean;
  disabledReason?: string;
  /** The message being replied to, shown above the text box. */
  replyTo?: ReplyQuote;
  onCancelReply?: () => void;
  /** Sends the text, or the files with the text as their caption; a text with a link can carry its card. */
  onSend: (text: string, files: File[], link?: OutgoingLink) => Promise<void>;
  /** Files waiting to be sent. */
  files?: File[];
  /** Adds files to send; absent where files can't be sent. */
  onAddFiles?: (files: File[]) => void;
  onRemoveFile?: (index: number) => void;
  /** Tells the chat we are typing, or stopped; absent where it isn't sent. */
  onTyping?: (chatId: string, state: TypingState) => void;
  /** ↑ in an empty box: edits our last message, if one can be. */
  onEditLast?: () => void;
  /** Sends a sticker or GIF, answering `replyTo`; absent where they can't be sent. */
  onSendMedia?: (item: OutgoingMedia) => Promise<void>;
  stickers?: StickerShelf;
  /** Records voice messages, sent through `onSendMedia`; absent where they can't be. */
  voice?: VoiceRecorder;
  onError?: (message: string) => void;
}

/** "Typing" is said again this often while it goes on: the other side forgets it after a while. */
const TYPING_REPEAT_MS = 10_000;
/** A pause this long in typing counts as stopping. */
const TYPING_IDLE_MS = 5_000;

/** Says "typing" while the text changes, and "stopped" after a pause, a send or leaving the chat. */
function useTypingSignal(chatId: string, onTyping?: (chatId: string, state: TypingState) => void) {
  const send = useRef(onTyping);
  send.current = onTyping;
  const state = useRef<{ chatId: string; sentAt: number; idle?: number } | null>(null);

  const stop = useCallback(() => {
    const was = state.current;
    if (!was) return;
    window.clearTimeout(was.idle);
    state.current = null;
    send.current?.(was.chatId, "paused");
  }, []);

  const typed = useCallback(
    (hasText: boolean) => {
      if (!send.current || !hasText) return stop();
      const now = Date.now();
      const was = state.current;
      if (!was || was.chatId !== chatId || now - was.sentAt > TYPING_REPEAT_MS) {
        if (was && was.chatId !== chatId) stop();
        send.current(chatId, "typing");
        state.current = { chatId, sentAt: now };
      }
      window.clearTimeout(state.current?.idle);
      if (state.current) state.current.idle = window.setTimeout(stop, TYPING_IDLE_MS);
    },
    [chatId, stop],
  );

  // Leaving the chat stops it there.
  useEffect(() => stop, [chatId, stop]);
  return { typed, stop };
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

/** Bars kept of a recording's live waveform: enough for a wide window. */
const LIVE_BARS = 120;

/**
 * Recording a voice message, in place of the text box: throw it away, the
 * time and loudness so far, pause or go on, send.
 */
function VoiceBar({
  chatId,
  voice,
  onSend,
  onDone,
  onTyping,
  onError,
}: {
  chatId: string;
  voice: VoiceRecorder;
  onSend: () => Promise<void>;
  /** The recording ended: sent, thrown away or failed. */
  onDone: () => void;
  onTyping?: (chatId: string, state: TypingState) => void;
  onError?: (message: string) => void;
}) {
  const [started, setStarted] = useState(false);
  const [paused, setPaused] = useState(false);
  const [sending, setSending] = useState(false);
  const [ms, setMs] = useState(0);
  const [levels, setLevels] = useState<number[]>([]);
  // Whether the recording still has to be thrown away when this goes.
  const live = useRef(true);
  const done = useRef(onDone);
  done.current = onDone;
  const fail = useRef(onError);
  fail.current = onError;

  useEffect(() => {
    live.current = true;
    const stopLevels = voice.onLevel((update) => {
      setMs(update.ms);
      setLevels((prev) => [...prev.slice(1 - LIVE_BARS), update.level]);
    });
    const stopErrors = voice.onError((message) => {
      live.current = false;
      fail.current?.(message);
      done.current();
    });
    voice.start().then(
      () => setStarted(true),
      (e) => {
        live.current = false;
        fail.current?.(String(e));
        done.current();
      },
    );
    return () => {
      stopLevels();
      stopErrors();
      // Leaving the chat throws the recording away.
      if (live.current) void voice.cancel();
    };
  }, [voice]);

  // The chat sees "recording audio…" while it goes on.
  useEffect(() => {
    if (!onTyping || !started || paused || sending) return;
    onTyping(chatId, "recording");
    const timer = window.setInterval(() => onTyping(chatId, "recording"), TYPING_REPEAT_MS);
    return () => {
      window.clearInterval(timer);
      onTyping(chatId, "paused");
    };
  }, [chatId, onTyping, started, paused, sending]);

  function discard() {
    live.current = false;
    void voice.cancel();
    onDone();
  }

  async function send() {
    if (!started || sending) return;
    setSending(true);
    live.current = false;
    try {
      await onSend();
    } catch (e) {
      onError?.(String(e));
    }
    onDone();
  }

  function togglePause() {
    const next = !paused;
    setPaused(next);
    void voice.pause(next);
  }

  // Escape throws it away, Enter sends it.
  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.defaultPrevented || document.querySelector('[aria-modal="true"], [role="menu"]')) return;
      if (event.key === "Escape") {
        event.preventDefault();
        discard();
      } else if (event.key === "Enter" && !event.shiftKey) {
        event.preventDefault();
        void send();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  const round = "flex h-10 w-10 shrink-0 items-center justify-center rounded-full";
  return (
    <div className="flex min-h-15.5 items-center gap-2 px-4 py-2.5" role="group" aria-label="Recording a voice message">
      <button
        type="button"
        onClick={discard}
        disabled={sending}
        aria-label="Discard"
        title="Discard (Esc)"
        className={`${round} ml-auto hover:bg-(--hover-bg) disabled:opacity-50`}
      >
        {/* Colours go on the icon: buttons inherit theirs (see tokens.css). */}
        <span className="text-(--icon)">
          <TrashIcon size={22} />
        </span>
      </button>
      <span className="flex shrink-0 items-center gap-2 text-[15px] text-(--text-primary) tabular-nums" aria-live="off">
        <span
          aria-hidden
          className={["h-2.5 w-2.5 rounded-full bg-(--danger)", started && !paused ? "animate-pulse" : "opacity-60"].join(" ")}
        />
        {formatVoiceDuration(ms / 1000)}
      </span>
      <div className="flex h-8 max-w-[480px] min-w-0 flex-1 items-center justify-end gap-0.75 overflow-hidden" aria-hidden>
        {levels.map((level, i) => (
          <span
            key={levels.length - i}
            className={`w-0.75 shrink-0 rounded-full ${paused ? "bg-(--text-secondary)" : "bg-(--accent)"}`}
            style={{ height: Math.round(3 + level * 25) }}
          />
        ))}
      </div>
      <button
        type="button"
        onClick={togglePause}
        disabled={!started || sending}
        aria-label={paused ? "Resume recording" : "Pause recording"}
        title={paused ? "Resume" : "Pause"}
        className={`${round} hover:bg-(--hover-bg) disabled:opacity-50`}
      >
        <span className="text-(--danger)">{paused ? <MicIcon size={22} /> : <PauseIcon size={20} />}</span>
      </button>
      <button
        type="button"
        onClick={() => void send()}
        disabled={!started || sending}
        aria-label={sending ? "Sending" : "Send voice message"}
        title="Send (Enter)"
        className={`${round} bg-(--accent) hover:brightness-95 disabled:opacity-50`}
      >
        <span className="text-white">{sending ? <Spinner size={20} /> : <SendIcon size={22} />}</span>
      </button>
    </div>
  );
}

/** How long typing has to pause before a link in it is looked up. */
const LINK_PAUSE_MS = 400;
/** Suggestions shown for ":name". */
const SUGGESTIONS = 8;
/** What starts an emoji suggestion: a colon, at the start or after a space, and two letters. */
const SHORTCODE = /(?:^|\s):([\p{L}\p{N}_+-]{2,})$/u;
/** A whole shortcode, closed with a colon: ":laughing:". */
const CLOSED_SHORTCODE = /(?:^|\s):([\p{L}\p{N}_+-]+):$/u;

/** The card for the link being typed, once it has been read; `undefined` while it is looked up. */
function useTypedLink(text: string, enabled: boolean) {
  const url = enabled ? text.match(URL_PATTERN)?.[0] : undefined;
  const [found, setFound] = useState<{ url: string; preview: LinkPreview | null } | null>(null);
  useEffect(() => {
    if (!url) return;
    let alive = true;
    const timer = window.setTimeout(() => {
      void fetchLinkPreview(url).then((preview) => alive && setFound({ url, preview }));
    }, LINK_PAUSE_MS);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [url]);
  if (!url) return { url: undefined, preview: null, loading: false };
  return found?.url === url ? { url, preview: found.preview, loading: false } : { url, preview: null, loading: true };
}

export function Composer({ chatId, account, disabled, disabledReason, replyTo, onCancelReply, onSend, files = [], onAddFiles, onRemoveFile, onTyping, onEditLast, onSendMedia, stickers, voice, onError }: ComposerProps) {
  const { settings } = useSettings();
  const [text, setText] = useState("");
  const [caret, setCaret] = useState(0);
  const [sending, setSending] = useState(false);
  const [emojiOpen, setEmojiOpen] = useState(false);
  // Recording a voice message, which puts its bar in place of the text box.
  const [recording, setRecording] = useState(false);
  const input = useRef<MessageInputHandle>(null);
  const picker = useRef<HTMLInputElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const emojiButton = useRef<HTMLButtonElement>(null);
  const typing = useTypingSignal(chatId, onTyping);
  // The Stickers tab, while open, makes pasted pictures into stickers.
  const pasteIntoStickers = useRef<((files: File[]) => boolean) | null>(null);

  useEffect(() => {
    input.current?.setText("");
    setEmojiOpen(false);
    setRecording(false);
    input.current?.focus();
  }, [chatId]);

  // Picking Reply puts the cursor where the answer goes.
  useEffect(() => {
    if (replyTo) input.current?.focus();
  }, [replyTo?.author, replyTo?.text]);

  // The emoji panel closes on a click outside it or Escape.
  useEffect(() => {
    if (!emojiOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      // A sticker's menu opens outside the panel but belongs to it.
      const inMenu = target instanceof Element && target.closest('[role="menu"]');
      if (!panel.current?.contains(target) && !emojiButton.current?.contains(target) && !inMenu) setEmojiOpen(false);
    };
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented && !document.querySelector('[role="menu"]')) {
        event.preventDefault();
        setEmojiOpen(false);
        input.current?.focus();
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [emojiOpen]);

  // Ctrl+E opens or closes the emoji panel, from anywhere in the chat.
  useEffect(() => {
    if (disabled) return;
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.defaultPrevented || !SHORTCUTS.emoji.combos.some((combo) => matches(event, combo))) return;
      if (document.querySelector('[aria-modal="true"], [role="menu"]')) return;
      event.preventDefault();
      setEmojiOpen((open) => !open);
      input.current?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [disabled]);

  // ":joy" offers 😂 and its neighbours.
  const shortcode = SHORTCODE.exec(text.slice(0, caret));
  const [dismissed, setDismissed] = useState<string | null>(null);
  const query = shortcode && shortcode[1] !== dismissed ? shortcode[1] : null;
  const suggestions = useMemo(() => (query ? searchEmoji(query, SUGGESTIONS) : []), [query]);
  const [chosen, setChosen] = useState(0);
  useEffect(() => setChosen(0), [query]);

  function suggest(emoji: string) {
    if (!query) return;
    input.current?.replaceBefore(caret - query.length - 1, emoji);
  }

  // Closing the shortcode with a colon puts its emoji in at once.
  useEffect(() => {
    const closed = CLOSED_SHORTCODE.exec(text.slice(0, caret));
    const emoji = closed && emojiForShortcode(closed[1]);
    if (emoji) input.current?.replaceBefore(caret - closed[1].length - 2, emoji);
  }, [text, caret]);

  // The link being typed gets its card above the box, as on the phone.
  const typed = useTypedLink(files.length ? "" : text, settings.fetchLinkPreviews && !disabled);
  const [hiddenLink, setHiddenLink] = useState<string | null>(null);
  // A link with nothing to show gets no card, as on the phone.
  const linkCard = typed.url && typed.url !== hiddenLink && (typed.loading || typed.preview) ? typed : null;

  async function submit() {
    const value = text.trim();
    if ((!value && !files.length) || disabled || sending) return;
    setSending(true);
    typing.stop();
    try {
      const card = !files.length && linkCard?.preview ? await outgoingLink(linkCard.preview) : undefined;
      await onSend(value, files, card);
      input.current?.setText("");
      setHiddenLink(null);
    } finally {
      setSending(false);
      requestAnimationFrame(() => input.current?.focus());
    }
  }

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    void submit();
  }

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.nativeEvent.isComposing) return;
    if (suggestions.length) {
      const move = { ArrowDown: 1, ArrowUp: -1 }[e.key];
      if (move) {
        e.preventDefault();
        setChosen((at) => (at + move + suggestions.length) % suggestions.length);
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        suggest(suggestions[chosen].emoji);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setDismissed(query);
        return;
      }
    }
    if (e.key === "ArrowUp" && !e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey && !text && !files.length && onEditLast) {
      e.preventDefault();
      onEditLast();
      return;
    }
    if (e.key === "Escape" && replyTo) {
      e.preventDefault();
      onCancelReply?.();
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submit();
    }
  }

  const hasText = text.trim().length > 0;
  const canSubmit = hasText || files.length > 0;
  const iconButton =
    "mb-0.75 flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg) disabled:opacity-50";

  return (
    <div className="relative shrink-0 bg-(--composer-bg)">
      {emojiOpen ? (
        <div
          ref={panel}
          role="dialog"
          aria-label={onSendMedia ? "Emoji, GIFs and stickers" : "Emoji"}
          className={[
            "reaction-pop absolute bottom-full left-3 z-40 mb-2 flex w-[364px] max-w-[calc(100%-24px)] flex-col overflow-hidden rounded-2xl bg-(--panel-bg)",
            onSendMedia ? "h-[420px] max-h-[calc(100vh-160px)]" : "h-[340px]",
          ].join(" ")}
          style={{ boxShadow: "0 2px 5px rgba(11,20,26,.26), 0 2px 10px rgba(11,20,26,.16)" }}
        >
          <MediaPanel
            account={account}
            onEmoji={(emoji) => input.current?.insert(emoji)}
            onSend={onSendMedia && !disabled ? onSendMedia : undefined}
            stickers={stickers}
            onError={onError}
            pasteInto={pasteIntoStickers}
          />
        </div>
      ) : null}
      {suggestions.length ? (
        <ul
          role="listbox"
          aria-label="Emoji suggestions"
          className="absolute bottom-full left-16 z-40 mb-2 w-72 overflow-hidden rounded-xl bg-(--panel-bg) py-1"
          style={{ boxShadow: "0 2px 5px rgba(11,20,26,.26), 0 2px 10px rgba(11,20,26,.16)" }}
        >
          {suggestions.map((entry, index) => (
            <li
              key={entry.emoji}
              role="option"
              aria-selected={index === chosen}
              // Pressing down here would take the cursor out of the box.
              onPointerDown={(event) => event.preventDefault()}
              onClick={() => suggest(entry.emoji)}
              onPointerEnter={() => setChosen(index)}
              className={[
                "flex cursor-pointer items-center gap-3 px-3 py-1.5 text-[14px] text-(--text-primary)",
                index === chosen ? "bg-(--hover-bg)" : "",
              ].join(" ")}
            >
              <Emoji emoji={entry.emoji} size={22} />
              <span className="min-w-0 flex-1 truncate">{entry.name}</span>
              {entry.code ? <span className="shrink-0 text-[12.5px] text-(--text-secondary)">:{entry.code}:</span> : null}
            </li>
          ))}
        </ul>
      ) : null}
      {replyTo ? (
        <div className="flex items-center gap-2 px-4 pt-2">
          <div className="min-w-0 flex-1 overflow-hidden rounded-[7.5px] border-l-4 border-(--accent) bg-(--quote-bg) px-3 py-2">
            <p className="text-[12.8px] font-medium text-(--accent)">{replyTo.author}</p>
            <p className="truncate text-[13.2px] text-(--text-secondary)">
              <FormattedLine text={replyTo.text} emojiSize={17} />
            </p>
          </div>
          <button type="button" onClick={onCancelReply} aria-label="Cancel reply" className={iconButton.replace("mb-0.75 ", "")}>
            <CloseIcon size={20} />
          </button>
        </div>
      ) : null}
      {linkCard ? (
        <div className="flex items-center gap-2 px-4 pt-2">
          <div className="flex h-16 min-w-0 flex-1 items-center overflow-hidden rounded-[7.5px] bg-(--quote-bg)">
            {linkCard.preview?.thumbnail ? (
              <img src={linkCard.preview.thumbnail} alt="" draggable={false} className="h-16 w-16 shrink-0 object-cover" />
            ) : null}
            <div className="min-w-0 px-3">
              {linkCard.loading ? (
                <p className="text-[13px] text-(--text-secondary)">Getting the link's preview…</p>
              ) : (
                <>
                  <p className="truncate text-[14px] font-medium text-(--text-primary)">{linkCard.preview?.title ?? linkCard.url}</p>
                  {linkCard.preview?.description ? (
                    <p className="truncate text-[12.5px] text-(--text-secondary)">{linkCard.preview.description}</p>
                  ) : null}
                  <p className="truncate text-[12px] text-(--text-secondary)">{hostOf(linkCard.url)}</p>
                </>
              )}
            </div>
          </div>
          <button type="button" onClick={() => setHiddenLink(linkCard.url)} aria-label="Send without the preview" className={iconButton.replace("mb-0.75 ", "")}>
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
      {recording && voice && onSendMedia ? (
        <VoiceBar
          key={chatId}
          chatId={chatId}
          voice={voice}
          onSend={() => onSendMedia({ kind: "voice" })}
          onDone={() => {
            setRecording(false);
            requestAnimationFrame(() => input.current?.focus());
          }}
          onTyping={onTyping}
          onError={onError}
        />
      ) : null}
      <form onSubmit={onSubmit} className={`min-h-15.5 items-end gap-1 px-4 py-2.5 ${recording ? "hidden" : "flex"}`}>
        <button
          ref={emojiButton}
          type="button"
          onClick={() => setEmojiOpen((open) => !open)}
          disabled={disabled}
          className={[iconButton, emojiOpen ? "text-(--accent)" : ""].join(" ")}
          aria-label="Emoji"
          aria-expanded={emojiOpen}
          title="Emoji"
        >
          <SmileIcon size={24} />
        </button>
        <button
          type="button"
          onClick={() => picker.current?.click()}
          disabled={disabled || sending || !onAddFiles}
          className={`${iconButton} mr-1`}
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
            input.current?.focus();
          }}
        />

        <MessageInput
          ref={input}
          onChange={(value, at) => {
            typing.typed(value.trim().length > 0);
            setText(value);
            setCaret(at);
            setDismissed(null);
          }}
          onCaret={setCaret}
          onKeyDown={onKeyDown}
          onPasteFiles={(pasted) => {
            if (emojiOpen && pasteIntoStickers.current?.(pasted)) return;
            onAddFiles?.(pasted);
          }}
          placeholder={disabled ? (disabledReason ?? "Can't send messages right now") : files.length ? "Add a caption" : "Type a message"}
          disabled={disabled}
          label="Message"
        />

        {canSubmit ? (
          <button
            type="submit"
            disabled={disabled || sending}
            className="mb-0.75 ml-1 flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-(--icon) transition-colors hover:text-(--accent) disabled:opacity-50"
            aria-label={sending ? "Sending" : "Send"}
          >
            {sending ? <Spinner size={20} /> : <SendIcon size={24} />}
          </button>
        ) : (
          <button
            type="button"
            onClick={() => {
              typing.stop();
              setEmojiOpen(false);
              setRecording(true);
            }}
            disabled={disabled || !voice || !onSendMedia}
            className="mb-0.75 ml-1 flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-(--icon) transition-colors hover:text-(--accent) disabled:opacity-50"
            aria-label="Record voice message"
            title={voice && onSendMedia ? "Record voice message" : "Voice messages can't be sent here yet"}
          >
            <MicIcon size={24} />
          </button>
        )}
      </form>
    </div>
  );
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
}
