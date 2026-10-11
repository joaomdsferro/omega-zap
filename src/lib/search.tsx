import type { ReactNode } from "react";

/** Text as search compares it: lower case, without accents, so "nao" finds "Não". */
export function fold(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

/** The words of a search, folded. */
export function searchWords(query: string): string[] {
  return fold(query).split(/\s+/).filter(Boolean);
}

/** Whether every word of `query` is in `text`. */
export function matches(text: string, query: string): boolean {
  const words = searchWords(query);
  const folded = fold(text);
  return words.length > 0 && words.every((word) => folded.includes(word));
}

/**
 * Where the words are in `text`, as [start, end) ranges of the original text.
 * Folding keeps one character per character for the letters people type, so
 * positions in the folded text are positions in the original.
 */
export function ranges(text: string, words: string[]): [number, number][] {
  const folded = fold(text);
  // Characters whose folded form is longer or shorter would shift positions:
  // then nothing is marked rather than the wrong letters.
  if (folded.length !== text.length) return [];
  const found: [number, number][] = [];
  for (const word of words) {
    for (let at = folded.indexOf(word); at !== -1; at = folded.indexOf(word, at + word.length)) {
      found.push([at, at + word.length]);
    }
  }
  found.sort((a, b) => a[0] - b[0]);
  const merged: [number, number][] = [];
  for (const range of found) {
    const last = merged[merged.length - 1];
    if (last && range[0] <= last[1]) last[1] = Math.max(last[1], range[1]);
    else merged.push(range);
  }
  return merged;
}

/** A line of a message around its first match, so the match is in sight. */
export function snippet(text: string, query: string, before = 24): string {
  const line = text.replace(/\s+/g, " ").trim();
  const first = ranges(line, searchWords(query))[0];
  if (!first || first[0] <= before) return line;
  // From the start of a word, unless that leaves too little to cut.
  const start = line.lastIndexOf(" ", first[0] - before);
  return start === -1 ? line : `…${line.slice(start + 1)}`;
}

/** `text` with the words of `query` picked out. */
export function Highlighted({ text, query }: { text: string; query: string }) {
  const found = ranges(text, searchWords(query));
  if (!found.length) return text;
  const parts: ReactNode[] = [];
  let at = 0;
  for (const [start, end] of found) {
    if (start > at) parts.push(text.slice(at, start));
    parts.push(
      <mark key={start} className="bg-transparent font-semibold text-(--accent)">
        {text.slice(start, end)}
      </mark>,
    );
    at = end;
  }
  if (at < text.length) parts.push(text.slice(at));
  return <>{parts}</>;
}
