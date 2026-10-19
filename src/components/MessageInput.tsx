import { forwardRef, useImperativeHandle, useLayoutEffect, useRef } from "react";
import type { ClipboardEvent, KeyboardEvent } from "react";
import { editorHtml } from "../lib/markup";

/**
 * A text box that shows what it holds as the message will look: formatting
 * as it is typed, links in colour and Apple's emoji. A textarea can only
 * hold plain text, so this is an editable element whose contents are drawn
 * again from the text after every change, with the cursor put back where it
 * was. The text itself is the only state; the drawing is derived from it.
 */

export interface MessageInputHandle {
  focus: () => void;
  /** Replaces the selection, or the last one before the box lost focus. */
  insert: (text: string) => void;
  /** Replaces the text from `start` to the cursor, as an autocomplete does. */
  replaceBefore: (start: number, text: string) => void;
  /** Puts `before` and `after` around the selection, or takes them away. */
  wrap: (before: string, after?: string) => void;
  setText: (text: string) => void;
}

interface MessageInputProps {
  /** The text was changed; `caret` is where the cursor is in it. */
  onChange: (text: string, caret: number) => void;
  /** The cursor moved without the text changing. */
  onCaret?: (caret: number) => void;
  onKeyDown?: (event: KeyboardEvent<HTMLDivElement>) => void;
  /** Files pasted in. Text is pasted as plain text. */
  onPasteFiles?: (files: File[]) => void;
  placeholder: string;
  disabled?: boolean;
  label: string;
  /** People picked to mention, "@Name", marked where they are in the text. */
  mentions?: string[];
}

interface Snapshot {
  text: string;
  start: number;
  end: number;
}

/** Typing within this long is undone in one step. */
const UNDO_GROUP_MS = 700;
const UNDO_LIMIT = 200;

/** A block the browser added, say on paste, starts a new line when it isn't first. */
function startsLine(node: Node, index: number): boolean {
  return index > 0 && node instanceof HTMLElement && /^(DIV|P)$/.test(node.tagName);
}

/** The text a part of the box stands for: emoji images count as the emoji they show. */
function textOf(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.nodeValue ?? "";
  if (node instanceof HTMLImageElement) return node.alt;
  if (node instanceof HTMLBRElement) return node.hasAttribute("data-end") ? "" : "\n";
  let text = "";
  node.childNodes.forEach((child, index) => {
    if (startsLine(child, index)) text += "\n";
    text += textOf(child);
  });
  return text;
}

/** How much text the first `end` children of `parent` stand for. */
function lengthBefore(parent: Node, end: number): number {
  let length = 0;
  for (let index = 0; index < end; index++) {
    const child = parent.childNodes[index];
    if (startsLine(child, index)) length += 1;
    length += textOf(child).length;
  }
  return length;
}

/** Where a point in the box falls in its text. Counted up the tree from the
 * point rather than from a copy of everything before it, which every
 * keystroke would otherwise make, twice. */
function offsetOf(root: HTMLElement, node: Node, offset: number): number {
  let length = node.nodeType === Node.TEXT_NODE ? offset : lengthBefore(node, Math.min(offset, node.childNodes.length));
  for (let child = node; child !== root && child.parentNode; child = child.parentNode) {
    const index = Array.prototype.indexOf.call(child.parentNode.childNodes, child);
    length += lengthBefore(child.parentNode, index);
    if (startsLine(child, index)) length += 1;
  }
  return length;
}

/** The point in the box at `offset` in its text. */
function pointAt(root: HTMLElement, offset: number): [Node, number] {
  let left = offset;
  const walk = (node: Node): [Node, number] | null => {
    for (let index = 0; index < node.childNodes.length; index++) {
      const child = node.childNodes[index];
      if (child.nodeType === Node.TEXT_NODE) {
        const length = child.nodeValue?.length ?? 0;
        if (left <= length) return [child, left];
        left -= length;
      } else if (child instanceof HTMLImageElement || child instanceof HTMLBRElement) {
        const length = textOf(child).length;
        if (left === 0) return [node, index];
        if (left < length) return [node, index + 1];
        left -= length;
        if (left === 0 && !(child instanceof HTMLBRElement)) return [node, index + 1];
      } else {
        const found = walk(child);
        if (found) return found;
      }
    }
    return null;
  };
  return walk(root) ?? [root, root.childNodes.length];
}

