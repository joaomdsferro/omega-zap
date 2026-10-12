import { useCallback, useEffect, useMemo, useRef, useState, type ClipboardEvent, type MutableRefObject, type ReactNode } from "react";
import type { AccountId } from "../accounts/accounts";
import type { GifResult, LibrarySticker, OutgoingMedia, StickerLibrary, StickerPack, StickerResult, StickerShelf, StorePack } from "../backend";
import { formatFileSize } from "../lib/format";
import { klipyAvailable, searchGifs, searchStickers } from "../lib/gifs";
import { GifVideo, useNear } from "./Attachments";
import { BackIcon, ClockIcon, CloseIcon, CopyIcon, EditIcon, GifIcon, PlusIcon, SmileIcon, StarIcon, StickerIcon, TrashIcon } from "./Icons";
import { PopupMenu, type MenuEntry } from "./PopupMenu";
import { EmojiGrid } from "./ReactionPicker";

export type PanelTab = "emoji" | "gif" | "stickers";

const TAB_KEY = "omega-zap.media-tab";

/** Why there is no search: the app was built without a KLIPY key. */
const NO_KEY = "Search needs a KLIPY key: KLIPY_API_KEY in the build, or in .env for development.";

function savedTab(): PanelTab {
  try {
    const tab = localStorage.getItem(TAB_KEY);
    if (tab === "gif" || tab === "stickers") return tab;
  } catch {
    // Storage can be off; the emoji tab it is.
  }
  return "emoji";
}

interface MediaPanelProps {
  account: AccountId;
  onEmoji: (emoji: string) => void;
  /** Sends a sticker or GIF; absent where they can't be sent. */
  onSend?: (item: OutgoingMedia) => Promise<void>;
  stickers?: StickerShelf;
  onError?: (message: string) => void;
  /**
   * Set while the Stickers tab is open: takes pictures pasted into the
   * message box and makes them into a sticker. Returns false to leave them.
   */
  pasteInto?: MutableRefObject<((files: File[]) => boolean) | null>;
}

/** The composer's panel: emoji, and where the network takes them, GIFs and stickers. */
export function MediaPanel({ account, onEmoji, onSend, stickers, onError, pasteInto }: MediaPanelProps) {
  const [tab, setTab] = useState<PanelTab>(() => (onSend ? savedTab() : "emoji"));
  const choose = (next: PanelTab) => {
    setTab(next);
    try {
      localStorage.setItem(TAB_KEY, next);
    } catch {
      // Only a convenience.
    }
  };
  const report = useCallback((e: unknown) => onError?.(String(e)), [onError]);
  const tabs: { id: PanelTab; label: string; icon: ReactNode }[] = [
    { id: "emoji", label: "Emoji", icon: <SmileIcon size={20} /> },
    { id: "gif", label: "GIFs", icon: <GifIcon size={20} /> },
    { id: "stickers", label: "Stickers", icon: <StickerIcon size={20} /> },
  ];

  return (
    <>
      {onSend ? (
        <div className="flex shrink-0 gap-1 border-b border-(--divider) px-2 pt-2" role="tablist" aria-label="Emoji, GIFs and stickers">
          {tabs.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={tab === t.id}
              onClick={() => choose(t.id)}
              className={[
                "-mb-px flex h-9 flex-1 items-center justify-center gap-1.5 border-b-2 text-[13px]",
                tab === t.id ? "border-(--accent) text-(--accent)" : "border-transparent text-(--icon) hover:text-(--text-primary)",
              ].join(" ")}
            >
              {t.icon}
              <span className="text-[13px]">{t.label}</span>
            </button>
          ))}
        </div>
      ) : null}
      {tab === "emoji" || !onSend ? (
        <EmojiGrid account={account} autoFocus={false} characters onPick={onEmoji} />
      ) : tab === "gif" ? (
        <GifTab stickers={stickers} onSend={onSend} onError={report} />
      ) : stickers ? (
        <StickerTab stickers={stickers} onSend={onSend} onError={report} pasteInto={pasteInto} />
      ) : (
        <p className="p-6 text-center text-[13px] text-(--text-secondary)">Stickers can't be sent here.</p>
      )}
    </>
  );
}

/** Runs a send, marking `key` busy meanwhile so it can't be sent twice. */
function useSending(onSend: (item: OutgoingMedia) => Promise<void>, onError: (e: unknown) => void) {
  const [busy, setBusy] = useState<Set<string>>(new Set());
  const send = useCallback(
    (key: string, item: OutgoingMedia) => {
      setBusy((prev) => new Set(prev).add(key));
      onSend(item)
        .catch(onError)
        .finally(() =>
          setBusy((prev) => {
            const next = new Set(prev);
            next.delete(key);
            return next;
          }),
        );
    },
    [onSend, onError],
  );
  return { busy, send };
}

