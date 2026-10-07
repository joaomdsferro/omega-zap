import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
// @ts-expect-error type error without @types/node package
import { readFile } from "node:fs/promises";
// @ts-expect-error type error without @types/node package
import { homedir } from "node:os";
// @ts-expect-error type error without @types/node package
import { join } from "node:path";

const host = process.env.TAURI_DEV_HOST;

/** Tauri's app data directory for `rocks.omegazap.desktop`. */
function appDataDir(): string {
  const id = "rocks.omegazap.desktop";
  switch (process.platform) {
    case "darwin":
      return join(homedir(), "Library", "Application Support", id);
    case "win32":
      return join(process.env.APPDATA ?? join(homedir(), "AppData", "Roaming"), id);
    default:
      return join(process.env.XDG_DATA_HOME || join(homedir(), ".local", "share"), id);
  }
}

const MIME: Record<string, string> = {
  png: "image/png",
  gif: "image/gif",
  webp: "image/webp",
  heic: "image/heic",
  mp4: "video/mp4",
  webm: "video/webm",
  mov: "video/quicktime",
  aac: "audio/aac",
  m4a: "audio/mp4",
  mp3: "audio/mpeg",
  ogg: "audio/ogg",
  opus: "audio/ogg",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
};

/**
 * Dev-only stand-in for the `avatar://` and `media://` schemes, which
 * WebKitGTK 2.54 refuses to load into the http dev page
 * (tauri-apps/tauri#16201). Same folders, same order, same name check as
 * `src-tauri/src/lib.rs`.
 */
function devFiles(): Plugin {
  const folders: Record<string, string[]> = {
    avatar: ["avatars", "signal/avatars"],
    media: ["signal/media", "media", "link-thumbs"],
  };
  return {
    name: "omega-zap-dev-files",
    apply: "serve",
    configureServer(server) {
      const dir = appDataDir();
      server.middlewares.use(async (req, res, next) => {
        const match = /^\/__(avatar|media)\/([^?]+)/.exec((req as { url?: string }).url ?? "");
        if (!match) return next();
        const name = decodeURIComponent(match[2]);
        if (!/^[A-Za-z0-9_.-]+$/.test(name) || name.startsWith(".")) {
          res.statusCode = 404;
          return res.end();
        }
        for (const folder of folders[match[1]]) {
          try {
            const body = await readFile(join(dir, folder, name));
            const ext = name.split(".").pop() ?? "";
            res.setHeader("Content-Type", match[1] === "avatar" ? "image/jpeg" : MIME[ext] ?? "application/octet-stream");
            res.setHeader("Cache-Control", "max-age=31536000, immutable");
            return res.end(body);
          } catch {
            // Not in this folder; try the next.
          }
        }
        res.statusCode = 404;
        res.end();
      });
    },
  };
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react(), tailwindcss(), devFiles()],

  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
