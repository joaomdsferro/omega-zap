import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";

/** Types the networks treat differently, by extension: anything else goes as a plain file. */
const MIME_TYPES: Record<string, string> = {
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  png: "image/png",
  webp: "image/webp",
  gif: "image/gif",
  heic: "image/heic",
  svg: "image/svg+xml",
  mp4: "video/mp4",
  m4v: "video/mp4",
  "3gp": "video/3gpp",
  webm: "video/webm",
  mov: "video/quicktime",
  mkv: "video/x-matroska",
  mp3: "audio/mpeg",
  m4a: "audio/mp4",
  aac: "audio/aac",
  ogg: "audio/ogg",
  opus: "audio/ogg",
  wav: "audio/wav",
  flac: "audio/flac",
  amr: "audio/amr",
  pdf: "application/pdf",
  txt: "text/plain",
  zip: "application/zip",
};

function mimeFor(name: string) {
  const extension = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  return MIME_TYPES[extension] ?? "";
}

/** Reads a file the system says was dropped on the window. */
async function readDropped(path: string): Promise<File> {
  const bytes = await invoke<ArrayBuffer>("read_dropped_file", { path });
  const name = path.split(/[\\/]/).pop() || "file";
  return new File([bytes], name, { type: mimeFor(name) });
}

interface DropHandlers {
  /** Files are being dragged over the window, at this point in page pixels. */
  over: (x: number, y: number) => void;
  leave: () => void;
  /** Files were dropped at this point. `read` loads them: the ones that could be read, and why the rest couldn't. */
  drop: (x: number, y: number, read: () => Promise<{ files: File[]; errors: string[] }>) => void;
}

/**
 * Follows files dragged in from the desktop. The system hands them to the
 * app rather than to the page, so they never replace it. Returns a function
 * that stops listening.
 */
export function listenForFileDrops(handlers: DropHandlers): () => void {
  if (!isTauri()) return () => {};
  // The event counts physical pixels; the page lays out in CSS ones.
  const page = (position: { x: number; y: number }) => [position.x / devicePixelRatio, position.y / devicePixelRatio] as const;
  let stop: (() => void) | undefined;
  let stopped = false;
  void getCurrentWebview()
    .onDragDropEvent(({ payload }) => {
      switch (payload.type) {
        case "enter":
        case "over":
          handlers.over(...page(payload.position));
          break;
        case "leave":
          handlers.leave();
          break;
        case "drop": {
          const paths = payload.paths;
          handlers.drop(...page(payload.position), async () => {
            const read = await Promise.allSettled(paths.map(readDropped));
            const files = read.flatMap((r) => (r.status === "fulfilled" ? [r.value] : []));
            const errors = read.flatMap((r) => (r.status === "rejected" ? [String(r.reason)] : []));
            if (!paths.length) errors.push("Nothing that was dropped could be read as a file.");
            return { files, errors };
          });
          break;
        }
      }
    })
    .then((unlisten) => (stopped ? unlisten() : (stop = unlisten)));
  return () => {
    stopped = true;
    stop?.();
  };
}