function Spinner() {
  return <span aria-hidden className="block h-6 w-6 animate-spin rounded-full border-2 border-white border-t-transparent" />;
}

/* ---------------------------------------------------------------- Stickers */

/** The library, kept up to date with the phone; null while it loads or where there is none. */
function useLibrary(stickers: StickerShelf | undefined, onError: (e: unknown) => void) {
  const [library, setLibrary] = useState<StickerLibrary | null>(null);
  useEffect(() => {
    if (!stickers) return;
    let alive = true;
    const load = () =>
      stickers
        .library()
        .then((l) => alive && setLibrary(l))
        .catch(onError);
    load();
    const stop = stickers.subscribe(load);
    return () => {
      alive = false;
      stop();
    };
  }, [stickers, onError]);
  return library;
}

/** A library file's address once it comes near; null while it loads, "failed" when it can't. */
function useShelfUrl(stickers: StickerShelf, id: string, wanted: boolean, blob = false) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!wanted) return;
    let alive = true;
    let objectUrl: string | null = null;
    stickers
      .fileUrl(id)
      .then(async (loaded) => {
        // WebKitGTK plays videos only from blob: URLs.
        if (!blob) return loaded;
        const response = await fetch(loaded);
        if (!response.ok) throw new Error(`media answered ${response.status}`);
        objectUrl = URL.createObjectURL(await response.blob());
        return objectUrl;
      })
      .then((loaded) => alive && setUrl(loaded))
      .catch(() => alive && setUrl("failed"));
    return () => {
      alive = false;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [stickers, id, wanted, blob]);
  return url;
}

function StickerImage({ stickers, id, className }: { stickers: StickerShelf; id: string; className: string }) {
  const box = useRef<HTMLSpanElement>(null);
  const url = useShelfUrl(stickers, id, useNear(box, "200px 0px"));
  return (
    <span ref={box} className={`flex items-center justify-center ${className}`}>
      {url && url !== "failed" ? (
        <img src={url} alt="" draggable={false} className="max-h-full max-w-full object-contain" />
      ) : url === "failed" ? (
        <StickerIcon size={22} className="text-(--text-secondary)" />
      ) : null}
    </span>
  );
}

type SectionId = "recent" | "favorites" | string;

/** The first picture among pasted or dropped files. */
function pictureIn(files: File[]): File | undefined {
  return files.find((f) => f.type.startsWith("image/"));
}

