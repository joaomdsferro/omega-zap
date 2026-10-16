import { useId } from "react";
import type { SVGProps } from "react";
import type { AccountId } from "../accounts/accounts";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function Svg({ size = 24, children, ...rest }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      {...rest}
    >
      {children}
    </svg>
  );
}

export const ChatsIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M20 11.5a8 8 0 0 1-11.7 7.1L4 20l1.4-4.2A8 8 0 1 1 20 11.5z" />
  </Svg>
);

export const WhatsAppIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M20 11.5a8 8 0 0 1-11.7 7.1L4 20l1.4-4.2A8 8 0 1 1 20 11.5z" />
    <path
      d="M9.4 8.3c-.5 1.3.3 3 1.6 4.3s3 2.1 4.3 1.6l.5-1.3-1.7-.9-.8.7c-.8-.4-1.5-1.1-1.9-1.9l.7-.8-.9-1.7z"
      fill="currentColor"
      stroke="none"
    />
  </Svg>
);

export const SignalIcon = (p: IconProps) => (
  <Svg {...p}>
    <path
      d="M21 11.5a9 9 0 0 1-13.2 8L3 21l1.5-4.8A9 9 0 1 1 21 11.5z"
      strokeWidth={1.5}
      strokeDasharray="2.6 2"
    />
    <path
      d="M17.5 11.6a5.5 5.5 0 0 1-8.1 4.8l-2.9.9.9-2.9a5.5 5.5 0 1 1 10.1-2.8z"
      fill="currentColor"
      stroke="none"
    />
  </Svg>
);

export const SettingsIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
  </Svg>
);

export const SearchIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="11" cy="11" r="6.5" />
    <path d="m16.5 16.5 4 4" />
  </Svg>
);

export const BackIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M19 12H5M11 18l-6-6 6-6" />
  </Svg>
);

export const ChevronLeftIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="m15 6-6 6 6 6" />
  </Svg>
);

export const ChevronRightIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="m9 6 6 6-6 6" />
  </Svg>
);

export const CloseIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M6 6l12 12M18 6 6 18" />
  </Svg>
);

export const PlusIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 5v14M5 12h14" />
  </Svg>
);

export const MoreIcon = (p: IconProps) => (
  <Svg {...p} fill="currentColor" stroke="none">
    <circle cx="6" cy="12" r="1.7" />
    <circle cx="12" cy="12" r="1.7" />
    <circle cx="18" cy="12" r="1.7" />
  </Svg>
);

export const StarIcon = ({ filled, ...p }: IconProps & { filled?: boolean }) => (
  <Svg {...p} fill={filled ? "currentColor" : "none"}>
    <path d="m12 3.5 2.6 5.3 5.9.9-4.3 4.1 1 5.8-5.2-2.7-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z" />
  </Svg>
);

export const ChevronDownIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="m6 9 6 6 6-6" />
  </Svg>
);

export const CheckIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="m5 12.5 4.5 4.5L19 7.5" />
  </Svg>
);

export const SelectIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="4" y="4" width="16" height="16" rx="3" />
    <path d="m8.5 12.2 2.4 2.4 4.6-4.8" />
  </Svg>
);

export const EditIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16zM13.5 6.5l4 4" />
  </Svg>
);

export const PhoneIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M6.6 3.5h2.6l1.4 4-2 1.3a11 11 0 0 0 6.6 6.6l1.3-2 4 1.4v2.6a2 2 0 0 1-2.2 2A16.5 16.5 0 0 1 4.6 5.7a2 2 0 0 1 2-2.2z" />
  </Svg>
);

export const VideoIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="6.5" width="12.5" height="11" rx="2.2" />
    <path d="m15.5 10.5 5-3v9l-5-3z" />
  </Svg>
);

/** A speech bubble with a dot: a chat with something unread. */
export const UnreadIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M20 12.5a8 8 0 1 1-3.4-6.55" />
    <circle cx="19" cy="5" r="2.2" fill="currentColor" stroke="none" />
  </Svg>
);

export const NewChatIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M20 11.5a8 8 0 0 1-11.7 7.1L4 20l1.4-4.2A8 8 0 1 1 20 11.5z" />
    <path d="M12 8v7M8.5 11.5h7" />
  </Svg>
);

export const SmileIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="M8 14a4.5 4.5 0 0 0 8 0" />
    <path d="M9 9.5v.5M15 9.5v.5" strokeWidth={2.4} />
  </Svg>
);

