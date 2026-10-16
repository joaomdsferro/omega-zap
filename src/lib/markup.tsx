import type { MouseEvent as ReactMouseEvent, ReactNode } from "react";
import type { Mention } from "../backend/types";
import { emojiImage, withEmoji } from "./emoji";
import { URL_PATTERN } from "./links";
import { ranges } from "./search";

// WhatsApp's formatting, which the composer also shows as it is typed:
// *bold*, _italic_, ~strikethrough~, `inline code`, ```monospace```, and,
// at the start of a line, "> " quotes, "- " or "* " bullets and "1. " lists.

export const BOLD = 1;
export const ITALIC = 2;
export const STRIKE = 4;
/** `inline code`, drawn on a tint. */
export const CODE = 8;
/** ```monospace```, which can run over several lines. */
export const MONO = 16;

/** A stretch of text with one look. Markers are the `*` and `_` that make it. */
export interface Run {
  kind: "text" | "marker" | "link";
  text: string;
  /** Where it starts in the text, in UTF-16 units. */
  start: number;
  styles: number;
}

const MARKS: Record<string, number> = { "*": BOLD, _: ITALIC, "~": STRIKE, "`": CODE };
const WORD = /[\p{L}\p{N}]/u;
const SPACE = /\s/;

/** Splits text into runs, every character in exactly one, in order. */
export function tokenize(text: string): Run[] {
  const urls = [...text.matchAll(URL_PATTERN)].map((m) => [m.index, m.index + m[0].length] as const);
  const runs: Run[] = [];

  function push(kind: Run["kind"], start: number, end: number, styles: number) {
    if (end <= start) return;
    const last = runs[runs.length - 1];
    if (kind === "text" && last?.kind === "text" && last.styles === styles && last.start + last.text.length === start) {
      last.text += text.slice(start, end);
      return;
    }
    runs.push({ kind, text: text.slice(start, end), start, styles });
  }

  const inUrl = (at: number) => urls.some(([start, end]) => at >= start && at < end);

  // An opening marker follows a space, punctuation or the start, and comes
  // before something that isn't a space.
  function opens(at: number) {
    const next = text[at + 1];
    return (at === 0 || !WORD.test(text[at - 1])) && next !== undefined && !SPACE.test(next) && next !== text[at];
  }

  // Its closing one is on the same line, after something that isn't a
  // space, and before a space, punctuation or the end.
  function closing(from: number, to: number, mark: string) {
    for (let at = from + 2; at < to; at++) {
      const c = text[at];
      if (c === "\n") return -1;
      if (c !== mark || inUrl(at)) continue;
      if (!SPACE.test(text[at - 1]) && (at + 1 >= text.length || !WORD.test(text[at + 1]))) return at;
    }
    return -1;
  }

  function parse(from: number, to: number, styles: number) {
    let at = from;
    let plain = from;
    while (at < to) {
      const url = urls.find(([start]) => start === at);
      if (url && url[1] <= to) {
        push("text", plain, at, styles);
        push("link", at, url[1], styles);
        at = plain = url[1];
        continue;
      }
      if (!(styles & (CODE | MONO)) && text.startsWith("```", at)) {
        const close = text.indexOf("```", at + 3);
        if (close > at + 3 && close + 3 <= to) {
          push("text", plain, at, styles);
          push("marker", at, at + 3, styles);
          push("text", at + 3, close, styles | MONO);
          push("marker", close, close + 3, styles);
          at = plain = close + 3;
          continue;
        }
      }
      const mark = text[at];
      const flag = MARKS[mark];
      if (flag && !(styles & (flag | CODE | MONO)) && opens(at)) {
        const close = closing(at, to, mark);
        if (close !== -1) {
          push("text", plain, at, styles);
          push("marker", at, at + 1, styles);
          if (flag === CODE) push("text", at + 1, close, styles | CODE);
          else parse(at + 1, close, styles | flag);
          push("marker", close, close + 1, styles);
          at = plain = close + 1;
          continue;
        }
      }
      at++;
    }
    push("text", plain, to, styles);
  }

  parse(0, text.length, 0);
  return runs;
}