function StickerTab({ stickers, onSend, onError, pasteInto }: {
  stickers: StickerShelf;
  onSend: (item: OutgoingMedia) => Promise<void>;
  onError: (e: unknown) => void;
  pasteInto?: MutableRefObject<((files: File[]) => boolean) | null>;
}) {
  const library = useLibrary(stickers, onError);
  const [adding, setAdding] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  const { busy, send } = useSending(onSend, onError);
  const [menu, setMenu] = useState<{ x: number; y: number; sticker: LibrarySticker; pack?: StickerPack } | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const picker = useRef<HTMLInputElement>(null);
  const [addInto, setAddInto] = useState<string | null>(null);
  const [making, setMaking] = useState<string | null>(null);
  const [available, setAvailable] = useState(false);
  const [query, setQuery] = useState("");
  const found = useKlipy(searchStickers, query, available, false, onError);
  const [foundMenu, setFoundMenu] = useState<{ x: number; y: number; sticker: StickerResult } | null>(null);
  // A picture chosen or pasted, waiting to be sent or put in a pack.
  const [picked, setPicked] = useState<{ picture: Blob; url: string } | null>(null);
  const [sendingPicked, setSendingPicked] = useState(false);

  useEffect(() => {
    void klipyAvailable().then(setAvailable);
  }, []);

  const pick = useCallback((picture: Blob) => {
    setAdding(false);
    setPicked((prev) => {
      if (prev) URL.revokeObjectURL(prev.url);
      return { picture, url: URL.createObjectURL(picture) };
    });
  }, []);
  useEffect(() => () => setPicked((prev) => (prev && URL.revokeObjectURL(prev.url), null)), []);

  // Pictures pasted into the message box come here while this tab is open.
  useEffect(() => {
    if (!pasteInto) return;
    pasteInto.current = (files) => {
      const picture = pictureIn(files);
      if (picture) pick(picture);
      return Boolean(picture);
    };
    return () => {
      pasteInto.current = null;
    };
  }, [pasteInto, pick]);

  function onPaste(event: ClipboardEvent) {
    const picture = pictureIn(Array.from(event.clipboardData.files));
    if (!picture) return;
    event.preventDefault();
    pick(picture);
  }

  if (adding) {
    return (
      <div className="flex min-h-0 flex-1 flex-col" onPaste={onPaste}>
        <AddStickers stickers={stickers} onPicture={pick} onError={onError} onBack={() => setAdding(false)} />
      </div>
    );
  }
  if (!library) return <p className="p-6 text-center text-[13px] text-(--text-secondary)">Loading stickers…</p>;

  const ownPacks = library.packs.filter((p) => !p.store);
  const sections: { id: SectionId; name: string; icon: ReactNode; stickers: LibrarySticker[]; pack?: StickerPack; empty: string }[] = [
    { id: "recent", name: "Recent", icon: <ClockIcon size={20} />, stickers: library.recent, empty: "Stickers you send show here." },
    { id: "favorites", name: "Favorites", icon: <StarIcon size={20} />, stickers: library.favorites, empty: "Stickers you star show here, and on your phone." },
    ...library.packs.map((pack) => ({
      id: pack.id,
      name: pack.name,
      pack,
      stickers: pack.stickers,
      empty: "No stickers yet. Add pictures with +.",
      icon: pack.tray ? (
        <img src={pack.tray} alt="" draggable={false} className="h-6 w-6 object-contain" />
      ) : pack.stickers[0] ? (
        <StickerImage stickers={stickers} id={pack.stickers[0].id} className="h-6 w-6" />
      ) : (
        <StickerIcon size={20} />
      ),
    })),
  ];

  function favorite(sticker: LibrarySticker) {
    stickers.setFavorite({ id: sticker.id }, !sticker.favorite).catch(onError);
  }

  function menuFor(sticker: LibrarySticker, pack?: StickerPack): MenuEntry[] {
    const items: MenuEntry[] = [
      { label: "Send", icon: <StickerIcon size={20} />, onSelect: () => send(sticker.id, { kind: "sticker", id: sticker.id }) },
      {
        label: sticker.favorite ? "Remove from favorites" : "Add to favorites",
        icon: <StarIcon size={20} filled={sticker.favorite} />,
        onSelect: () => favorite(sticker),
      },
    ];
    const targets = ownPacks.filter((p) => p.id !== pack?.id);
    if (targets.length) {
      items.push("separator", { heading: "Add to pack" });
      for (const p of targets) {
        items.push({ label: p.name, onSelect: () => stickers.addToPack(p.id, { id: sticker.id }).catch(onError) });
      }
    }
    if (pack && !pack.store) {
      items.push("separator", {
        label: "Remove from this pack",
        icon: <TrashIcon size={20} />,
        danger: true,
        onSelect: () => stickers.removeFromPack(pack.id, sticker.id).catch(onError),
      });
    }
    return items;
  }

  const searching = available && query.trim().length > 0;
  function foundMenuFor(sticker: StickerResult): MenuEntry[] {
    const items: MenuEntry[] = [
      { label: "Send", icon: <StickerIcon size={20} />, onSelect: () => send(sticker.slug, { kind: "found-sticker", pick: sticker.send }) },
    ];
    if (ownPacks.length) {
      items.push("separator", { heading: "Add to pack" });
      for (const p of ownPacks) items.push({ label: p.name, onSelect: () => stickers.addFoundSticker(p.id, sticker.send).catch(onError) });
    }
    return items;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col" onPaste={onPaste}>
      {picked ? (
        <PickedSticker
          url={picked.url}
          packs={ownPacks}
          busy={sendingPicked}
          onSend={() => {
            const sent = picked;
            setSendingPicked(true);
            onSend({ kind: "new-sticker", picture: sent.picture })
              .then(() => {
                URL.revokeObjectURL(sent.url);
                setPicked((prev) => (prev === sent ? null : prev));
              })
              .catch(onError)
              .finally(() => setSendingPicked(false));
          }}
          onAddTo={(pack) => {
            const picture = picked.picture;
            setPicked(null);
            URL.revokeObjectURL(picked.url);
            setMaking(pack.id);
            stickers
              .addNewSticker(pack.id, picture)
              .catch(onError)
              .finally(() => setMaking(null));
          }}
          onCancel={() => {
            URL.revokeObjectURL(picked.url);
            setPicked(null);
          }}
        />
      ) : null}
      <input
        type="search"
        value={query}
        disabled={!available}
        onChange={(event) => setQuery(event.target.value)}
        placeholder={available ? "Search KLIPY stickers" : "Sticker search is off in this build"}
        title={available ? undefined : NO_KEY}
        aria-label="Search stickers"
        className="mx-2 mt-2 mb-1 h-8 shrink-0 rounded-full bg-(--hover-bg) px-3 text-[14px] outline-none placeholder:text-(--text-secondary) disabled:opacity-60"
      />
      {searching ? (
        <>
          <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
            {found.results && found.results.items.length === 0 && !found.loading ? (
              <p className="px-1 py-2 text-[13px] text-(--text-secondary)">No stickers found.</p>
            ) : null}
            <div className="grid grid-cols-4 gap-1 pt-1">
              {(found.results?.items ?? []).map((sticker) => (
                <button
                  key={sticker.slug}
                  type="button"
                  title={sticker.title}
                  aria-label={sticker.title ? `Send sticker ${sticker.title}` : "Send sticker"}
                  disabled={busy.has(sticker.slug)}
                  onClick={() => send(sticker.slug, { kind: "found-sticker", pick: sticker.send })}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    setFoundMenu({ x: event.clientX, y: event.clientY, sticker });
                  }}
                  className="relative flex aspect-square w-full items-center justify-center rounded-lg p-1 hover:bg-(--hover-bg) disabled:cursor-wait"
                >
                  <img src={sticker.preview} alt="" loading="lazy" draggable={false} className="max-h-full max-w-full object-contain" />
                  {busy.has(sticker.slug) ? (
                    <span className="absolute inset-0 flex items-center justify-center rounded-lg bg-black/30">
                      <Spinner />
                    </span>
                  ) : null}
                </button>
              ))}
            </div>
            <div ref={found.end} className="h-px" />
            {found.loading ? <p className="px-1 py-2 text-center text-[12px] text-(--text-secondary)">Loading…</p> : null}
          </div>
          <p className="shrink-0 border-t border-(--divider) px-3 py-1 text-right text-[11px] text-(--text-secondary)">Powered by KLIPY</p>
        </>
      ) : (
        <>
          <div className="flex shrink-0 items-center gap-0.5 overflow-x-auto border-b border-(--divider) px-2 py-1" role="tablist" aria-label="Sticker sections">
            {sections.map((section, index) => (
              <button
                key={section.id}
                type="button"
                role="tab"
                title={section.name}
                aria-label={section.name}
                onClick={() => list.current?.children[index]?.scrollIntoView({ block: "start" })}
                className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-(--icon) hover:bg-(--hover-bg)"
              >
                {section.icon}
              </button>
            ))}
            <button
              type="button"
              title="Add stickers"
              aria-label="Add stickers"
              onClick={() => setAdding(true)}
              className="ml-auto flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-(--icon) hover:bg-(--hover-bg)"
            >
              <PlusIcon size={20} />
            </button>
          </div>
          <div ref={list} className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
            {sections.map((section) => (
              <section key={section.id} style={{ contentVisibility: "auto", containIntrinsicSize: "auto 200px" }}>
                <div className="flex items-center gap-1 px-1 pt-2 pb-1">
                  {renaming === section.id && section.pack ? (
                    <NameInput
                      initial={section.name}
                      label="Pack name"
                      onDone={(name) => {
                        setRenaming(null);
                        if (name && name !== section.name) stickers.renamePack(section.id, name).catch(onError);
                      }}
                    />
                  ) : (
                    <h3 className="min-w-0 flex-1 truncate text-[12px] text-(--text-secondary)">
                      {section.name}
                      {section.pack?.publisher ? <span> · {section.pack.publisher}</span> : null}
                    </h3>
                  )}
                  {section.pack && !section.pack.store ? (
                    <>
                      <SmallButton
                        label={`Add a picture to ${section.name}`}
                        onClick={() => {
                          setAddInto(section.id);
                          picker.current?.click();
                        }}
                      >
                        {making === section.id ? <span className="block h-3.5 w-3.5 animate-spin rounded-full border-2 border-current border-t-transparent" /> : <PlusIcon size={16} />}
                      </SmallButton>
                      <SmallButton label={`Rename ${section.name}`} onClick={() => setRenaming(section.id)}>
                        <EditIcon size={15} />
                      </SmallButton>
                    </>
                  ) : null}
                  {section.pack ? (
                    <SmallButton
                      label={`Remove ${section.name}`}
                      onClick={() => {
                        if (section.pack!.store || section.stickers.length === 0 || window.confirm(`Delete the pack "${section.name}"?`)) {
                          stickers.removePack(section.id).catch(onError);
                        }
                      }}
                    >
                      <TrashIcon size={15} />
                    </SmallButton>
                  ) : null}
                </div>
                {section.stickers.length ? (
                  <div className="grid grid-cols-4 gap-1">
                    {section.stickers.map((sticker) => (
                      <div key={sticker.id} className="group/sticker relative">
                        <button
                          type="button"
                          title={sticker.emojis.join(" ") || "Send sticker"}
                          aria-label={sticker.emojis.length ? `Send sticker ${sticker.emojis.join(" ")}` : "Send sticker"}
                          disabled={busy.has(sticker.id)}
                          onClick={() => send(sticker.id, { kind: "sticker", id: sticker.id })}
                          onContextMenu={(event) => {
                            event.preventDefault();
                            setMenu({ x: event.clientX, y: event.clientY, sticker, pack: section.pack });
                          }}
                          className="relative flex aspect-square w-full items-center justify-center rounded-lg p-1 hover:bg-(--hover-bg) disabled:cursor-wait"
                        >
                          <StickerImage stickers={stickers} id={sticker.id} className="h-full w-full" />
                          {busy.has(sticker.id) ? (
                            <span className="absolute inset-0 flex items-center justify-center rounded-lg bg-black/30">
                              <Spinner />
                            </span>
                          ) : null}
                        </button>
                        <button
                          type="button"
                          aria-label={sticker.favorite ? "Remove from favorites" : "Add to favorites"}
                          title={sticker.favorite ? "Remove from favorites" : "Add to favorites"}
                          onClick={() => favorite(sticker)}
                          className={[
                            "absolute top-0.5 right-0.5 flex h-6 w-6 items-center justify-center rounded-full bg-(--panel-bg) text-(--icon) shadow-sm",
                            sticker.favorite && section.id !== "favorites" ? "text-(--accent)" : "",
                            "opacity-0 group-hover/sticker:opacity-100 focus-visible:opacity-100",
                          ].join(" ")}
                        >
                          <StarIcon size={14} filled={sticker.favorite} />
                        </button>
                      </div>
                    ))}
                  </div>
                ) : (
                  <p className="px-1 pb-2 text-[13px] text-(--text-secondary)">{section.empty}</p>
                )}
              </section>
            ))}
          </div>
        </>
      )}
      <input
        ref={picker}
        type="file"
        accept="image/png,image/jpeg,image/gif,image/webp"
        hidden
        onChange={(event) => {
          const file = event.target.files?.[0];
          event.target.value = "";
          const pack = addInto;
          if (!file || !pack) return;
          setMaking(pack);
          stickers
            .addNewSticker(pack, file)
            .catch(onError)
            .finally(() => setMaking(null));
        }}
      />
      {menu ? (
        <PopupMenu x={menu.x} y={menu.y} label="Sticker options" items={menuFor(menu.sticker, menu.pack)} onClose={() => setMenu(null)} />
      ) : null}
      {foundMenu ? (
        <PopupMenu x={foundMenu.x} y={foundMenu.y} label="Sticker options" items={foundMenuFor(foundMenu.sticker)} onClose={() => setFoundMenu(null)} />
      ) : null}
    </div>
  );
}

