import { useEffect, useMemo, useRef, useState, type DragEvent, type ReactNode } from "react";
import type { LibraryInfo, LibrarySticker, NetworkPack, StickerNetwork, StickerPack, StickerShelf } from "../backend";
import { formatFileSize } from "../lib/format";
import { GifVideo, useNear } from "./Attachments";
import { ClockIcon, CopyIcon, EditIcon, GifIcon, PlusIcon, SearchIcon, StarIcon, StickerIcon, TrashIcon } from "./Icons";
import { AddStickers, NameInput, StickerImage, useLibrary, useNetworkPacks, useShelfUrl } from "./MediaPanel";
import { PopupMenu, type MenuEntry } from "./PopupMenu";

interface StickerManagerProps {
  shelf: StickerShelf;
  /** Where the library's packs can also go (Signal), when that account is linked. */
  network?: StickerNetwork;
  onError: (message: string) => void;
  /** Opens the editor, to make a sticker into `packId` (or a new pack), or a GIF. */
  onMake?: (packId: string | null, picture?: Blob, gif?: boolean) => void;
}

/** What the right side shows: a section of the library, the GIFs, or the store. */
type Section = "recent" | "favorites" | "gifs" | "store" | string;

const STICKER_DRAG = "application/x-omega-sticker";

/** Where a pack came from, as its line says. */
function origin(pack: StickerPack): string {
  if (pack.store) return pack.publisher ? `${pack.publisher} · WhatsApp's store` : "WhatsApp's store";
  if (pack.id.startsWith("shared-")) return pack.publisher ? `${pack.publisher} · shared in a chat` : "Shared in a chat";
  return "Made here";
}

function plural(n: number, one: string, many = `${one}s`) {
  return `${n} ${n === 1 ? one : many}`;
}

/**
 * The sticker library on a screen of its own: its sections on the left, the
 * open one large on the right with where it stands on each network.
 */
