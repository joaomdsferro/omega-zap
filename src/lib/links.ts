import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./platform";

// Trailing punctuation belongs to the sentence, not the address.
export const URL_PATTERN = /https?:\/\/[^\s<>]*[^\s<>.,;:!?)\]}'"]/g;

export type VideoSite = "tiktok" | "instagram" | "youtube";

export interface VideoLink {
  site: VideoSite;
  /** The site's player for this video. Missing for a short share link, until it is followed. */
  embed?: string;
}

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

export function isPlayable(url: string): boolean {
  return videoLink(url) !== null;
}

/** The player address for a video link, following a short share link first when it has to. */
export async function embedUrl(raw: string): Promise<string> {
  const link = videoLink(raw);
  if (link?.embed) {
    if (link.site === "instagram" && isTauri && !(await invoke<boolean>("instagram_embed_plays", { url: link.embed }))) {
      throw new Error("Instagram only plays this video on its own site.");
    }
    return link.embed;
  }
  if (!link || !isTauri) throw new Error("This link cannot be played here.");
  const followed = await invoke<string>("resolve_short_link", { url: raw.replace(/^http:/, "https:") });
  const embed = videoLink(followed)?.embed;
  if (!embed) throw new Error("This link does not lead to a video.");
  return embed;
}
