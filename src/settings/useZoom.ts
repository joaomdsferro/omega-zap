import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { isTauri } from "../lib/platform";
import { useSettings } from "./settings";

async function applyZoom(zoom: number) {
  if (isTauri) {
    await getCurrentWebview().setZoom(zoom);
  } else {
    // Browser preview only; the desktop app scales the whole webview.
    document.documentElement.style.setProperty("zoom", String(zoom));
  }
}

/**
 * Applies the zoom setting and wires Ctrl/Cmd + =/-/0 and Ctrl/Cmd + wheel.
 * Returns the zoom level to announce, or null when nothing changed recently.
 */
export function useZoom(): number | null {
  const { settings, zoomIn, zoomOut, resetZoom } = useSettings();
  const [announce, setAnnounce] = useState<number | null>(null);
  const applied = useRef(settings.zoom);

  useEffect(() => {
    applyZoom(settings.zoom).catch((e) => console.warn("zoom failed", e));
    if (applied.current === settings.zoom) return;
    applied.current = settings.zoom;
    setAnnounce(settings.zoom);
    const id = window.setTimeout(() => setAnnounce(null), 1400);
    return () => window.clearTimeout(id);
  }, [settings.zoom]);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      if (e.key === "=" || e.key === "+") {
        e.preventDefault();
        zoomIn();
      } else if (e.key === "-" || e.key === "_") {
        e.preventDefault();
        zoomOut();
      } else if (e.key === "0") {
        e.preventDefault();
        resetZoom();
      }
    }

    let lastWheel = 0;
    function onWheel(e: WheelEvent) {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      const now = performance.now();
      if (now - lastWheel < 90 || e.deltaY === 0) return;
      lastWheel = now;
      if (e.deltaY < 0) zoomIn();
      else zoomOut();
    }

    window.addEventListener("keydown", onKey);
    window.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("wheel", onWheel);
    };
  }, [zoomIn, zoomOut, resetZoom]);

  return announce;
}