export function StickerManager({ shelf, network, onError, onMake }: StickerManagerProps) {
  const report = useMemo(() => (e: unknown) => onError(String(e)), [onError]);
  const library = useLibrary(shelf, report);
  const networkPacks = useNetworkPacks(network, report);
  const [section, setSection] = useState<Section>("favorites");
  const [query, setQuery] = useState("");
  const [info, setInfo] = useState<LibraryInfo | null>(null);

  useEffect(() => {
    if (!library || !shelf.info) return;
    let alive = true;
    shelf
      .info()
      .then((i) => alive && setInfo(i))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [library, shelf]);

  const packs = library?.packs ?? [];
  const pack = packs.find((p) => p.id === section);
  // A pack that went away (removed here or on the phone) falls back to Favorites.
  useEffect(() => {
    if (library && !["recent", "favorites", "gifs", "store"].includes(section) && !pack) setSection("favorites");
  }, [library, section, pack]);

  const needle = query.trim().toLowerCase();
  const shownPacks = needle ? packs.filter((p) => p.name.toLowerCase().includes(needle) || p.stickers.some((s) => s.emojis.some((e) => e.includes(needle)))) : packs;

  if (!library) {
    return <p className="m-auto text-[14px] text-(--text-secondary)">Loading your stickers…</p>;
  }

  const stickers: LibrarySticker[] = section === "recent" ? library.recent : section === "favorites" ? library.favorites : (pack?.stickers ?? []);
  const filtered = needle ? stickers.filter((s) => s.emojis.some((e) => e.includes(needle))) : stickers;

  return (
    <div className="flex h-full min-w-0 flex-1">
      <aside className="flex w-75 shrink-0 flex-col border-r border-(--divider) bg-(--panel-bg)" aria-label="Sticker library">
        <div className="flex items-center gap-2 px-4 pt-4 pb-2">
          <h1 className="min-w-0 flex-1 truncate font-(family-name:--font-display) text-[22px] font-bold text-(--text-primary)">Stickers</h1>
          {onMake ? (
            <button
              type="button"
              onClick={() => onMake(pack && !pack.store ? pack.id : null, undefined, section === "gifs")}
              className="flex h-8 items-center gap-1 rounded-full bg-(--accent) px-3 text-[13px] font-semibold text-(--on-accent)"
            >
              <PlusIcon size={16} />
              New
            </button>
          ) : null}
        </div>
        <div className="flex gap-1 px-4 pb-2" role="tablist" aria-label="Stickers or GIFs">
          {(["stickers", "gifs"] as const).map((tab) => {
            const on = tab === "gifs" ? section === "gifs" : section !== "gifs";
            return (
              <button
                key={tab}
                type="button"
                role="tab"
                aria-selected={on}
                onClick={() => setSection(tab === "gifs" ? "gifs" : "favorites")}
                className={[
                  "flex h-8 items-center gap-1.5 rounded-full px-3 text-[13px]",
                  on ? "bg-(--chip-active-bg) font-semibold text-(--chip-active-text)" : "text-(--text-secondary) hover:bg-(--hover-bg)",
                ].join(" ")}
              >
                {tab === "gifs" ? <GifIcon size={16} /> : <StickerIcon size={16} />}
                {tab === "gifs" ? "GIFs" : "Stickers"}
              </button>
            );
          })}
        </div>
        <label className="mx-4 mb-2 flex h-9 items-center gap-2 rounded-lg bg-(--search-bg) px-3 text-(--icon)">
          <SearchIcon size={16} />
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search packs or emoji"
            aria-label="Search packs or emoji"
            className="min-w-0 flex-1 bg-transparent text-[14px] text-(--text-primary) outline-none placeholder:text-(--text-secondary)"
          />
        </label>
        <nav className="min-h-0 flex-1 overflow-y-auto pb-2" aria-label="Sections">
          <SectionRow icon={<ClockIcon size={22} />} name="Recent" count={library.recent.length} on={section === "recent"} onClick={() => setSection("recent")} />
          <SectionRow
            icon={<StarIcon size={22} />}
            name="Favorites"
            count={library.favorites.length}
            on={section === "favorites"}
            onClick={() => setSection("favorites")}
            dots={<NetworkDots network={network} state={networkPacks.get("favorites")} />}
          />
          <h2 className="px-4 pt-3 pb-1 text-[12px] text-(--text-secondary)">Packs</h2>
          {shownPacks.map((p) => (
            <PackRow
              key={p.id}
              pack={p}
              shelf={shelf}
              on={section === p.id}
              onClick={() => setSection(p.id)}
              dots={<NetworkDots network={network} state={networkPacks.get(p.id)} />}
              onError={report}
            />
          ))}
          {shownPacks.length === 0 ? <p className="px-4 py-2 text-[13px] text-(--text-secondary)">{needle ? "No pack matches." : "No packs yet."}</p> : null}
          <SectionRow icon={<PlusIcon size={22} />} name="Add packs" on={section === "store"} onClick={() => setSection("store")} />
        </nav>
        {info ? (
          <p className="border-t border-(--divider) px-4 py-3 text-[12px] text-(--text-secondary)" title="Kept on this computer, encrypted, whichever accounts are linked">
            {plural(info.stickers, "sticker")} · {plural(info.gifs, "GIF")} · {formatFileSize(info.bytes)} on this computer
          </p>
        ) : null}
      </aside>

      <section className="flex min-w-0 flex-1 flex-col bg-(--panel-bg)">
        {section === "store" ? (
          <div className="flex min-h-0 flex-1 flex-col">
            <AddStickers stickers={shelf} onPicture={(picture) => onMake?.(null, picture)} onError={report} onBack={() => setSection("favorites")} />
          </div>
        ) : section === "gifs" ? (
          <GifSection shelf={shelf} gifs={library.gifs} onMake={onMake && shelf.makeGif ? () => onMake(null, undefined, true) : undefined} />
        ) : (
          <>
            <PackHeader
              shelf={shelf}
              section={section}
              pack={pack}
              count={stickers.length}
              network={network}
              networkPack={networkPacks.get(section)}
              onError={report}
            />
            <StickerGrid
              shelf={shelf}
              stickers={filtered}
              pack={pack}
              packs={packs}
              empty={
                needle
                  ? "No sticker here goes with that emoji."
                  : section === "recent"
                    ? "Stickers you send, here or on your phone, show here."
                    : section === "favorites"
                      ? "Star a sticker to keep it here and on your phone."
                      : "No stickers yet."
              }
              onAdd={pack && !pack.store && onMake ? () => onMake(pack.id) : undefined}
              onError={report}
            />
          </>
        )}
      </section>
    </div>
  );
}

function SectionRow({ icon, name, count, on, onClick, dots }: { icon: ReactNode; name: string; count?: number; on: boolean; onClick: () => void; dots?: ReactNode }) {
  return (
    <button
      type="button"
      aria-current={on ? "page" : undefined}
      onClick={onClick}
      className={["flex w-full items-center gap-3 px-4 py-2 text-left", on ? "bg-(--selected-bg)" : "hover:bg-(--hover-bg)"].join(" ")}
    >
      <span className="flex h-9 w-9 shrink-0 items-center justify-center text-(--icon)">{icon}</span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[15px] text-(--text-primary)">{name}</span>
        {dots}
      </span>
      {count !== undefined ? <span className="text-[12px] text-(--text-secondary)">{count}</span> : null}
    </button>
  );
}

/** A pack in the list; stickers dragged onto one of ours go into it. */
function PackRow({ pack, shelf, on, onClick, dots, onError }: { pack: StickerPack; shelf: StickerShelf; on: boolean; onClick: () => void; dots: ReactNode; onError: (e: unknown) => void }) {
  const [over, setOver] = useState(false);
  const takes = !pack.store;
  return (
    <div
      onDragOver={(e: DragEvent) => {
        if (!takes || !e.dataTransfer.types.includes(STICKER_DRAG)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e: DragEvent) => {
        setOver(false);
        const id = e.dataTransfer.getData(STICKER_DRAG);
        if (!takes || !id) return;
        e.preventDefault();
        shelf.addToPack(pack.id, { id }).catch(onError);
      }}
      className={over ? "outline-2 -outline-offset-2 outline-(--accent)" : undefined}
    >
      <SectionRow
        icon={
          pack.tray ? (
            <img src={pack.tray} alt="" draggable={false} className="h-9 w-9 object-contain" />
          ) : pack.stickers[0] ? (
            <StickerImage stickers={shelf} id={pack.stickers[0].id} className="h-9 w-9" />
          ) : (
            <StickerIcon size={22} />
          )
        }
        name={pack.name}
        count={pack.stickers.length}
        on={on}
        onClick={onClick}
        dots={dots}
      />
    </div>
  );
}

/** Small dots under a pack's name: green for WhatsApp, Signal's blue when it is there (faint while behind). */
function NetworkDots({ network, state }: { network?: StickerNetwork; state?: NetworkPack }) {
  const signal = network && state?.url ? (state.onNetwork < state.count ? 0.4 : 1) : 0;
  return (
    <span className="mt-0.5 flex gap-1" aria-hidden>
      <span className="h-2 w-2 rounded-full bg-(--brand-whatsapp)" />
      {signal ? <span className="h-2 w-2 rounded-full bg-(--brand-signal)" style={{ opacity: signal }} /> : null}
    </span>
  );
}

function PackHeader({ shelf, section, pack, count, network, networkPack, onError }: {
  shelf: StickerShelf;
  section: Section;
  pack?: StickerPack;
  count: number;
  network?: StickerNetwork;
  networkPack?: NetworkPack;
  onError: (e: unknown) => void;
}) {
  const [renaming, setRenaming] = useState(false);
  const name = pack?.name ?? (section === "recent" ? "Recent" : "Favorites");
  const line = pack
    ? `${plural(count, "sticker")} · ${origin(pack)}`
    : section === "recent"
      ? `${plural(count, "sticker")} you sent lately`
      : `${plural(count, "sticker")} · in sync with your phone`;
  return (
    <header className="shrink-0 border-b border-(--divider) px-6 pt-5 pb-4">
      <div className="flex items-center gap-4">
        <span className="flex h-14 w-14 shrink-0 items-center justify-center text-(--icon)">
          {pack?.tray ? (
            <img src={pack.tray} alt="" draggable={false} className="h-14 w-14 object-contain" />
          ) : pack?.stickers[0] ? (
            <StickerImage stickers={shelf} id={pack.stickers[0].id} className="h-14 w-14" />
          ) : section === "recent" ? (
            <ClockIcon size={32} />
          ) : (
            <StarIcon size={32} />
          )}
        </span>
        <div className="min-w-0 flex-1">
          {renaming && pack ? (
            <NameInput
              initial={pack.name}
              label="Pack name"
              onDone={(next) => {
                setRenaming(false);
                if (next && next !== pack.name) shelf.renamePack(pack.id, next).catch(onError);
              }}
            />
          ) : (
            <h2 className="truncate font-(family-name:--font-display) text-[20px] font-bold text-(--text-primary)">{name}</h2>
          )}
          <p className="truncate text-[13px] text-(--text-secondary)">{line}</p>
        </div>
        {pack && !pack.store ? (
          <HeaderButton label={`Rename ${pack.name}`} onClick={() => setRenaming(true)}>
            <EditIcon size={16} />
          </HeaderButton>
        ) : null}
        {pack ? (
          <HeaderButton
            label={`Remove ${pack.name}`}
            onClick={() => {
              if (pack.store || pack.stickers.length === 0 || window.confirm(`Delete the pack "${pack.name}"? Its stickers leave this computer unless another pack, Favorites or Recent has them.`)) {
                shelf.removePack(pack.id).catch(onError);
              }
            }}
          >
            <TrashIcon size={16} />
          </HeaderButton>
        ) : null}
      </div>
      {section !== "recent" ? (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Chip tone="ok">
            <span className="h-2 w-2 rounded-full bg-(--brand-whatsapp)" />
            WhatsApp · {section === "favorites" ? "synced with your phone" : "in your sticker panel"}
          </Chip>
          {network && networkPack ? <SignalChip network={network} pack={networkPack} onError={onError} /> : null}
          <Chip tone="off">
            <span className="h-2 w-2 rounded-full bg-(--brand-telegram)" />
            Telegram · soon
          </Chip>
        </div>
      ) : null}
    </header>
  );
}

function HeaderButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-(--chip-bg) text-(--icon) hover:text-(--text-primary)"
    >
      {children}
    </button>
  );
}

