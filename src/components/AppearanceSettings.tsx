import { useRef, useState, type ReactNode } from "react";
import { modKey } from "../lib/platform";
import {
  ACCENTS,
  BUBBLE_SHAPES,
  DEFAULT_APPEARANCE,
  SIDEBAR_DEFAULT,
  TEXT_SIZES,
  THEMES,
  WALLPAPERS,
  WALLPAPER_DIM,
  ZOOM_STEPS,
  useSettings,
  useSidebarWidth,
  type Appearance,
  type BubbleShape,
  type ResolvedTheme,
  type Wallpaper,
} from "../settings/settings";
import { CheckIcon, PlusIcon, Ticks } from "./Icons";
import { Tail } from "./MessageBubble";

/** A heading and the choices under it. */
function Group({ title, id, children }: { title: string; id: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3 px-6 py-4" aria-labelledby={id}>
      <h3 id={id} className="text-[14px] font-medium text-(--accent-strong)">
        {title}
      </h3>
      {children}
    </section>
  );
}

/** One choice in a row of them: a picture over its name, ringed when chosen. */
function Option({ label, active, onClick, wide, children }: {
  label: string;
  active: boolean;
  onClick: () => void;
  /** Takes the whole row of its grid. */
  wide?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={active}
      onClick={onClick}
      className={["group/option flex min-w-0 flex-col items-center gap-1.5 rounded-xl p-1 text-center", wide ? "col-span-full" : ""].join(" ")}
    >
      <span
        className={[
          "relative block w-full rounded-[10px] ring-offset-2 ring-offset-(--panel-bg) transition-shadow",
          active ? "ring-2 ring-(--accent)" : "ring-1 ring-(--divider) group-hover/option:ring-2 group-hover/option:ring-(--text-secondary)/50",
        ].join(" ")}
      >
        {children}
        {active ? (
          <span className="absolute -top-1.5 -right-1.5 flex h-5 w-5 items-center justify-center rounded-full bg-(--accent) text-(--on-accent)">
            <CheckIcon size={13} strokeWidth={3} />
          </span>
        ) : null}
      </span>
      <span className={["text-[13px] leading-4", active ? "font-medium text-(--text-primary)" : "text-(--text-secondary)"].join(" ")}>
        {label}
      </span>
    </button>
  );
}

/** A small window in a theme's own colours: the list beside a chat. */
function ThemeThumb({ theme }: { theme: ResolvedTheme }) {
  return (
    <span data-theme={theme} className="flex h-full min-w-0 flex-1 bg-(--panel-bg)">
      <span className="flex w-[38%] flex-col gap-1.5 border-r border-(--divider) px-1.5 py-2">
        {[0, 1, 2].map((row) => (
          <span key={row} className="flex items-center gap-1">
            <span className="h-2.5 w-2.5 shrink-0 rounded-full bg-(--chip-bg)" />
            <span className="h-1.5 flex-1 rounded-full bg-(--search-bg)" />
          </span>
        ))}
      </span>
      <span data-wp="theme" className="chat-wallpaper flex flex-1 flex-col justify-end gap-1 p-1.5">
        <span className="h-2.5 w-3/5 rounded-[3px] bg-(--bubble-in)" />
        <span className="h-2.5 w-1/2 self-end rounded-[3px] bg-(--bubble-out)" />
        <span className="mt-0.5 flex items-center gap-1">
          <span className="h-2 flex-1 rounded-full bg-(--composer-input-bg)" />
          <span className="h-2 w-2 rounded-full bg-(--accent)" />
        </span>
      </span>
    </span>
  );
}

/** Two bubbles in a shape, drawn small: one in, one out. */
function BubbleThumb({ shape }: { shape: BubbleShape }) {
  // Rounded rectangles with the corner at the tail side set apart.
  const r = { classic: 4, rounded: 9, sharp: 1.5 }[shape];
  const corner = { classic: 0, rounded: 3, sharp: 1.5 }[shape];
  const box = (x: number, y: number, w: number, h: number, tl: number, tr: number) =>
    `M${x + tl},${y}H${x + w - tr}Q${x + w},${y} ${x + w},${y + tr}V${y + h - r}Q${x + w},${y + h} ${x + w - r},${y + h}` +
    `H${x + r}Q${x},${y + h} ${x},${y + h - r}V${y + tl}Q${x},${y} ${x + tl},${y}Z`;
  return (
    <svg viewBox="0 0 120 56" className="block h-14 w-full rounded-[10px] bg-(--chat-bg)" aria-hidden>
      <path d={box(14, 9, 64, 17, corner, r)} fill="var(--bubble-in)" />
      <path d={box(42, 31, 64, 17, r, corner)} fill="var(--bubble-out)" />
      {shape === "classic" ? (
        <>
          <path d="M14,9H7.5Q6,9 7,10.4L14,18Z" fill="var(--bubble-in)" />
          <path d="M106,31H112.5Q114,31 113,32.4L106,40Z" fill="var(--bubble-out)" />
        </>
      ) : null}
    </svg>
  );
}

