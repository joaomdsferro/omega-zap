import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent } from "react";
import type { Attachment, Message } from "../backend";
import { formatFileSize, formatVoiceDuration } from "../lib/format";
import { useMediaUrl } from "./Attachments";
import type { LoadMedia } from "./Attachments";
import { PauseIcon, PlayIcon } from "./Icons";

/** Bars drawn across the waveform. */
const BARS = 40;
/** Bar heights in pixels: silence, and the loudest moment. */
const BAR_MIN = 3;
const BAR_MAX = 24;
/** Arrow keys move this many seconds. */
const KEY_STEP = 5;
const RATES = [1, 1.5, 2];
const RATE_KEY = "omega-zap:voice-rate";

/** One speed for every voice message, as WhatsApp keeps it. */
let rate = (() => {
  try {
    const saved = Number(localStorage.getItem(RATE_KEY));
    return RATES.includes(saved) ? saved : 1;
  } catch {
    return 1;
  }
})();

/** The one playing now: starting another pauses it. */
let current: HTMLAudioElement | null = null;

/** Waveforms worked out from the sound itself, for messages sent without one. */
const decoded = new Map<string, { levels: number[]; seconds: number }>();

async function decodeLevels(url: string) {
  const data = await (await fetch(url)).arrayBuffer();
  const context = new AudioContext();
  try {
    const buffer = await context.decodeAudioData(data);
    const samples = buffer.getChannelData(0);
    const size = Math.max(1, Math.floor(samples.length / 64));
    // Every 16th sample is plenty to find a bucket's loudest moment.
    const levels = Array.from({ length: 64 }, (_, bucket) => {
      let peak = 0;
      for (let i = bucket * size, end = Math.min(samples.length, i + size); i < end; i += 16) {
        peak = Math.max(peak, Math.abs(samples[i]));
      }
      return peak;
    });
    return { levels, seconds: buffer.duration };
  } finally {
    void context.close();
  }
}

/** Bar heights for `levels`, taken loudest per bar; flat dots when there are none. */
function barHeights(levels: number[] | undefined) {
  if (!levels?.length) return Array<number>(BARS).fill(BAR_MIN);
  const bars = Array.from({ length: BARS }, (_, bar) => {
    const from = Math.floor((bar * levels.length) / BARS);
    const to = Math.max(from + 1, Math.floor(((bar + 1) * levels.length) / BARS));
    return Math.max(...levels.slice(from, to));
  });
  const loudest = Math.max(...bars) || 1;
  return bars.map((level) => Math.round(BAR_MIN + (level / loudest) * (BAR_MAX - BAR_MIN)));
}

function Bars({ heights, className }: { heights: number[]; className: string }) {
  return (
    <div className="absolute inset-0 flex items-center justify-between" aria-hidden>
      {heights.map((height, i) => (
        <span key={i} className={`w-0.75 rounded-full ${className}`} style={{ height }} />
      ))}
    </div>
  );
}

/** A voice message's waveform, drawn still: for messages that can't be played. */
export function Waveform({ levels }: { levels?: number[] }) {
  const heights = useMemo(() => barHeights(levels), [levels]);
  return (
    <div className="relative h-7 min-w-0 flex-1">
      <Bars heights={heights} className="bg-(--bubble-meta) opacity-50" />
    </div>
  );
}