function Chip({ tone, children }: { tone: "ok" | "warn" | "off" | "plain"; children: ReactNode }) {
  const colors = {
    ok: "bg-(--chip-active-bg) text-(--chip-active-text)",
    warn: "bg-[color-mix(in_oklab,#f0b040_18%,transparent)] text-(--text-primary)",
    off: "bg-(--chip-bg) text-(--text-secondary) opacity-60",
    plain: "bg-(--chip-bg) text-(--text-secondary)",
  }[tone];
  return <span className={`inline-flex h-7 items-center gap-1.5 rounded-full px-3 text-[12px] whitespace-nowrap ${colors}`}>{children}</span>;
}

/** Where a pack stands on Signal, with what can be done about it. */
function SignalChip({ network, pack, onError }: { network: StickerNetwork; pack: NetworkPack; onError: (e: unknown) => void }) {
  const dot = <span className="h-2 w-2 rounded-full bg-(--brand-signal)" />;
  if (pack.uploading) {
    return (
      <Chip tone="plain">
        <span className="block h-3 w-3 animate-spin rounded-full border-2 border-current border-t-transparent" />
        Adding to {network.name} and your phone…
      </Chip>
    );
  }
  const missing = pack.count - pack.onNetwork;
  const upload = () => network.upload(pack.source).catch(onError);
  if (pack.url && missing <= 0) {
    const url = pack.url;
    return (
      <Chip tone="ok">
        {dot}
        {network.name} · on your phone too
        <button type="button" aria-label={`Copy the ${network.name} link`} title={`Copy the ${network.name} link, to share the pack`} onClick={() => void navigator.clipboard.writeText(url).catch(onError)} className="ml-1 flex items-center">
          <CopyIcon size={13} />
        </button>
      </Chip>
    );
  }
  return (
    <Chip tone={pack.url ? "warn" : "plain"}>
      {dot}
      {network.name} · {pack.url ? `${plural(missing, "new sticker")}` : "not there yet; they go as pictures"}
      <button type="button" onClick={upload} className="ml-1 font-semibold text-(--accent)">
        {pack.url ? "Update" : `Add to ${network.name}`}
      </button>
    </Chip>
  );
}

