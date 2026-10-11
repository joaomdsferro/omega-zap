import { useEffect, useRef, useState } from "react";
import type { CSSProperties, RefObject } from "react";
import type { Attachment, Message } from "../backend";
import { formatFileSize } from "../lib/format";
import { AudioPlayer } from "./AudioPlayer";
import { FileIcon, PlayIcon } from "./Icons";

export type LoadMedia = (message: Message, index?: number) => Promise<string>;

/** Videos up to this size are fetched as they scroll near, so their first frame shows. */
const VIDEO_PRELOAD_BYTES = 8 * 1024 * 1024;
/** The largest a single photo or video is drawn, in pixels. */
const SINGLE_MAX_WIDTH = 330;
const SINGLE_MAX_HEIGHT = 340;
/** An album grid shows this many tiles; the last one counts the rest. */
const GRID_TILES = 4;

const placeholderClass =
  "bg-[linear-gradient(145deg,color-mix(in_oklab,var(--accent)_35%,#0b141a),#1f3a33)]";

/** Shown in the viewer: photos, GIFs and videos. */
export function isVisual(attachment: Attachment) {
  return attachment.kind === "image" || attachment.kind === "gif" || attachment.kind === "video";
}

/** How long one frame of a GIF lasts when its frames have not been timed yet. */
const DEFAULT_FRAME_SECONDS = 1 / 15;

/**
 * A GIF: a muted video that plays over and over. WebKitGTK's own `loop` jumps
 * back to the start partway through some videos, and going back to the start
 * by hand shows a black frame. So two copies take turns in the same place:
 * while one plays, the other waits on its first frame, and they swap as soon
 * as the last frame is on screen. Waiting out that frame lets the player
 * reach its end first, and the end is what goes black.
 */
export function GifVideo({ src, className, wrapperClassName, style, onError }: {
  src: string;
  /** For each copy of the video. */
  className: string;
  wrapperClassName: string;
  style?: CSSProperties;
  onError?: () => void;
}) {
  const copies = [useRef<HTMLVideoElement>(null), useRef<HTMLVideoElement>(null)];
  const [front, setFront] = useState(0);
  const shown = useRef(0);
  const frame = useRef(DEFAULT_FRAME_SECONDS);

  function swap() {
    const current = copies[shown.current].current;
    const next = copies[1 - shown.current].current;
    if (!current || !next) return;
    shown.current = 1 - shown.current;
    void next.play().catch(() => {});
    setFront(shown.current);
    current.pause();
    current.currentTime = 0;
    watch(next);
  }

  // Follows the frames on screen to swap at the last one.
  function watch(video: HTMLVideoElement) {
    if (!("requestVideoFrameCallback" in video)) return;
    let previous = -1;
    const onFrame: VideoFrameRequestCallback = (_now, { mediaTime }) => {
      if (video !== copies[shown.current].current) return;
      const step = mediaTime - previous;
      if (previous >= 0 && step > 0.01 && step < 0.5) frame.current = step;
      previous = mediaTime;
      if (Number.isFinite(video.duration) && mediaTime >= video.duration - frame.current * 1.5) swap();
      else video.requestVideoFrameCallback(onFrame);
    };
    video.requestVideoFrameCallback(onFrame);
  }

  return (
    <span className={`grid ${wrapperClassName}`}>
      {copies.map((ref, index) => (
        <video
          key={index}
          ref={ref}
          src={src}
          onError={onError}
          onLoadedData={(event) => index === 0 && watch(event.currentTarget)}
          // Where frames cannot be followed, the end is the cue.
          onEnded={(event) => event.currentTarget === copies[shown.current].current && swap()}
          autoPlay={index === 0}
          muted
          playsInline
          preload="auto"
          className={[className, "[grid-area:1/1]", index === front ? "" : "opacity-0"].join(" ")}
          style={style}
        />
      ))}
    </span>
  );
}

export function plays(attachment: Attachment) {
  return attachment.mime.startsWith("video/");
}

