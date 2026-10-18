import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import type { StickerPack, StickerShelf } from "../backend";
import { CloseIcon, StickerIcon } from "./Icons";

/** The preview square, in CSS pixels; the sticker itself is 512. */
const VIEW = 360;
const SIDE = 512;
/** What WhatsApp plays of an animated sticker. */
const MAX_CLIP_MS = 10_000;
/** A video's frames are taken this often. */
const VIDEO_FPS = 15;
/** A video's frames are taken at most this large: the square never needs more. */
const FRAME_MAX = 512;
const CAPTION_FONT = '"Anton", Impact, "Arial Black", "Helvetica Neue", sans-serif';
/** Emojis offered at once; a sticker goes with up to three. */
const QUICK_EMOJIS = ["😂", "❤️", "👍", "😍", "😭", "🙏", "😮", "😡", "🎉", "🔥", "😎", "🥲"];

type Kind = "image" | "animated" | "video";

interface Source {
  blob: Blob;
  url: string;
  kind: Kind;
  width: number;
  height: number;
  /** For an animation or a video. */
  durationMs: number;
}

interface StickerEditorProps {
  shelf: StickerShelf;
  /** The pack it goes into at first; null for a new one. */
  pack: string | null;
  /** A picture to start from; without one, the editor asks for it. */
  picture?: Blob;
  onDone: (packId: string) => void;
  onClose: () => void;
  onError: (message: string) => void;
}

/** The size that makes `text` fit `width` at most, from `max` down. */
function captionSize(text: string, width: number, max: number): number {
  const ctx = document.createElement("canvas").getContext("2d");
  if (!ctx || !text) return max;
  let size = max;
  for (; size > 10; size -= 2) {
    ctx.font = `900 ${size}px ${CAPTION_FONT}`;
    if (ctx.measureText(text).width <= width) break;
  }
  return size;
}

/** The caption drawn on a transparent 512 square, as the backend lays it over the sticker. */
async function captionLayer(text: string, top: boolean): Promise<Blob | undefined> {
  if (!text.trim()) return undefined;
  const canvas = document.createElement("canvas");
  canvas.width = SIDE;
  canvas.height = SIDE;
  const ctx = canvas.getContext("2d");
  if (!ctx) return undefined;
  const size = captionSize(text, SIDE * 0.9, 76);
  ctx.font = `900 ${size}px ${CAPTION_FONT}`;
  ctx.textAlign = "center";
  ctx.textBaseline = top ? "top" : "bottom";
  ctx.lineJoin = "round";
  ctx.lineWidth = Math.max(4, size / 7);
  ctx.strokeStyle = "#000";
  ctx.fillStyle = "#fff";
  const y = top ? SIDE * 0.05 : SIDE * 0.95;
  ctx.strokeText(text, SIDE / 2, y);
  ctx.fillText(text, SIDE / 2, y);
  return new Promise((resolve) => canvas.toBlob((b) => resolve(b ?? undefined), "image/png"));
}

/** A video's frames between two times, as PNGs, by seeking a copy of it. */
async function videoFrames(url: string, startMs: number, endMs: number, onProgress: (done: number) => void): Promise<Blob[]> {
  const video = document.createElement("video");
  video.muted = true;
  video.preload = "auto";
  video.src = url;
  await new Promise<void>((resolve, reject) => {
    video.onloadeddata = () => resolve();
    video.onerror = () => reject(new Error("The video could not be read."));
  });
  const scale = Math.min(1, FRAME_MAX / Math.max(video.videoWidth, video.videoHeight));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(video.videoWidth * scale));
  canvas.height = Math.max(1, Math.round(video.videoHeight * scale));
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("The video could not be read.");
  const frames: Blob[] = [];
  const step = 1000 / VIDEO_FPS;
  for (let t = startMs; t < endMs; t += step) {
    // A seek to where it already is may never say it is done.
    if (Math.abs(video.currentTime - t / 1000) > 0.001) {
      await new Promise<void>((resolve) => {
        const late = window.setTimeout(resolve, 2000);
        video.onseeked = () => {
          window.clearTimeout(late);
          resolve();
        };
        video.currentTime = t / 1000;
      });
    }
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
    const frame = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
    if (frame) frames.push(frame);
    onProgress((t - startMs) / (endMs - startMs));
  }
  video.removeAttribute("src");
  video.load();
  return frames;
}

