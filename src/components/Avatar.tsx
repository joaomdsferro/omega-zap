import { useState } from "react";
import { isLogo } from "../lib/logos";
import { NoteIcon, PersonSilhouette, TimerIcon } from "./Icons";

interface AvatarProps {
  name: string;
  initials: string;
  hue: number;
  src?: string;
  size: number;
  group?: boolean;
  online?: boolean;
  /** Signal's Note to Self, pictured as a notepad. */
  note?: boolean;
  /** Messages in the chat disappear: a timer badge, at the bottom right or, where another badge sits there, left. */
  timer?: "right" | "left";
}

export function Avatar({ name, initials, hue, src, size, group, online, note, timer }: AvatarProps) {
  // The placeholder stays underneath, so a picture that fails to load just disappears.
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  // Drawn alone: over the other layers, their edges showed round it.
  if (note) {
    return (
      <div className="relative shrink-0" style={{ width: size, height: size }} role="img" aria-label={name}>
        <div
          className="flex h-full w-full items-center justify-center rounded-full"
          style={{ background: "color-mix(in oklab, #2c6bed 22%, var(--panel-bg))", color: "color-mix(in oklab, #2c6bed 80%, var(--text-primary))" }}
        >
          <NoteIcon size={size * 0.5} />
        </div>
        {timer ? <AvatarTimer side={timer} size={size} /> : null}
      </div>
    );
  }
  return (
    <div
      className="relative shrink-0"
      style={{ width: size, height: size }}
      role="img"
      aria-label={name}
    >
      <div
        className="avatar-silhouette flex h-full w-full items-end justify-center overflow-hidden rounded-full"
        style={{
          background: "color-mix(in oklab, var(--text-secondary) 28%, var(--panel-bg))",
          color: "color-mix(in oklab, var(--panel-bg) 88%, transparent)",
        }}
      >
        <PersonSilhouette group={group} />
      </div>
      <div
        className="avatar-initials flex h-full w-full items-center justify-center rounded-full font-semibold text-white"
        style={{
          background: `linear-gradient(145deg, hsl(${hue} 48% 40%), hsl(${hue} 42% 26%))`,
          fontSize: size * 0.36,
        }}
      >
        {initials}
      </div>
      {src && src !== failedSrc ? (
        <img
          src={src}
          alt=""
          // Long lists ask only for the pictures in sight.
          loading="lazy"
          decoding="async"
          draggable={false}
          onError={() => setFailedSrc(src)}
          className={`absolute inset-0 h-full w-full rounded-full ${isLogo(src) ? "object-contain p-[18%]" : "object-cover"}`}
          style={isLogo(src) ? { background: "color-mix(in oklab, var(--text-secondary) 16%, var(--panel-bg))" } : undefined}
        />
      ) : null}
      {timer ? (
        <AvatarTimer side={timer} size={size} />
      ) : online ? (
        <span
          className="absolute right-0 bottom-0 rounded-full border-2 border-(--panel-bg) bg-(--accent)"
          style={{ width: size * 0.26, height: size * 0.26 }}
        />
      ) : null}
    </div>
  );
}

function AvatarTimer({ side, size }: { side: "right" | "left"; size: number }) {
  return (
    <span
      className={[
        "absolute -bottom-0.75 flex items-center justify-center rounded-full border-2 border-(--panel-bg) bg-(--accent) text-(--on-accent)",
        side === "left" ? "-left-0.75" : "-right-0.75",
      ].join(" ")}
      style={{ width: Math.round(size * 0.42), height: Math.round(size * 0.42) }}
    >
      <TimerIcon size={Math.round(size * 0.25)} strokeWidth={2.4} />
    </span>
  );
}

/**
 * Who wrote a group message, beside it as Signal shows it: their picture on
 * the last message in a row of theirs, the same room left empty on the rest.
 */
export function SenderAvatar({ member, onOpen }: { member: { id: string; name: string; initials: string; avatarHue: number; avatarUrl?: string } | null; onOpen?: (id: string) => void }) {
  if (!member) return <div aria-hidden className="w-7 shrink-0" />;
  const avatar = <Avatar name={member.name} initials={member.initials} hue={member.avatarHue} src={member.avatarUrl} size={28} />;
  return onOpen ? (
    <button type="button" onClick={() => onOpen(member.id)} aria-label={`Contact info for ${member.name}`} className="shrink-0 self-end rounded-full">
      {avatar}
    </button>
  ) : (
    <div className="shrink-0 self-end">{avatar}</div>
  );
}
