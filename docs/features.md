# Features

Everything Omega Zap does today, network by network. For what is still missing, see
[Not done yet](#not-done-yet).

## Accounts

- **Link WhatsApp.** Scan the QR code from your phone under **Linked devices → Link a device**. Recent
  history syncs right away, and older history is fetched from the phone in the background, chat by
  chat, with progress at the bottom of the chat list.
- **Link Signal.** **Settings → Accounts → Link Signal**, then scan from Signal under **Linked devices**.
  Choose **Transfer message history** to bring your chats (text; media shows as placeholders). That
  offer only exists while linking. Each code lasts 90 seconds and is replaced automatically.
- **Separate or together.** Each account is its own space, picked in the side rail. **Settings →
  Accounts → Show all chats together** merges them into one list; a coloured edge and a badge show
  which network a chat is on.
- **Log out** of each account from Settings. WhatsApp removes this computer from your linked devices.
  Signal only lets the phone remove a linked device, so also remove "Omega Zap" there.

## Chats

- **Chat list** with filters: All, Unread, Favorites, Private, Groups and your own lists, each with its
  unread count. Right-click a chat to add it to favorites or a list. On WhatsApp these are the phone's
  own favorites and lists; Signal has none, so they are kept on this computer.
- **Archive, pin and mute.** Right-click a chat to archive it, pin it to the top, or mute it for 8 hours,
  a week or always. Archived chats sit under **Archived** at the top of the list, and searches look in
  them too. On WhatsApp all three sync with the phone, which allows three pins and unpins a chat it
  archives. Signal keeps them on this computer, allows four pins, and brings an archived chat back when
  a message arrives, unless it is muted.
- **Unread.** Chats open at the first unread message, under an "N unread messages" divider.
- **Jump to present.** Scrolled up in a chat, a button takes you back to the newest message and counts
  what came in meanwhile.
  Right-click a chat to **Mark as unread** or **Mark as read**; a marked chat shows a dot and "Marked
  unread". On WhatsApp the mark syncs with the phone both ways.
- **Contact info.** Click a sender's name in a group, a mention, or the header of a chat with one
  person: their picture, number, About, media from your chat, starred messages, a mute switch and the
  groups you share.
- **Group info.** Click a group's header: its picture, description, who made it and when, media,
  starred messages, a mute switch, and the members (admins marked), searchable, each opening their
  contact info. The pencils change the group's name and description (admins only, unless the group
  lets everyone). Admins can **Add member** from their contacts and, from the arrow on a member, make
  them an admin, dismiss them as admin or remove them. Changes made on other devices show here too.
  WhatsApp only.
- **Chat menu.** The **⋮** in a chat's header: contact or group info, select messages, mute, clear,
  block, and delete or exit.
- **Clear and delete.** **Clear chat** empties a chat but keeps it in the list, starred messages kept
  unless you untick it; **Delete chat** removes it with its messages. On WhatsApp both sync with the
  phone both ways; on Signal they change this computer only. Cleared messages don't come back when
  history is fetched again.
- **Block.** Block or unblock someone from the chat menu, the list's right-click menu or their contact
  info. A blocked chat shows **Unblock** in place of the message box. WhatsApp only.
- **Exit group.** From the chat menu, the list or group info. Only the group's admins are told. The
  chat stays, read-only, until you delete it. WhatsApp only.
- **Pinned messages.** Messages pinned in a WhatsApp chat show in a bar over the conversation, one at a
  time; click it to go to the message and bring up the next. Pinned messages carry a small pin, and the
  conversation says who pinned or unpinned what, and when. Pin or unpin from the message menu, for
  24 hours, 7 days or 30 days.
- **Edits.** An edited message shows its new text with "Edited". Your own texts can be edited from
  the message menu for 15 minutes after sending on WhatsApp, and for 24 hours on Signal.
- **Polls.** A poll shows each option's share of the votes; click an option to vote, again to take
  the vote back, and **View votes** for who picked what. WhatsApp only; creating polls isn't done yet.
- **Locations and contact cards.** A shared place shows its map, name and address and opens in maps.
  A contact card shows the name and number, and **Message** opens a chat when the number has
  WhatsApp. WhatsApp only.
- **Search.** The sidebar search finds chats and then messages across every chat; **Ctrl+F** searches
  inside a chat and marks the words. Case and accents are ignored.
- **New chat.** Your contacts A to Z, and yourself. On WhatsApp you can also type a full phone number.
- **Starred messages** from every chat under **⋮ → Starred messages**. WhatsApp stars sync with the
  phone; Signal's stay on this computer.

## Typing and online

WhatsApp only, for now.

- **Typing.** Someone typing or recording shows in the chat list, with a tinted row, a ring around their
  picture and pulsing dots, and in the chat's header. In a group it names who. It goes away when they
  stop or their message arrives.