/** The classes that give a run its look. */
export function styleClass(styles: number): string {
  const classes = [];
  if (styles & BOLD) classes.push("font-semibold");
  if (styles & ITALIC) classes.push("italic");
  if (styles & STRIKE) classes.push("line-through");
  if (styles & (CODE | MONO)) classes.push("font-mono text-[0.9em]");
  if (styles & CODE) classes.push("rounded-sm bg-(--quote-bg) px-1 py-px");
  return classes.join(" ");
}

type Block = "quote" | "bullet" | "number";

/** What starts a line that is a quote or a list item, and how long that start is. */
function blockOf(line: string): { block: Block; cut: number; label?: string } | null {
  if (line.startsWith("> ")) return { block: "quote", cut: 2 };
  if (line.startsWith("- ") || line.startsWith("* ")) return { block: "bullet", cut: 2 };
  const number = /^(\d{1,3})\. /.exec(line);
  return number ? { block: "number", cut: number[0].length, label: `${number[1]}.` } : null;
}

/** Runs split at line breaks. */
function lines(runs: Run[]): Run[][] {
  const out: Run[][] = [[]];
  for (const run of runs) {
    const parts = run.text.split("\n");
    let start = run.start;
    parts.forEach((part, index) => {
      if (index > 0) out.push([]);
      if (part) out[out.length - 1].push({ ...run, text: part, start });
      start += part.length + 1;
    });
  }
  return out;
}

/** A run's text with the words of a search marked. */
function marked(text: string, emojiSize: number, words?: string[]): ReactNode {
  const found = words?.length ? ranges(text, words) : [];
  if (!found.length) return withEmoji(text, emojiSize);
  const parts: ReactNode[] = [];
  let at = 0;
  for (const [start, end] of found) {
    if (start > at) parts.push(withEmoji(text.slice(at, start), emojiSize));
    parts.push(
      <mark key={start} className="search-mark">
        {withEmoji(text.slice(start, end), emojiSize)}
      </mark>,
    );
    at = end;
  }
  if (at < text.length) parts.push(withEmoji(text.slice(at), emojiSize));
  return parts;
}

function drawRun(
  run: Run,
  key: number,
  emojiSize: number,
  onLink?: (event: ReactMouseEvent, url: string) => void,
  words?: string[],
  mentions?: Mention[],
  onMention?: (id: string) => void,
): ReactNode {
  if (run.kind === "marker") return null;
  const className = styleClass(run.styles);
  if (run.kind === "link") {
    return (
      <a
        key={key}
        href={run.text}
        target="_blank"
        rel="noreferrer"
        onClick={onLink ? (event) => onLink(event, run.text) : undefined}
        className={`break-all text-(--accent) underline ${className}`}
      >
        {run.text}
      </a>
    );
  }
  const content = withMentions(run.text, mentions, (text) => marked(text, emojiSize, words), onMention);
  return className ? (
    <span key={key} className={className}>
      {content}
    </span>
  ) : (
    <span key={key}>{content}</span>
  );
}

/** `text` with each of `mentions` ("@Name") marked as WhatsApp does; clicking one opens their contact info. */
function withMentions(
  text: string,
  mentions: Mention[] | undefined,
  draw: (text: string) => ReactNode,
  onMention?: (id: string) => void,
): ReactNode {
  if (!mentions?.length) return draw(text);
  const parts: ReactNode[] = [];
  let at = 0;
  while (at < text.length) {
    let next = -1;
    let found: Mention | null = null;
    for (const mention of mentions) {
      const index = text.indexOf(mention.text, at);
      // The longest wins when two names start alike ("@Ana" and "@Ana Rita").
      if (index >= 0 && (next < 0 || index < next || (index === next && mention.text.length > (found?.text.length ?? 0)))) {
        next = index;
        found = mention;
      }
    }
    if (next < 0 || !found) break;
    if (next > at) parts.push(draw(text.slice(at, next)));
    const mention = found;
    parts.push(
      onMention ? (
        <button key={next} type="button" className="mention" onClick={() => onMention(mention.id)}>
          {draw(mention.text)}
        </button>
      ) : (
        <span key={next} className="mention">
          {draw(mention.text)}
        </span>
      ),
    );
    at = next + mention.text.length;
  }
  if (at < text.length) parts.push(draw(text.slice(at)));
  return parts;
}