/** Reads what a chosen file is: a still, an animation or a video, and its size and length. */
async function readSource(blob: Blob, shelf: StickerShelf): Promise<Source> {
  const url = URL.createObjectURL(blob);
  try {
    if (blob.type.startsWith("video/")) {
      const video = document.createElement("video");
      video.preload = "metadata";
      video.src = url;
      await new Promise<void>((resolve, reject) => {
        video.onloadedmetadata = () => resolve();
        video.onerror = () => reject(new Error("That video can't be played here."));
      });
      return { blob, url, kind: "video", width: video.videoWidth, height: video.videoHeight, durationMs: Math.round(video.duration * 1000) };
    }
    if (shelf.probe && (blob.type === "image/gif" || blob.type === "image/webp")) {
      const probe = await shelf.probe(blob);
      return { blob, url, kind: probe.animated ? "animated" : "image", width: probe.width, height: probe.height, durationMs: probe.durationMs };
    }
    const img = new Image();
    img.src = url;
    await img.decode().catch(() => {
      throw new Error("That file is not a picture this app can read.");
    });
    return { blob, url, kind: "image", width: img.naturalWidth, height: img.naturalHeight, durationMs: 0 };
  } catch (e) {
    URL.revokeObjectURL(url);
    throw e;
  }
}

/**
 * Makes a sticker: from a picture, a GIF or a short video, cropped and
 * zoomed in the square, with a white outline and a caption, and for
 * something that moves, the part to keep. The preview is live; the sticker
 * itself is made by the app with the same settings.
 */
