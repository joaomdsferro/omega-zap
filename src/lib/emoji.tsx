import type { ReactNode } from "react";
import { ODD, PLAIN } from "../data/emoji-images";

const graphemes = new Intl.Segmenter(undefined, { granularity: "grapheme" });
// A cheap test that skips the split for text with no emoji in it.
const MAYBE_EMOJI = /[\p{Extended_Pictographic}\p{Regional_Indicator}⃣]/u;

/** Where Apple's image of an emoji is, or undefined when there is none. */
export function emojiImage(emoji: string): string | undefined {
  const key = emoji.replaceAll("️", "");
  const stem = PLAIN.has(key)
    ? [...key].map((c) => c.codePointAt(0)!.toString(16)).join("-")
    : ODD[key];
  return stem ? `/emoji/${stem}.png` : undefined;
}

/** Whether there is an image to draw this emoji with. */
export function hasEmojiImage(emoji: string): boolean {
  return emojiImage(emoji) !== undefined;
}

/** One emoji drawn as Apple's image, or as text when it has none. */
export function Emoji({ emoji, size, className = "" }: { emoji: string; size: number; className?: string }) {
  const src = emojiImage(emoji);
  if (!src) return <span className={className}>{emoji}</span>;
  return (
    <img
      src={src}
      alt={emoji}
      draggable={false}
      loading="lazy"
      width={size}
      height={size}
      className={`inline-block align-[-0.2em] select-text ${className}`}
      style={{ width: size, height: size }}
    />
  );
}

/** Text with each emoji in it swapped for Apple's image, sized to the text. */
export function withEmoji(text: string, size = 20): ReactNode {
  if (!MAYBE_EMOJI.test(text)) return text;
  const parts: ReactNode[] = [];
  let run = "";
  let index = 0;
  for (const { segment } of graphemes.segment(text)) {
    if (MAYBE_EMOJI.test(segment) && hasEmojiImage(segment)) {
      if (run) parts.push(run);
      run = "";
      parts.push(<Emoji key={index} emoji={segment} size={size} />);
    } else {
      run += segment;
    }
    index++;
  }
  if (run) parts.push(run);
  return parts;
}

/** Up to three emoji and nothing else: WhatsApp draws those big, without a bubble's text size. */
export function emojiOnlyCount(text: string): number {
  if (!MAYBE_EMOJI.test(text)) return 0;
  let count = 0;
  for (const { segment } of graphemes.segment(text.trim())) {
    if (/^\s$/.test(segment)) continue;
    if (!MAYBE_EMOJI.test(segment) || !hasEmojiImage(segment)) return 0;
    if (++count > 3) return 0;
  }
  return count;
}