/** A picture about to become a sticker: send it, or keep it in one of our packs. */
function PickedSticker({ url, packs, busy, onSend, onAddTo, onCancel }: {
  url: string;
  packs: StickerPack[];
  busy: boolean;
  onSend: () => void;
  onAddTo: (pack: StickerPack) => void;
  onCancel: () => void;
}) {
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  return (
    <div className="mx-2 mt-2 flex shrink-0 items-center gap-3 rounded-xl bg-(--hover-bg) p-2" aria-label="New sticker">
      <img src={url} alt="New sticker" draggable={false} className="h-16 w-16 shrink-0 object-contain" />
      <span className="flex min-w-0 flex-1 flex-col gap-1.5">
        <span className="text-[14px] text-(--text-primary)">New sticker</span>
        <span className="flex items-center gap-1">
          <button type="button" disabled={busy} onClick={onSend} className="flex h-7 items-center justify-center rounded-full bg-(--accent) px-3 disabled:cursor-wait">
            <span className="text-[13px] font-medium text-white">{busy ? "Sending…" : "Send"}</span>
          </button>
          {packs.length ? (
            <button
              type="button"
              disabled={busy}
              onClick={(event) => {
                const box = event.currentTarget.getBoundingClientRect();
                setMenu({ x: box.left, y: box.bottom + 4 });
              }}
              className="h-7 rounded-full px-3 hover:bg-(--panel-bg)"
            >
              <span className="text-[13px] font-medium text-(--accent)">Add to pack</span>
            </button>
          ) : null}
        </span>
      </span>
      <SmallButton label="Discard the new sticker" onClick={onCancel}>
        <CloseIcon size={16} />
      </SmallButton>
      {menu ? (
        <PopupMenu
          x={menu.x}
          y={menu.y}
          label="Add to pack"
          items={packs.map((pack) => ({ label: pack.name, onSelect: () => onAddTo(pack) }))}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </div>
  );
}

function SmallButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
    >
      {children}
    </button>
  );
}