export function StickerEditor({ shelf, pack, picture, onDone, onClose, onError }: StickerEditorProps) {
  const [source, setSource] = useState<Source | null>(null);
  const [loading, setLoading] = useState(false);
  const [zoom, setZoom] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [outline, setOutline] = useState(0);
  const [caption, setCaption] = useState("");
  const [captionTop, setCaptionTop] = useState(false);
  const [clip, setClip] = useState<[number, number]>([0, 0]);
  const [emojis, setEmojis] = useState<string[]>([]);
  const [packs, setPacks] = useState<StickerPack[]>([]);
  const [target, setTarget] = useState<string>(pack ?? "");
  const [newPack, setNewPack] = useState("");
  const [making, setMaking] = useState<string | null>(null);
  const dialog = useRef<HTMLDivElement>(null);
  const media = useRef<HTMLVideoElement>(null);
  const drag = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null);
  const picker = useRef<HTMLInputElement>(null);

  useEffect(() => {
    shelf
      .library()
      .then((l) => setPacks(l.packs.filter((p) => !p.store)))
      .catch(() => {});
  }, [shelf]);

  const open = useCallback(
    (blob: Blob) => {
      setLoading(true);
      readSource(blob, shelf)
        .then((next) => {
          setSource((prev) => {
            if (prev) URL.revokeObjectURL(prev.url);
            return next;
          });
          setZoom(1);
          setOffset({ x: 0, y: 0 });
          setClip([0, Math.min(next.durationMs, next.kind === "video" ? 3000 : MAX_CLIP_MS)]);
        })
        .catch((e) => onError(String(e instanceof Error ? e.message : e)))
        .finally(() => setLoading(false));
    },
    [shelf, onError],
  );
  useEffect(() => {
    if (picture) open(picture);
  }, [picture, open]);
  useEffect(() => () => setSource((prev) => (prev && URL.revokeObjectURL(prev.url), null)), []);
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    dialog.current?.focus();
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  // A pasted picture or video replaces the one being edited.
  useEffect(() => {
    const onPaste = (event: ClipboardEvent) => {
      const file = Array.from(event.clipboardData?.files ?? []).find((f) => f.type.startsWith("image/") || f.type.startsWith("video/"));
      if (!file) return;
      event.preventDefault();
      open(file);
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }, [open]);

  // A video plays only the part that is kept.
  useEffect(() => {
    const video = media.current;
    if (!video || source?.kind !== "video") return;
    const [start, end] = clip;
    const onTime = () => {
      if (video.currentTime * 1000 >= end || video.currentTime * 1000 < start - 50) video.currentTime = start / 1000;
    };
    video.currentTime = start / 1000;
    video.addEventListener("timeupdate", onTime);
    return () => video.removeEventListener("timeupdate", onTime);
  }, [clip, source]);

  // The picture at zoom 1 fits the square whole.
  const base = source ? VIEW / Math.max(source.width, source.height) : 1;
  const shown = source ? { w: source.width * base * zoom, h: source.height * base * zoom } : { w: 0, h: 0 };
  const left = VIEW / 2 - shown.w / 2 + offset.x;
  const top = VIEW / 2 - shown.h / 2 + offset.y;
  const crop: [number, number, number, number] = source ? [-left / shown.w, -top / shown.h, VIEW / shown.w, VIEW / shown.h] : [0, 0, 1, 1];

  function fit(mode: "whole" | "fill") {
    if (!source) return;
    const cover = Math.max(source.width, source.height) / Math.min(source.width, source.height);
    setZoom(mode === "whole" ? 1 : cover);
    setOffset({ x: 0, y: 0 });
  }

  const outlinePx = (outline * VIEW) / SIDE;
  const outlineFilter = outline
    ? [
        [1, 0],
        [-1, 0],
        [0, 1],
        [0, -1],
        [0.7, 0.7],
        [-0.7, 0.7],
        [0.7, -0.7],
        [-0.7, -0.7],
      ]
        .map(([x, y]) => `drop-shadow(${(x * outlinePx).toFixed(1)}px ${(y * outlinePx).toFixed(1)}px 0 #fff)`)
        .join(" ")
    : undefined;
  const previewCaption = useMemo(() => captionSize(caption, VIEW * 0.9, (76 * VIEW) / SIDE), [caption]);

  const moves = source?.kind === "animated" || source?.kind === "video";
  const clipMs = clip[1] - clip[0];
  const packName = newPack.trim();
  const canMake = Boolean(source) && !making && (target !== "" || packName !== "" || packs.length === 0);

  async function make() {
    if (!source || !shelf.make) return;
    setMaking("Making the sticker…");
    try {
      const overlay = await captionLayer(caption.trim(), captionTop);
      let sticker: Parameters<NonNullable<StickerShelf["make"]>>[0]["source"];
      if (source.kind === "video") {
        const frames = await videoFrames(source.url, clip[0], clip[1], (done) => setMaking(`Reading the video… ${Math.round(done * 100)}%`));
        setMaking("Making the sticker…");
        sticker = { frames, frameMs: frames.map(() => Math.round(1000 / VIDEO_FPS)) };
      } else {
        sticker = { file: source.blob };
      }
      const packId = await shelf.make({
        source: sticker,
        crop,
        outline,
        startMs: source.kind === "animated" ? clip[0] : 0,
        endMs: source.kind === "animated" ? clip[1] : undefined,
        overlay,
        emojis,
        pack: target || undefined,
        newPack: target ? undefined : packName || "My stickers",
      });
      onDone(packId);
    } catch (e) {
      onError(String(e instanceof Error ? e.message : e));
      setMaking(null);
    }
  }

  function onPointerDown(event: ReactPointerEvent) {
    if (!source) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { x: event.clientX, y: event.clientY, ox: offset.x, oy: offset.y };
  }
  function onPointerMove(event: ReactPointerEvent) {
    const from = drag.current;
    if (!from) return;
    setOffset({ x: from.ox + event.clientX - from.x, y: from.oy + event.clientY - from.y });
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape" && !making) {
      event.preventDefault();
      onClose();
    }
  }

  const tab = (on: boolean) =>
    ["h-8 rounded-full px-3 text-[13px]", on ? "bg-(--chip-active-bg) font-semibold text-(--chip-active-text)" : "bg-(--chip-bg) text-(--text-secondary) hover:text-(--text-primary)"].join(" ");

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6" onPointerDown={(e) => e.target === e.currentTarget && !making && onClose()}>
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-label="New sticker"
        tabIndex={-1}
        onKeyDown={onKeyDown}
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => {
          const file = Array.from(e.dataTransfer.files).find((f) => f.type.startsWith("image/") || f.type.startsWith("video/"));
          if (!file) return;
          e.preventDefault();
          open(file);
        }}
        className="flex h-[min(640px,100%)] w-[min(980px,100%)] overflow-hidden rounded-2xl bg-(--panel-bg) shadow-2xl outline-none"
      >
        <aside className="flex w-65 shrink-0 flex-col gap-5 overflow-y-auto border-r border-(--divider) p-5">
          <h2 className="font-(family-name:--font-display) text-[19px] font-bold text-(--text-primary)">New sticker</h2>

          <div>
            <h3 className="mb-1.5 text-[12px] text-(--text-secondary)">Fit</h3>
            <div className="flex gap-1.5">
              <button type="button" disabled={!source} className={tab(zoom === 1 && !offset.x && !offset.y)} onClick={() => fit("whole")}>
                Whole
              </button>
              <button type="button" disabled={!source} className={tab(zoom > 1 && !offset.x && !offset.y)} onClick={() => fit("fill")}>
                Fill
              </button>
            </div>
            <label className="mt-3 flex items-center gap-2 text-[12px] text-(--text-secondary)">
              Zoom
              <input type="range" min={0.5} max={4} step={0.01} value={zoom} disabled={!source} onChange={(e) => setZoom(Number(e.target.value))} className="flex-1 accent-(--accent)" />
            </label>
            <p className="mt-1 text-[12px] text-(--text-secondary)">Drag the picture to place it.</p>
          </div>

          <div>
            <label className="flex items-center justify-between text-[14px] text-(--text-primary)">
              White outline
              <input type="checkbox" checked={outline > 0} onChange={(e) => setOutline(e.target.checked ? 10 : 0)} className="h-4 w-4 accent-(--accent)" />
            </label>
            {outline > 0 ? (
              <input type="range" min={4} max={24} value={outline} onChange={(e) => setOutline(Number(e.target.value))} aria-label="Outline width" className="mt-2 w-full accent-(--accent)" />
            ) : null}
          </div>

          <div>
            <h3 className="mb-1.5 text-[12px] text-(--text-secondary)">Caption</h3>
            <input
              value={caption}
              onChange={(e) => setCaption(e.target.value.toUpperCase())}
              placeholder="Optional"
              maxLength={40}
              className="h-9 w-full rounded-lg bg-(--search-bg) px-3 text-[14px] text-(--text-primary) outline-none placeholder:text-(--text-secondary)"
            />
            <div className="mt-2 flex gap-1.5">
              <button type="button" className={tab(captionTop)} onClick={() => setCaptionTop(true)}>
                Top
              </button>
              <button type="button" className={tab(!captionTop)} onClick={() => setCaptionTop(false)}>
                Bottom
              </button>
            </div>
          </div>

          {moves && source ? (
            <div>
              <h3 className="mb-1.5 text-[12px] text-(--text-secondary)">
                {source.kind === "video" ? "Part of the video" : "Part of the animation"} · {(clipMs / 1000).toFixed(1)} s
              </h3>
              <label className="flex items-center gap-2 text-[12px] text-(--text-secondary)">
                From
                <input
                  type="range"
                  min={0}
                  max={source.durationMs}
                  step={50}
                  value={clip[0]}
                  onChange={(e) => {
                    const start = Math.min(Number(e.target.value), clip[1] - 100);
                    setClip([start, Math.min(clip[1], start + MAX_CLIP_MS)]);
                  }}
                  className="flex-1 accent-(--accent)"
                />
              </label>
              <label className="mt-1 flex items-center gap-2 text-[12px] text-(--text-secondary)">
                To
                <input
                  type="range"
                  min={0}
                  max={source.durationMs}
                  step={50}
                  value={clip[1]}
                  onChange={(e) => {
                    const end = Math.max(Number(e.target.value), clip[0] + 100);
                    setClip([Math.max(clip[0], end - MAX_CLIP_MS), end]);
                  }}
                  className="flex-1 accent-(--accent)"
                />
              </label>
              <p className="mt-1 text-[12px] text-(--text-secondary)">WhatsApp plays up to 10 s.</p>
            </div>
          ) : null}

          <div className="flex-1" />
          <button type="button" onClick={() => picker.current?.click()} className="h-8 self-start rounded-full bg-(--chip-bg) px-3 text-[13px] text-(--text-primary)">
            {source ? "Another picture…" : "Choose a picture…"}
          </button>
        </aside>

        <section className="flex min-w-0 flex-1 flex-col bg-(--chat-bg)">
          <div className="flex justify-end p-2">
            <button type="button" aria-label="Close" disabled={Boolean(making)} onClick={onClose} className="flex h-9 w-9 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)">
              <CloseIcon size={20} />
            </button>
          </div>
          <div className="flex min-h-0 flex-1 items-center justify-center">
            {source ? (
              <div
                className="relative cursor-grab touch-none overflow-hidden rounded-xl active:cursor-grabbing"
                style={{ width: VIEW, height: VIEW, background: "repeating-conic-gradient(rgba(127,127,127,.22) 0 25%, transparent 0 50%) 0 0/20px 20px" }}
                onPointerDown={onPointerDown}
                onPointerMove={onPointerMove}
                onPointerUp={() => (drag.current = null)}
                onWheel={(e) => setZoom((z) => Math.min(4, Math.max(0.5, z * (e.deltaY < 0 ? 1.06 : 1 / 1.06))))}
              >
                {source.kind === "video" ? (
                  <video
                    ref={media}
                    src={source.url}
                    muted
                    autoPlay
                    loop
                    playsInline
                    draggable={false}
                    className="pointer-events-none absolute max-w-none"
                    style={{ left, top, width: shown.w, height: shown.h, filter: outlineFilter }}
                  />
                ) : (
                  <img
                    src={source.url}
                    alt="The sticker being made"
                    draggable={false}
                    className="pointer-events-none absolute max-w-none"
                    style={{ left, top, width: shown.w, height: shown.h, filter: outlineFilter }}
                  />
                )}
                {caption.trim() ? (
                  <span
                    className="pointer-events-none absolute inset-x-0 text-center text-white"
                    style={{
                      [captionTop ? "top" : "bottom"]: VIEW * 0.05,
                      fontFamily: CAPTION_FONT,
                      fontWeight: 900,
                      fontSize: previewCaption,
                      lineHeight: 1,
                      WebkitTextStroke: `${Math.max(2, previewCaption / 7) * 0.9}px #000`,
                      paintOrder: "stroke fill",
                    }}
                  >
                    {caption}
                  </span>
                ) : null}
              </div>
            ) : (
              <button
                type="button"
                onClick={() => picker.current?.click()}
                className="flex flex-col items-center justify-center gap-2 rounded-2xl border-2 border-dashed border-(--divider) text-[14px] text-(--text-secondary) hover:border-(--accent) hover:text-(--accent)"
                style={{ width: VIEW, height: VIEW }}
              >
                <StickerIcon size={40} />
                {loading ? "Opening…" : "Drop, paste or choose a picture, GIF or short video"}
              </button>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-3 border-t border-(--divider) bg-(--panel-bg) px-5 py-3">
            <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1" role="group" aria-label="Emoji it goes with">
              <span className="mr-1 text-[12px] text-(--text-secondary)">Emoji</span>
              {QUICK_EMOJIS.map((emoji) => {
                const on = emojis.includes(emoji);
                return (
                  <button
                    key={emoji}
                    type="button"
                    aria-pressed={on}
                    onClick={() => setEmojis((prev) => (on ? prev.filter((e) => e !== emoji) : [...prev, emoji].slice(-3)))}
                    className={["flex h-8 w-8 items-center justify-center rounded-lg text-[18px]", on ? "bg-(--chip-active-bg)" : "hover:bg-(--hover-bg)"].join(" ")}
                  >
                    {emoji}
                  </button>
                );
              })}
            </div>
            <select
              value={target}
              onChange={(e) => setTarget(e.target.value)}
              aria-label="Pack"
              className="h-9 rounded-lg bg-(--search-bg) px-2 text-[14px] text-(--text-primary) outline-none"
            >
              {packs.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
              <option value="">New pack…</option>
            </select>
            {target === "" ? (
              <input
                value={newPack}
                onChange={(e) => setNewPack(e.target.value)}
                placeholder="My stickers"
                aria-label="New pack's name"
                className="h-9 w-36 rounded-lg bg-(--search-bg) px-3 text-[14px] text-(--text-primary) outline-none placeholder:text-(--text-secondary)"
              />
            ) : null}
            <button
              type="button"
              disabled={!canMake}
              onClick={() => void make()}
              className="h-9 rounded-full bg-(--accent) px-4 text-[14px] font-semibold text-(--on-accent) disabled:opacity-50"
            >
              {making ?? "Add to pack"}
            </button>
          </div>
        </section>
      </div>
      <input
        ref={picker}
        type="file"
        accept="image/png,image/jpeg,image/gif,image/webp,video/mp4,video/webm,video/quicktime"
        hidden
        onChange={(e) => {
          const file = e.target.files?.[0];
          e.target.value = "";
          if (file) open(file);
        }}
      />
    </div>
  );
}