/** True once the element comes within `margin` of the screen. */
export function useNear(el: RefObject<Element | null>, margin = "600px 0px") {
  const [near, setNear] = useState(false);
  useEffect(() => {
    const node = el.current;
    if (!node || near) return;
    const observer = new IntersectionObserver(
      (entries) => entries.some((entry) => entry.isIntersecting) && setNear(true),
      { rootMargin: margin },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [el, margin, near]);
  return near;
}

/**
 * Fetches an attachment once `wanted`; null while it loads, "failed" when it can't.
 * With `blob`, the file is read into memory first: WebKitGTK's video and audio
 * players refuse the app's own `media://` scheme but play a `blob:` URL.
 */
export function useMediaUrl(message: Message, index: number, loadMedia: LoadMedia | undefined, wanted: boolean, blob = false) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    setUrl(null);
    if (!wanted || !loadMedia) return;
    let alive = true;
    let objectUrl: string | null = null;
    loadMedia(message, index)
      .then(async (loaded) => {
        if (!blob) return loaded;
        const response = await fetch(loaded);
        if (!response.ok) throw new Error(`media answered ${response.status}`);
        objectUrl = URL.createObjectURL(await response.blob());
        if (!alive) URL.revokeObjectURL(objectUrl);
        return objectUrl;
      })
      .then((loaded) => alive && setUrl(loaded))
      .catch(() => alive && setUrl("failed"));
    return () => {
      alive = false;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [message.id, index, loadMedia, wanted, blob]);
  return loadMedia ? url : "failed";
}

function label(attachment: Attachment) {
  switch (attachment.kind) {
    case "gif":
      return "GIF";
    case "video":
      return "Video";
    case "sticker":
      return "Sticker";
    default:
      return "Photo";
  }
}

interface TileProps {
  message: Message;
  index: number;
  attachment: Attachment;
  loadMedia?: LoadMedia;
  /** Opens the viewer at this attachment. */
  onOpen?: () => void;
  /** Drawn alone, at the size the sender gave. */
  single: boolean;
  /** Attachments the grid has no room for, counted on the last tile. */
  more?: number;
}

function Tile({ message, index, attachment, loadMedia, onOpen, single, more }: TileProps) {
  const box = useRef<HTMLDivElement>(null);
  const near = useNear(box);
  const video = plays(attachment);
  const isVideo = attachment.kind === "video";
  const [playing, setPlaying] = useState(false);
  // GIFs and photos load as they come near; big videos wait for a press.
  const eager = !isVideo || attachment.size <= VIDEO_PRELOAD_BYTES;
  const url = useMediaUrl(message, index, loadMedia, near && (eager || playing), video);
  const [broken, setBroken] = useState(false);
  const ready = url && url !== "failed" && !broken;
  const name = label(attachment);

  const { width: w, height: h } = attachment;
  const sized =
    single && w && h
      ? { width: Math.round(w * Math.min(SINGLE_MAX_WIDTH / w, SINGLE_MAX_HEIGHT / h, 1)), aspectRatio: `${w} / ${h}` }
      : undefined;

  let body;
  if (ready && isVideo && playing && single) {
    body = (
      <video
        src={url}
        onError={() => setBroken(true)}
        controls
        autoPlay
        playsInline
        className="block h-full w-full bg-black object-contain"
      />
    );
  } else if (ready && video && attachment.kind === "gif") {
    body = (
      <GifVideo
        src={url}
        onError={() => setBroken(true)}
        wrapperClassName="h-full w-full"
        className="block h-full w-full object-cover"
      />
    );
  } else if (ready && video) {
    // Already in memory: loaded in full so WebKit paints a frame, a little
    // way in, since many videos open on black.
    body = (
      <video
        src={`${url}#t=0.1`}
        onError={() => setBroken(true)}
        muted
        playsInline
        preload="auto"
        className="block h-full w-full object-cover"
      />
    );
  } else if (ready) {
    body = <img src={url} alt={name} draggable={false} className="block h-full w-full object-cover" />;
  } else {
    body = (
      <span className={`flex h-full w-full items-end p-2 ${placeholderClass}`}>
        <span className="rounded bg-black/40 px-2 py-0.5 text-[12px] text-white/90">
          {url === "failed" || broken ? `${name} unavailable` : eager || playing ? `Loading ${name.toLowerCase()}…` : formatFileSize(attachment.size)}
        </span>
      </span>
    );
  }

  const playingInline = isVideo && playing && single && ready;
  function press() {
    // A lone video plays where it is; in an album it opens the viewer.
    if (isVideo && single) setPlaying(true);
    else onOpen?.();
  }

  return (
    <div
      ref={box}
      style={sized}
      className={[
        "relative overflow-hidden",
        single ? (sized ? "max-w-full rounded-md" : "h-55 w-70 max-w-full rounded-md") : "h-full w-full",
      ].join(" ")}
    >
      {body}
      {playingInline ? null : (
        <button
          type="button"
          onClick={press}
          disabled={!onOpen && !(isVideo && single)}
          aria-label={`${isVideo ? "Play" : "Open"} ${name.toLowerCase()}`}
          className="absolute inset-0 flex items-center justify-center disabled:cursor-default"
        >
          {isVideo ? (
            <span className="flex h-12 w-12 items-center justify-center rounded-full bg-black/50 text-white">
              <PlayIcon size={28} />
            </span>
          ) : null}
          {attachment.kind === "gif" ? (
            <span className="absolute bottom-1.5 left-1.5 rounded bg-black/50 px-1.5 text-[11px] font-medium text-white">GIF</span>
          ) : null}
          {more ? (
            <span className="absolute inset-0 flex items-center justify-center bg-black/50 text-[26px] font-medium text-white">
              +{more}
            </span>
          ) : null}
        </button>
      )}
    </div>
  );
}

interface Indexed {
  attachment: Attachment;
  index: number;
}

function Album({ message, items, loadMedia, onOpen }: {
  message: Message;
  items: Indexed[];
  loadMedia?: LoadMedia;
  onOpen?: (index: number) => void;
}) {
  if (items.length === 1) {
    const [{ attachment, index }] = items;
    return (
      <div className="mb-1">
        <Tile message={message} index={index} attachment={attachment} loadMedia={loadMedia} single onOpen={onOpen && (() => onOpen(index))} />
      </div>
    );
  }
  const shown = items.slice(0, GRID_TILES);
  const more = items.length - shown.length;
  return (
    <div className="mb-1 grid w-75 max-w-full grid-cols-2 gap-0.5 overflow-hidden rounded-md">
      {shown.map(({ attachment, index }, at) => (
        <div
          key={index}
          className={shown.length === 3 && at === 0 ? "col-span-2 aspect-[2/1]" : "aspect-square"}
        >
          <Tile
            message={message}
            index={index}
            attachment={attachment}
            loadMedia={loadMedia}
            single={false}
            more={at === shown.length - 1 && more > 0 ? more : undefined}
            onOpen={onOpen && (() => onOpen(index))}
          />
        </div>
      ))}
    </div>
  );
}

function Sticker({ message, index, loadMedia }: { message: Message; index: number; loadMedia?: LoadMedia }) {
  const box = useRef<HTMLDivElement>(null);
  const url = useMediaUrl(message, index, loadMedia, useNear(box));
  return (
    <div ref={box} className="mb-1 flex h-36 w-36 items-center justify-center">
      {url && url !== "failed" ? (
        <img src={url} alt="Sticker" draggable={false} className="max-h-full max-w-full object-contain" />
      ) : (
        <span className="text-[13px] text-(--text-secondary)">{url === "failed" ? "Sticker unavailable" : "Sticker"}</span>
      )}
    </div>
  );
}

function Document({ message, index, attachment, openMedia }: {
  message: Message;
  index: number;
  attachment: Attachment;
  openMedia?: (message: Message, index: number) => Promise<void>;
}) {
  const [state, setState] = useState<"idle" | "opening" | "failed">("idle");
  const name = attachment.fileName ?? "Document";
  const extension = name.includes(".") ? name.split(".").pop()?.toUpperCase() : undefined;

  function open() {
    if (!openMedia || state === "opening") return;
    setState("opening");
    openMedia(message, index)
      .then(() => setState("idle"))
      .catch(() => setState("failed"));
  }

  return (
    <button
      type="button"
      onClick={open}
      disabled={!openMedia}
      title={openMedia ? `Open ${name}` : undefined}
      className="mb-1 flex w-70 max-w-full items-center gap-3 rounded-md bg-(--quote-bg) px-3 py-2.5 text-left disabled:cursor-default"
    >
      <FileIcon size={26} className="shrink-0 text-(--icon)" />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[14px]">{name}</span>
        <span className="block text-[12px] text-(--text-secondary)">
          {state === "opening"
            ? "Opening…"
            : state === "failed"
              ? "Could not open it"
              : [extension, formatFileSize(attachment.size)].filter(Boolean).join(" · ")}
        </span>
      </span>
    </button>
  );
}

interface AttachmentsProps {
  message: Message;
  attachments: Attachment[];
  loadMedia?: LoadMedia;
  openMedia?: (message: Message, index: number) => Promise<void>;
  /** Opens the viewer at one of the message's photos or videos. */
  onOpen?: (message: Message, index: number) => void;
}

/** Everything sent with a message: albums, stickers, audio and documents. */
export function Attachments({ message, attachments, loadMedia, openMedia, onOpen }: AttachmentsProps) {
  const indexed = attachments.map((attachment, index) => ({ attachment, index }));
  const visual = indexed.filter(({ attachment }) => isVisual(attachment));
  return (
    <>
      {visual.length > 0 ? (
        <Album message={message} items={visual} loadMedia={loadMedia} onOpen={onOpen && ((index) => onOpen(message, index))} />
      ) : null}
      {indexed.map(({ attachment, index }) => {
        switch (attachment.kind) {
          case "sticker":
            return <Sticker key={index} message={message} index={index} loadMedia={loadMedia} />;
          case "voice":
          case "audio":
            return <AudioPlayer key={index} message={message} index={index} attachment={attachment} loadMedia={loadMedia} />;
          case "file":
            return <Document key={index} message={message} index={index} attachment={attachment} openMedia={openMedia} />;
          default:
            return null;
        }
      })}
    </>
  );
}
