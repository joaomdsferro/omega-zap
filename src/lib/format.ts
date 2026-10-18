function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function dayDiff(date: Date, now = new Date()): number {
  return Math.round((startOfDay(now).getTime() - startOfDay(date).getTime()) / 86_400_000);
}

export function formatListTime(iso: string): string {
  const date = new Date(iso);
  const diff = dayDiff(date);
  if (diff <= 0) {
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  if (diff === 1) return "Yesterday";
  if (diff < 7) return date.toLocaleDateString([], { weekday: "long" });
  return date.toLocaleDateString([], { day: "2-digit", month: "2-digit", year: "numeric" });
}

export function formatDayLabel(iso: string): string {
  const date = new Date(iso);
  const diff = dayDiff(date);
  if (diff <= 0) return "Today";
  if (diff === 1) return "Yesterday";
  if (diff < 7) return date.toLocaleDateString([], { weekday: "long" });
  return date.toLocaleDateString([], { day: "numeric", month: "long", year: "numeric" });
}

export function sameDay(a: string, b: string): boolean {
  return startOfDay(new Date(a)).getTime() === startOfDay(new Date(b)).getTime();
}

export function formatMessageTime(iso: string): string {
  return new Date(iso).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** When someone was last online, as WhatsApp words it: "last seen today at 14:02". */
export function formatLastSeen(iso: string): string {
  const date = new Date(iso);
  const diff = dayDiff(date);
  const time = formatMessageTime(iso);
  if (diff <= 0) return `last seen today at ${time}`;
  if (diff === 1) return `last seen yesterday at ${time}`;
  return `last seen ${date.toLocaleDateString([], { day: "2-digit", month: "2-digit", year: "numeric" })} at ${time}`;
}

export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatVoiceDuration(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

const TIMER_UNITS: [number, string, string][] = [
  [604_800, "week", "w"],
  [86_400, "day", "d"],
  [3_600, "hour", "h"],
  [60, "minute", "m"],
  [1, "second", "s"],
];

function timerUnit(seconds: number): [number, string, string] {
  const [size, unit, short] = TIMER_UNITS.find(([size]) => seconds >= size && seconds % size === 0) ?? TIMER_UNITS[4];
  return [seconds / size, unit, short];
}

/** A disappearing-message timer in words: "1 week", "8 hours". */
export function timerLabel(seconds: number): string {
  const [n, unit] = timerUnit(seconds);
  return `${n} ${unit}${n === 1 ? "" : "s"}`;
}

/** The same, as short as Signal puts it beside a chat's name: "1w", "8h". */
export function timerShort(seconds: number): string {
  const [n, , short] = timerUnit(seconds);
  return `${n}${short}`;
}