export function AudioPlayer({ message, index, attachment, loadMedia }: {
  message: Message;
  index: number;
  attachment: Attachment;
  loadMedia?: LoadMedia;
}) {
  const audio = useRef<HTMLAudioElement>(null);
  const track = useRef<HTMLDivElement>(null);
  const [wanted, setWanted] = useState(false);
  const url = useMediaUrl(message, index, loadMedia, wanted, true);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [mediaSeconds, setMediaSeconds] = useState(0);
  const [speed, setSpeed] = useState(rate);
  const cacheKey = `${message.id}:${index}`;
  const [found, setFound] = useState(() => decoded.get(cacheKey));
  /** Where a press on the waveform asked to start, before the file had arrived. */
  const pendingSeek = useRef<number | null>(null);
  const ready = Boolean(url && url !== "failed");
  const failed = url === "failed" || !loadMedia;
  const voice = attachment.kind === "voice";

  // WebKitGTK reports no length for some Opus files; the sender's count stands in.
  const seconds = mediaSeconds || found?.seconds || message.voiceSeconds || 0;
  const levels = message.waveform ?? found?.levels;
  const heights = useMemo(() => barHeights(levels), [levels]);
  const started = playing || time > 0;

  function paint(at: number) {
    const share = seconds ? Math.min(1, at / seconds) : 0;
    track.current?.style.setProperty("--played", `${share * 100}%`);
  }

  useLayoutEffect(() => {
    if (!playing) paint(time);
  });

  // While playing, the waveform fills every frame; the clock below it ticks with `timeupdate`.
  useEffect(() => {
    if (!playing) return;
    let frame = requestAnimationFrame(function step() {
      if (audio.current) paint(audio.current.currentTime);
      frame = requestAnimationFrame(step);
    });
    return () => cancelAnimationFrame(frame);
  });

  // The first press fetches the file; it plays once it has arrived.
  useEffect(() => {
    if (ready && wanted) void audio.current?.play().catch(() => setPlaying(false));
  }, [ready, wanted]);

  useEffect(() => {
    if (!ready || !url || message.waveform?.length || found) return;
    let alive = true;
    decodeLevels(url)
      .then((result) => {
        decoded.set(cacheKey, result);
        if (alive) setFound(result);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [ready, url, cacheKey, message.waveform, found]);

  useEffect(
    () => () => {
      if (current === audio.current) current = null;
    },
    [],
  );

  function toggle() {
    const el = audio.current;
    if (!ready) return setWanted(true);
    if (!el) return;
    if (el.paused) void el.play();
    else el.pause();
  }

  function seekTo(share: number) {
    const el = audio.current;
    if (!ready || !el) {
      pendingSeek.current = share;
      setWanted(true);
      return;
    }
    if (!seconds) return;
    el.currentTime = share * seconds;
    setTime(el.currentTime);
    paint(el.currentTime);
  }

  function seekFromPointer(event: PointerEvent<HTMLDivElement>) {
    const box = event.currentTarget.getBoundingClientRect();
    seekTo(Math.min(1, Math.max(0, (event.clientX - box.left) / box.width)));
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (!seconds) return;
    const at = audio.current?.currentTime ?? time;
    const to =
      event.key === "ArrowRight" ? at + KEY_STEP
      : event.key === "ArrowLeft" ? at - KEY_STEP
      : event.key === "Home" ? 0
      : event.key === "End" ? seconds
      : null;
    if (to === null) return;
    event.preventDefault();
    seekTo(Math.min(seconds, Math.max(0, to)) / seconds);
  }

  function cycleSpeed() {
    rate = RATES[(RATES.indexOf(speed) + 1) % RATES.length];
    try {
      localStorage.setItem(RATE_KEY, String(rate));
    } catch {
      // Kept for this run only.
    }
    setSpeed(rate);
    if (audio.current) audio.current.playbackRate = rate;
  }

  const loading = wanted && !url;
  const clock = failed
    ? "Unavailable"
    : seconds
      ? formatVoiceDuration(started ? time : seconds)
      : formatFileSize(attachment.size);

  return (
    <div className="mb-1 flex w-75 max-w-full items-center gap-3 pt-1 pr-1">
      <button
        type="button"
        onClick={toggle}
        disabled={failed}
        aria-label={playing ? "Pause" : voice ? "Play voice message" : "Play audio"}
        className="relative flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-(--accent) text-(--on-accent) disabled:opacity-50"
      >
        {loading ? (
          <span className="absolute inset-0 animate-spin rounded-full border-2 border-transparent border-t-(--on-accent)" aria-hidden />
        ) : null}
        {playing ? <PauseIcon size={20} /> : <PlayIcon size={20} className="translate-x-px" />}
      </button>
      <div className="min-w-0 flex-1">
        {!voice && attachment.fileName ? (
          <p className="truncate text-[13px] text-(--text-primary)">{attachment.fileName}</p>
        ) : null}
        <div
          ref={track}
          role="slider"
          tabIndex={failed ? -1 : 0}
          aria-label="Position"
          aria-valuemin={0}
          aria-valuemax={Math.round(seconds)}
          aria-valuenow={Math.round(time)}
          aria-valuetext={`${formatVoiceDuration(time)} of ${formatVoiceDuration(seconds)}`}
          aria-disabled={failed}
          onPointerDown={(event) => {
            if (failed) return;
            event.currentTarget.setPointerCapture(event.pointerId);
            seekFromPointer(event);
          }}
          onPointerMove={(event) => {
            if (event.currentTarget.hasPointerCapture(event.pointerId)) seekFromPointer(event);
          }}
          onKeyDown={onKeyDown}
          className="relative h-7 touch-none rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-(--focus-ring)"
        >
          <Bars heights={heights} className="bg-(--bubble-meta) opacity-45" />
          <div className="absolute inset-0 [clip-path:inset(0_calc(100%-var(--played,0%))_0_0)]">
            <Bars heights={heights} className="bg-(--accent)" />
          </div>
          <span
            className={`pointer-events-none absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-(--accent) shadow-sm transition-opacity ${started ? "opacity-100" : "opacity-0"}`}
            style={{ left: "var(--played, 0%)" }}
            aria-hidden
          />
        </div>
        <div className="mt-0.5 flex h-4 items-center gap-2 text-[11px] tabular-nums text-(--bubble-meta)">
          <span>{clock}</span>
        </div>
      </div>
      {/* Its room is kept while hidden, so the waveform doesn't shift when playing starts. */}
      <button
        type="button"
        onClick={cycleSpeed}
        tabIndex={started ? 0 : -1}
        aria-hidden={!started}
        aria-label={`Playback speed ${speed}×`}
        className={`shrink-0 self-center rounded-full bg-(--quote-bg) px-2 py-1 ${started ? "" : "invisible"}`}
      >
        <span className="block min-w-7 text-center text-[12px] font-semibold tabular-nums text-(--text-secondary)">{speed}×</span>
      </button>
      {ready ? (
        <audio
          ref={audio}
          src={url ?? undefined}
          preload="metadata"
          onPlay={(event) => {
            const el = event.currentTarget;
            if (current && current !== el) current.pause();
            current = el;
            el.playbackRate = rate;
            setSpeed(rate);
            setPlaying(true);
          }}
          onPause={() => setPlaying(false)}
          onEnded={() => {
            setPlaying(false);
            setTime(0);
          }}
          onTimeUpdate={(event) => setTime(event.currentTarget.currentTime)}
          onLoadedMetadata={(event) => {
            const el = event.currentTarget;
            const length = Number.isFinite(el.duration) ? el.duration : 0;
            setMediaSeconds(length);
            const share = pendingSeek.current;
            pendingSeek.current = null;
            const total = length || seconds;
            if (share !== null && total) el.currentTime = share * total;
          }}
          onDurationChange={(event) => setMediaSeconds(Number.isFinite(event.currentTarget.duration) ? event.currentTarget.duration : 0)}
        />
      ) : null}
    </div>
  );
}