/** Drops the first `cut` characters of a line, the marker of its block. */
function cutLine(line: Run[], cut: number): Run[] {
  let left = cut;
  const out: Run[] = [];
  for (const run of line) {
    if (left >= run.text.length) {
      left -= run.text.length;
      continue;
    }
    out.push(left ? { ...run, text: run.text.slice(left), start: run.start + left } : run);
    left = 0;
  }
  return out;
}

/** A message's text as WhatsApp shows it: formatted, with links and Apple's emoji. */
export function Formatted({
  text,
  emojiSize = 20,
  onLink,
  highlight,
  mentions,
  onMention,
}: {
  text: string;
  emojiSize?: number;
  onLink?: (event: ReactMouseEvent, url: string) => void;
  /** Words of a search to mark, already folded. */
  highlight?: string[];
  /** People the text mentions, as "@Name". */
  mentions?: Mention[];
  /** Opens a mentioned person's contact info. */
  onMention?: (id: string) => void;
}) {
  const parts: ReactNode[] = [];
  let key = 0;
  let previousPlain = false;
  const all = lines(tokenize(text));
  // Consecutive quote lines share one bar.
  let quote: ReactNode[] | null = null;
  const endQuote = () => {
    if (!quote) return;
    parts.push(
      <span key={key++} className="my-0.5 block border-l-[3px] border-(--text-secondary)/50 pl-2 text-(--text-secondary)">
        {quote}
      </span>,
    );
    quote = null;
  };
  all.forEach((line, index) => {
    const raw = line.map((run) => run.text).join("");
    // Inside a ``` block a line is never a quote or a list item.
    const block = line[0] && line[0].styles & MONO ? null : blockOf(raw);
    if (block?.block !== "quote") endQuote();
    if (!block) {
      if (previousPlain) parts.push("\n");
      parts.push(...line.map((run) => drawRun(run, key++, emojiSize, onLink, highlight, mentions, onMention)));
      if (!line.length && index > 0 && !previousPlain) parts.push("\n");
      previousPlain = true;
      return;
    }
    const content = cutLine(line, block.cut).map((run) => drawRun(run, key++, emojiSize, onLink, highlight, mentions, onMention));
    previousPlain = false;
    if (block.block === "quote") {
      if (quote) quote.push(<br key={key++} />);
      (quote ??= []).push(...content);
      return;
    }
    parts.push(
      <span key={key++} className="relative block pl-5">
        <span className="absolute left-1" aria-hidden={block.block === "bullet"}>
          {block.block === "bullet" ? "•" : block.label}
        </span>
        {content}
      </span>,
    );
  });
  endQuote();
  return <>{parts}</>;
}

/** Formatted text on one line, as a chat list or a quote shows it: no markers, no blocks. */
export function FormattedLine({ text, emojiSize = 18 }: { text: string; emojiSize?: number }) {
  return (
    <>
      {tokenize(text.replaceAll("\n", " ")).map((run, index) =>
        run.kind === "marker" ? null : run.kind === "link" ? (
          <span key={index}>{run.text}</span>
        ) : (
          drawRun(run, index, emojiSize)
        ),
      )}
    </>
  );
}

/** What Signal calls each look, for the ranges it sends beside the text. */
const SIGNAL_STYLES: [number, SignalStyle][] = [
  [BOLD, "bold"],
  [ITALIC, "italic"],
  [STRIKE, "strikethrough"],
  [CODE | MONO, "monospace"],
];

export type SignalStyle = "bold" | "italic" | "strikethrough" | "monospace";

export interface StyleRange {
  start: number;
  length: number;
  style: SignalStyle;
}

