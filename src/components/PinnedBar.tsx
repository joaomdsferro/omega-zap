import { useEffect, useState } from "react";
import type { Message } from "../backend";
import { FormattedLine } from "../lib/markup";
import { PinIcon } from "./Icons";

interface PinnedBarProps {
  /** The pinned messages, the latest pin first. */
  pins: Message[];
  /** Same for the same chat, so the bar starts over in another one. */
  chatKey: string;
  onJump: (messageId: string) => void;
}

/** What a pinned message says, on one line. */
function pinText(message: Message): string {
  const label = message.mediaLabel ?? message.imageLabel;
  if (message.text && label) return `${label}: ${message.text}`;
  return message.text ?? label ?? (message.kind === "voice" ? "Voice message" : "Message");
}

/**
 * The messages pinned in a chat, over the conversation as on WhatsApp: one
 * shows at a time; clicking it goes to it and brings up the next one.
 */
export function PinnedBar({ pins, chatKey, onJump }: PinnedBarProps) {
  const [at, setAt] = useState(0);
  useEffect(() => setAt(0), [chatKey]);
  if (!pins.length) return null;
  const index = at % pins.length;
  const shown = pins[index];
  return (
    <button
      type="button"
      onClick={() => {
        onJump(shown.id);
        setAt(index + 1);
      }}
      aria-label={`Pinned message${pins.length > 1 ? ` ${index + 1} of ${pins.length}` : ""}: ${pinText(shown)}. Go to message`}
      className="relative z-20 flex h-12 w-full shrink-0 items-center gap-3 border-b border-l border-(--divider) bg-(--panel-header-bg) pr-4 pl-4 text-left hover:bg-(--hover-bg)"
    >
      {pins.length > 1 ? (
        // One mark per pin; the one shown is solid.
        <span className="flex h-8 w-0.75 shrink-0 flex-col gap-0.5" aria-hidden>
          {pins.map((pin, i) => (
            <span
              key={pin.id}
              className={["min-h-0 flex-1 rounded-full", i === index ? "bg-(--text-secondary)" : "bg-(--text-secondary) opacity-30"].join(" ")}
            />
          ))}
        </span>
      ) : null}
      <span className="min-w-0 flex-1">
        {pins.length > 1 ? (
          <span className="block text-[12.5px] leading-4 text-(--accent)">
            Pinned message {index + 1} of {pins.length}
          </span>
        ) : null}
        <span className="block truncate text-[14px] leading-5 text-(--text-secondary)">
          <FormattedLine text={pinText(shown)} emojiSize={16} />
        </span>
      </span>
      <PinIcon size={18} className="shrink-0 text-(--icon)" />
    </button>
  );
}
