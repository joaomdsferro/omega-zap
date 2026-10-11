import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import type { Contact, GroupInfo as Group, GroupMember, MemberChange } from "../backend";
import { Formatted } from "../lib/markup";
import { fold } from "../lib/search";
import { AddMembersDialog } from "./AddMembersDialog";
import type { LoadMedia } from "./Attachments";
import { Avatar } from "./Avatar";
import { DangerRow, MuteSwitch, Row, Section, Thumb } from "./ContactInfo";
import { openLink } from "./LinkCard";
import { AddPersonIcon, BackIcon, ChevronDownIcon, EditIcon, ExitIcon, FileIcon, SearchIcon, StarIcon, TrashIcon } from "./Icons";
import { PopupMenu } from "./PopupMenu";

/** WhatsApp's limits on a group's name and description. */
const NAME_MAX = 100;
const DESCRIPTION_MAX = 2048;

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
  /** Renames the group or changes its description. */
  onEdit?: (changes: { name?: string; description?: string }) => Promise<Group>;
  /** Adds, removes, promotes or demotes members; removing asks first and fails when cancelled. */
  onChangeMembers?: (change: MemberChange, members: GroupMember[]) => Promise<Group>;
  /** Contacts that can be added. */
  loadContacts?: () => Promise<Contact[]>;
  /** Asks to leave the group; settles once it is done, failing when cancelled. */
  onExit?: () => Promise<void>;
  /** Asks to delete the group's chat, once we are out of it. */
  onDelete?: () => void;
  onError?: (message: string) => void;
  /** Changes when the group changed elsewhere: it is read again. */
  version?: number;
}

