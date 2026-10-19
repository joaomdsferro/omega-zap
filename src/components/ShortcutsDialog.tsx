import { useEffect, useRef } from "react";
import { OTHER_SHORTCUTS, SHORTCUTS } from "../lib/shortcuts";
import { CloseIcon } from "./Icons";

const SECTIONS = [
  {
    section: "App",
    items: (["newChat", "search", "nextChat", "previousChat", "closeChat", "settings", "emoji", "help"] as const).map((id) => SHORTCUTS[id]),
  },
  {
    section: "Open chat",
    items: (["markUnread", "mute", "archive", "pin"] as const).map((id) => SHORTCUTS[id]),
  },
  ...OTHER_SHORTCUTS,
];

/** One combination as keycaps: Ctrl, Alt, M. */
function Keys({ combo }: { combo: string }) {
  const keys = combo.split(/\+(?!$)/);
  return (
    <span className="flex items-center gap-1">
      {keys.map((key, i) => (
        <kbd
          key={i}
          className="min-w-6 rounded-md border border-(--divider) bg-(--panel-header-bg) px-1.5 py-0.5 text-center font-sans text-[12px] leading-4 text-(--text-primary)"
        >
          {key}
        </kbd>
      ))}
    </span>
  );
}

/** Every keyboard shortcut, as WhatsApp Web lists its own. */
export function ShortcutsDialog({ onClose }: { onClose: () => void }) {
  const close = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    close.current?.focus();
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  return (
    <div
      className="animate-fade-in fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4"
      onPointerDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="shortcuts-title"
        onKeyDown={(event) => {
          if (event.key !== "Escape") return;
          event.preventDefault();
          onClose();
        }}
        className="animate-fade-rise flex max-h-[85vh] w-full max-w-170 flex-col rounded-2xl bg-(--panel-bg)"
        style={{ boxShadow: "0 17px 50px rgba(11,20,26,.19), 0 12px 15px rgba(11,20,26,.24)" }}
      >
        <header className="flex shrink-0 items-center justify-between px-6 pt-5 pb-3">
          <h2 id="shortcuts-title" className="text-[18px] text-(--text-primary)">
            Keyboard shortcuts
          </h2>
          <button
            ref={close}
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="flex h-9 w-9 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
          >
            <CloseIcon size={20} />
          </button>
        </header>
        <div className="grid min-h-0 gap-x-10 gap-y-5 overflow-y-auto px-6 pb-6 sm:grid-cols-2">
          {SECTIONS.map(({ section, items }) => (
            <section key={section}>
              <h3 className="mb-1.5 text-[13px] font-medium text-(--accent)">{section}</h3>
              <dl>
                {items.map((item) => (
                  <div key={item.label} className="flex items-start justify-between gap-4 py-1.5">
                    <dt className="pt-0.5 text-[14px] text-(--text-primary)">{item.label}</dt>
                    <dd className="flex shrink-0 flex-col items-end gap-1">
                      {item.combos.map((combo) => (
                        <Keys key={combo} combo={combo} />
                      ))}
                    </dd>
                  </div>
                ))}
              </dl>
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}