export const MessageInput = forwardRef<MessageInputHandle, MessageInputProps>(function MessageInput(
  { onChange, onCaret, onKeyDown, onPasteFiles, placeholder, disabled, label, mentions },
  ref,
) {
  const box = useRef<HTMLDivElement>(null);
  const text = useRef("");
  /** The selection, kept for when the box isn't focused, as from the emoji panel. */
  const selection = useRef({ start: 0, end: 0 });
  const composing = useRef(false);
  const undo = useRef<Snapshot[]>([]);
  const redo = useRef<Snapshot[]>([]);
  const lastEdit = useRef(0);
  const report = useRef(onChange);
  report.current = onChange;
  const reportCaret = useRef(onCaret);
  reportCaret.current = onCaret;
  const marked = useRef(mentions);
  marked.current = mentions;

  function readSelection(): { start: number; end: number } | null {
    const root = box.current;
    const current = window.getSelection();
    if (!root || !current || current.rangeCount === 0) return null;
    const range = current.getRangeAt(0);
    if (!root.contains(range.startContainer) || !root.contains(range.endContainer)) return null;
    return {
      start: offsetOf(root, range.startContainer, range.startOffset),
      end: offsetOf(root, range.endContainer, range.endOffset),
    };
  }

  function placeSelection(start: number, end = start) {
    const root = box.current;
    if (!root) return;
    selection.current = { start, end };
    if (document.activeElement !== root) return;
    const range = document.createRange();
    range.setStart(...pointAt(root, start));
    range.setEnd(...pointAt(root, end));
    const current = window.getSelection();
    current?.removeAllRanges();
    current?.addRange(range);
  }

  /** Draws `value` and puts the cursor at `start`–`end`. With `typed`, the
   * browser has just put the text and the cursor there itself: when the
   * drawing comes out the same, the cursor is left alone, since setting the
   * selection again makes WebKit lay out and spell-check the box anew. */
  function draw(value: string, start: number, end = start, typed = false) {
    const root = box.current;
    if (!root) return;
    const html = editorHtml(value, 20, marked.current);
    if (root.innerHTML !== html) root.innerHTML = html;
    else if (typed) {
      selection.current = { start, end };
      return;
    }
    placeSelection(start, end);
  }

  function remember(kind: "type" | "step") {
    const now = Date.now();
    const snapshot = { text: text.current, ...selection.current };
    // Typing in one go is one step to undo.
    if (kind === "step" || now - lastEdit.current > UNDO_GROUP_MS || undo.current.length === 0) {
      undo.current.push(snapshot);
      if (undo.current.length > UNDO_LIMIT) undo.current.shift();
    }
    lastEdit.current = kind === "step" ? 0 : now;
    redo.current = [];
  }

  /** Makes `value` the text, as one change. */
  function commit(value: string, start: number, end = start, kind: "type" | "step" = "step") {
    if (value !== text.current) remember(kind);
    text.current = value;
    draw(value, start, end);
    report.current(value, end);
  }

  function replaceSelection(insert: string, kind: "type" | "step" = "step") {
    const { start, end } = readSelection() ?? selection.current;
    const value = text.current.slice(0, start) + insert + text.current.slice(end);
    commit(value, start + insert.length, start + insert.length, kind);
  }

  /** After the browser changed the box itself: reads the text back and draws it properly. */
  function sync() {
    const root = box.current;
    if (!root || composing.current) return;
    const value = textOf(root);
    const at = readSelection() ?? { start: value.length, end: value.length };
    // The selection kept is still the one from before this change.
    if (value !== text.current) remember("type");
    text.current = value;
    draw(value, at.start, at.end, true);
    report.current(value, at.end);
  }

  function step(from: Snapshot[], to: Snapshot[]) {
    const snapshot = from.pop();
    if (!snapshot) return;
    to.push({ text: text.current, ...selection.current });
    text.current = snapshot.text;
    lastEdit.current = 0;
    draw(snapshot.text, snapshot.start, snapshot.end);
    report.current(snapshot.text, snapshot.end);
  }

  function wrap(before: string, after = before) {
    const { start, end } = readSelection() ?? selection.current;
    const current = text.current;
    const inner = current.slice(start, end);
    // Already wrapped: the markers come off.
    if (current.slice(start - before.length, start) === before && current.slice(end, end + after.length) === after) {
      const value = current.slice(0, start - before.length) + inner + current.slice(end + after.length);
      return commit(value, start - before.length, end - before.length);
    }
    const value = current.slice(0, start) + before + inner + after + current.slice(end);
    commit(value, start + before.length, end + before.length);
  }

  useImperativeHandle(ref, () => ({
    focus: () => {
      box.current?.focus();
      placeSelection(selection.current.start, selection.current.end);
    },
    insert: (value) => {
      box.current?.focus();
      const { start, end } = selection.current;
      placeSelection(start, end);
      replaceSelection(value);
    },
    replaceBefore: (start, value) => {
      const end = readSelection()?.end ?? selection.current.end;
      commit(text.current.slice(0, start) + value + text.current.slice(end), start + value.length);
    },
    wrap,
    setText: (value) => {
      undo.current = [];
      redo.current = [];
      text.current = value;
      draw(value, value.length);
      report.current(value, value.length);
    },
  }));

  // A mention just picked is marked once the text with it has been drawn.
  const mentionKey = mentions?.join("\n") ?? "";
  useLayoutEffect(() => {
    const at = readSelection() ?? selection.current;
    draw(text.current, at.start, at.end);
  }, [mentionKey]);

  // React's onBeforeInput is a different event, without `inputType`.
  const beforeInput = useRef<(event: InputEvent) => void>(() => {});
  beforeInput.current = (event) => {
    const type = event.inputType ?? "";
    // The browser's own bold and italic would put markup in the text.
    if (type.startsWith("format")) event.preventDefault();
    if (type === "historyUndo" || type === "historyRedo") {
      event.preventDefault();
      if (type === "historyUndo") step(undo.current, redo.current);
      else step(redo.current, undo.current);
    }
    if (type === "insertParagraph" || type === "insertLineBreak") {
      event.preventDefault();
      replaceSelection("\n");
    }
  };
  useLayoutEffect(() => {
    const root = box.current;
    if (!root) return;
    const listener = (event: Event) => beforeInput.current(event as InputEvent);
    root.addEventListener("beforeinput", listener);
    return () => root.removeEventListener("beforeinput", listener);
  }, []);

  // The cursor is followed so the emoji panel and autocomplete know where it is.
  useLayoutEffect(() => {
    const onSelection = () => {
      const at = readSelection();
      if (!at) return;
      selection.current = at;
      reportCaret.current?.(at.end);
    };
    document.addEventListener("selectionchange", onSelection);
    return () => document.removeEventListener("selectionchange", onSelection);
  }, []);

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    onKeyDown?.(event);
    if (event.isDefaultPrevented() || event.nativeEvent.isComposing) return;
    const mod = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    if (mod && key === "z") {
      event.preventDefault();
      return event.shiftKey ? step(redo.current, undo.current) : step(undo.current, redo.current);
    }
    if (mod && key === "y") {
      event.preventDefault();
      return step(redo.current, undo.current);
    }
    // The shortcuts WhatsApp Web has for its formatting.
    const marks: Record<string, string> = { b: "*", i: "_" };
    const shifted: Record<string, string> = { x: "~", m: "```" };
    const mark = event.shiftKey ? shifted[key] : marks[key];
    // Ctrl+Alt+Shift+M is the app's mute, not monospace.
    if (mod && mark && !event.altKey) {
      event.preventDefault();
      return wrap(mark);
    }
    if (event.key === "Enter" && event.shiftKey) {
      event.preventDefault();
      replaceSelection("\n");
    }
  }

  function handlePaste(event: ClipboardEvent<HTMLDivElement>) {
    event.preventDefault();
    const files = Array.from(event.clipboardData.files);
    if (files.length && onPasteFiles) return onPasteFiles(files);
    const pasted = event.clipboardData.getData("text/plain").replace(/\r\n?/g, "\n");
    if (pasted) replaceSelection(pasted);
  }

  return (
    <div
      ref={box}
      role="textbox"
      aria-multiline="true"
      aria-label={label}
      aria-placeholder={placeholder}
      aria-disabled={disabled}
      data-placeholder={placeholder}
      contentEditable={!disabled}
      suppressContentEditableWarning
      spellCheck
      onInput={sync}
      onCompositionStart={() => (composing.current = true)}
      onCompositionEnd={() => {
        composing.current = false;
        sync();
      }}
      onKeyDown={handleKeyDown}
      onPaste={handlePaste}
      onDrop={(event) => {
        // Dropped files are taken by the conversation; dropped text goes in as text.
        event.preventDefault();
        const dropped = event.dataTransfer.getData("text/plain");
        if (dropped) replaceSelection(dropped);
      }}
      className="composer-input max-h-35 min-h-10.5 flex-1 overflow-y-auto rounded-lg bg-(--composer-input-bg) px-3 py-2.5 text-[15px] leading-5.5 wrap-break-word whitespace-pre-wrap text-(--text-primary) outline-none focus-visible:outline-none aria-disabled:opacity-60"
    />
  );
});