/** A group's info, as WhatsApp shows it beside the chat. */
export function GroupInfo({ load, loadDetails, loadMedia, onClose, onJump, onOpenPerson, onStarred, onMute, onEdit, onChangeMembers, loadContacts, onExit, onDelete, onError, version }: GroupInfoProps) {
  const [group, setGroup] = useState<Group | null>(null);
  /** The network has answered, or failed to. */
  const [fresh, setFresh] = useState(!loadDetails);
  const [allMembers, setAllMembers] = useState(false);
  const [query, setQuery] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  /** A member's admin menu, open at a point. */
  const [memberMenu, setMemberMenu] = useState<{ member: GroupMember; x: number; y: number } | null>(null);

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

  const firstVersion = useRef(version);
  useEffect(() => {
    if (version === firstVersion.current) return;
    let alive = true;
    load()
      .then((loaded) => alive && setGroup(loaded))
      .catch(console.error);
    return () => {
      alive = false;
    };
  }, [version, load]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // A menu or dialog over the panel takes its own Escape.
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  async function toggleMute() {
    if (!group || !onMute) return;
    const muted = !group.muted;
    setGroup({ ...group, muted });
    await onMute(muted).catch(() => setGroup({ ...group, muted: !muted }));
  }

  const fail = (e: unknown) => {
    if (e instanceof Error && e.message === "cancelled") return;
    onError?.(String(e instanceof Error ? e.message : e));
  };

  async function edit(changes: { name?: string; description?: string }) {
    if (!onEdit) return;
    setGroup(await onEdit(changes));
  }

  function changeMembers(change: MemberChange, members: GroupMember[]) {
    if (!onChangeMembers) return Promise.resolve();
    return onChangeMembers(change, members).then(setGroup, (e) => {
      // Some may have changed before one failed: what the network says now.
      (loadDetails ?? load)().then(setGroup, () => {});
      throw e;
    });
  }

  const canEdit = Boolean(onEdit && group && !group.left && (group.isAdmin || !group.infoLocked));
  const canManage = Boolean(onChangeMembers && group?.isAdmin && !group.left);
  const memberIds = useMemo(() => new Set(group?.participants.map((m) => m.id) ?? []), [group]);

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
            <EditableText
              value={group.name}
              label="Group name"
              max={NAME_MAX}
              onSave={canEdit ? (name) => edit({ name }) : undefined}
              onError={fail}
              className="mt-4 text-[22px] leading-7 text-(--text-primary)"
            />
            <p className="mt-1 text-[15px] text-(--text-secondary)">
              Group{count ? ` · ${count} ${count === 1 ? "member" : "members"}` : ""}
            </p>
          </section>

          {group.description || group.createdAt || canEdit ? (
            <Section>
              {group.description || canEdit ? (
                <>
                  <h4 className="px-3 text-[14px] text-(--text-secondary)">Description</h4>
                  <EditableText
                    value={group.description ?? ""}
                    label="Description"
                    placeholder="Add group description"
                    max={DESCRIPTION_MAX}
                    multiline
                    onSave={canEdit ? (description) => edit({ description }) : undefined}
                    onError={fail}
                    className="mt-1 text-[15px] text-(--text-primary)"
                  >
                    {group.description ? <Formatted text={group.description} emojiSize={18} onLink={(event, url) => openLink(event, url)} /> : null}
                  </EditableText>
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
            {canManage && loadContacts && query === null ? (
              <Row icon={<AddPersonIcon size={20} />} label="Add member" onClick={() => setAdding(true)} />
            ) : null}
            {group.participants.length === 0 ? (
              <p className="px-3 py-2 text-[14px] text-(--text-secondary)">{fresh ? "Members could not be looked up." : "Looking up members…"}</p>
            ) : (
              <ul className="mt-1">
                {shown.map((member) => (
                  <MemberRow
                    key={member.id}
                    member={member}
                    onOpen={onOpenPerson && (() => onOpenPerson(member.id))}
                    menuSpace={canManage}
                    onMenu={canManage && !member.isMe ? (x, y) => setMemberMenu({ member, x, y }) : undefined}
                  />
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

          {group.left ? (
            <>
              <p className="px-8 pt-4 text-center text-[13px] text-(--text-secondary)">You're no longer a member of this group.</p>
              {onDelete ? (
                <Section>
                  <DangerRow icon={<TrashIcon size={20} />} label="Delete group" onClick={onDelete} />
                </Section>
              ) : null}
            </>
          ) : onExit ? (
            <Section>
              <DangerRow
                icon={<ExitIcon size={20} />}
                label="Exit group"
                onClick={() =>
                  onExit().then(
                    () => load().then(setGroup, () => setGroup((now) => now && { ...now, left: true, isAdmin: false })),
                    fail,
                  )
                }
              />
            </Section>
          ) : null}
        </div>
      ) : null}

      {memberMenu ? (
        <PopupMenu
          x={memberMenu.x}
          y={memberMenu.y}
          label={`Options for ${memberMenu.member.name}`}
          onClose={() => setMemberMenu(null)}
          items={[
            ...(onOpenPerson ? [{ label: `Contact info`, onSelect: () => onOpenPerson(memberMenu.member.id) }] : []),
            memberMenu.member.admin
              ? { label: "Dismiss as admin", onSelect: () => void changeMembers("demote", [memberMenu.member]).catch(fail) }
              : { label: "Make group admin", onSelect: () => void changeMembers("promote", [memberMenu.member]).catch(fail) },
            { label: `Remove ${memberMenu.member.name}`, danger: true, onSelect: () => void changeMembers("remove", [memberMenu.member]).catch(fail) },
          ]}
        />
      ) : null}

      {adding && loadContacts ? (
        <AddMembersDialog
          loadContacts={loadContacts}
          members={memberIds}
          onAdd={(people) =>
            changeMembers(
              "add",
              people.map((p) => ({ id: p.id, name: p.name, avatarHue: p.avatarHue, initials: p.initials, isMe: false, admin: false })),
            )
          }
          onClose={() => setAdding(false)}
        />
      ) : null}
    </aside>
  );
}

interface EditableTextProps {
  value: string;
  label: string;
  placeholder?: string;
  max: number;
  multiline?: boolean;
  /** Saves the new text; absent where it can't be changed. */
  onSave?: (text: string) => Promise<void>;
  onError: (e: unknown) => void;
  className: string;
  /** How the value shows when not editing; the plain text otherwise. */
  children?: ReactNode;
}

/** A group's name or description, with a pencil to change it, as WhatsApp has. */
function EditableText({ value, label, placeholder, max, multiline, onSave, onError, className, children }: EditableTextProps) {
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const field = useRef<HTMLTextAreaElement & HTMLInputElement>(null);

  // On opening only: typing moves the caret itself.
  const editing = draft !== null;
  useEffect(() => {
    const el = field.current;
    if (!editing || !el) return;
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
  }, [editing]);

  async function save() {
    if (draft === null || !onSave) return;
    const text = draft.trim();
    if (text === value.trim() || (!multiline && !text)) {
      setDraft(null);
      return;
    }
    setBusy(true);
    try {
      await onSave(text);
      setDraft(null);
    } catch (e) {
      onError(e);
    } finally {
      setBusy(false);
    }
  }

  function onKeyDown(event: ReactKeyboardEvent) {
    if (event.key === "Escape") {
      // Closes the editing, not the panel.
      event.stopPropagation();
      event.preventDefault();
      setDraft(null);
    } else if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void save();
    }
  }

  if (draft !== null) {
    const Field = multiline ? "textarea" : "input";
    return (
      <div className={["w-full px-3", multiline ? "" : "mt-4"].join(" ")}>
        <div className="flex items-end gap-2 border-b-2 border-(--accent) pb-1">
          <Field
            ref={field}
            value={draft}
            maxLength={max}
            rows={multiline ? Math.min(8, Math.max(2, draft.split("\n").length)) : undefined}
            disabled={busy}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={onKeyDown}
            onBlur={() => !busy && void save()}
            aria-label={label}
            className={["min-w-0 flex-1 resize-none bg-transparent outline-none", multiline ? "text-[15px]" : "text-[18px]", "text-(--text-primary)"].join(" ")}
          />
          <span className="shrink-0 text-[12px] text-(--text-secondary) tabular-nums">{max - draft.length}</span>
        </div>
      </div>
    );
  }

  const shown = children ?? (value || null);
  return (
    <div className={["group/edit flex w-full items-start gap-2 px-3", multiline ? "" : "justify-center"].join(" ")}>
      {shown ? (
        <p className={[className, "min-w-0 break-words whitespace-pre-wrap", multiline ? "flex-1" : ""].join(" ")}>{shown}</p>
      ) : onSave ? (
        <button type="button" onClick={() => setDraft("")} className="mt-1 flex-1 text-left">
          <span className="text-[15px] text-(--accent)">{placeholder}</span>
        </button>
      ) : null}
      {onSave && shown ? (
        <button
          type="button"
          onClick={() => setDraft(value)}
          aria-label={`Edit ${label.toLowerCase()}`}
          title={`Edit ${label.toLowerCase()}`}
          className={["flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)", multiline ? "" : "mt-4"].join(" ")}
        >
          <EditIcon size={18} />
        </button>
      ) : null}
    </div>
  );
}

function MemberRow({
  member,
  onOpen,
  onMenu,
  menuSpace,
}: {
  member: GroupMember;
  onOpen?: () => void;
  onMenu?: (x: number, y: number) => void;
  /** Rows keep room for the menu button, so badges line up. */
  menuSpace?: boolean;
}) {
  return (
    <li className="group/member relative">
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
        {/* Room for the menu button. */}
        {menuSpace ? <span className="w-7 shrink-0" aria-hidden /> : null}
      </button>
      {onMenu ? (
        <button
          type="button"
          onClick={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            onMenu(rect.right - 200, rect.bottom);
          }}
          aria-label={`Options for ${member.name}`}
          aria-haspopup="menu"
          className="absolute top-1/2 right-2 flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-full text-(--icon) opacity-0 group-hover/member:opacity-100 hover:bg-(--hover-bg) focus-visible:opacity-100"
        >
          <ChevronDownIcon size={18} />
        </button>
      ) : null}
    </li>
  );
}
