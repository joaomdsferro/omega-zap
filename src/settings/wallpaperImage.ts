/*
 * The picture chosen for behind the chats. It lives in IndexedDB rather than
 * with the other settings: localStorage holds a few MB at most, and the
 * settings are written out whole on every change.
 */

const DB = "omega-zap-appearance";
const STORE = "files";
const KEY = "wallpaper";
/** Longest side kept: enough for a large screen, small enough to paint fast. */
const MAX_SIDE = 2560;

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function run<T>(mode: IDBTransactionMode, action: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await open();
  try {
    return await new Promise<T>((resolve, reject) => {
      const request = action(db.transaction(STORE, mode).objectStore(STORE));
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  } finally {
    db.close();
  }
}

export async function loadWallpaperImage(): Promise<Blob | null> {
  try {
    return ((await run("readonly", (store) => store.get(KEY))) as Blob | undefined) ?? null;
  } catch {
    return null;
  }
}

export async function saveWallpaperImage(image: Blob | null): Promise<void> {
  if (image) await run("readwrite", (store) => store.put(image, KEY));
  else await run("readwrite", (store) => store.delete(KEY));
}

/** The file scaled down to MAX_SIDE, as a JPEG. Throws when it isn't a picture. */
export async function prepareWallpaperImage(file: File): Promise<Blob> {
  const bitmap = await createImageBitmap(file);
  const scale = Math.min(1, MAX_SIDE / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(bitmap.width * scale);
  canvas.height = Math.round(bitmap.height * scale);
  canvas.getContext("2d")!.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  return new Promise((resolve, reject) =>
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("not encoded"))), "image/jpeg", 0.88),
  );
}