/**
 * Signal formats with ranges over plain text rather than markers in it: the
 * markers come out, and each look becomes a range over what is left.
 */
export function signalBody(text: string): { body: string; ranges: StyleRange[] } {
  let body = "";
  const open = new Map<SignalStyle, number>();
  const ranges: StyleRange[] = [];
  const close = (style: SignalStyle) => {
    const start = open.get(style);
    if (start !== undefined && body.length > start) ranges.push({ start, length: body.length - start, style });
    open.delete(style);
  };
  for (const run of tokenize(text)) {
    if (run.kind === "marker") continue;
    for (const [flags, style] of SIGNAL_STYLES) {
      if (run.styles & flags) {
        if (!open.has(style)) open.set(style, body.length);
      } else close(style);
    }
    body += run.text;
  }
  for (const [, style] of SIGNAL_STYLES) close(style);
  return { body, ranges };
}

function escapeHtml(text: string): string {
  return text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

const graphemes = new Intl.Segmenter(undefined, { granularity: "grapheme" });
const MAYBE_EMOJI = /[\p{Extended_Pictographic}\p{Regional_Indicator}⃣]/u;

/** Text as HTML with Apple's emoji, for the composer. */
function emojiHtml(text: string, size: number): string {
  if (!MAYBE_EMOJI.test(text)) return escapeHtml(text);
  let html = "";
  for (const { segment } of graphemes.segment(text)) {
    const src = MAYBE_EMOJI.test(segment) ? emojiImage(segment) : undefined;
    html += src
      ? `<img src="${src}" alt="${escapeHtml(segment)}" draggable="false" class="composer-emoji" width="${size}" height="${size}">`
      : escapeHtml(segment);
  }
  return html;
}

/**
 * The composer's contents: the same formatting a message gets, with its
 * markers kept and dimmed so they can still be edited.
 */
export function editorHtml(text: string, emojiSize = 20): string {
  let html = "";
  for (const run of tokenize(text)) {
    const body = emojiHtml(run.text, emojiSize);
    const classes = [styleClass(run.styles)];
    if (run.kind === "marker") classes.push("composer-marker");
    if (run.kind === "link") classes.push("text-(--accent)");
    const className = classes.filter(Boolean).join(" ");
    html += className ? `<span class="${className}">${body}</span>` : body;
  }
  // A line break at the very end only takes up a line with something after it.
  if (text.endsWith("\n")) html += "<br data-end>";
  return html;
}

/** Not worth a "Read more" for the last few words. */
const SHORT_SLACK = 80;
/** How far back a cut looks for the end of a line, then of a word. */
const LINE_BACK = 160;
const WORD_BACK = 40;

/**
 * A long text's start, as it shows before "Read more": at most `chars`
 * characters and `lines` lines, cut at the end of a line or a word and never
 * inside formatting, a link or a ``` block. Undefined when the text is short
 * enough to show whole.
 */
export function shortened(text: string, chars: number, lines: number): string | undefined {
  let end = Math.min(text.length, chars);
  let count = 0;
  for (let at = text.indexOf("\n"); at !== -1 && at < end; at = text.indexOf("\n", at + 1)) {
    if (++count === lines) end = at;
  }
  if (text.length - end < SHORT_SLACK) return undefined;
  if (count < lines) {
    const head = text.slice(0, end);
    const line = head.lastIndexOf("\n");
    const space = head.search(/\s\S*$/);
    if (line > end - LINE_BACK) end = line;
    else if (space > end - WORD_BACK) end = space;
  }
  // Back to the start of whatever formatted stretch the cut falls in.
  let stretch = -1;
  for (const run of tokenize(text)) {
    if (run.start >= end) break;
    const plain = run.kind === "text" && run.styles === 0;
    stretch = plain ? -1 : stretch === -1 ? run.start : stretch;
    if (!plain && run.start + run.text.length > end) {
      end = stretch;
      break;
    }
  }
  const cut = text.slice(0, end).trimEnd();
  return cut ? cut : undefined;
}