export const SendIcon =(p: IconProps) => (
  <Svg {...p} fill="currentColor" stroke="none">
    <path d="M3.4 20.4 21 12 3.4 3.6 3.4 10l12.6 2-12.6 2z" />
  </Svg>
);

export const MicIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="9" y="3" width="6" height="11" rx="3" />
    <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
  </Svg>
);

export const PlayIcon = (p: IconProps) => (
  <Svg {...p} fill="currentColor" stroke="none">
    <path d="M8 5.5v13l11-6.5z" />
  </Svg>
);

export const PauseIcon = (p: IconProps) => (
  <Svg {...p} fill="currentColor" stroke="none">
    <path d="M7 5h3.5v14H7zM13.5 5H17v14h-3.5z" />
  </Svg>
);

export const PinIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M9 4h6l-1 6 3 3H7l3-3zM12 13v7" />
  </Svg>
);

/** A map marker, filled with `fill` when given. */
export const MapPinIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 22s-7-6.1-7-12a7 7 0 0 1 14 0c0 5.9-7 12-7 12z" />
    <circle cx="12" cy="10" r="2.6" fill="white" stroke="none" />
  </Svg>
);

export const ArchiveIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="4" width="18" height="4" rx="1" />
    <path d="M5 8v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8M12 11v5M9.5 13.5 12 16l2.5-2.5" />
  </Svg>
);

export const UnarchiveIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="4" width="18" height="4" rx="1" />
    <path d="M5 8v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8M12 16v-5M9.5 13.5 12 11l2.5 2.5" />
  </Svg>
);

export const ReplyIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M9 7 4 12l5 5" />
    <path d="M4 12h10a6 6 0 0 1 6 6v1" />
  </Svg>
);

export const ForwardIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="m15 7 5 5-5 5" />
    <path d="M20 12H10a6 6 0 0 0-6 6v1" />
  </Svg>
);

export const CopyIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="8" y="8" width="12" height="12" rx="2" />
    <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
  </Svg>
);

export const TrashIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3" />
  </Svg>
);

export const InfoIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 11v5M12 8h.01" />
  </Svg>
);

/** Leaving a group. */
export const ExitIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M14 4h4a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-4M9 8l-4 4 4 4M5 12h10" />
  </Svg>
);

/** Clearing a chat of its messages. */
export const ClearIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M8 12h8" />
  </Svg>
);

export const AddPersonIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="10" cy="8" r="3.5" />
    <path d="M3.5 20a6.5 6.5 0 0 1 11.6-4M18 14v6M15 17h6" />
  </Svg>
);

/** The mark on a message its sender deleted, and blocking someone. */
export const BlockedIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="8.5" />
    <path d="m6 6 12 12" />
  </Svg>
);

export const MuteIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M11 5 6 9H3v6h3l5 4zM22 9l-6 6M16 9l6 6" />
  </Svg>
);

export const VolumeIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M11 5 6 9H3v6h3l5 4zM15.5 8.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13" />
  </Svg>
);

/** Two corners pulling apart: show bigger. */
export const ExpandIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7" />
  </Svg>
);

export const ShrinkIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" />
  </Svg>
);

export const FileIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" />
    <path d="M14 3v5h5" />
  </Svg>
);

export const PhoneLinkIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="7" y="2.5" width="10" height="19" rx="2" />
    <path d="M11 18.5h2" />
  </Svg>
);

export const LockIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="5" y="11" width="14" height="10" rx="2" />
    <path d="M8 11V8a4 4 0 1 1 8 0v3" />
  </Svg>
);

export const MonitorIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="4" width="18" height="12" rx="2" />
    <path d="M8 20h8M12 16v4" />
  </Svg>
);