function StickerGrid({ shelf, stickers, pack, packs, empty, onAdd, onError }: {
  shelf: StickerShelf;
  stickers: LibrarySticker[];
  pack?: StickerPack;
  packs: StickerPack[];
  empty: string;
  onAdd?: () => void;
  onError: (e: unknown) => void;
}) {
  const [menu, setMenu] = useState<{ x: number; y: number; sticker: LibrarySticker } | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const reorder = pack && !pack.store && shelf.moveInPack ? shelf.moveInPack.bind(shelf) : undefined;
  const own = packs.filter((p) => !p.store);

  function menuFor(sticker: LibrarySticker): MenuEntry[] {
    const items: MenuEntry[] = [
      {
        label: sticker.favorite ? "Remove from favorites" : "Add to favorites",
        icon: <StarIcon size={20} filled={sticker.favorite} />,
        onSelect: () => shelf.setFavorite({ id: sticker.id }, !sticker.favorite).catch(onError),
      },
    ];
    const targets = own.filter((p) => p.id !== pack?.id);
    if (targets.length) {
      items.push("separator", { heading: "Add to pack" });
      for (const p of targets) items.push({ label: p.name, onSelect: () => shelf.addToPack(p.id, { id: sticker.id }).catch(onError) });
    }
    if (pack && !pack.store) {
      items.push("separator", { label: "Remove from this pack", icon: <TrashIcon size={20} />, danger: true, onSelect: () => shelf.removeFromPack(pack.id, sticker.id).catch(onError) });
    }
    return items;
  }

  if (!stickers.length && !onAdd) {
    return <p className="m-auto max-w-80 text-center text-[14px] text-(--text-secondary)">{empty}</p>;
  }
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
      <div className="grid grid-cols-[repeat(auto-fill,minmax(96px,1fr))] gap-1">
        {stickers.map((sticker, index) => (
          <div
            key={sticker.id}
            draggable
            onDragStart={(e) => {
              e.dataTransfer.setData(STICKER_DRAG, sticker.id);
              e.dataTransfer.effectAllowed = "copyMove";
              setDragging(sticker.id);
            }}
            onDragEnd={() => setDragging(null)}
            onDragOver={(e) => {
              if (reorder && dragging && dragging !== sticker.id) e.preventDefault();
            }}
            onDrop={(e) => {
              const id = e.dataTransfer.getData(STICKER_DRAG);
              if (!reorder || !pack || !id || id === sticker.id) return;
              e.preventDefault();
              reorder(pack.id, id, index).catch(onError);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu({ x: e.clientX, y: e.clientY, sticker });
            }}
            className={["group/tile relative aspect-square rounded-xl p-2 hover:bg-(--hover-bg)", dragging === sticker.id ? "opacity-40" : ""].join(" ")}
            title={sticker.emojis.join(" ") || undefined}
          >
            <StickerImage stickers={shelf} id={sticker.id} className="h-full w-full" />
            {sticker.animated ? <span className="absolute bottom-1 left-1 rounded bg-black/50 px-1 text-[10px] text-white">GIF</span> : null}
            <button
              type="button"
              aria-label={`Options for this sticker`}
              onClick={(e) => setMenu({ x: e.clientX, y: e.clientY, sticker })}
              className="absolute top-1 right-1 flex h-6 w-6 items-center justify-center rounded-full bg-(--panel-bg) text-(--icon) opacity-0 shadow-sm group-hover/tile:opacity-100 focus-visible:opacity-100"
            >
              <span className="text-[14px] leading-none">⋯</span>
            </button>
            {sticker.favorite ? <StarIcon size={12} filled className="absolute top-1.5 left-1.5 text-(--accent)" /> : null}
          </div>
        ))}
        {onAdd ? (
          <button
            type="button"
            onClick={onAdd}
            className="flex aspect-square flex-col items-center justify-center gap-1 rounded-xl border-[1.5px] border-dashed border-(--divider) text-[12px] text-(--text-secondary) hover:border-(--accent) hover:text-(--accent)"
          >
            <PlusIcon size={22} />
            Add
          </button>
        ) : null}
      </div>
      {!stickers.length ? <p className="pt-4 text-[14px] text-(--text-secondary)">{empty}</p> : null}
      {reorder && stickers.length > 1 ? <p className="pt-4 text-[12px] text-(--text-secondary)">Drag stickers to reorder them, or onto a pack on the left to add them there.</p> : null}
      {menu ? <PopupMenu x={menu.x} y={menu.y} label="Sticker options" items={menuFor(menu.sticker)} onClose={() => setMenu(null)} /> : null}
    </div>
  );
}

