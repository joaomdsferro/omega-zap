import { CHARACTERS } from "../data/characters";
import { EMOJI_SECTIONS } from "../data/emoji";
import { SHORTCODES } from "../data/shortcodes";
import { hasEmojiImage } from "./emoji";

export interface EmojiEntry {
  emoji: string;
  name: string;
  /** What a search runs against: the name, then the words people use for it. */
  search: string;
  /** Its ":name:" shortcodes, as Slack and GitHub spell them. */
  codes?: string[];
}

/** The shortcode a suggestion shows: the one typed, or its first. */
export interface EmojiMatch extends EmojiEntry {
  code?: string;
}

function parse(entry: string): EmojiEntry {
  const cut = entry.indexOf(" ");
  const [name, keywords = ""] = entry.slice(cut + 1).split("\t");
  return { emoji: entry.slice(0, cut), name, search: `${name} ${keywords}`.toLowerCase() };
}

/** Every emoji Apple's set can draw, by section, then the characters that
 *  are not emoji (€, Ω, →…). An emoji too new for the set is not on offer. */
export const ALL_SECTIONS = [
  ...EMOJI_SECTIONS.map((section) => ({
    ...section,
    emoji: section.emoji.map(parse).filter(({ emoji }) => hasEmojiImage(emoji)),
  })),
  { name: "Characters", icon: "Ω", emoji: CHARACTERS.map(parse) },
];

let everything: EmojiEntry[] | null = null;
let byCode: Map<string, EmojiEntry> | null = null;

const bare = (emoji: string) => emoji.replaceAll("\uFE0F", "");

/** Every emoji on offer, each with its shortcodes. */
function entries(): EmojiEntry[] {
  if (everything) return everything;
  everything = ALL_SECTIONS.flatMap((section) => section.emoji);
  const codes = new Map<string, string[]>();
  for (const [code, emoji] of Object.entries(SHORTCODES)) {
    const key = bare(emoji);
    codes.set(key, [...(codes.get(key) ?? []), code]);
  }
  byCode = new Map();
  for (const entry of everything) {
    entry.codes = codes.get(bare(entry.emoji));
    for (const code of entry.codes ?? []) byCode.set(code, entry);
  }
  return everything;
}

/** The emoji a whole shortcode names, as in "laughing" for 😆. */
export function emojiForShortcode(code: string): string | undefined {
  entries();
  return byCode!.get(code.toLowerCase())?.emoji;
}

/** The emoji people send most, most sent first, offered before the rest when they match. */
const POPULAR = new Map(
  "😂 ❤️ 🤣 👍 😭 🙏 😘 🥰 😍 😊 🎉 😁 💕 🥺 😅 🔥 ☺️ 🤦 ♥️ 🤷 🙄 😆 🤗 😉 🎂 🤔 👏 🙂 😳 🥳 😎 👌 💜 😔 💪 ✨ 💖 👀 😋 😏 😢 👉 💗 😩 💯 🌹 💞 🎈 💙 😃 😡 💐 😜 🙈 🤞 😄 🤤 🙌 🤪 ❣️ 😀 💋 💀 👇 💔 😌 💓 🤩 🙃 😬 😱 😴 🤭 😐 🌞 😒 😇 🌸 😈 🎶 ✌️ 🎊 🥵 😞 💚 ☀️ 🖤 💰 😚 👑 🎁 💥 🙋 ☹️ 😑 🥴 👈 💩 ✅".split(" ").map((emoji, rank) => [emoji, rank]),
);

/**
 * The emoji whose shortcode or words start with what was typed after a
 * colon, as in ":joy". The one whose shortcode it is comes first, then
 * the most sent, then ones whose name starts with it, then the rest by
 * where they are in the keyboards' order.
 */
export function searchEmoji(query: string, limit: number): EmojiMatch[] {
  const typed = query.toLowerCase();
  const words = typed.split(/[\s_]+/).filter(Boolean);
  if (!words.length) return [];
  const startsWord = (haystack: string, word: string) => haystack.startsWith(word) || haystack.includes(` ${word}`);
  const ranked: [number, EmojiMatch][] = [];
  for (const entry of entries()) {
    const code = entry.codes?.find((c) => c === typed) ?? entry.codes?.find((c) => c.startsWith(typed));
    if (!code && !words.every((word) => startsWord(entry.search, word))) continue;
    const inName = words.every((word) => startsWord(entry.name, word));
    const rank = code === typed ? -1 : (POPULAR.get(entry.emoji) ?? 1000 + (code || entry.name.startsWith(words[0]) ? 0 : inName ? 1 : 2));
    ranked.push([rank, { ...entry, code: code ?? entry.codes?.[0] }]);
  }
  return ranked
    .sort((a, b) => a[0] - b[0])
    .slice(0, limit)
    .map(([, entry]) => entry);
}
