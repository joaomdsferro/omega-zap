import { isTauri as tauriDetected } from "@tauri-apps/api/core";

/** True inside the desktop app, false when the UI runs in a plain browser (`pnpm dev`). */
export const isTauri = tauriDetected();

export const isMac = /Mac|iPhone|iPad/.test(navigator.platform);
export const modKey = isMac ? "⌘" : "Ctrl";

/** The desktop app on Linux, where the page is drawn by WebKitGTK. */
export const isWebKitGtk = isTauri && /Linux/.test(navigator.userAgent) && !/Android/.test(navigator.userAgent);
