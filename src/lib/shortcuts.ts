/**
 * The app's keyboard shortcuts: WhatsApp Web's (Ctrl+Alt…), and the plain
 * Ctrl ones its desktop app and most chat apps use, which a browser keeps
 * for itself but this window doesn't.
 */

export type ShortcutId =
  | "newChat"
  | "search"
  | "nextChat"
  | "previousChat"
  | "closeChat"
  | "markUnread"
  | "mute"
  | "archive"
  | "pin"
  | "settings"
  | "emoji"
  | "help";

/** A key combination, as written in the list: "Ctrl+Alt+Shift+M". */
type Combo = string;

export const SHORTCUTS: Record<ShortcutId, { label: string; combos: Combo[] }> = {
  newChat: { label: "New chat", combos: ["Ctrl+N", "Ctrl+Alt+N"] },
  search: { label: "Search chats", combos: ["Ctrl+K", "Ctrl+Alt+/"] },
  nextChat: { label: "Next chat", combos: ["Ctrl+Tab", "Alt+↓"] },
  previousChat: { label: "Previous chat", combos: ["Ctrl+Shift+Tab", "Alt+↑"] },
  closeChat: { label: "Close chat", combos: ["Escape"] },
  markUnread: { label: "Mark as read or unread", combos: ["Ctrl+Alt+Shift+U"] },
  mute: { label: "Mute or unmute chat", combos: ["Ctrl+Alt+Shift+M"] },
  archive: { label: "Archive or unarchive chat", combos: ["Ctrl+Alt+Shift+E"] },
  pin: { label: "Pin or unpin chat", combos: ["Ctrl+Alt+Shift+P"] },
  settings: { label: "Settings", combos: ["Ctrl+,", "Ctrl+Alt+,"] },
  emoji: { label: "Emoji panel", combos: ["Ctrl+E", "Ctrl+Alt+E"] },
  help: { label: "Keyboard shortcuts", combos: ["Ctrl+/"] },
};

/** What the help lists besides the shortcuts above, which other parts handle. */
export const OTHER_SHORTCUTS: { section: string; items: { label: string; combos: Combo[] }[] }[] = [
  {
    section: "Chats",
    items: [
      { label: "Open chat 1 to 9 in the list", combos: ["Ctrl+1…9"] },
      { label: "Search in chat", combos: ["Ctrl+F"] },
      { label: "Edit your last message", combos: ["↑"] },
    ],
  },
  {
    section: "Message box",
    items: [
      { label: "Emoji by name", combos: [":name", ":name:"] },
      { label: "New line", combos: ["Shift+Enter"] },
      { label: "Bold", combos: ["Ctrl+B"] },
      { label: "Italic", combos: ["Ctrl+I"] },
      { label: "Strikethrough", combos: ["Ctrl+Shift+X"] },
      { label: "Monospace", combos: ["Ctrl+Shift+M"] },
      { label: "Undo", combos: ["Ctrl+Z"] },
      { label: "Redo", combos: ["Ctrl+Y", "Ctrl+Shift+Z"] },
    ],
  },
  {
    section: "View",
    items: [
      { label: "Zoom in", combos: ["Ctrl+="] },
      { label: "Zoom out", combos: ["Ctrl+-"] },
      { label: "Actual size", combos: ["Ctrl+0"] },
    ],
  },
];

const KEY_CODES: Record<string, string> = { "↑": "ArrowUp", "↓": "ArrowDown", Tab: "Tab", Escape: "Escape" };

/**
 * Whether a key press is `combo`. Letters and digits go by the key's place,
 * so Ctrl+Alt still finds them on any layout; punctuation goes by what it
 * types, so Ctrl+/ works where "/" needs Shift.
 */
export function matches(event: KeyboardEvent, combo: Combo): boolean {
  const parts = combo.split("+");
  const key = parts.pop() || "+";
  const ctrl = parts.includes("Ctrl");
  if ((event.ctrlKey || event.metaKey) !== ctrl || event.altKey !== parts.includes("Alt")) return false;
  const punctuation = !/^[A-Z0-9]$/.test(key) && !(key in KEY_CODES);
  if (punctuation) return event.key === key;
  if (event.shiftKey !== parts.includes("Shift")) return false;
  if (key in KEY_CODES) return event.key === KEY_CODES[key];
  return event.code === (/\d/.test(key) ? `Digit${key}` : `Key${key}`);
}

/** The shortcut a key press is, if any. */
export function shortcutFor(event: KeyboardEvent): ShortcutId | null {
  for (const [id, { combos }] of Object.entries(SHORTCUTS) as [ShortcutId, { combos: Combo[] }][]) {
    if (combos.some((combo) => matches(event, combo))) return id;
  }
  return null;
}

/** Which of chats 1 to 9 Ctrl+digit asks for, counting from 0. */
export function chatNumber(event: KeyboardEvent): number | null {
  if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return null;
  const digit = /^Digit([1-9])$/.exec(event.code);
  return digit ? Number(digit[1]) - 1 : null;
}
