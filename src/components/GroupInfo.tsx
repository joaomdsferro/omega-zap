import { useEffect, useMemo, useState } from "react";
import type { GroupInfo as Group, GroupMember } from "../backend";
import { Formatted } from "../lib/markup";
import { fold } from "../lib/search";
import type { LoadMedia } from "./Attachments";
import { Avatar } from "./Avatar";
import { MuteSwitch, Row, Section, Thumb } from "./ContactInfo";
import { openLink } from "./LinkCard";
import { BackIcon, FileIcon, SearchIcon, StarIcon } from "./Icons";

/** Members listed before "View all", as WhatsApp does. */
const FIRST_MEMBERS = 10;

interface GroupInfoProps {
  load: () => Promise<Group>;
  /** Members and their roles, description and full-size picture, from the network. */
  loadDetails?: () => Promise<Group>;
  loadMedia?: LoadMedia;
  onClose: () => void;
  /** Goes to one of the group's messages. */
  onJump?: (messageId: string) => void;
  /** Opens a member's contact info. */
  onOpenPerson?: (id: string) => void;
  onStarred?: () => void;
  onMute?: (muted: boolean) => Promise<void>;
}

/** A group's info, as WhatsApp shows it beside the chat. */
export function GroupInfo({ load, loadDetails, loadMedia, onClose, onJump, onOpenPerson, onStarred, onMute }: GroupInfoProps) {
  const [group, setGroup] = useState<Group | null>(null);
  /** The network has answered, or failed to. */
  const [fresh, setFresh] = useState(!loadDetails);
  const [allMembers, setAllMembers] = useState(false);
  const [query, setQuery] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    load()
      .then((loaded) => alive && setGroup((prev) => prev ?? loaded))
      .catch(console.error);
    loadDetails?.()
      .then((loaded) => alive && setGroup(loaded))
      .catch(console.error)
      .finally(() => alive && setFresh(true));
    return () => {
      alive = false;
    };
  }, [load, loadDetails]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => event.key === "Escape" && onClose();
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  async function toggleMute() {
    if (!group || !onMute) return;
    const muted = !group.muted;
    setGroup({ ...group, muted });
    await onMute(muted).catch(() => setGroup({ ...group, muted: !muted }));
  }

  const found = useMemo(() => {
    const members = group?.participants ?? [];
    const q = fold(query?.trim() ?? "");
    if (!q) return members;
    return members.filter((m) => fold(m.name).includes(q) || (m.phone && m.phone.replace(/\D/g, "").includes(q.replace(/\D/g, "") || "\0")));
  }, [group, query]);
  const shown = allMembers || query ? found : found.slice(0, FIRST_MEMBERS);
  const count = group?.members ?? group?.participants.length ?? 0;

  return (
    <aside
      className="absolute inset-y-0 right-0 z-40 flex w-[min(420px,100%)] flex-col border-l border-(--divider) bg-(--panel-bg)"
      aria-label="Group info"
    >
      <header className="flex h-15 shrink-0 items-center gap-4 bg-(--panel-header-bg) px-3">
        <button
          type="button"
          onClick={onClose}
          aria-label="Back"
          className="flex h-10 w-10 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
        >
          <BackIcon size={22} />
        </button>
        <h2 className="text-[16px] text-(--text-primary)">Group info</h2>
      </header>
      {group ? (
        <div className="flex-1 overflow-y-auto">
          <section className="flex flex-col items-center px-6 pt-7 pb-6 text-center">
            <Avatar name={group.name} initials={group.initials} hue={group.avatarHue} src={group.avatarUrl} size={200} group />
            <h3 className="mt-4 text-[22px] leading-7 break-words text-(--text-primary)">{group.name}</h3>
            <p className="mt-1 text-[15px] text-(--text-secondary)">
              Group{count ? ` · ${count} ${count === 1 ? "member" : "members"}` : ""}
            </p>
          </section>

          {group.description || group.createdAt ? (
            <Section>
              {group.description ? (
                <>
                  <h4 className="px-3 text-[14px] text-(--text-secondary)">Description</h4>
                  <p className="mt-1 px-3 text-[15px] break-words whitespace-pre-wrap text-(--text-primary)">
                    <Formatted text={group.description} emojiSize={18} onLink={(event, url) => openLink(event, url)} />
                  </p>
                </>
              ) : null}
              {group.createdAt ? (
                <p className={["px-3 text-[13px] text-(--text-secondary)", group.description ? "mt-3" : ""].join(" ")}>
                  {group.createdBy ? `Created by ${group.createdBy}, ` : "Created "}
                  {new Date(group.createdAt).toLocaleDateString(undefined, { day: "numeric", month: "long", year: "numeric" })}
                </p>
              ) : null}
            </Section>
          ) : null}

          {group.mediaCount > 0 ? (
            <Section>
              <Row
                icon={<FileIcon size={20} />}
                label="Media, links and docs"
                value={String(group.mediaCount)}
                onClick={onJump && group.recentMedia[0] ? () => onJump(group.recentMedia[0].id) : undefined}
              />
              {group.recentMedia.length > 0 ? (
                <div className="mt-2 grid grid-cols-3 gap-2 px-2">
                  {group.recentMedia.map((message) => (
                    <Thumb key={message.id} message={message} loadMedia={loadMedia} onOpen={onJump && (() => onJump(message.id))} />
                  ))}
                </div>
              ) : null}
            </Section>
          ) : null}

          <Section>
            {group.starred > 0 ? (
              <Row icon={<StarIcon size={20} />} label="Starred messages" value={String(group.starred)} onClick={onStarred} />
            ) : null}
            {onMute ? <MuteSwitch muted={group.muted} onToggle={toggleMute} /> : null}
          </Section>

          <Section>
            <div className="flex h-10 items-center gap-2 px-3">
              {query === null ? (
                <>
                  <h4 className="flex-1 text-[14px] text-(--text-secondary)">
                    {count ? `${count} ${count === 1 ? "member" : "members"}` : "Members"}
                  </h4>
                  {group.participants.length > FIRST_MEMBERS ? (
                    <button
                      type="button"
                      onClick={() => setQuery("")}
                      aria-label="Search members"
                      className="flex h-9 w-9 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
                    >
                      <SearchIcon size={18} />
                    </button>
                  ) : null}
                </>
              ) : (
                <input
                  autoFocus
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key !== "Escape") return;
                    // Closes the search, not the panel.
                    event.stopPropagation();
                    setQuery(null);
                  }}
                  onBlur={() => !query && setQuery(null)}
                  placeholder="Search members"
                  aria-label="Search members"
                  className="h-9 flex-1 rounded-lg bg-(--search-bg,var(--hover-bg)) px-3 text-[14px] text-(--text-primary) outline-none placeholder:text-(--text-secondary)"
                />
              )}
            </div>
            {group.participants.length === 0 ? (
              <p className="px-3 py-2 text-[14px] text-(--text-secondary)">{fresh ? "Members could not be looked up." : "Looking up members…"}</p>
            ) : (
              <ul className="mt-1">
                {shown.map((member) => (
                  <MemberRow key={member.id} member={member} onOpen={onOpenPerson && (() => onOpenPerson(member.id))} />
                ))}
              </ul>
            )}
            {query && found.length === 0 ? <p className="px-3 py-2 text-[14px] text-(--text-secondary)">No members found</p> : null}
            {!allMembers && !query && found.length > FIRST_MEMBERS ? (
              <button
                type="button"
                onClick={() => setAllMembers(true)}
                className="w-full rounded-lg px-3 py-3 text-left text-[15px] text-(--accent) hover:bg-(--hover-bg)"
              >
                View all ({found.length - FIRST_MEMBERS} more)
              </button>
            ) : null}
          </Section>
        </div>
      ) : null}
    </aside>
  );
}

function MemberRow({ member, onOpen }: { member: GroupMember; onOpen?: () => void }) {
  return (
    <li>
      <button
        type="button"
        onClick={onOpen}
        disabled={!onOpen}
        className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-(--hover-bg) disabled:hover:bg-transparent"
      >
        <Avatar name={member.name} initials={member.initials} hue={member.avatarHue} src={member.avatarUrl} size={40} />
        <span className="min-w-0 flex-1">
          <span className="flex items-baseline gap-2">
            <span className="truncate text-[15px] text-(--text-primary)">{member.isMe ? "You" : member.phone && member.phone !== member.name ? `~${member.name}` : member.name}</span>
            {member.phone && member.phone !== member.name ? (
              <span className="ml-auto shrink-0 text-[13px] text-(--text-secondary)">{member.phone}</span>
            ) : null}
          </span>
          {member.about ? <span className="block truncate text-[13px] text-(--text-secondary)">{member.about}</span> : null}
        </span>
        {member.admin ? (
          <span className="shrink-0 rounded-md bg-(--accent)/15 px-1.5 py-0.5 text-[11.5px] text-(--accent)">Group admin</span>
        ) : null}
      </button>
    </li>
  );
}