/** The chat as it will look: real bubbles on the real background, so every
 * choice below shows here as it is made. */
function Preview() {
  return (
    <div className="chat-wallpaper flex flex-col gap-0.5 overflow-hidden rounded-xl border border-(--divider) px-4 py-3" aria-hidden>
      <div className="flex">
        <div
          data-bubble
          data-tail="in"
          className="relative max-w-[80%] rounded-[7.5px] rounded-tl-none px-1.75 pt-1.5 pb-2 text-[14.2px] leading-4.75 text-(--text-primary)"
          style={{ background: "var(--bubble-in)", boxShadow: "var(--bubble-shadow)" }}
        >
          <Tail mine={false} />
          Did you change how the app looks? 😄
          <span className="float-right mt-1 ml-2 text-[11px] leading-4 text-(--bubble-meta)">10:41</span>
        </div>
      </div>
      <div className="mt-2 flex justify-end">
        <div
          data-bubble
          data-tail="out"
          className="relative max-w-[80%] rounded-[7.5px] rounded-tr-none px-1.75 pt-1.5 pb-2 text-[14.2px] leading-4.75 text-(--text-primary)"
          style={{ background: "var(--bubble-out)", boxShadow: "var(--bubble-shadow)" }}
        >
          <Tail mine />
          Yes! Colours, bubbles and background are all mine now.
          <span className="float-right mt-1 ml-2 inline-flex items-center gap-0.5 text-[11px] leading-4 text-(--bubble-meta)">
            10:42 <Ticks status="read" />
          </span>
        </div>
      </div>
      <div className="flex justify-end">
        <div
          data-bubble
          className="relative max-w-[80%] rounded-[7.5px] px-1.75 pt-1.5 pb-2 text-[14.2px] leading-4.75 text-(--text-primary)"
          style={{ background: "var(--bubble-out)", boxShadow: "var(--bubble-shadow)" }}
        >
          Change anything back whenever you like.
          <span className="float-right mt-1 ml-2 inline-flex items-center gap-0.5 text-[11px] leading-4 text-(--bubble-meta)">
            10:42 <Ticks status="read" />
          </span>
        </div>
      </div>
    </div>
  );
}

function StepButton({ label, onClick, disabled, children }: { label: string; onClick: () => void; disabled?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      disabled={disabled}
      className="flex h-9 w-9 items-center justify-center rounded-full border border-(--divider) text-[18px] text-(--text-primary) hover:bg-(--hover-bg) disabled:opacity-40"
    >
      {children}
    </button>
  );
}

