import { useState } from "react";
import type { ContactCard, Place, SharedStickerPack } from "../backend";
import { hueFor, initialsFor } from "../lib/avatar";
import { Avatar } from "./Avatar";
import { openExternal } from "./LinkCard";
import { CheckIcon, MapPinIcon, StickerIcon } from "./Icons";

/** A sticker pack shared in the chat, to add to the library (and from there to Signal). */
export function StickerPackCard({ pack, onAdd }: { pack: SharedStickerPack; onAdd?: () => Promise<void> }) {
  const [state, setState] = useState<"idle" | "adding" | "added">("idle");
  return (
    <div className="mb-1 flex w-70 max-w-full items-center gap-3 rounded-md bg-(--quote-bg) px-3 py-2.5">
      <StickerIcon size={30} className="shrink-0 text-(--icon)" />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-[14px] text-(--text-primary)">{pack.name}</span>
        <span className="truncate text-[12px] text-(--text-secondary)">
          {[pack.publisher, `${pack.count} sticker${pack.count === 1 ? "" : "s"}`].filter(Boolean).join(" · ")}
        </span>
      </span>
      {onAdd && pack.count > 0 ? (
        state === "added" ? (
          <span className="flex shrink-0 items-center gap-0.5 text-[12px] text-(--accent)">
            <CheckIcon size={14} />
            Added
          </span>
        ) : (
          <button
            type="button"
            disabled={state === "adding"}
            onClick={() => {
              setState("adding");
              onAdd().then(
                () => setState("added"),
                () => setState("idle"),
              );
            }}
            className="h-7 shrink-0 rounded-full bg-(--accent) px-3 text-[12px] font-medium text-white disabled:cursor-wait disabled:opacity-70"
          >
            {state === "adding" ? "Adding…" : "Add to stickers"}
          </button>
        )
      ) : null}
    </div>
  );
}

/** Where a place opens: the page the sender picked, or a map at its spot. */
export function placeUrl(place: Place): string {
  return place.url ?? `https://maps.google.com/maps?q=${place.lat},${place.lng}`;
}

/** A shared place: its small map, then its name and address. Opens the map. */
export function PlaceCard({ place }: { place: Place }) {
  const title = place.name ?? (place.live ? "Live location" : "Location");
  return (
    <button
      type="button"
      onClick={() => openExternal(placeUrl(place))}
      aria-label={`Open ${title} in maps`}
      className="mb-1 block w-70 max-w-full overflow-hidden rounded-md bg-(--quote-bg) text-left"
    >
      <div className="relative h-36 w-full overflow-hidden">
        {place.thumbnail ? (
          <img src={place.thumbnail} alt="" className="h-full w-full object-cover" draggable={false} />
        ) : (
          <div className="h-full w-full bg-[color-mix(in_oklab,var(--accent)_14%,var(--panel-bg))]" />
        )}
        {/* The thumbnail's centre is the spot; without one, the pin says what this is. */}
        <span className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-full text-(--danger) drop-shadow">
          <MapPinIcon size={30} fill="currentColor" stroke="white" strokeWidth={1.2} />
        </span>
      </div>
      {place.name || place.address || place.live ? (
        <div className="px-2.5 py-1.5">
          {place.live ? <p className="text-[12.5px] text-(--accent)">Live location</p> : null}
          {place.name ? <p className="truncate text-[14px] text-(--text-primary)">{place.name}</p> : null}
          {place.address ? <p className="line-clamp-2 text-[12.5px] text-(--text-secondary)">{place.address}</p> : null}
        </div>
      ) : null}
    </button>
  );
}

interface ContactCardsProps {
  cards: ContactCard[];
  /** Opens a chat with the number, digits only. */
  onMessage?: (waId: string, name: string) => void;
}

/** Shared contact cards: each name with its number, and "Message" when it has WhatsApp. */
export function ContactCards({ cards, onMessage }: ContactCardsProps) {
  return (
    <div className="mb-1 w-70 max-w-full overflow-hidden rounded-md bg-(--quote-bg)">
      {cards.map((card, index) => {
        const waId = card.phones?.find((p) => p.waId)?.waId;
        const number = card.phones?.[0]?.number;
        return (
          <div key={index} className={index > 0 ? "border-t border-(--divider)" : undefined}>
            <div className="flex items-center gap-3 px-3 py-2.5">
              <Avatar name={card.name} initials={initialsFor(card.name)} hue={hueFor(card.name)} size={40} />
              <div className="min-w-0">
                <p className="truncate text-[14.5px] text-(--text-primary)">{card.name}</p>
                {number ? <p className="truncate text-[12.5px] text-(--text-secondary)">{number}</p> : null}
              </div>
            </div>
            {waId && onMessage ? (
              <button
                type="button"
                onClick={() => onMessage(waId, card.name)}
                className="w-full border-t border-(--divider) py-2 text-center hover:bg-(--hover-bg)"
              >
                <span className="text-[14px] text-(--accent)">Message</span>
              </button>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
