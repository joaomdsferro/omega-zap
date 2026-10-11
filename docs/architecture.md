# Architecture

## Stack

| Layer | Choice |
| --- | --- |
| Shell | Tauri 2 |
| UI | React 19, TypeScript, Vite, Tailwind CSS 4 |
| WhatsApp | [`whatsapp-rust`](https://github.com/oxidezap/whatsapp-rust), pinned to a fork revision |
| Signal | [`presage`](https://github.com/whisperfish/presage), pinned, in the separate `omega-zap-signal` process |
| Storage | SQLCipher for databases, AES-256-GCM for files |

## Layout

| Path | What it holds |
| --- | --- |
| `src/backend/` | The `ChatBackend` interface the UI talks to: one `TauriBackend` per linked account |
| `src/components/` | The UI |
| `src/lib/` | Formatting, emoji, search, links and other UI helpers |
| `src-tauri/src/store.rs` | Chat state both networks share: chats, messages, names, unread, reactions |
| `src-tauri/src/store/history.rs` | The encrypted history database behind the store |
| `src-tauri/src/wa/` | The WhatsApp connection and its events (`convert.rs` maps messages to the store) |
| `src-tauri/src/signal/` | Starts the Signal bridge and turns its events into the same chat state |
| `src-tauri/src/vault.rs`, `sqlcipher.rs` | The storage key and encryption |
| `src-tauri/src/tray.rs`, `notify.rs` | Background mode, tray and notifications |
| `signal-sidecar/` | The Signal bridge, its own crate |

The bridge is a separate binary for two reasons: `presage` and `whatsapp-rust` need different versions
of `libsqlite3-sys` and Cargo links only one per binary, and `presage` is AGPL-3.0, so keeping it in its
own process keeps the app MIT. The two exchange JSON lines over stdin and stdout. Tauri bundles the
bridge next to the main executable.

## Where data lives

Linux: `~/.local/share/rocks.omegazap.desktop/` (the directory is `0700`). Everything below is
encrypted with a key kept in the desktop keyring.

| Path | Contents |
| --- | --- |
| `vault.json` | Marks the folder as encrypted; holds no key |
| `session.db` | The WhatsApp linked-device keys (SQLCipher) |
| `history.db` | WhatsApp chats and every stored message (SQLCipher) |
| `signal/signal.db` | The Signal linked-device keys and message store (SQLCipher) |
| `signal/history.db` | Signal chats and every stored message (SQLCipher) |
| `media/`, `signal/media/` | Attachments you have opened or scrolled past |
| `avatars/`, `signal/avatars/` | Profile pictures |
| `link-thumbs/`, `link-previews/` | Link card pictures and the cards made for links without one |
| `linked`, `signal/linked` | Markers that an account is linked |

Files handed to other apps (a document to open, a notification's emoji) are written unencrypted to
`$XDG_RUNTIME_DIR/omega-zap/`, which lives in memory and is cleared at logout.
