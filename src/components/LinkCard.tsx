import { useEffect, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { LinkPreview } from "../backend";
import { embedUrl, videoLink } from "../lib/links";
import { isTauri } from "../lib/platform";
import { useSettings } from "../settings/settings";
import { CloseIcon, PlayIcon } from "./Icons";

function openExternal(url: string) {
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

interface LinkCardProps {
  preview: LinkPreview;
  /** Fetches the full-size picture, when the account can. */
  loadLarge?: () => Promise<string>;
}

export function LinkCard({ preview, loadLarge }: LinkCardProps) {
  const { settings } = useSettings();
  const [wide, setWide] = useState(false);
  const [broken, setBroken] = useState(false);
  const [largeUrl, setLargeUrl] = useState<string | null>(null);
  const [player, setPlayer] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const video = settings.inlineVideos ? videoLink(preview.url) : null;
  const wantLarge = Boolean(settings.largeLinkPreviews && preview.hasLarge && loadLarge);

  useEffect(() => {
    setLargeUrl(null);
    if (!wantLarge || !loadLarge) return;
    let alive = true;
    loadLarge()
      .then((loaded) => alive && setLargeUrl(loaded))
      // The small picture stays.
      .catch(() => {});
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
  const stacked = Boolean(player) || (wide && Boolean(picture));

  async function play() {
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
      {player ? (
        <div className="flex justify-end">
          <button
            type="button"
            onClick={() => setPlayer(null)}
            className="flex h-8 items-center gap-1.5 px-2.5 text-[12.5px] text-(--text-secondary) hover:text-(--text-primary)"
          >
            Close video
            <CloseIcon size={16} />
          </button>
        </div>
      ) : null}
      {player ? (
        <iframe
          src={player}
          title={preview.title ?? "Video"}
          allow="autoplay; encrypted-media; fullscreen; picture-in-picture"
          allowFullScreen
          referrerPolicy="strict-origin-when-cross-origin"
          sandbox="allow-scripts allow-same-origin allow-popups allow-presentation"
          // Instagram wraps the video in its own header and footer.
          className={["w-full border-0 bg-black", video?.site === "instagram" ? "h-150" : "aspect-9/16"].join(" ")}
        />
      ) : picture || video ? (
        <div
          onClick={video ? undefined : () => openExternal(preview.url)}
          className={[
            "relative shrink-0",
            stacked ? "w-full" : "h-22 w-22",
            video ? "" : "cursor-pointer",
          ].join(" ")}
        >
          {picture ? (
            <img
              src={picture}
              alt=""
              draggable={false}
              onLoad={(event) => setWide(event.currentTarget.naturalWidth >= LARGE_PREVIEW_PX)}
              onError={() => (largeUrl ? setLargeUrl(null) : setBroken(true))}
              className={stacked ? "block max-h-82.5 w-full object-cover" : "h-full w-full object-cover"}
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
