import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
// @ts-expect-error type error without @types/node package
import { cp, readFile } from "node:fs/promises";
// @ts-expect-error type error without @types/node package
import { request } from "node:http";
// @ts-expect-error type error without @types/node package
import { tmpdir } from "node:os";
// @ts-expect-error type error without @types/node package
import { join } from "node:path";

const host = process.env.TAURI_DEV_HOST;

/**
 * Dev-only stand-in for the `avatar://` and `media://` schemes, which
 * WebKitGTK 2.54 refuses to load into the http dev page
 * (tauri-apps/tauri#16201). The files are encrypted on disk, so they are
 * fetched from the app itself: a debug build serves them decrypted on
 * loopback, at the port and with the token it writes to its runtime folder.
 */
function devFiles(): Plugin {
  const published = join(process.env.XDG_RUNTIME_DIR || tmpdir(), "omega-zap", "dev-files.json");
  return {
    name: "omega-zap-dev-files",
    apply: "serve",
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        const incoming = req as { url?: string; headers: Record<string, string | undefined>; method?: string };
        const match = /^\/__(avatar|media)\/([^?]+)/.exec(incoming.url ?? "");
        if (!match) return next();
        const name = decodeURIComponent(match[2]);
        let app: { port: number; token: string };
        try {
          app = JSON.parse(await readFile(published, "utf8"));
        } catch {
          res.statusCode = 503;
          return res.end();
        }
        if (!/^[A-Za-z0-9_.-]+$/.test(name) || name.startsWith(".")) {
          res.statusCode = 404;
          return res.end();
        }
        const headers: Record<string, string> = { "x-dev-token": app.token };
        if (incoming.headers.range) headers.range = incoming.headers.range;
        const upstream = request(
          { host: "127.0.0.1", port: app.port, path: `/${match[1]}/${name}`, method: "GET", headers },
          (answer: { statusCode?: number; headers: Record<string, string>; pipe: (to: unknown) => void }) => {
            res.writeHead(answer.statusCode ?? 502, answer.headers);
            answer.pipe(res);
          },
        );
        upstream.on("error", () => {
          res.statusCode = 502;
          res.end();
        });
        upstream.end();
      });
    },
  };
}

const EMOJI_DIR = "node_modules/emoji-datasource-apple/img/apple/64";

/**
 * Apple's emoji images, as WhatsApp Web and Signal Desktop draw them, served
 * at /emoji/. Each is fetched only when an emoji on screen needs it.
 */
function emojiImages(): Plugin {
  let outDir = "dist";
  return {
    name: "omega-zap-emoji-images",
    configResolved(config) {
      outDir = config.build.outDir;
    },
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        const match = /^\/emoji\/([0-9a-f-]+\.png)$/.exec((req as { url?: string }).url ?? "");
        if (!match) return next();
        try {
          const image = await readFile(join(EMOJI_DIR, match[1]));
          res.setHeader("Content-Type", "image/png");
          res.setHeader("Cache-Control", "max-age=31536000, immutable");
          res.end(image);
        } catch {
          res.statusCode = 404;
          res.end();
        }
      });
    },
    async writeBundle() {
      await cp(EMOJI_DIR, join(outDir, "emoji"), { recursive: true });
    },
  };
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react(), tailwindcss(), devFiles(), emojiImages()],

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
