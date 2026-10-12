import { invoke } from "@tauri-apps/api/core";
import type { GifResult, StickerResult } from "../backend";

let available: Promise<boolean> | undefined;

/**
 * Whether this build can search GIFs and stickers (it has a KLIPY key). A
 * "no" is asked again next time: a development build reads the key from
 * `.env` as it goes.
 */
export function klipyAvailable(): Promise<boolean> {
  available ??= invoke<boolean>("gif_search_available").catch(() => false);
  const answer = available;
  void answer.then((yes) => {
    if (!yes && available === answer) available = undefined;
  });
  return answer;
}

/** A page of GIFs from KLIPY: trending when the query is empty. */
export function searchGifs(query: string, page = 1): Promise<{ items: GifResult[]; next: number | null }> {
  return invoke("gif_search", { query, page });
}

/** A page of stickers from KLIPY: trending when the query is empty. */
export function searchStickers(query: string, page = 1): Promise<{ items: StickerResult[]; next: number | null }> {
  return invoke("sticker_search", { query, page });
}
