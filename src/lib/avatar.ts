function hash(text: string): number {
  let h = 0;
  for (let i = 0; i < text.length; i++) {
    h = (h * 31 + text.charCodeAt(i)) | 0;
  }
  return Math.abs(h);
}

export function initialsFor(name: string): string {
  const words = name
    .replace(/[^\p{L}\p{N}\s]/gu, "")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (words.length === 0) return "#";
  if (words.length === 1) return words[0].slice(0, 2).toUpperCase();
  return (words[0][0] + words[words.length - 1][0]).toUpperCase();
}

export function hueFor(key: string): number {
  return hash(key) % 360;
}

/** WhatsApp-style sender name colours for group chats. */
const AUTHOR_COLORS = [
  "#1fa855",
  "#e26a2c",
  "#a62c71",
  "#3b82c4",
  "#c4532d",
  "#6b4fbb",
  "#0f9d9a",
  "#d4462b",
  "#8f6c00",
  "#c2185b",
];

export function authorColor(name: string): string {
  return AUTHOR_COLORS[hash(name) % AUTHOR_COLORS.length];
}
