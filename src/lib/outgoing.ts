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

function thumbnail(source: CanvasImageSource, width: number, height: number): string | undefined {
  if (!width || !height) return undefined;
  const scale = Math.min(1, THUMBNAIL_SIDE / Math.max(width, height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(width * scale));
  canvas.height = Math.max(1, Math.round(height * scale));
  const context = canvas.getContext("2d");
  if (!context) return undefined;
  context.drawImage(source, 0, 0, canvas.width, canvas.height);
  const url = canvas.toDataURL("image/jpeg", 0.7);
  return url.slice(url.indexOf(",") + 1) || undefined;
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
    const done = (details: FileDetails) => {
      URL.revokeObjectURL(url);
      player.removeAttribute("src");
      resolve(details);
    };
    player.preload = "auto";
    player.muted = true;
    player.onerror = () => {
      URL.revokeObjectURL(url);
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
