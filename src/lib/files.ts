import { convertFileSrc } from "@tauri-apps/api/core";

/**
 * URL of a saved profile picture or downloaded file. In dev the page is served
 * over http, and WebKitGTK 2.54 refuses custom-scheme images on an http page
 * (tauri-apps/tauri#16201), so the dev server hands them out instead.
 */
export function fileUrl(name: string, scheme: "avatar" | "media"): string {
  return import.meta.env.DEV ? `/__${scheme}/${encodeURIComponent(name)}` : convertFileSrc(name, scheme);
}