export function AppearanceSettings() {
  const {
    settings,
    resolvedTheme,
    setTheme,
    setAccent,
    setBubbles,
    setWallpaper,
    setWallpaperDim,
    wallpaperImage,
    setWallpaperImage,
    setTextSize,
    setAppearance,
    zoomIn,
    zoomOut,
    resetZoom,
  } = useSettings();
  const picker = useRef<HTMLInputElement>(null);
  const [pictureError, setPictureError] = useState<string | null>(null);

  async function choosePicture(file: File) {
    setPictureError(null);
    try {
      await setWallpaperImage(file);
      setWallpaper("image");
    } catch {
      setPictureError(`“${file.name}” couldn't be opened as a picture. Try a JPEG, PNG or WebP.`);
    }
  }
  const { width: sidebarWidth, setWidth: setSidebarWidth } = useSidebarWidth();
  // What "Restore defaults" replaced, so it can be undone.
  const [restored, setRestored] = useState<Appearance | null>(null);

  const zoomIndex = ZOOM_STEPS.indexOf(settings.zoom);
  const themeAccent = THEMES.find((t) => t.id === resolvedTheme)?.swatch[2] ?? "var(--accent)";
  const current: Appearance = {
    theme: settings.theme,
    accent: settings.accent,
    bubbles: settings.bubbles,
    wallpaper: settings.wallpaper,
    textSize: settings.textSize,
  };
  const isDefault = (Object.keys(DEFAULT_APPEARANCE) as (keyof Appearance)[]).every((k) => current[k] === DEFAULT_APPEARANCE[k]);

  return (
    <>
      <div className="sticky top-0 z-10 border-b border-(--divider) bg-(--panel-bg) px-6 pt-1 pb-4">
        <Preview />
      </div>

      <Group title="Theme" id="appearance-theme">
        <div className="grid grid-cols-3 gap-x-2 gap-y-3" role="radiogroup" aria-labelledby="appearance-theme">
          {THEMES.map((theme) => (
            <Option
              key={theme.id}
              label={theme.id === "whatsapp-system" ? `${theme.label}: ${theme.description.toLowerCase()}` : theme.label}
              active={settings.theme === theme.id}
              onClick={() => setTheme(theme.id)}
              wide={theme.id === "whatsapp-system"}
            >
              <span className="flex h-18 overflow-hidden rounded-[10px]" title={theme.description}>
                {theme.id === "whatsapp-system" ? (
                  <>
                    <ThemeThumb theme="whatsapp-light" />
                    <ThemeThumb theme="whatsapp-dark" />
                  </>
                ) : (
                  <ThemeThumb theme={theme.id} />
                )}
              </span>
            </Option>
          ))}
        </div>
      </Group>

      <Group title="Accent colour" id="appearance-accent">
        <div className="grid grid-cols-[repeat(auto-fill,minmax(52px,1fr))] gap-x-1 gap-y-2" role="radiogroup" aria-labelledby="appearance-accent">
          {ACCENTS.map((accent) => {
            const active = settings.accent === accent.id;
            return (
              <button
                key={accent.id}
                type="button"
                role="radio"
                aria-checked={active}
                onClick={() => setAccent(accent.id)}
                className="group/option flex flex-col items-center gap-1.5 rounded-lg p-1"
              >
                <span
                  className={[
                    "flex h-8 w-8 items-center justify-center rounded-full text-white ring-offset-2 ring-offset-(--panel-bg) transition-shadow",
                    active ? "ring-2 ring-(--text-primary)" : "group-hover/option:ring-2 group-hover/option:ring-(--divider)",
                  ].join(" ")}
                  style={{ background: accent.id === "theme" ? themeAccent : accent.color }}
                >
                  {active ? <CheckIcon size={16} strokeWidth={3} /> : null}
                </span>
                <span className={["text-[12px] leading-4", active ? "font-medium text-(--text-primary)" : "text-(--text-secondary)"].join(" ")}>
                  {accent.id === "theme" ? "Theme" : accent.label}
                </span>
              </button>
            );
          })}
        </div>
      </Group>

      <Group title="Bubbles" id="appearance-bubbles">
        <div className="grid grid-cols-3 gap-2" role="radiogroup" aria-labelledby="appearance-bubbles">
          {BUBBLE_SHAPES.map((shape) => (
            <Option key={shape.id} label={shape.label} active={settings.bubbles === shape.id} onClick={() => setBubbles(shape.id)}>
              <BubbleThumb shape={shape.id} />
            </Option>
          ))}
        </div>
      </Group>

      <Group title="Chat background" id="appearance-wallpaper">
        <div className="grid grid-cols-5 gap-2" role="radiogroup" aria-labelledby="appearance-wallpaper">
          {WALLPAPERS.filter((w) => w.id !== "image").map((wallpaper) => (
            <Option
              key={wallpaper.id}
              label={wallpaper.label}
              active={settings.wallpaper === wallpaper.id}
              onClick={() => setWallpaper(wallpaper.id)}
            >
              <WallpaperThumb wallpaper={wallpaper.id} />
            </Option>
          ))}
          <Option
            label={wallpaperImage ? "Your picture" : "Choose a picture…"}
            active={settings.wallpaper === "image" && Boolean(wallpaperImage)}
            onClick={() => (wallpaperImage ? setWallpaper("image") : picker.current?.click())}
          >
            {wallpaperImage ? (
              <WallpaperThumb wallpaper="image" />
            ) : (
              <span className="flex h-16 items-center justify-center rounded-[10px] border border-dashed border-(--text-secondary)/50 text-(--icon)">
                <PlusIcon size={22} />
              </span>
            )}
          </Option>
        </div>
        <input
          ref={picker}
          type="file"
          accept="image/*"
          className="hidden"
          onChange={(event) => {
            const file = event.target.files?.[0];
            event.target.value = "";
            if (file) void choosePicture(file);
          }}
        />
        {pictureError ? (
          <p className="text-[13px] text-(--danger)" role="alert">
            {pictureError}
          </p>
        ) : null}
        {settings.wallpaper === "image" && wallpaperImage ? (
          <div className="flex flex-col gap-3 rounded-xl bg-(--search-bg) px-4 py-3">
            <label className="flex items-center gap-3 text-[14px] text-(--text-primary)" htmlFor="wallpaper-dim">
              <span className="w-16 shrink-0">Darken</span>
              <input
                id="wallpaper-dim"
                type="range"
                min={WALLPAPER_DIM.min}
                max={WALLPAPER_DIM.max}
                step={WALLPAPER_DIM.step}
                value={settings.wallpaperDim}
                onChange={(event) => setWallpaperDim(Number(event.target.value))}
                className="min-w-0 flex-1 accent-(--accent)"
              />
              <output htmlFor="wallpaper-dim" className="w-10 text-right tabular-nums text-(--text-secondary)">
                {settings.wallpaperDim}%
              </output>
            </label>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => picker.current?.click()}
                className="h-8 rounded-full border border-(--divider) px-3 text-[13px] text-(--text-primary) hover:bg-(--hover-bg)"
              >
                Change picture
              </button>
              <button
                type="button"
                onClick={() => {
                  setWallpaper("theme");
                  void setWallpaperImage(null);
                }}
                className="h-8 rounded-full px-3 text-[13px] text-(--danger) hover:bg-(--hover-bg)"
              >
                Remove picture
              </button>
            </div>
          </div>
        ) : null}
      </Group>

      <Group title="Message text" id="appearance-text">
        <div className="grid grid-cols-4 gap-2" role="radiogroup" aria-labelledby="appearance-text">
          {TEXT_SIZES.map((size) => (
            <Option key={size.id} label={size.label} active={settings.textSize === size.id} onClick={() => setTextSize(size.id)}>
              <span className="flex h-12 items-center justify-center rounded-[10px] bg-(--search-bg) text-(--text-primary)" style={{ fontSize: size.px + 2 }} aria-hidden>
                Aa
              </span>
            </Option>
          ))}
        </div>
      </Group>

      <Group title="Zoom" id="appearance-zoom">
        <div className="flex items-center gap-3">
          <StepButton label="Zoom out" onClick={zoomOut} disabled={zoomIndex === 0}>
            −
          </StepButton>
          <output className="w-14 text-center text-[15px] tabular-nums text-(--text-primary)" aria-live="polite">
            {Math.round(settings.zoom * 100)}%
          </output>
          <StepButton label="Zoom in" onClick={zoomIn} disabled={zoomIndex === ZOOM_STEPS.length - 1}>
            +
          </StepButton>
          <button
            type="button"
            onClick={resetZoom}
            disabled={settings.zoom === 1}
            className="ml-2 h-9 rounded-full px-3 text-[14px] text-(--accent-strong) hover:bg-(--hover-bg) disabled:opacity-40"
          >
            Reset
          </button>
        </div>
        <p className="text-[13px] text-(--text-secondary)">
          Everything grows or shrinks together. {modKey} + and {modKey} − change it, {modKey} 0 resets it; {modKey} + scroll works too.
        </p>
      </Group>

      <Group title="Chat list width" id="appearance-width">
        <div className="flex items-center justify-between gap-3">
          <p className="text-[14px] text-(--text-primary)">
            {sidebarWidth}px <span className="text-(--text-secondary)">· drag the list's edge, or double-click it to reset</span>
          </p>
          <button
            type="button"
            onClick={() => setSidebarWidth(SIDEBAR_DEFAULT)}
            disabled={sidebarWidth === SIDEBAR_DEFAULT}
            className="h-9 shrink-0 rounded-full px-3 text-[14px] text-(--accent-strong) hover:bg-(--hover-bg) disabled:opacity-40"
          >
            Reset width
          </button>
        </div>
      </Group>

      <div className="flex items-center gap-3 px-6 pt-2 pb-8">
        {restored ? (
          <>
            <p className="flex-1 text-[14px] text-(--text-secondary)" role="status">Appearance is back to the defaults.</p>
            <button
              type="button"
              onClick={() => {
                setAppearance(restored);
                setRestored(null);
              }}
              className="h-9 rounded-full px-3 text-[14px] font-medium text-(--accent-strong) hover:bg-(--hover-bg)"
            >
              Undo
            </button>
          </>
        ) : (
          <button
            type="button"
            disabled={isDefault}
            onClick={() => {
              setRestored(current);
              setAppearance(DEFAULT_APPEARANCE);
            }}
            className="h-9 rounded-full border border-(--divider) px-4 text-[14px] text-(--text-primary) hover:bg-(--hover-bg) disabled:opacity-40"
          >
            Restore default look
          </button>
        )}
      </div>
    </>
  );
}

function WallpaperThumb({ wallpaper }: { wallpaper: Wallpaper }) {
  return (
    <span
      data-wp={wallpaper}
      className="chat-wallpaper flex h-16 flex-col justify-center gap-1.5 overflow-hidden rounded-[10px] px-2.5"
    >
      <span className="h-2.5 w-3/5 rounded-[3px] bg-(--bubble-in)" />
      <span className="h-2.5 w-1/2 self-end rounded-[3px] bg-(--bubble-out)" />
    </span>
  );
}