export function PersonSilhouette({ group }: { group?: boolean }) {
  if (group) {
    return (
      <svg viewBox="0 0 212 212" width="100%" height="100%" aria-hidden>
        <path
          fill="currentColor"
          d="M102.5 116c-17 0-30-14-30-31s13-31 30-31 31 14 31 31-14 31-31 31zm-51 51c0-22 23-38 51-38s52 16 52 38v7h-103zM152 111c13 0 24-11 24-25s-11-25-24-25c-3 0-6 1-9 2 5 7 7 15 7 23 0 9-3 17-8 24 3 1 6 1 10 1zm12 13c12 5 21 15 21 27v7h-22v-7c0-11-5-20-14-27h15zM52 111c-13 0-24-11-24-25s11-25 24-25c3 0 6 1 9 2-5 7-7 15-7 23 0 9 3 17 8 24-3 1-6 1-10 1zm-12 13c-12 5-21 15-21 27v7h22v-7c0-11 5-20 14-27H40z"
        />
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 212 212" width="100%" height="100%" aria-hidden>
      <path
        fill="currentColor"
        d="M106 111c-21 0-37-17-37-38s16-38 37-38 38 17 38 38-17 38-38 38zm-68 64c0-27 30-47 68-47s68 20 68 47v9H38z"
      />
    </svg>
  );
}

type TickStatus = "sent" | "delivered" | "read";

/** Signal's ticks: one circle when sent, two when delivered, filled once read. */
function SignalTicks({ status, size }: { status: TickStatus; size: number }) {
  const id = useId();
  const double = status !== "sent";
  const filled = status === "read";
  const width = double ? 17.5 : 12;
  const front = double ? 11.5 : 6;
  const check = (cx: number) => `M${cx - 2.3} 6.2l1.6 1.6 3-3.6`;
  const stroke = { fill: "none", strokeWidth: 1.2, strokeLinecap: "round", strokeLinejoin: "round" } as const;
  const circle = (cx: number, mask: string) => (
    <g mask={`url(#${mask})`}>
      {filled ? (
        <circle cx={cx} cy="6" r="5.4" fill="currentColor" />
      ) : (
        <>
          <circle cx={cx} cy="6" r="4.8" stroke="currentColor" {...stroke} />
          <path d={check(cx)} stroke="currentColor" {...stroke} />
        </>
      )}
    </g>
  );
  return (
    <svg width={(size * 0.75 * width) / 12} height={size * 0.75} viewBox={`0 0 ${width} 12`} aria-label={status} role="img">
      <defs>
        <mask id={`${id}-back`} maskUnits="userSpaceOnUse" x="0" y="0" width={width} height="12">
          <rect width={width} height="12" fill="white" />
          <circle cx={front} cy="6" r="6.4" fill="black" />
          {filled ? <path d={check(6)} stroke="black" {...stroke} /> : null}
        </mask>
        <mask id={`${id}-front`} maskUnits="userSpaceOnUse" x="0" y="0" width={width} height="12">
          <rect width={width} height="12" fill="white" />
          {filled ? <path d={check(front)} stroke="black" {...stroke} /> : null}
        </mask>
      </defs>
      {double ? circle(6, `${id}-back`) : null}
      {circle(front, `${id}-front`)}
    </svg>
  );
}

/** Delivery ticks, drawn the way the chat's own network draws them. */
export function Ticks({ status, account, size = 16 }: { status?: TickStatus; account?: AccountId; size?: number }) {
  if (!status) return null;
  if (account === "signal") return <SignalTicks status={status} size={size} />;
  const color = status === "read" ? "var(--tick-read)" : "currentColor";
  return (
    <svg width={size} height={size * 0.7} viewBox="0 0 16 11" aria-label={status} role="img">
      <path d="M1 6l3 3 6.5-7.5" fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
      {status !== "sent" ? (
        <path d="M6.5 8.5 7 9l6.5-7.5" fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
      ) : null}
    </svg>
  );
}

/** A filled person, small enough for a tag. */
export const PersonIcon = ({ size = 24, ...rest }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden {...rest}>
    <circle cx="12" cy="7.5" r="5" />
    <path d="M2.5 22a9.5 9.5 0 0 1 19 0z" />
  </svg>
);

export const KeyboardIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="2.5" y="6" width="19" height="12" rx="2" />
    <path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M6 14h.01M18 14h.01M9 14h6" />
  </Svg>
);

export const GifIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="5" width="18" height="14" rx="3" />
    <path d="M10 10.2a2 2 0 0 0-1.6-.7c-1.3 0-2 1-2 2.5s.7 2.5 2 2.5c.9 0 1.6-.5 1.6-1.5v-.6H8.8M12.7 9.6v4.8M15.4 14.4V9.6h2.4M15.4 12h2" strokeWidth={1.5} />
  </Svg>
);

export const StickerIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M20 13V7a3 3 0 0 0-3-3H7a3 3 0 0 0-3 3v10a3 3 0 0 0 3 3h6z" />
    <path d="M20 13h-4a3 3 0 0 0-3 3v4" />
    <path d="M9 10v.5M14 10v.5" strokeWidth={2.2} />
  </Svg>
);

export const ClockIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 7v5l3 2" />
  </Svg>
);
