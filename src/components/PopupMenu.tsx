import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { CheckIcon } from "./Icons";

export interface MenuAction {
  label: string;
  onSelect: () => void;
  /** A checkbox item: shows a tick when set. */
  checked?: boolean;
  icon?: ReactNode;
  danger?: boolean;
}

export type MenuEntry = MenuAction | { heading: string } | "separator";

interface PopupMenuProps {
  /** Where the menu opens, in viewport pixels. It moves in to stay on screen. */
  x: number;
  y: number;
  label: string;
  items: MenuEntry[];
  onClose: () => void;
}

const EDGE = 8;

export function PopupMenu({ x, y, label, items, onClose }: PopupMenuProps) {
  const menu = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState({ left: x, top: y });
  const close = useRef(onClose);
  close.current = onClose;

  useLayoutEffect(() => {
    const el = menu.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setPlace({
      left: Math.max(EDGE, Math.min(x, window.innerWidth - width - EDGE)),
      top: Math.max(EDGE, Math.min(y, window.innerHeight - height - EDGE)),
    });
  }, [x, y]);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    menu.current?.querySelector<HTMLElement>("[role^='menuitem']")?.focus();
    const onClose = () => close.current();
    const onPointerDown = (event: PointerEvent) => {
      if (!menu.current?.contains(event.target as Node)) onClose();
    };
    // Only the user scrolling closes it: chats also scroll themselves as pictures load.
    const onScroll = (event: Event) => {
      if (!menu.current?.contains(event.target as Node)) onClose();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("wheel", onScroll, true);
    document.addEventListener("touchmove", onScroll, true);
    window.addEventListener("resize", onClose);
    window.addEventListener("blur", onClose);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("wheel", onScroll, true);
      document.removeEventListener("touchmove", onScroll, true);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("blur", onClose);
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  function onKeyDown(event: KeyboardEvent) {
    const entries = [...(menu.current?.querySelectorAll<HTMLElement>("[role^='menuitem']") ?? [])];
    const at = entries.indexOf(document.activeElement as HTMLElement);
    const move = (index: number) => {
      event.preventDefault();
      entries[(index + entries.length) % entries.length]?.focus();
    };
    if (event.key === "ArrowDown") move(at + 1);
    else if (event.key === "ArrowUp") move(at - 1);
    else if (event.key === "Home") move(0);
    else if (event.key === "End") move(entries.length - 1);
    else if (event.key === "Escape" || event.key === "Tab") {
      event.preventDefault();
      onClose();
    }
  }

  // On the body: the panels use `contain`, which would clip a fixed menu.
  return createPortal(
    <div
      ref={menu}
      role="menu"
      aria-label={label}
      onKeyDown={onKeyDown}
      onContextMenu={(event) => event.preventDefault()}
      className="animate-fade-rise fixed z-50 min-w-[200px] max-w-[300px] rounded-xl bg-(--menu-bg) py-2"
      style={{ ...place, boxShadow: "0 2px 5px rgba(11,20,26,.26), 0 2px 10px rgba(11,20,26,.16)" }}
    >
      {items.map((item, index) => {
        if (item === "separator") {
          return <div key={index} role="separator" className="my-1.5 h-px bg-(--divider)" />;
        }
        if ("heading" in item) {
          return (
            <p key={index} className="px-5 pt-1.5 pb-1 text-[12.5px] text-(--text-secondary)">
              {item.heading}
            </p>
          );
        }
        const checkable = item.checked !== undefined;
        return (
          <button
            key={index}
            type="button"
            role={checkable ? "menuitemcheckbox" : "menuitem"}
            aria-checked={checkable ? item.checked : undefined}
            onClick={() => {
              onClose();
              item.onSelect();
            }}
            className={[
              "flex w-full items-center gap-3 px-5 py-2 text-left text-[14.5px] hover:bg-(--hover-bg) focus-visible:bg-(--hover-bg) focus-visible:outline-none",
              item.danger ? "text-(--danger)" : "text-(--text-primary)",
            ].join(" ")}
          >
            {item.icon || checkable ? (
              <span className="flex w-5 shrink-0 justify-center text-(--icon)" aria-hidden>
                {checkable ? (item.checked ? <CheckIcon size={18} className="text-(--accent)" /> : null) : item.icon}
              </span>
            ) : null}
            <span className="min-w-0 flex-1 truncate">{item.label}</span>
          </button>
        );
      })}
    </div>,
    document.body,
  );
}
