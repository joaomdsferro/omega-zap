import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent, PointerEvent } from "react";
import { formatVoiceDuration } from "../lib/format";
import { ExpandIcon, MuteIcon, PauseIcon, PlayIcon, ShrinkIcon, VolumeIcon } from "./Icons";

/** Arrow keys on a video's timeline move this many seconds. */
const VIDEO_KEY_STEP = 5;
/** How long after a seek ends its new frame is waited for at most. */
const HOLD_AFTER_SEEK_MS = 1000;

/**
 * A video with controls of its own. WebKitGTK's scatter to the corners of a
 * small video, crowd the time onto the timeline of a large one, and cannot be
 * styled. A seek also shows black in WebKitGTK until the new frame is
 * decoded, so the frame from before it stays on screen until then.
 */
export function VideoPlayer({ src, poster, onError, onOpen, className = "", videoClassName = "", videoStyle }: {
  src: string;
  poster?: string;
  onError: () => void;
  /** Opens the video in the viewer; shown as a button next to the others. */
  onOpen?: () => void;
  className?: string;
  videoClassName?: string;
  videoStyle?: CSSProperties;
}) {
  const box = useRef<HTMLDivElement>(null);
  const video = useRef<HTMLVideoElement>(null);
  const still = useRef<HTMLCanvasElement>(null);
  const holding = useRef<number | null>(null);
  const track = useRef<HTMLDivElement>(null);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [seconds, setSeconds] = useState(0);
  const [muted, setMuted] = useState(false);
  const [full, setFull] = useState(false);

  function paint(at: number) {
    const share = seconds ? Math.min(1, at / seconds) : 0;
    track.current?.style.setProperty("--played", `${share * 100}%`);
  }

  useLayoutEffect(() => {
    if (!playing) paint(time);
  });

  // While playing, the timeline fills every frame; the clock ticks with `timeupdate`.
  useEffect(() => {
    if (!playing) return;
    let frame = requestAnimationFrame(function step() {
      if (video.current) paint(video.current.currentTime);
      frame = requestAnimationFrame(step);
    });
    return () => cancelAnimationFrame(frame);
  });

  useEffect(() => {
    const onChange = () => setFull(document.fullscreenElement === box.current);
    document.addEventListener("fullscreenchange", onChange);
    return () => document.removeEventListener("fullscreenchange", onChange);
  }, []);

  useEffect(() => () => {
    if (holding.current !== null) clearTimeout(holding.current);
  }, []);

  // Covers the video with its current frame, before a seek blanks it. A
  // seek in a phone video, with keyframes seconds apart, can take longer
  // than a second, so the cover stays for as long as the seek lasts.
  function hold() {
    const el = video.current;
    const canvas = still.current;
    if (holding.current !== null) clearTimeout(holding.current);
    // Still covering an earlier seek, whose frame has not come yet.
    else if (el && canvas && el.readyState >= 2 && el.videoWidth) {
      canvas.width = el.videoWidth;
      canvas.height = el.videoHeight;
      canvas.getContext("2d")?.drawImage(el, 0, 0);
      canvas.style.visibility = "visible";
    } else return;
    holding.current = 0;
  }

  function release() {
    if (holding.current !== null) clearTimeout(holding.current);
    holding.current = null;
    if (still.current) still.current.style.visibility = "hidden";
  }

  // Uncovers the video once the frame it seeked to is on screen, or after a
  // while if no frame is reported.
  function onSeeked() {
    const el = video.current;
    if (!el || holding.current === null) return;
    const done = () => !el.seeking && release();
    if ("requestVideoFrameCallback" in el) el.requestVideoFrameCallback(done);
    else requestAnimationFrame(() => requestAnimationFrame(done));
    clearTimeout(holding.current);
    holding.current = window.setTimeout(done, HOLD_AFTER_SEEK_MS);
  }

  function toggle() {
    const el = video.current;
    if (!el) return;
    if (el.paused) void el.play().catch(() => {});
    else el.pause();
  }

  function toggleFull() {
    if (document.fullscreenElement) void document.exitFullscreen().catch(() => {});
    else void box.current?.requestFullscreen().catch(() => {});
  }

  function seekTo(at: number) {
    const el = video.current;
    if (!el || !seconds) return;
    hold();
    el.currentTime = Math.min(seconds, Math.max(0, at));
    setTime(el.currentTime);
    paint(el.currentTime);
  }

  function seekFromPointer(event: PointerEvent<HTMLDivElement>) {
    const area = event.currentTarget.getBoundingClientRect();
    seekTo(((event.clientX - area.left) / area.width) * seconds);
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const at = video.current?.currentTime ?? time;
    const to =
      event.key === "ArrowRight" ? at + VIDEO_KEY_STEP
      : event.key === "ArrowLeft" ? at - VIDEO_KEY_STEP
      : event.key === "Home" ? 0
      : event.key === "End" ? seconds
      : null;
    if (to === null) return;
    event.preventDefault();
    seekTo(to);
  }

  const controlClass = "flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-white hover:bg-white/15";
  const fill = full ? "h-full w-full object-contain" : videoClassName;

  return (
    <div ref={box} className={`group relative bg-black ${full ? "flex items-center justify-center" : className}`}>
      <video
        ref={video}
        src={src}
        poster={poster}
        onError={() => {
          release();
          onError();
        }}
        autoPlay
        playsInline
        onClick={toggle}
        onDoubleClick={toggleFull}
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onEnded={() => setPlaying(false)}
        onSeeking={hold}
        onSeeked={onSeeked}
        onTimeUpdate={(event) => setTime(event.currentTarget.currentTime)}
        onDurationChange={(event) => {
          const length = event.currentTarget.duration;
          setSeconds(Number.isFinite(length) ? length : 0);
        }}
        onVolumeChange={(event) => setMuted(event.currentTarget.muted)}
        className={`block cursor-pointer ${fill}`}
        style={full ? undefined : videoStyle}
      />
      <canvas
        ref={still}
        aria-hidden
        className="pointer-events-none invisible absolute inset-0 h-full w-full object-contain"
      />
      <div
        className={[
          "absolute inset-x-0 bottom-0 bg-linear-to-t from-black/70 to-transparent px-2.5 pt-8 pb-1.5 transition-opacity",
          playing ? "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100" : "",
        ].join(" ")}
      >
        <div
          ref={track}
          role="slider"
          tabIndex={0}
          aria-label="Position"
          aria-valuemin={0}
          aria-valuemax={Math.round(seconds)}
          aria-valuenow={Math.round(time)}
          aria-valuetext={`${formatVoiceDuration(time)} of ${formatVoiceDuration(seconds)}`}
          onPointerDown={(event) => {
            event.currentTarget.setPointerCapture(event.pointerId);
            seekFromPointer(event);
          }}
          onPointerMove={(event) => {
            if (event.currentTarget.hasPointerCapture(event.pointerId)) seekFromPointer(event);
          }}
          onKeyDown={onKeyDown}
          className="relative flex h-4 cursor-pointer touch-none items-center rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-(--focus-ring)"
        >
          <span className="h-1 w-full rounded-full bg-white/35" aria-hidden />
          <span className="absolute left-0 h-1 rounded-full bg-white" style={{ width: "var(--played, 0%)" }} aria-hidden />
          <span
            className="absolute h-3 w-3 -translate-x-1/2 rounded-full bg-white shadow-sm"
            style={{ left: "var(--played, 0%)" }}
            aria-hidden
          />
        </div>
        <div className="mt-1 flex items-center gap-1">
          <button type="button" onClick={toggle} aria-label={playing ? "Pause" : "Play"} className={controlClass}>
            {playing ? <PauseIcon size={18} /> : <PlayIcon size={18} />}
          </button>
          <span className="flex-1 pl-1 text-[12px] tabular-nums text-white/90">
            {formatVoiceDuration(time)} / {formatVoiceDuration(seconds)}
          </span>
          <button
            type="button"
            onClick={() => video.current && (video.current.muted = !video.current.muted)}
            aria-label={muted ? "Unmute" : "Mute"}
            className={controlClass}
          >
            {muted ? <MuteIcon size={18} /> : <VolumeIcon size={18} />}
          </button>
          {onOpen && !full ? (
            <button
              type="button"
              onClick={() => {
                video.current?.pause();
                onOpen();
              }}
              aria-label="Open in viewer"
              className={controlClass}
            >
              <ExpandIcon size={16} />
            </button>
          ) : (
            <button type="button" onClick={toggleFull} aria-label={full ? "Exit full screen" : "Full screen"} className={controlClass}>
              {full ? <ShrinkIcon size={16} /> : <ExpandIcon size={16} />}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
