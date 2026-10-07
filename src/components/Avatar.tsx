import { useState } from "react";
import { PersonSilhouette } from "./Icons";

interface AvatarProps {
  name: string;
  initials: string;
  hue: number;
  src?: string;
  size: number;
  group?: boolean;
  online?: boolean;
}

export function Avatar({ name, initials, hue, src, size, group, online }: AvatarProps) {
  // The placeholder stays underneath, so a picture that fails to load just disappears.
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
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
          draggable={false}
          onError={() => setFailedSrc(src)}
          className="absolute inset-0 h-full w-full rounded-full object-cover"
        />
      ) : null}
      {online ? (
        <span
          className="absolute right-0 bottom-0 rounded-full border-2 border-(--panel-bg) bg-(--accent)"
          style={{ width: size * 0.26, height: size * 0.26 }}
        />
      ) : null}
    </div>
  );
}
