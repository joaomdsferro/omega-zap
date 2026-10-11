import { invoke } from "@tauri-apps/api/core";
import type { GifResult } from "../backend";

let available: Promise<boolean> | undefined;

/** Whether this build can search GIFs (it carries a KLIPY key). */
export function gifSearchAvailable(): Promise<boolean> {
  available ??= invoke<boolean>("gif_search_available").catch(() => false);
  return available;
}

/** A page of GIFs from KLIPY: trending when the query is empty. */
export function searchGifs(query: string, page = 1): Promise<{ items: GifResult[]; next: number | null }> {
  return invoke("gif_search", { query, page });
}