function GifSection({ shelf, gifs, onMake }: { shelf: StickerShelf; gifs: { id: string; width: number; height: number }[]; onMake?: () => void }) {
  return (
    <>
      <header className="flex shrink-0 items-start gap-4 border-b border-(--divider) px-6 pt-5 pb-4">
        <div className="min-w-0 flex-1">
          <h2 className="font-(family-name:--font-display) text-[20px] font-bold text-(--text-primary)">GIFs</h2>
          <p className="text-[13px] text-(--text-secondary)">The {plural(gifs.length, "GIF")} you sent or made lately, kept here to send again from any chat.</p>
        </div>
        {onMake ? (
          <button type="button" onClick={onMake} className="flex h-8 shrink-0 items-center gap-1 rounded-full bg-(--chip-bg) px-3 text-[13px] text-(--text-primary) hover:bg-(--hover-bg)">
            <PlusIcon size={16} />
            Make a GIF
          </button>
        ) : null}
      </header>
      {gifs.length ? (
        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
          <div className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-2">
            {gifs.map((gif) => (
              <GifTile key={gif.id} shelf={shelf} id={gif.id} />
            ))}
          </div>
        </div>
      ) : (
        <p className="m-auto max-w-80 text-center text-[14px] text-(--text-secondary)">GIFs you send or make show here.</p>
      )}
    </>
  );
}

function GifTile({ shelf, id }: { shelf: StickerShelf; id: string }) {
  const box = useRef<HTMLDivElement>(null);
  const url = useShelfUrl(shelf, id, useNear(box, "200px 0px"), true);
  return (
    <div ref={box} className="aspect-video overflow-hidden rounded-xl bg-(--hover-bg)">
      {url && url !== "failed" ? <GifVideo src={url} wrapperClassName="h-full w-full" className="h-full w-full object-cover" /> : null}
    </div>
  );
}

