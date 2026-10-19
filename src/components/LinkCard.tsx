import { useEffect, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { LinkPreview } from "../backend";
import { embedUrl, videoLink, xVideo, type XVideo } from "../lib/links";
import { isTauri } from "../lib/platform";
import { useSettings } from "../settings/settings";
import { CloseIcon, MuteIcon, PlayIcon, VolumeIcon } from "./Icons";
import { VideoPlayer } from "./VideoPlayer";

/** Opens a page in the system browser, or a new tab in a plain browser. */
export function openExternal(url: string) {
  if (isTauri) void openUrl(url);
  else window.open(url, "_blank", "noopener");
}

/** The desktop app hands links to the system browser; a plain browser follows them itself. */
export function openLink(event: ReactMouseEvent, url: string) {
  if (!isTauri) return;
  event.preventDefault();
  void openUrl(url);
}

/** Below this width a picture is a thumbnail: it sits beside the title instead of above it. */
const LARGE_PREVIEW_PX = 300;

// Kept across chat switches, by link, so a card reopens at its final size
// with its picture instead of growing once the picture is fetched again.
/** The full-size pictures fetched so far. */
const largeUrls = new Map<string, string>();
/** Whether a link's small picture turned out wide enough to sit above the title. */
const wideThumbs = new Map<string, boolean>();

const VOLUME_KEY = "omega-zap.embed-volume";

/** The volume embedded players play at, the same for all and kept for next time. */
function savedVolume(): number {
  try {
    const level = Number(localStorage.getItem(VOLUME_KEY) ?? "1");
    return level >= 0 && level <= 1 ? level : 1;
  } catch {
    return 1;
  }
}

/** Sets a player page's volume: a script the app puts in each player page takes it. */
function sendVolume(frame: HTMLIFrameElement | null, level: number) {
  frame?.contentWindow?.postMessage({ omegaZapVolume: level }, "*");
}

interface LinkCardProps {
  preview: LinkPreview;
  /** Fetches the full-size picture, when the account can. */
  loadLarge?: () => Promise<string>;
}

export function LinkCard({ preview, loadLarge }: LinkCardProps) {
  const { settings } = useSettings();
  const [wide, setWide] = useState(() => wideThumbs.get(preview.url) ?? false);
  const [broken, setBroken] = useState(false);
  const [largeUrl, setLargeUrl] = useState(() => largeUrls.get(preview.url) ?? null);
  const [largeFailed, setLargeFailed] = useState(false);
  const [player, setPlayer] = useState<string | null>(null);
  const frame = useRef<HTMLIFrameElement>(null);
  const [volume, setVolume] = useState(savedVolume);
  // What unmuting goes back to.
  const unmuted = useRef(volume || 1);
  const localVolume = useRef(volume);
  localVolume.current = volume;
  const [starting, setStarting] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const linked = settings.inlineVideos ? videoLink(preview.url) : null;
  // A post on X is only a video once X says it has one.
  const [xClip, setXClip] = useState<XVideo | null>(null);
  const [clip, setClip] = useState<XVideo | null>(null);
  const post = linked?.post;
  useEffect(() => {
    setXClip(null);
    if (!post) return;
    let alive = true;
    void xVideo(post).then((found) => alive && setXClip(found));
    return () => {
      alive = false;
    };
  }, [post]);
  const video = linked?.site === "x" && !xClip ? null : linked;
  const wantLarge = Boolean(settings.largeLinkPreviews && preview.hasLarge && loadLarge) && !largeFailed;

  useEffect(() => {
    const known = largeUrls.get(preview.url) ?? null;
    setLargeUrl(known);
    if (known || !wantLarge || !loadLarge) return;
    let alive = true;
    loadLarge()
      .then((loaded) => {
        largeUrls.set(preview.url, loaded);
        if (alive) setLargeUrl(loaded);
      })
      // The small picture stays.
      .catch(() => alive && setLargeFailed(true));
    return () => {
      alive = false;
    };
  }, [preview.url, wantLarge]);

  let host = preview.url;
  try {
    host = new URL(preview.url).hostname;
  } catch {
    // Keep what the sender's phone matched.
  }
  const picture = largeUrl ?? (broken ? null : preview.thumbnail) ?? null;
  // A card that will get the full-size picture is laid out for it from the
  // start, in a box of fixed shape, so the picture arriving changes nothing
  // around it.
  const playing = Boolean(player || clip);
  const large = wantLarge && !playing;
  const stacked = playing || large || (wide && Boolean(picture));

  function changeVolume(level: number) {
    setVolume(level);
    if (level > 0) unmuted.current = level;
    sendVolume(frame.current, level);
    try {
      localStorage.setItem(VOLUME_KEY, String(level));
    } catch {
      // Only remembered for this time.
    }
  }

  async function play() {
    if (xClip) return setClip(xClip);
    setStarting(true);
    setNote(null);
    try {
      setPlayer(await embedUrl(preview.url));
    } catch {
      setNote("This video only plays on its own site. It was opened in your browser.");
      openExternal(preview.url);
    } finally {
      setStarting(false);
    }
  }

  return (
    <div
      className={[
        "mb-1 flex w-82.5 max-w-full overflow-hidden rounded-md bg-(--quote-bg)",
        stacked ? "flex-col" : "flex-row",
      ].join(" ")}
    >
      {playing ? (
        <div className="flex justify-end">
          <button
            type="button"
            onClick={() => {
              setPlayer(null);
              setClip(null);
            }}
            className="flex h-8 items-center gap-1.5 px-2.5 text-[12.5px] text-(--text-secondary) hover:text-(--text-primary)"
          >
            Close video
            <CloseIcon size={16} />
          </button>
        </div>
      ) : null}
      {clip ? (
        <VideoPlayer
          src={clip.url}
          poster={clip.poster ?? undefined}
          onError={() => {
            setClip(null);
            setNote("This video only plays on its own site. It was opened in your browser.");
            openExternal(preview.url);
          }}
          className="w-full"
          videoClassName="max-h-110 w-full object-contain"
          videoStyle={{ aspectRatio: `${clip.width} / ${clip.height}` }}
        />
      ) : player ? (
        <>
          <iframe
            ref={frame}
            src={player}
            // The player's script is in place once its page loads; it is told again as it starts, in case.
            onLoad={() => {
              sendVolume(frame.current, volume);
              window.setTimeout(() => sendVolume(frame.current, localVolume.current), 1500);
            }}
            title={preview.title ?? "Video"}
            allow="autoplay; encrypted-media; fullscreen; picture-in-picture"
            allowFullScreen
            referrerPolicy="strict-origin-when-cross-origin"
            sandbox="allow-scripts allow-same-origin allow-popups allow-presentation"
            // Instagram wraps the video in its own header and footer.
            className={["w-full border-0 bg-black", video?.site === "instagram" ? "h-150" : "aspect-9/16"].join(" ")}
          />
          <div className="flex items-center gap-1 py-0.5 pr-3 pl-1">
            <button
              type="button"
              onClick={() => changeVolume(volume > 0 ? 0 : unmuted.current)}
              aria-label={volume > 0 ? "Mute" : "Unmute"}
              title={volume > 0 ? "Mute" : "Unmute"}
              className="flex h-8 w-8 items-center justify-center rounded-full text-(--text-secondary) hover:text-(--text-primary)"
            >
              {volume > 0 ? <VolumeIcon size={18} /> : <MuteIcon size={18} />}
            </button>
            <input
              type="range"
              min={0}
              max={1}
              step={0.05}
              value={volume}
              onChange={(event) => changeVolume(Number(event.target.value))}
              aria-label="Volume"
              aria-valuetext={`${Math.round(volume * 100)}%`}
              className="min-w-0 flex-1 accent-(--accent)"
            />
          </div>
        </>
      ) : picture || video ? (
        <div
          onClick={video ? undefined : () => openExternal(preview.url)}
          className={[
            "relative shrink-0",
            large ? "aspect-video w-full bg-black/40" : stacked ? "w-full" : "h-22 w-22",
            video ? "" : "cursor-pointer",
          ].join(" ")}
        >
          {picture ? (
            <img
              src={picture}
              alt=""
              draggable={false}
              onLoad={(event) => {
                if (event.currentTarget.getAttribute("src") !== preview.thumbnail) return;
                const isWide = event.currentTarget.naturalWidth >= LARGE_PREVIEW_PX;
                wideThumbs.set(preview.url, isWide);
                setWide(isWide);
              }}
              onError={() => {
                if (!largeUrl) return setBroken(true);
                largeUrls.delete(preview.url);
                setLargeUrl(null);
                setLargeFailed(true);
              }}
              className={
                large ? "h-full w-full object-cover" : stacked ? "block max-h-82.5 w-full object-cover" : "h-full w-full object-cover"
              }
            />
          ) : (
            <div className="h-full w-full bg-black/40" />
          )}
          {video ? (
            <button
              type="button"
              onClick={play}
              disabled={starting}
              aria-label="Play video"
              className="absolute inset-0 flex items-center justify-center"
            >
              <span
                className={[
                  "flex items-center justify-center rounded-full bg-black/55 text-white",
                  stacked ? "h-14 w-14" : "h-10 w-10",
                  starting ? "animate-pulse" : "",
                ].join(" ")}
              >
                <PlayIcon size={stacked ? 30 : 22} />
              </span>
            </button>
          ) : null}
        </div>
      ) : null}
      <a
        href={preview.url}
        target="_blank"
        rel="noreferrer"
        onClick={(event) => openLink(event, preview.url)}
        className="flex min-w-0 flex-1 flex-col justify-center px-2.5 py-1.5"
      >
        {preview.title ? <p className="line-clamp-2 text-[14px] font-medium">{preview.title}</p> : null}
        {preview.description ? (
          <p className="line-clamp-2 text-[12.5px] text-(--text-secondary)">{preview.description}</p>
        ) : null}
        <p className="truncate text-[12px] text-(--text-secondary)">{host}</p>
        {note ? <p className="mt-0.5 text-[12px] text-(--text-secondary)">{note}</p> : null}
      </a>
    </div>
  );
}
