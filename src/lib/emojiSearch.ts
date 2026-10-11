import { CHARACTERS } from "../data/characters";
import { EMOJI_SECTIONS } from "../data/emoji";
import { hasEmojiImage } from "./emoji";

export interface EmojiEntry {
  emoji: string;
  name: string;
  /** What a search runs against: the name, then the words people use for it. */
  search: string;
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

/** The emoji people send most, most sent first, offered before the rest when they match. */
const POPULAR = new Map(
  "😂 ❤️ 🤣 👍 😭 🙏 😘 🥰 😍 😊 🎉 😁 💕 🥺 😅 🔥 ☺️ 🤦 ♥️ 🤷 🙄 😆 🤗 😉 🎂 🤔 👏 🙂 😳 🥳 😎 👌 💜 😔 💪 ✨ 💖 👀 😋 😏 😢 👉 💗 😩 💯 🌹 💞 🎈 💙 😃 😡 💐 😜 🙈 🤞 😄 🤤 🙌 🤪 ❣️ 😀 💋 💀 👇 💔 😌 💓 🤩 🙃 😬 😱 😴 🤭 😐 🌞 😒 😇 🌸 😈 🎶 ✌️ 🎊 🥵 😞 💚 ☀️ 🖤 💰 😚 👑 🎁 💥 🙋 ☹️ 😑 🥴 👈 💩 ✅".split(" ").map((emoji, rank) => [emoji, rank]),
);

/**
 * The emoji whose words start with what was typed after a colon, as in
 * ":joy". Ones whose name starts with it come first, then the rest by
 * where they are in the keyboards' order.
 */
export function searchEmoji(query: string, limit: number): EmojiEntry[] {
  const words = query.toLowerCase().split(/[\s_]+/).filter(Boolean);
  if (!words.length) return [];
  everything ??= ALL_SECTIONS.flatMap((section) => section.emoji);
  const startsWord = (haystack: string, word: string) => haystack.startsWith(word) || haystack.includes(` ${word}`);
  const ranked: [number, EmojiEntry][] = [];
  for (const entry of everything) {
    if (!words.every((word) => startsWord(entry.search, word))) continue;
    const inName = words.every((word) => startsWord(entry.name, word));
    const rank = POPULAR.get(entry.emoji) ?? 1000 + (entry.name.startsWith(words[0]) ? 0 : inName ? 1 : 2);
    ranked.push([rank, entry]);
  }
  return ranked
    .sort((a, b) => a[0] - b[0])
    .slice(0, limit)
    .map(([, entry]) => entry);
}
