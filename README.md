# Omega Zap

**WhatsApp and Signal, native — with a modern UI.** Omega Zap is a desktop
companion client inspired by [ZapFast](https://github.com/crmne/zapfast): no
Chromium browser shell, linked to your phone. The UI is Tauri 2 + React; the
WhatsApp Web protocol runs in Rust through
[`whatsapp-rust`](https://github.com/oxidezap/whatsapp-rust), and Signal through
[`presage`](https://github.com/whisperfish/presage).

> [!WARNING]
> **Extremely early stage (v0.0.x).** Expect bugs, missing features, breaking
> changes and data being wiped between versions. Omega Zap is an unofficial
> client, not affiliated with WhatsApp/Meta or Signal; using unofficial clients
> may put your account at risk. Use at your own risk.

## Run

Requirements: Rust (stable, 1.94+), Node.js, [pnpm](https://pnpm.io), and the
Linux GUI build deps for Tauri (see
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)).

```sh
pnpm install
pnpm tauri dev
```

`pnpm tauri dev` first builds the Signal bridge (`pnpm signal`), which takes a
few minutes the first time. `pnpm dev` alone serves the UI in a browser with the
demo chats only; linking needs the desktop app.

## What it does

- **Looks like WhatsApp Web by default.** The default theme follows your
  desktop's light or dark preference. **Settings → Theme** also offers WhatsApp
  light, WhatsApp dark, and two Omega Zap palettes (ink and mint).
- **Zoom.** `Ctrl +` / `Ctrl −` step between 50% and 200%, `Ctrl 0` resets, and
  `Ctrl` + mouse wheel works too (`⌘` on macOS). The zoom is also in Settings
  and is remembered.
- **Resizable chat list.** Drag the edge between the chat list and the
  conversation. It stays between 300 and 640 px and never squeezes the
  conversation below 380 px. Double-click the edge to reset; with the edge
  focused, the arrow keys, Home and End resize it too.
- **Link WhatsApp.** Until you link, the app shows demo chats. Click **Link
  WhatsApp** (in the chat list, the empty screen, or Settings) and scan the QR
  code from your phone under **Linked devices → Link a device**. After linking,
  recent history syncs from the phone and new messages arrive live.
- **Link Signal too.** **Settings → Accounts → Link Signal**, then scan the QR
  code from Signal on your phone under **Linked devices → Link a new device**.
  Linking one account never logs out the other. Choose **Transfer message
  history** on the phone to bring your existing chats (text; media shows as
  placeholders). That offer only exists while linking: without it, chats appear
  as new messages arrive. Each code is valid for 90 seconds and is replaced
  automatically. Each time it starts, the bridge also re-offers the messages
  of the last 7 days that it has stored, so one that arrived while the app
  was closing is not lost.
- **Reactions, on both networks.** Hover a message to react, and click a
  reaction to take yours back.
- **Profile pictures, on both networks.** Chats show their picture, and your
  own sits in the side rail and next to each account in Settings. They are
  checked again once a day. On Signal a person's picture needs their profile
  key, which comes from your account's storage service or their messages.
- **Photos, on both networks.** Photos load in the conversation as you scroll
  to them. A WhatsApp album shows as one message per photo.
- **Every attachment.** A Signal message with several photos or videos
  shows them as a grid, and the viewer steps through each one. Videos play in
  the chat (those up to 8 MB load as you scroll, so their first frame shows;
  larger ones load when you press play). GIFs loop, voice messages and audio
  play with a seek bar, stickers show as pictures, and a document opens in
  your system's app for it. WhatsApp's videos, GIFs, video messages, voice
  messages, audio, stickers and documents work the same way, and one that
  has expired from WhatsApp's servers is asked for again from the phone that
  sent it.
- **Send files, on both networks.** Press **+** next to the message box, drop
  files on the conversation, or paste a picture. They wait above the box,
  where each can be removed, and what you type goes under the first one as
  its caption. On WhatsApp, JPEG, PNG and WebP pictures, MP4 videos and
  common audio go as photos, videos and audio, and anything else as a
  document; on Signal every file is an attachment. Files up to 100 MB.
- **Switch accounts, or see everything together.** By default each account is
  its own space: pick WhatsApp or Signal in the side rail. **Settings → Accounts
  → Show all chats together** merges them into one list, where a coloured edge
  and a badge on the picture show which network each chat is on.
- **Favorites and lists.** Filter the chat list by All, Unread, Favorites,
  Private (no groups), Groups, or your own lists; each filter counts its unread
  chats. Right-click a chat (or use the arrow on hover) to add it to your
  favorites or a list, and press **+** after the filters to make a list.
  Right-click a list to edit or delete it. On WhatsApp these are the phone's own
  favorites and lists and stay in sync with it; Signal has none, so there they
  are kept on this computer.
- **Starred messages.** Hover a message and press the star. **⋮ → Starred
  messages** lists them from every chat; click one to jump to it. WhatsApp
  stars sync with the phone; Signal's stay on this computer.
- **New chat.** The button next to the title lists your contacts A to Z, and
  yourself. On WhatsApp you can also type a full phone number, with its country
  code, to start a chat with anyone on WhatsApp.
- **Notifications.** New messages, replies ("Replied to you", "Maya replied to
  Rui") and reactions to your messages show a desktop notification, except for
  muted chats and the chat you are looking at. Clicking one opens the chat (on
  Linux). **Settings → Notifications** turns them off or hides the preview.
- **Basic chatting.** Chat list with search,
  text messages with quotes (with the name of whoever wrote the quoted
  message), group sender names, delivery ticks, and sending text. Locations,
  contacts and polls appear as labelled placeholders on both networks.
- **Link cards.** Links are clickable, and a WhatsApp link shows its title and
  picture. The full-size picture is downloaded from WhatsApp; **Settings →
  Links** turns that off and keeps the small one that comes with the message.
- **Short videos play in the chat.** TikTok, Instagram and YouTube Shorts links
  get a play button that loads the site's own player in the conversation.
  Nothing is loaded from those sites until you press it. Turn it off under
  **Settings → Links** to open them in your browser instead.
- **Read receipts, on both networks.** Opening a chat marks it read on your
  phone and tells the senders, unless read receipts are turned off in the
  account's privacy settings. Chats read on the phone lose their unread count
  here too.
  Your own messages carry each network's ticks: WhatsApp's turn blue once the
  message was read (in a group, by everyone), Signal's circles fill in.
- **Log out** of each account from Settings. For WhatsApp this removes the
  computer from your linked devices and deletes what Omega Zap stored locally.
  For Signal it deletes the local session and chats; Signal only lets the phone
  remove a linked device, so also remove "Omega Zap" under Linked devices there.

## Where data lives

Linux: `~/.local/share/rocks.omegazap.desktop/` (the directory is `0700`).

| File | Contents |
| --- | --- |
| `session.db` | The linked-device keys, owned by `whatsapp-rust` |
| `chats.json` | Chat list and the last 400 messages per chat, **unencrypted** (`0600`) |
| `linked` | Marker that a session was paired |
| `signal/signal.db` | The Signal linked-device keys and message store, owned by `presage`, **unencrypted** |
| `signal/chats.json` | Signal chat list and the last 400 messages per chat, **unencrypted** (`0600`) |
| `signal/linked` | Marker that a Signal session was paired |
| `media/`, `signal/media/` | Attachments you have scrolled past or opened, decrypted |
| `link-thumbs/` | Full-size link pictures from WhatsApp, decrypted |
| `avatars/`, `signal/avatars/` | Profile pictures, yours included |

## Not done yet

- Recording voice messages, on both networks
- Message edits and deletions are not shown
- Group names come from history sync; groups first seen live show as "Group"
- Encrypted local archive, tray
- Signal: edits and deletions, starting a chat by phone number (saved
  contacts work)
- WhatsApp: albums as a grid
- More accounts: WhatsApp Business and Telegram, and more than one account per
  network

## Stack

| Layer | Choice |
| --- | --- |
| Shell | Tauri 2 |
| UI | React 19 + TypeScript + Vite + Tailwind CSS 4 |
| WhatsApp | `whatsapp-rust` (pinned git revision; the crates.io 0.7.0 release needs nightly) |
| Signal | `presage` (pinned git revision), in the separate `omega-zap-signal` process |

The UI talks to a `ChatBackend` interface (`src/backend`): `MockBackend` for
the demo, one `TauriBackend` per linked account. On the Rust side,
`src-tauri/src/store.rs` holds the chat state and persistence both networks
share. `src-tauri/src/wa/` owns the WhatsApp connection and events (`convert.rs`
maps WhatsApp messages to the UI model). `src-tauri/src/signal/` starts the
Signal bridge and turns its events into the same chat state.

The bridge is its own crate, `signal-sidecar/`. `presage` and `whatsapp-rust`
need different versions of `libsqlite3-sys`, and Cargo links only one per
binary, so Signal runs as a second process that exchanges JSON lines with the
app over stdin and stdout. Tauri bundles it next to the main executable.

## Disclaimer

Omega Zap is unofficial and not affiliated with WhatsApp, Meta or Signal. Using
an unofficial client may violate the networks' terms of service and could risk
account suspension. Use at your own risk.

ZapFast is MIT-licensed; Omega Zap is a separate project inspired by its goals,
not a fork of its codebase.

## License

MIT, except `signal-sidecar/`: it links `presage` and `libsignal`, which are
AGPL-3.0, so the `omega-zap-signal` binary is AGPL-3.0-only. A build that ships
that binary has to offer its source under the AGPL.
