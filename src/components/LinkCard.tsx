import { useEffect, useId, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { LinkPreview } from "../backend";
import { embedUrl, videoLink, xVideo, type VideoSite, type XVideo } from "../lib/links";
import { isTauri } from "../lib/platform";
import { useSettings } from "../settings/settings";
import { CloseIcon, CopyIcon, InfoIcon, MuteIcon, PlayIcon, VolumeIcon } from "./Icons";
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
  // Why the video can't play in the chat, shown in its place with ways to open it elsewhere.
  const [blocked, setBlocked] = useState<Blocked | null>(null);
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
  const playing = Boolean(player || clip || blocked);
  const large = wantLarge && !playing;
  const stacked = playing || large || (wide && Boolean(picture));

  // Instagram's player says, through the app's script in it, when it won't play the video here.
  useEffect(() => {
    if (!player) return;
    const onMessage = (event: MessageEvent) => {
      if (event.source !== frame.current?.contentWindow || !event.data?.omegaZapUnplayable) return;
      setPlayer(null);
      setBlocked("music");
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [player, preview.url]);

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
    setBlocked(null);
    try {
      setPlayer(await embedUrl(preview.url));
    } catch {
      setBlocked("site");
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
              setBlocked(null);
            }}
            className="flex h-8 items-center gap-1.5 px-2.5 text-[12.5px] text-(--text-secondary) hover:text-(--text-primary)"
          >
            Close video
            <CloseIcon size={16} />
          </button>
        </div>
      ) : null}
      {blocked ? (
        <BlockedVideo site={video?.site} why={blocked} url={preview.url} />
      ) : clip ? (
        <VideoPlayer
          src={clip.url}
          poster={clip.poster ?? undefined}
          onError={() => {
            setClip(null);
            setBlocked("site");
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
      </a>
    </div>
  );
}

/** Why a video can't play here: Instagram keeps some reels to its site, often over their music, or the site's player wouldn't load. */
type Blocked = "music" | "site";

const SITE_NAMES: Record<VideoSite, string> = { tiktok: "TikTok", instagram: "Instagram", youtube: "YouTube", x: "X" };

interface Browser {
  id: string;
  name: string;
  default: boolean;
}

/** In place of a video that won't play in the chat: why, and where it can be watched. */
function BlockedVideo({ site, why, url }: { site?: VideoSite; why: Blocked; url: string }) {
  const name = site ? SITE_NAMES[site] : "This site";
  const [browsers, setBrowsers] = useState<Browser[] | null>(isTauri ? null : []);
  const [copied, setCopied] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  // The play button it replaces had the focus; it moves here, so the reason is read out.
  const panel = useRef<HTMLDivElement>(null);
  const titleId = useId();
  useEffect(() => panel.current?.focus({ preventScroll: true }), []);
  useEffect(() => {
    if (!isTauri) return;
    let alive = true;
    invoke<Browser[]>("browsers")
      .catch(() => [])
      .then((found) => alive && setBrowsers(found));
    return () => {
      alive = false;
    };
  }, []);

  const main = browsers?.find((browser) => browser.default) ?? browsers?.[0];
  const others = browsers?.filter((browser) => browser !== main) ?? [];

  function open(browser?: Browser) {
    setFailed(null);
    if (!browser) return openExternal(url);
    invoke("open_in_browser", { id: browser.id, url }).catch(() => setFailed(`${browser.name} didn't open. Try another way below.`));
  }

  function copy() {
    void navigator.clipboard.writeText(url).then(
      () => setCopied(true),
      () => setFailed("The link couldn't be copied."),
    );
  }

  const button = "flex h-9 items-center justify-center gap-1.5 rounded-full px-4 text-[13.5px] font-medium";
  return (
    <div ref={panel} tabIndex={-1} role="group" aria-labelledby={titleId} className="flex flex-col gap-3 px-3.5 pt-1 pb-3.5 outline-none">
      <div className="flex gap-2.5">
        <span className="mt-0.5 shrink-0 text-(--accent)">
          <InfoIcon size={20} />
        </span>
        <div className="min-w-0">
          <p id={titleId} className="text-[14px] font-medium">
            {why === "music" ? `${name} won't play this video in other apps` : "This video can't play here"}
          </p>
          <p className="mt-0.5 text-[12.5px] leading-4.5 text-(--text-secondary)">
            {why === "music"
              ? `${name} keeps some videos to its own site, often because of their music. Watch it in a browser instead.`
              : `${name} only plays it on its own site. Watch it in a browser instead.`}
          </p>
        </div>
      </div>
      <div className="flex flex-col gap-2">
        <button
          type="button"
          onClick={() => open(main)}
          disabled={browsers === null}
          className={`${button} bg-(--accent) text-(--on-accent) hover:brightness-110 disabled:opacity-50`}
        >
          {main ? `Open in ${main.name}` : "Open in your browser"}
        </button>
        {others.length ? (
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-[12.5px] text-(--text-secondary)">Or in</span>
            {others.map((browser) => (
              <button
                key={browser.id}
                type="button"
                onClick={() => open(browser)}
                className="h-8 rounded-full border border-(--divider) px-3 text-[12.5px] hover:bg-(--hover-bg)"
              >
                {browser.name}
              </button>
            ))}
          </div>
        ) : null}
        <button type="button" onClick={copy} className={`${button} border border-(--divider) hover:bg-(--hover-bg)`}>
          <CopyIcon size={16} />
          {copied ? "Link copied" : "Copy link"}
        </button>
      </div>
      <p role="status" className="text-[12px] text-(--danger) empty:hidden">
        {failed}
      </p>
    </div>
  );
}
