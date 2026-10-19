import { invoke } from "@tauri-apps/api/core";
import type { LinkPreview } from "../backend";
import { fileUrl } from "./files";
import { isTauri } from "./platform";

// Trailing punctuation belongs to the sentence, not the address.
export const URL_PATTERN = /https?:\/\/[^\s<>]*[^\s<>.,;:!?)\]}'"]/g;

// An address written without its scheme, like "docu.pt" or "google.com/maps",
// as WhatsApp and Signal show it: a domain ending in a real top-level one,
// not part of an email address or a longer word.
const BARE_PATTERN =
  /(?<![\w@./-])(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+([a-z]{2,})(?![\w-])(?:[/?#:][^\s<>]*[^\s<>.,;:!?)\]}'"])?/giu;

const TOP_LEVEL = new Set(
  ("com net org info biz io co app dev ai me tv xyz online site store shop tech blog news page link live cloud " +
    "gov edu mil int eu asia pro club design art email agency studio world space website fun top digital media " +
    "network group social today life one host wiki zone uk").split(" "),
);
/** Country domains that are mostly file names in a message: "readme.md", "main.rs". */
const FILE_LIKE = new Set(["md", "sh", "py", "rs", "pl", "ps"]);
const regions = new Intl.DisplayNames(["en"], { type: "region", fallback: "none" });

function realTopLevel(tld: string): boolean {
  const lower = tld.toLowerCase();
  if (TOP_LEVEL.has(lower)) return true;
  if (lower.length !== 2 || FILE_LIKE.has(lower)) return false;
  try {
    return regions.of(lower.toUpperCase()) !== undefined;
  } catch {
    return false;
  }
}

export interface FoundLink {
  index: number;
  /** As written. */
  text: string;
  /** What it opens: the text, with `https://` when it was written without. */
  href: string;
}

/** The web addresses in a text, in order, with or without their scheme. */
export function findLinks(text: string): FoundLink[] {
  const full = [...text.matchAll(URL_PATTERN)].map((m) => ({ index: m.index, text: m[0], href: m[0] }));
  const inFull = (at: number) => full.some((l) => at >= l.index && at < l.index + l.text.length);
  const bare = [...text.matchAll(BARE_PATTERN)]
    .filter((m) => !inFull(m.index) && realTopLevel(m[1]))
    .map((m) => ({ index: m.index, text: m[0], href: `https://${m[0]}` }));
  return [...full, ...bare].sort((a, b) => a.index - b.index);
}

export type VideoSite = "tiktok" | "instagram" | "youtube" | "x";

export interface VideoLink {
  site: VideoSite;
  /** The site's player for this video. Missing for a short share link, until it is followed. */
  embed?: string;
  /** A post on X, whose video is played from X's own file rather than a player page. */
  post?: string;
}

/** Hosts that show posts on X, including the ones that fix their previews. */
const X_HOSTS = new Set(["x.com", "twitter.com", "mobile.twitter.com", "fxtwitter.com", "vxtwitter.com", "fixupx.com"]);

/** The short video a link points at, when it is one we can play in the chat. */
export function videoLink(raw: string): VideoLink | null {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return null;
  }
  const host = url.hostname.replace(/^(www|m)\./, "");
  const path = url.pathname;

  if (host === "youtube.com") {
    const id = path.match(/^\/shorts\/([\w-]+)/)?.[1];
    return id ? { site: "youtube", embed: `https://www.youtube.com/embed/${id}?autoplay=1` } : null;
  }
  if (host === "instagram.com") {
    const match = path.match(/^\/(?:[\w.]+\/)?(reels?|p|tv)\/([\w-]+)/);
    if (!match) return null;
    const kind = match[1] === "p" ? "p" : "reel";
    return { site: "instagram", embed: `https://www.instagram.com/${kind}/${match[2]}/embed/` };
  }
  if (X_HOSTS.has(host)) {
    const id = path.match(/^\/\w+\/status(?:es)?\/(\d+)/)?.[1];
    return id ? { site: "x", post: id } : null;
  }
  if (host === "tiktok.com") {
    const id = path.match(/\/video\/(\d+)/)?.[1];
    if (id) return { site: "tiktok", embed: `https://www.tiktok.com/player/v1/${id}?autoplay=1` };
    return path.startsWith("/t/") ? { site: "tiktok" } : null;
  }
  if (host === "vm.tiktok.com" || host === "vt.tiktok.com") {
    return path.length > 1 ? { site: "tiktok" } : null;
  }
  return null;
}

/** Whether a link is a video for sure. A post on X may not be: its card only plays when it has one. */
export function isPlayable(url: string): boolean {
  const link = videoLink(url);
  return link !== null && link.site !== "x";
}

/** The player address for a video link, following a short share link first when it has to. */
export async function embedUrl(raw: string): Promise<string> {
  const link = videoLink(raw);
  if (link?.embed) return link.embed;
  if (!link || !isTauri) throw new Error("This link cannot be played here.");
  const followed = await invoke<string>("resolve_short_link", { url: raw.replace(/^http:/, "https:") });
  const embed = videoLink(followed)?.embed;
  if (!embed) throw new Error("This link does not lead to a video.");
  return embed;
}

/** A post's video on X: an MP4 the chat plays itself. */
export interface XVideo {
  url: string;
  poster: string | null;
  width: number;
  height: number;
}

const xVideos = new Map<string, Promise<XVideo | null>>();

/** The video in a post on X, or `null` when it has none or X can't be reached. */
export function xVideo(post: string): Promise<XVideo | null> {
  if (!isTauri) return Promise.resolve(null);
  let found = xVideos.get(post);
  if (!found) {
    found = invoke<XVideo | null>("x_video", { id: post }).catch(() => {
      // Asked again next time, in case X was only out of reach.
      xVideos.delete(post);
      return null;
    });
    xVideos.set(post, found);
  }
  return found;
}

interface FetchedPreview {
  url: string;
  title: string | null;
  description: string | null;
  /** File name of the kept picture. */
  image: string | null;
}

/** Cards asked for so far, by link: each is read from its site once. */
const previews = new Map<string, Promise<LinkPreview | null>>();
/** The ones that have come, so a card redrawn after a chat switch is there at once. */
const ready = new Map<string, LinkPreview | null>();

/**
 * The card for a link that came without one, from the page's own tags. The
 * Rust side keeps it, so asking again, even after a restart, stays local.
 */
export function fetchLinkPreview(url: string): Promise<LinkPreview | null> {
  if (!isTauri) return Promise.resolve(null);
  let preview = previews.get(url);
  if (!preview) {
    preview = invoke<FetchedPreview | null>("link_preview", { url })
      .then((found) =>
        found
          ? {
              url,
              title: found.title ?? undefined,
              description: found.description ?? undefined,
              thumbnail: found.image ? fileUrl(found.image, "media") : undefined,
            }
          : null,
      )
      .then((found) => {
        ready.set(url, found);
        return found;
      })
      // A site that could not be reached is tried again next time the app starts.
      .catch(() => null);
    previews.set(url, preview);
  }
  return preview;
}

/** The card already read for a link: `undefined` while it hasn't been, `null` when there is none. */
export function readyLinkPreview(url: string): LinkPreview | null | undefined {
  return ready.get(url);
}