function NameInput({ initial, label, onDone }: { initial: string; label: string; onDone: (name: string | null) => void }) {
  const [name, setName] = useState(initial);
  return (
    <input
      autoFocus
      value={name}
      aria-label={label}
      maxLength={60}
      onChange={(event) => setName(event.target.value)}
      onBlur={() => onDone(name.trim() || null)}
      onKeyDown={(event) => {
        if (event.key === "Enter") onDone(name.trim() || null);
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onDone(null);
        }
      }}
      className="h-7 min-w-0 flex-1 rounded-md bg-(--hover-bg) px-2 text-[13px] outline-none"
    />
  );
}

/** Making stickers, packs, and WhatsApp's sticker store. */
function AddStickers({ stickers, onPicture, onError, onBack }: {
  stickers: StickerShelf;
  /** A picture chosen or pasted, to make into a sticker. */
  onPicture: (picture: Blob) => void;
  onError: (e: unknown) => void;
  onBack: () => void;
}) {
  const [store, setStore] = useState<StorePack[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [working, setWorking] = useState<Set<string>>(new Set());
  const [naming, setNaming] = useState(false);
  const picker = useRef<HTMLInputElement>(null);

  async function paste() {
    try {
      for (const item of await navigator.clipboard.read()) {
        const type = item.types.find((t) => t.startsWith("image/"));
        if (type) return onPicture(await item.getType(type));
      }
      onError("There's no picture in the clipboard.");
    } catch {
      // WebKit may refuse to read the clipboard without a paste.
      onError("The clipboard couldn't be read here. Press Ctrl+V to paste the picture.");
    }
  }

  const load = useCallback(() => {
    stickers
      .store()
      .then(setStore)
      .catch((e) => {
        setFailed(true);
        onError(e);
      });
  }, [stickers, onError]);
  useEffect(() => {
    load();
    return stickers.subscribe(load);
  }, [stickers, load]);

  function toggle(pack: StorePack) {
    setWorking((prev) => new Set(prev).add(pack.id));
    (pack.installed ? stickers.removePack(pack.id) : stickers.installPack(pack.id))
      .catch(onError)
      .finally(() =>
        setWorking((prev) => {
          const next = new Set(prev);
          next.delete(pack.id);
          return next;
        }),
      );
  }

  const row = "flex w-full items-center gap-3 rounded-lg px-2 py-2 text-left text-[14px] text-(--text-primary) hover:bg-(--hover-bg)";
  return (
    <>
      <div className="flex shrink-0 items-center gap-1 border-b border-(--divider) px-1 py-1">
        <button type="button" onClick={onBack} aria-label="Back to stickers" className="flex h-8 w-8 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)">
          <BackIcon size={20} />
        </button>
        <h3 className="text-[14px] font-medium text-(--text-primary)">Add stickers</h3>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        <button type="button" className={`${row} mt-1`} onClick={() => picker.current?.click()}>
          <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-(--hover-bg) text-(--accent)">
            <StickerIcon size={22} />
          </span>
          <span className="min-w-0">
            <span className="block">Create a sticker</span>
            <span className="block text-[12.5px] text-(--text-secondary)">From a picture or GIF on this computer</span>
          </span>
        </button>
        <button type="button" className={row} onClick={() => void paste()}>
          <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-(--hover-bg) text-(--accent)">
            <CopyIcon size={20} />
          </span>
          <span className="min-w-0">
            <span className="block">Paste a picture</span>
            <span className="block text-[12.5px] text-(--text-secondary)">Or press Ctrl+V with the Stickers tab open</span>
          </span>
        </button>
        {naming ? (
          <div className="flex items-center gap-3 px-2 py-2">
            <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-(--hover-bg) text-(--accent)">
              <PlusIcon size={22} />
            </span>
            <NameInput
              initial=""
              label="New pack's name"
              onDone={(name) => {
                setNaming(false);
                if (name) stickers.createPack(name).then(onBack).catch(onError);
              }}
            />
          </div>
        ) : (
          <button type="button" className={row} onClick={() => setNaming(true)}>
            <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-(--hover-bg) text-(--accent)">
              <PlusIcon size={22} />
            </span>
            <span className="min-w-0">
              <span className="block">New pack</span>
              <span className="block text-[12.5px] text-(--text-secondary)">Your own, kept on this computer</span>
            </span>
          </button>
        )}
        <h3 className="px-1 pt-3 pb-1 text-[12px] text-(--text-secondary)">WhatsApp sticker store</h3>
        {store === null ? (
          <p className="px-1 py-2 text-[13px] text-(--text-secondary)">{failed ? "The store could not be reached." : "Loading the store…"}</p>
        ) : (
          <ul>
            {store.map((pack) => (
              <li key={pack.id} className="flex items-center gap-3 rounded-lg px-2 py-2">
                {pack.tray ? (
                  <img src={pack.tray} alt="" loading="lazy" draggable={false} className="h-10 w-10 shrink-0 object-contain" />
                ) : (
                  <StickerIcon size={28} className="m-1.5 shrink-0 text-(--icon)" />
                )}
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[14px] text-(--text-primary)">{pack.name}</span>
                  <span className="block truncate text-[12px] text-(--text-secondary)">
                    {[pack.publisher, pack.animated ? "Animated" : null, pack.size ? formatFileSize(pack.size) : null].filter(Boolean).join(" · ")}
                  </span>
                  <span className="mt-1 flex gap-1">
                    {pack.previews.slice(0, 5).map((src) => (
                      <img key={src} src={src} alt="" loading="lazy" draggable={false} className="h-8 w-8 object-contain" />
                    ))}
                  </span>
                </span>
                <button
                  type="button"
                  disabled={working.has(pack.id)}
                  onClick={() => toggle(pack)}
                  className={[
                    "flex h-8 min-w-16 shrink-0 items-center justify-center rounded-full px-3 disabled:cursor-wait",
                    pack.installed ? "bg-(--hover-bg)" : "bg-(--accent)",
                  ].join(" ")}
                >
                  <span className={`text-[13px] font-medium ${pack.installed ? "text-(--text-secondary)" : "text-white"}`}>{working.has(pack.id) ? "…" : pack.installed ? "Remove" : "Add"}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
      <input
        ref={picker}
        type="file"
        accept="image/png,image/jpeg,image/gif,image/webp"
        hidden
        onChange={(event) => {
          const file = event.target.files?.[0];
          event.target.value = "";
          if (file) onPicture(file);
        }}
      />
    </>
  );
}

/* -------------------------------------------------------------------- GIFs */

function RecentGif({ stickers, id, onClick, busy }: { stickers: StickerShelf; id: string; onClick: () => void; busy: boolean }) {
  const box = useRef<HTMLButtonElement>(null);
  const url = useShelfUrl(stickers, id, useNear(box, "200px 0px"), true);
  return (
    <button
      ref={box}
      type="button"
      aria-label="Send GIF"
      disabled={busy}
      onClick={onClick}
      className="relative h-24 w-32 shrink-0 overflow-hidden rounded-lg bg-(--hover-bg) disabled:cursor-wait"
    >
      {url && url !== "failed" ? (
        <GifVideo src={url} wrapperClassName="h-full w-full" className="h-full w-full object-cover" />
      ) : null}
      {busy ? (
        <span className="absolute inset-0 flex items-center justify-center bg-black/30">
          <Spinner />
        </span>
      ) : null}
    </button>
  );
}

/** Two columns, each result going under the shorter one, as GIF pickers lay them out. */
function columns(items: GifResult[]): GifResult[][] {
  const out: GifResult[][] = [[], []];
  const heights = [0, 0];
  for (const item of items) {
    const at = heights[0] <= heights[1] ? 0 : 1;
    out[at].push(item);
    heights[at] += item.width ? item.height / item.width : 1;
  }
  return out;
}

/**
 * KLIPY's results for what is typed, after a pause, a page at a time as the
 * end of the list (`end`) comes near. With nothing typed: what is trending,
 * or nothing when `trending` is off.
 */
function useKlipy<T extends { slug: string }>(
  search: (query: string, page?: number) => Promise<{ items: T[]; next: number | null }>,
  query: string,
  enabled: boolean,
  trending: boolean,
  onError: (e: unknown) => void,
) {
  const [results, setResults] = useState<{ query: string; items: T[]; next: number | null } | null>(null);
  const [loading, setLoading] = useState(false);
  const end = useRef<HTMLDivElement>(null);
  const [endSeen, setEndSeen] = useState(0);
  const asked = useRef("");

  useEffect(() => {
    const wanted = query.trim();
    asked.current = wanted;
    if (!enabled || (!wanted && !trending)) {
      setResults(null);
      setLoading(false);
      return;
    }
    const timer = window.setTimeout(
      () => {
        setLoading(true);
        search(wanted)
          .then((page) => asked.current === wanted && setResults({ query: wanted, ...page }))
          .catch(onError)
          .finally(() => asked.current === wanted && setLoading(false));
      },
      wanted ? 350 : 0,
    );
    return () => window.clearTimeout(timer);
  }, [search, query, enabled, trending, onError]);

  useEffect(() => {
    const node = end.current;
    if (!node) return;
    const observer = new IntersectionObserver((entries) => entries.some((e) => e.isIntersecting) && setEndSeen((n) => n + 1), {
      rootMargin: "300px 0px",
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [results?.items.length]);
  useEffect(() => {
    if (!results?.next || loading || endSeen === 0) return;
    const { query: q, next } = results;
    setLoading(true);
    search(q, next)
      .then((page) =>
        setResults((prev) =>
          prev && prev.query === q
            ? { query: q, items: [...prev.items, ...page.items.filter((i) => !prev.items.some((p) => p.slug === i.slug))], next: page.next }
            : prev,
        ),
      )
      .catch(onError)
      .finally(() => setLoading(false));
    // Only a new sighting of the end asks for more.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [endSeen]);

  return { results, loading, end };
}

function GifTab({ stickers, onSend, onError }: { stickers?: StickerShelf; onSend: (item: OutgoingMedia) => Promise<void>; onError: (e: unknown) => void }) {
  const [available, setAvailable] = useState<boolean | null>(null);
  const [query, setQuery] = useState("");
  const library = useLibrary(stickers, onError);
  const { busy, send } = useSending(onSend, onError);
  const { results, loading, end } = useKlipy(searchGifs, query, Boolean(available), true, onError);

  useEffect(() => {
    void klipyAvailable().then(setAvailable);
  }, []);

  const recent = library?.gifs ?? [];
  const cols = useMemo(() => columns(results?.items ?? []), [results]);

  return (
    <>
      <input
        type="search"
        value={query}
        disabled={available === false}
        onChange={(event) => setQuery(event.target.value)}
        placeholder={available === false ? "GIF search is off in this build" : "Search KLIPY"}
        title={available === false ? NO_KEY : undefined}
        aria-label="Search GIFs"
        className="mx-2 mt-2 h-8 shrink-0 rounded-full bg-(--hover-bg) px-3 text-[14px] outline-none placeholder:text-(--text-secondary) disabled:opacity-60"
      />
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {!query.trim() && stickers && recent.length ? (
          <section>
            <h3 className="px-1 pt-2 pb-1 text-[12px] text-(--text-secondary)">Recent</h3>
            <div className="flex gap-1 overflow-x-auto pb-1">
              {recent.map((gif) => (
                <RecentGif
                  key={gif.id}
                  stickers={stickers}
                  id={gif.id}
                  busy={busy.has(gif.id)}
                  onClick={() => send(gif.id, { kind: "recent-gif", id: gif.id })}
                />
              ))}
            </div>
          </section>
        ) : null}
        {available === false ? (
          <p className="px-1 py-4 text-[13px] text-(--text-secondary)">
            {recent.length ? NO_KEY : `GIFs you send show here. ${NO_KEY}`}
          </p>
        ) : available ? (
          <section>
            <h3 className="px-1 pt-2 pb-1 text-[12px] text-(--text-secondary)">{query.trim() ? "Results" : "Trending"}</h3>
            {results && results.items.length === 0 && !loading ? (
              <p className="px-1 py-2 text-[13px] text-(--text-secondary)">No GIFs found.</p>
            ) : null}
            <div className="flex gap-1">
              {cols.map((col, index) => (
                <div key={index} className="flex min-w-0 flex-1 flex-col gap-1">
                  {col.map((gif) => (
                    <button
                      key={gif.slug}
                      type="button"
                      title={gif.title}
                      aria-label={gif.title ? `Send GIF ${gif.title}` : "Send GIF"}
                      disabled={busy.has(gif.slug)}
                      onClick={() => send(gif.slug, { kind: "gif", pick: gif.send })}
                      className="relative block w-full overflow-hidden rounded-lg bg-(--hover-bg) disabled:cursor-wait"
                      style={{ aspectRatio: gif.width && gif.height ? `${gif.width} / ${gif.height}` : "1" }}
                    >
                      <img src={gif.preview} alt="" loading="lazy" draggable={false} className="h-full w-full object-cover" />
                      {busy.has(gif.slug) ? (
                        <span className="absolute inset-0 flex items-center justify-center bg-black/30">
                          <Spinner />
                        </span>
                      ) : null}
                    </button>
                  ))}
                </div>
              ))}
            </div>
            <div ref={end} className="h-px" />
            {loading ? <p className="px-1 py-2 text-center text-[12px] text-(--text-secondary)">Loading…</p> : null}
          </section>
        ) : null}
      </div>
      {available ? (
        <p className="shrink-0 border-t border-(--divider) px-3 py-1 text-right text-[11px] text-(--text-secondary)">Powered by KLIPY</p>
      ) : null}
    </>
  );
}
