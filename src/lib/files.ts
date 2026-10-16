import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { isTauri } from "./platform";

/**
 * URL of a saved profile picture or downloaded file. In dev the page is served
 * over http, and WebKitGTK 2.54 refuses custom-scheme images on an http page
 * (tauri-apps/tauri#16201), so the dev server hands them out instead.
 */
export function fileUrl(name: string, scheme: "avatar" | "media"): string {
  return import.meta.env.DEV ? `/__${scheme}/${encodeURIComponent(name)}` : convertFileSrc(name, scheme);
}

let streamBase: Promise<string | null> | null = null;

/**
 * Where a video or audio player streams a downloaded file from, given its
 * `fileUrl`, or null where the app does not serve files this way. On Linux it
 * serves them over loopback http (see `loopback.rs`): WebKitGTK's players take
 * no custom scheme, and play a `blob:` URL from corrupted data.
 */
export async function streamUrl(url: string): Promise<string | null> {
  if (!isTauri) return null;
  streamBase ??= invoke<string | null>("media_server").catch(() => null);
  const base = await streamBase;
  return base ? base + url.slice(url.lastIndexOf("/") + 1) : null;
}
