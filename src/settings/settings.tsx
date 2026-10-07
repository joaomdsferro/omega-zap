import { ACCOUNT_IDS, type AccountId } from "../accounts/accounts";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

export type ThemeId =
  | "whatsapp-system"
  | "whatsapp-light"
  | "whatsapp-dark"
  | "omegazap-dark"
  | "omegazap-light";

export type ResolvedTheme = Exclude<ThemeId, "whatsapp-system">;

export interface ThemeOption {
  id: ThemeId;
  label: string;
  description: string;
  swatch: [string, string, string];
}

export const THEMES: ThemeOption[] = [
  {
    id: "whatsapp-system",
    label: "WhatsApp (system)",
    description: "Light or dark, following your desktop",
    swatch: ["#ffffff", "#111b21", "#00a884"],
  },
  {
    id: "whatsapp-light",
    label: "WhatsApp light",
    description: "The WhatsApp Web look",
    swatch: ["#efeae2", "#d9fdd3", "#00a884"],
  },
  {
    id: "whatsapp-dark",
    label: "WhatsApp dark",
    description: "WhatsApp Web in dark mode",
    swatch: ["#0b141a", "#005c4b", "#00a884"],
  },
  {
    id: "omegazap-dark",
    label: "Omega Zap ink",
    description: "Near-black with teal accents",
    swatch: ["#0b1210", "#1a3d36", "#2dd4a8"],
  },
  {
    id: "omegazap-light",
    label: "Omega Zap mint",
    description: "Soft mint with teal accents",
    swatch: ["#eef5f1", "#c8ebe0", "#0f9f7a"],
  },
];

export const ZOOM_STEPS = [0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];
export const SIDEBAR_DEFAULT = 400;
export const SIDEBAR_MIN = 300;
export const SIDEBAR_MAX = 640;
/** The conversation pane never gets narrower than this while resizing. */
export const CHAT_MIN = 380;

export interface Settings {
  theme: ThemeId;
  zoom: number;
  sidebarWidth: number;
  /** The account whose chats are shown while accounts are kept apart. */
  activeAccount: AccountId;
  /** One chat list for every linked account, instead of one per account. */
  unifiedInbox: boolean;
  /** Fetch the full-size picture of a link card instead of keeping the small one. */
  largeLinkPreviews: boolean;
  /** Play TikTok, Instagram and YouTube Shorts links inside the chat. */
  inlineVideos: boolean;
  /** Desktop notifications for new messages and reactions. */
  notifications: boolean;
  /** Show what a message says in its notification. */
  notificationPreview: boolean;
}

const DEFAULTS: Settings = {
  theme: "whatsapp-system",
  zoom: 1,
  sidebarWidth: SIDEBAR_DEFAULT,
  activeAccount: "whatsapp",
  unifiedInbox: false,
  largeLinkPreviews: true,
  inlineVideos: true,
  notifications: true,
  notificationPreview: true,
};

const STORAGE_KEY = "omega-zap-settings";

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function load(): Settings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULTS;
    const parsed = JSON.parse(raw) as Partial<Settings>;
    return {
      theme: THEMES.some((t) => t.id === parsed.theme) ? (parsed.theme as ThemeId) : DEFAULTS.theme,
      zoom: ZOOM_STEPS.includes(parsed.zoom ?? NaN) ? (parsed.zoom as number) : DEFAULTS.zoom,
      sidebarWidth: clamp(
        Number(parsed.sidebarWidth) || SIDEBAR_DEFAULT,
        SIDEBAR_MIN,
        SIDEBAR_MAX,
      ),
      activeAccount: ACCOUNT_IDS.includes(parsed.activeAccount as AccountId)
        ? (parsed.activeAccount as AccountId)
        : DEFAULTS.activeAccount,
      unifiedInbox: parsed.unifiedInbox === true,
      largeLinkPreviews: parsed.largeLinkPreviews !== false,
      inlineVideos: parsed.inlineVideos !== false,
      notifications: parsed.notifications !== false,
      notificationPreview: parsed.notificationPreview !== false,
    };
  } catch {
    return DEFAULTS;
  }
}

interface SettingsContextValue {
  settings: Settings;
  resolvedTheme: ResolvedTheme;
  setTheme: (theme: ThemeId) => void;
  setZoom: (zoom: number) => void;
  zoomIn: () => void;
  zoomOut: () => void;
  resetZoom: () => void;
  setSidebarWidth: (width: number) => void;
  setActiveAccount: (account: AccountId) => void;
  setUnifiedInbox: (unified: boolean) => void;
  setLargeLinkPreviews: (large: boolean) => void;
  setInlineVideos: (inline: boolean) => void;
  setNotifications: (on: boolean) => void;
  setNotificationPreview: (on: boolean) => void;
}

const SettingsContext = createContext<SettingsContextValue | null>(null);

function useSystemDark() {
  const query = "(prefers-color-scheme: dark)";
  const [dark, setDark] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const media = window.matchMedia(query);
    const onChange = () => setDark(media.matches);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);
  return dark;
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings>(load);
  const systemDark = useSystemDark();

  const resolvedTheme: ResolvedTheme =
    settings.theme === "whatsapp-system"
      ? systemDark
        ? "whatsapp-dark"
        : "whatsapp-light"
      : settings.theme;

  useEffect(() => {
    document.documentElement.dataset.theme = resolvedTheme;
  }, [resolvedTheme]);

  useEffect(() => {
    const id = window.setTimeout(() => {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
    }, 150);
    return () => window.clearTimeout(id);
  }, [settings]);

  const setZoom = useCallback((zoom: number) => {
    setSettings((s) => ({ ...s, zoom: clamp(zoom, ZOOM_STEPS[0], ZOOM_STEPS[ZOOM_STEPS.length - 1]) }));
  }, []);

  const stepZoom = useCallback((direction: 1 | -1) => {
    setSettings((s) => {
      const index = ZOOM_STEPS.findIndex((z) => z >= s.zoom - 0.001);
      const current = index === -1 ? ZOOM_STEPS.indexOf(1) : index;
      const next = clamp(current + direction, 0, ZOOM_STEPS.length - 1);
      return { ...s, zoom: ZOOM_STEPS[next] };
    });
  }, []);

  const value = useMemo<SettingsContextValue>(
    () => ({
      settings,
      resolvedTheme,
      setTheme: (theme) => setSettings((s) => ({ ...s, theme })),
      setZoom,
      zoomIn: () => stepZoom(1),
      zoomOut: () => stepZoom(-1),
      resetZoom: () => setZoom(1),
      setSidebarWidth: (width) =>
        setSettings((s) => ({ ...s, sidebarWidth: clamp(Math.round(width), SIDEBAR_MIN, SIDEBAR_MAX) })),
      setActiveAccount: (activeAccount) => setSettings((s) => ({ ...s, activeAccount })),
      setUnifiedInbox: (unifiedInbox) => setSettings((s) => ({ ...s, unifiedInbox })),
      setLargeLinkPreviews: (largeLinkPreviews) => setSettings((s) => ({ ...s, largeLinkPreviews })),
      setInlineVideos: (inlineVideos) => setSettings((s) => ({ ...s, inlineVideos })),
      setNotifications: (notifications) => setSettings((s) => ({ ...s, notifications })),
      setNotificationPreview: (notificationPreview) => setSettings((s) => ({ ...s, notificationPreview })),
    }),
    [settings, resolvedTheme, setZoom, stepZoom],
  );

  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}

export function useSettings() {
  const value = useContext(SettingsContext);
  if (!value) throw new Error("useSettings must be used inside SettingsProvider");
  return value;
}