- **Online and last seen** show in the header of the chat you have open, when the person shares them.
- **What others see of you.** "typing…" while you write, which **Settings → Privacy → Show when I'm
  typing** turns off. You show as online only while the Omega Zap window is in front, and go offline
  10 seconds after it loses focus or closes, so your phone keeps ringing while the app sits in the tray.
- **Settings → Privacy** changes your WhatsApp account's own **Last seen**, **Online** and **Read
  receipts**, the same settings as on your phone. If you don't share your last seen or online, you
  can't see other people's. "My contacts except…" can only be set on the phone.

## Messages

- **Formatting** as WhatsApp writes it (`*bold*`, `_italic_`, `~strike~`, `` `code` ``, quotes and
  lists), also for Signal's styles. **Ctrl+B** and **Ctrl+I** in the message box.
- **Apple emoji**, as WhatsApp Web and Signal Desktop draw them, with an emoji and characters panel and
  `:name` suggestions while typing. The Slack and GitHub shortcodes work too: `:laughing`, `:+1`, and
  closing one with a colon (`:fire:`) puts the emoji in at once.
- **Mentions** show the person's name.
- **Replies**: clicking a quote jumps to the quoted message.
- **Reactions** on both networks. Hover a message to react. Each emoji shows its own count, yours
  highlighted; click them to see who reacted, and to take yours back.
- **Forwarding**, media included. Forwarded messages say so. A message can be added; it is sent after
  the forwarded ones, as on WhatsApp.
- **Selecting messages.** "Select" in a message's menu ticks messages (Shift+click ticks a run) to
  forward, star or delete them together. Escape stops selecting.
- **Read receipts and ticks.** Opening a chat marks it read on your phone and tells the senders, unless
  read receipts are off in the account's privacy settings.

## Media and links

- **Photos, videos, GIFs, voice messages, audio, stickers and documents** on both networks. Videos and
  audio play in the chat; documents open in your system's app. A WhatsApp file that expired from the
  server is asked for again from the phone that sent it.
- **Albums**: four or more photos in a row from the same person show as one grid, as on WhatsApp.
- **Sending files**: press **+**, drop files on the conversation, or paste a picture. Up to 100 MB.
- **Voice messages** (WhatsApp): the microphone button records from your default microphone, with the
  time and a live waveform. Pause and go on, **Escape** or the bin throws it away, **Enter** or the
  send button sends it. The chat sees "recording audio…" meanwhile. Leaving the chat throws the
  recording away.
- **Stickers** (WhatsApp): the emoji panel's **Stickers** tab has Recent (what you sent, here or on
  the phone), Favorites (in sync with the phone both ways) and your packs. **+** (or **Ctrl+V** with the tab open) turns a picture or
  GIF into a sticker, makes a pack of your own, or adds packs from WhatsApp's sticker store. Packs
  installed on the phone are not shared with linked devices, so they can't show here. Right-click a
  sticker, or open a sticker message's menu, to star it or add it to a pack. The search box finds
  stickers on KLIPY; they are made into WhatsApp stickers as they are sent.
- **GIFs** (WhatsApp): the **GIFs** tab searches KLIPY and shows what is trending and the GIFs you
  sent.
- **Link cards** for links that come with one, and for links that don't (fetched from the page's own
  tags; **Settings → Links** turns that off). Your own links go out with a card.
- **Short videos play in the chat**: TikTok, Instagram, YouTube Shorts and X. Nothing loads from those
  sites until you press play.

## Desktop

- **Runs in the tray.** Closing the window ends the page and keeps WhatsApp and Signal connected, using
  about half the memory. The tray icon shows the unread count.
- **Notifications** for messages, replies and reactions to your messages, except muted chats and the
  one you are looking at. Clicking one brings the window forward. A burst of messages is folded into
  one notification per chat. **Settings → Notifications** hides the preview or opens the chat directly.
- **Themes**: WhatsApp light and dark, Omega Zap ink and mint, or following the desktop.
- **Zoom** with **Ctrl +**, **Ctrl −**, **Ctrl 0** or **Ctrl** + mouse wheel, from 50% to 200%.
- **Resizable chat list**: drag its edge; double-click to reset.
- **Keyboard shortcuts**: WhatsApp Web's, plus **Ctrl+N** new chat, **Ctrl+K** search, **Ctrl+Tab** or
  **Alt+↑/↓** to move between chats, **Ctrl+1…9**, **Ctrl+E** emoji, **↑** to edit your last message and
  **Escape** to close a chat. **Ctrl+/** (or **⋮ → Keyboard shortcuts**) lists them all.

## Not done yet

- Windows and macOS builds
- Recording voice messages on Signal
- Signal stickers and GIFs (receiving works)
- Editing captions from this computer; Signal pinned messages
- An app lock (a password for the stored data)
- More than one account per network, WhatsApp Business, Telegram
- Signal: typing indicators, starting a chat by phone number (saved contacts work), fetching history
  older than the link
