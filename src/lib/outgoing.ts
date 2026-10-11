import type { LinkPreview, OutgoingLink } from "../backend";

/** The largest file that can be sent, matching the Rust side. */
export const MAX_SEND_BYTES = 100 * 1024 * 1024;

/** What the network is told about a file besides its bytes. */
export interface FileDetails {
  width?: number;
  height?: number;
  /** Videos and audio. */
  seconds?: number;
  /** A small JPEG, base64, for photos and videos. */
  thumbnail?: string;
}

/** The longest side of the preview WhatsApp shows before a photo is downloaded. */
const THUMBNAIL_SIDE = 100;
/** How long a file may take to report its size or length before it is sent without them. */
const PROBE_TIMEOUT_MS = 5000;

function withTimeout<T>(promise: Promise<T>): Promise<T | undefined> {
  return Promise.race([
    promise,
    new Promise<undefined>((resolve) => setTimeout(() => resolve(undefined), PROBE_TIMEOUT_MS)),
  ]);
}

/** A picture as a base64 JPEG no longer than `side` on its longest side. */
function jpeg(source: CanvasImageSource, width: number, height: number, side: number, quality: number) {
  if (!width || !height) return undefined;
  const scale = Math.min(1, side / Math.max(width, height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(width * scale));
  canvas.height = Math.max(1, Math.round(height * scale));
  const context = canvas.getContext("2d");
  if (!context) return undefined;
  context.drawImage(source, 0, 0, canvas.width, canvas.height);
  const url = canvas.toDataURL("image/jpeg", quality);
  const data = url.slice(url.indexOf(",") + 1);
  return data ? { data, width: canvas.width, height: canvas.height } : undefined;
}

function thumbnail(source: CanvasImageSource, width: number, height: number): string | undefined {
  return jpeg(source, width, height, THUMBNAIL_SIDE, 0.7)?.data;
}

/** The small picture a link card carries in the message, and the large one sent beside it. */
const LINK_THUMBNAIL_SIDE = 140;
const LINK_IMAGE_SIDE = 1200;
/** Narrower than this, a link's picture is only ever shown small. */
const LINK_LARGE_MIN_WIDTH = 300;

/** The card to send for a link, with its pictures made from the one shown in the composer. */
export async function outgoingLink(preview: LinkPreview): Promise<OutgoingLink> {
  const card: OutgoingLink = { url: preview.url, title: preview.title, description: preview.description };
  if (!preview.thumbnail) return card;
  try {
    const picture = new Image();
    // Kept readable by the canvas: the media protocol allows it.
    picture.crossOrigin = "anonymous";
    picture.src = preview.thumbnail;
    await picture.decode();
    const { naturalWidth: width, naturalHeight: height } = picture;
    const large = width >= LINK_LARGE_MIN_WIDTH ? jpeg(picture, width, height, LINK_IMAGE_SIDE, 0.85) : undefined;
    return {
      ...card,
      thumbnail: jpeg(picture, width, height, LINK_THUMBNAIL_SIDE, 0.7)?.data,
      image: large?.data,
      width: large?.width,
      height: large?.height,
    };
  } catch {
    return card;
  }
}

async function imageDetails(file: File): Promise<FileDetails> {
  const bitmap = await createImageBitmap(file);
  try {
    return { width: bitmap.width, height: bitmap.height, thumbnail: thumbnail(bitmap, bitmap.width, bitmap.height) };
  } finally {
    bitmap.close();
  }
}

function playerDetails(file: File, video: boolean): Promise<FileDetails> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const player = document.createElement(video ? "video" : "audio");
    // WebKitGTK only loads and decodes a player that is in the page.
    player.style.cssText = "position:fixed;left:-10000px;top:0;width:64px;height:64px;opacity:0;pointer-events:none";
    document.body.append(player);
    // Given up on by `withTimeout`: nothing is left behind.
    setTimeout(() => {
      if (!player.isConnected) return;
      URL.revokeObjectURL(url);
      player.removeAttribute("src");
      player.remove();
    }, PROBE_TIMEOUT_MS + 100);
    const done = (details: FileDetails) => {
      URL.revokeObjectURL(url);
      player.removeAttribute("src");
      player.remove();
      resolve(details);
    };
    player.preload = "auto";
    player.muted = true;
    player.onerror = () => {
      URL.revokeObjectURL(url);
      player.remove();
      reject(new Error("unreadable"));
    };
    player.onloadedmetadata = () => {
      const seconds = Number.isFinite(player.duration) ? Math.round(player.duration) : undefined;
      if (!(player instanceof HTMLVideoElement)) return done({ seconds });
      // The first frame, for the preview.
      player.onseeked = () =>
        done({
          seconds,
          width: player.videoWidth || undefined,
          height: player.videoHeight || undefined,
          thumbnail: thumbnail(player, player.videoWidth, player.videoHeight),
        });
      player.currentTime = Math.min(0.1, (player.duration || 0) / 2);
    };
    player.src = url;
  });
}

/** Reads what can be read about a file before it is sent; anything that can't is left out. */
export async function fileDetails(file: File): Promise<FileDetails> {
  try {
    const type = file.type.split("/")[0];
    const probing =
      type === "image" && file.type !== "image/gif" && file.type !== "image/svg+xml"
        ? imageDetails(file)
        : type === "video" || type === "audio"
          ? playerDetails(file, type === "video")
          : Promise.resolve({});
    return (await withTimeout(probing)) ?? {};
  } catch {
    return {};
  }
}

/** Base64 of a string's UTF-8 bytes: request headers carry ASCII only. */
export function base64Json(value: unknown): string {
  const bytes = new TextEncoder().encode(JSON.stringify(value));
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}
