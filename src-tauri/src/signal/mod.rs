//! Signal, through the `omega-zap-signal` bridge process (see `signal-sidecar`).
//! The bridge owns the protocol and its keys; this side keeps the chat list.

pub mod commands;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::oneshot;

use base64::Engine as _;
use crate::account::{AccountState, AccountStatus, Me, OutgoingFile, OutgoingLink, lock, render_qr, secure_dir};
use crate::store::{
    ChatStore, ChatSummary, History, ContactDto, Content, Deletion, Delivery, Incoming, LinkPreview, ListDto, MediaRef, MessageDto, MsgKind,
    OWN_AVATAR, ReplyRef, StarredDto, StoredMessage, avatar_file, now_ms,
};

const BRIDGE: &str = "omega-zap-signal";
const SESSION_FILE: &str = "signal.db";
const LINKED_MARKER: &str = "linked";
/// The most chats Signal lets you pin.
const MAX_PINNED: usize = 4;
const CACHE_FILE: &str = "chats.json";
const SAVE_INTERVAL: Duration = Duration::from_secs(3);
pub const MEDIA_DIR: &str = "media";
pub const AVATAR_DIR: &str = "avatars";
const SEND_TIMEOUT: Duration = Duration::from_secs(60);
const MEDIA_TIMEOUT: Duration = Duration::from_secs(150);
/// Uploading a file of up to 100 MB, then sending the message.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const AVATAR_TIMEOUT: Duration = Duration::from_secs(45);
/// Pause between profile picture lookups, to stay clear of the rate limit.
const AVATAR_PACE: Duration = Duration::from_millis(150);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Quote {
    author: Option<String>,
    ts: Option<i64>,
    text: String,
}

/// A text's formatting as Signal sends it, worked out by the UI.
#[derive(Deserialize)]
pub struct Styled {
    /// The text with its markers taken out.
    pub body: String,
    /// `{ start, length, style }` over `body`, passed on to the bridge.
    pub ranges: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct ReadRef {
    sender: String,
    ts: i64,
}

/// One JSON line from the bridge.
#[derive(Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", rename_all_fields = "camelCase")]
enum BridgeEvent {
    State { state: String, error: Option<String> },
    Qr { url: String, ttl_secs: u64 },
    Me { id: String, phone: String, name: Option<String> },
    Contact { id: String, name: String, saved: bool },
    Group { id: String, name: String },
    Message {
        chat: String,
        is_group: bool,
        sender: String,
        from_me: bool,
        ts: i64,
        kind: MsgKind,
        text: Option<String>,
        media_label: Option<String>,
        quote: Option<Quote>,
        #[serde(default)]
        attachments: Vec<MediaRef>,
        /// Imported from the phone's history instead of received live.
        #[serde(default)]
        history: bool,
        /// How far one of our own messages got, when the history says.
        #[serde(default)]
        delivery: Option<Delivery>,
    },
    History { state: String, percent: Option<u32>, error: Option<String> },
    Receipt { ids: Vec<String>, delivery: Delivery },
    /// Messages that were read on another of our devices.
    Read { messages: Vec<ReadRef> },
    Delete {
        chat: String,
        sender: String,
        target_ts: i64,
        #[serde(default)]
        history: bool,
    },
    Reaction {
        chat: String,
        sender: String,
        target_author: String,
        target_ts: i64,
        emoji: String,
        remove: bool,
        #[serde(default)]
        history: bool,
    },
    ProfileKey { id: String },
    Avatar { id: u64, picture: Option<String>, error: Option<String> },
    /// A file was uploaded and sent: where it now lives on Signal's CDN.
    Uploaded { id: u64, media: Option<serde_json::Value>, error: Option<String> },
    Done { id: u64, error: Option<String> },
}

struct Bridge {
    child: Child,
    stdin: ChildStdin,
}

pub struct SignalManager {
    app: AppHandle,
    dir: PathBuf,
    status: Mutex<AccountStatus>,
    store: Mutex<ChatStore>,
    /// Every message, and the store, on disk.
    history: History,
    bridge: tokio::sync::Mutex<Option<Bridge>>,
    /// Bumped whenever a bridge is started or torn down, so output from one we
    /// already abandoned is ignored.
    generation: AtomicU64,
    chats_dirty: AtomicBool,
    save_dirty: AtomicBool,
    /// Wakes the refresh / save loop, which otherwise sleeps.
    wake: tokio::sync::Notify,
    avatars_running: AtomicBool,
    next_request: AtomicU64,
    /// Commands the bridge has yet to answer. Only some answers carry a value.
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Option<String>, String>>>>,
}

/// A file extension for an attachment, so it is served and opened as what it is.
pub(crate) fn extension(mime: &str, file_name: Option<&str>) -> String {
    // Without parameters: WhatsApp's voice messages are `audio/ogg; codecs=opus`.
    let mime = mime.split(';').next().unwrap_or_default().trim();
    let known = match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/heic" => "heic",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "audio/aac" => "aac",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "audio/opus" => "opus",
        _ => "",
    };
    if !known.is_empty() {
        return known.into();
    }
    file_name
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| (1..=8).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or_else(|| if mime.starts_with("image/") { "jpg".into() } else { "bin".into() })
}

/// A sent file name that is safe to write: no folders, no hidden files.
pub(crate) fn safe_file_name(name: &str) -> Option<String> {
    let name: String = name
        .rsplit(['/', '\\'])
        .next()?
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    let name = name.trim().trim_start_matches('.').to_string();
    (!name.is_empty()).then_some(name)
}

/// Where a downloaded attachment is kept in the media directory.
fn media_file_name(media: &MediaRef) -> String {
    let safe: String = media
        .cdn_key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    match (media.kind(), media.file_name.as_deref().and_then(safe_file_name)) {
        ("file", Some(file_name)) => format!("{safe}/{file_name}"),
        _ => format!("{safe}.{}", extension(&media.mime, media.file_name.as_deref())),
    }
}

fn bridge_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = exe.with_file_name(format!("{BRIDGE}{}", std::env::consts::EXE_SUFFIX));
    if path.exists() {
        Ok(path)
    } else {
        Err(format!("The Signal bridge is missing ({}).", path.display()))
    }
}

impl SignalManager {
    pub fn new(app: AppHandle, dir: PathBuf) -> std::io::Result<Arc<Self>> {
        secure_dir(&dir)?;
        let linked = dir.join(LINKED_MARKER).exists();
        let history = History::new(&dir, "signal history", CACHE_FILE);
        let store = linked.then(|| history.load()).flatten().unwrap_or_default();
        Ok(Arc::new(Self {
            app,
            dir,
            status: Mutex::new(AccountStatus::idle(linked)),
            store: Mutex::new(store),
            history,
            bridge: tokio::sync::Mutex::new(None),
            generation: AtomicU64::new(0),
            chats_dirty: AtomicBool::new(false),
            save_dirty: AtomicBool::new(false),
            wake: tokio::sync::Notify::new(),
            avatars_running: AtomicBool::new(false),
            next_request: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
        }))
    }

    /// Starts the UI refresh / save loop and reconnects an existing session.
    pub fn start(self: &Arc<Self>) {
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut last_save = Instant::now();
            // Idle until something changes, so a quiet background costs no
            // wake-ups; then tick until the change is shown and saved.
            loop {
                mgr.wake.notified().await;
                loop {
                    tokio::time::sleep(Duration::from_millis(350)).await;
                    if mgr.chats_dirty.swap(false, Ordering::AcqRel) {
                        let _ = mgr.app.emit("signal://chats", ());
                        crate::tray::refresh(&mgr.app);
                    }
                    if !mgr.save_dirty.load(Ordering::Acquire) {
                        break;
                    }
                    if last_save.elapsed() >= SAVE_INTERVAL && mgr.save_dirty.swap(false, Ordering::AcqRel) {
                        mgr.save_now();
                        last_save = Instant::now();
                    }
                }
            }
        });

        if self.is_linked() {
            let mgr = self.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = mgr.start_bridge(false).await {
                    mgr.fail(e);
                }
            });
        }
    }

    pub fn unread_total(&self) -> u32 {
        lock(&self.store).unread_total()
    }

    fn is_linked(&self) -> bool {
        self.dir.join(LINKED_MARKER).exists()
    }

    pub fn status(&self) -> AccountStatus {
        lock(&self.status).clone()
    }

    fn set_status(&self, f: impl FnOnce(&mut AccountStatus)) {
        let snapshot = {
            let mut status = lock(&self.status);
            f(&mut status);
            status.seq += 1;
            status.clone()
        };
        let _ = self.app.emit("signal://status", snapshot);
    }

    fn fail(&self, error: String) {
        log::warn!("signal: {error}");
        self.set_status(|s| {
            s.state = AccountState::Error;
            s.error = Some(error);
            s.qr_svg = None;
        });
    }

    fn mark_dirty(&self) {
        self.chats_dirty.store(true, Ordering::Release);
        self.save_dirty.store(true, Ordering::Release);
        self.wake.notify_one();
    }

    pub fn save_now(&self) {
        if !self.is_linked() {
            return;
        }
        self.history.save(&self.store);
    }

    /// Brings a message that only the history database has back into
    /// memory, before something is done with it.
    fn recall(&self, chat_id: &str, message_id: &str) {
        let chat = {
            let store = lock(&self.store);
            if store.has_message(chat_id, message_id) {
                return;
            }
            store.chat_key(chat_id)
        };
        if let Some(message) = self.history.message(&chat, message_id) {
            lock(&self.store).restore(&chat, message);
        }
    }

    /// Messages of a chat from before `before_ms`, for scrolling back past
    /// what memory keeps.
    pub fn older_messages(&self, chat_id: &str, before_ms: i64, limit: usize) -> Vec<MessageDto> {
        let older = self.history.older(chat_id, before_ms, limit);
        lock(&self.store).dtos(chat_id, &older)
    }

    fn wipe_files(&self) {
        let names = [
            SESSION_FILE.to_string(),
            format!("{SESSION_FILE}-wal"),
            format!("{SESSION_FILE}-shm"),
            format!("{SESSION_FILE}-journal"),
            LINKED_MARKER.to_string(),
        ];
        self.history.close();
        for path in names.into_iter().map(|name| self.dir.join(name)).chain(self.history.files()) {
            if path.exists()
                && let Err(e) = fs::remove_file(&path)
            {
                log::warn!("signal: could not remove {}: {e}", path.display());
            }
        }
        for name in [MEDIA_DIR, AVATAR_DIR] {
            let dir = self.dir.join(name);
            if dir.exists()
                && let Err(e) = fs::remove_dir_all(&dir)
            {
                log::warn!("signal: could not remove {}: {e}", dir.display());
            }
        }
        *lock(&self.store) = ChatStore::default();
        self.save_dirty.store(false, Ordering::Release);
        self.chats_dirty.store(true, Ordering::Release);
        self.wake.notify_one();
    }

    async fn start_bridge(self: &Arc<Self>, link: bool) -> Result<(), String> {
        let mut slot = self.bridge.lock().await;
        if slot.is_some() {
            return Ok(());
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let linked = self.is_linked();
        self.set_status(|s| {
            s.state = if linked { AccountState::Connecting } else { AccountState::Starting };
            s.linked = linked;
            s.error = None;
            s.qr_svg = None;
        });

        let session = self.dir.join(SESSION_FILE);
        let key = crate::sqlcipher::key("signal session");
        {
            let (session, key) = (session.clone(), key.clone());
            tauri::async_runtime::spawn_blocking(move || crate::sqlcipher::encrypt_in_place(&session, &key))
                .await
                .map_err(|e| e.to_string())??;
        }
        let mut child = Command::new(bridge_path()?)
            .arg(&session)
            // Read once by the bridge to open its encrypted store.
            .env("OMEGA_ZAP_DB_KEY", key)
            .env("OMEGA_ZAP_RUNTIME_DIR", crate::vault::runtime_dir().map_err(|e| e.to_string())?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Could not start the Signal bridge: {e}"))?;
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err("Could not talk to the Signal bridge.".into());
        };
        if link {
            // Queued until the bridge has opened its store.
            stdin
                .write_all(b"{\"cmd\":\"link\"}\n")
                .await
                .map_err(|e| format!("Could not talk to the Signal bridge: {e}"))?;
        }
        *slot = Some(Bridge { child, stdin });

        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if mgr.generation.load(Ordering::SeqCst) != generation {
                    return;
                }
                match serde_json::from_str::<BridgeEvent>(&line) {
                    Ok(BridgeEvent::State { state, error }) if state == "logged_out" => {
                        return mgr.handle_unlinked(generation, error).await;
                    }
                    Ok(event) => mgr.handle_event(event),
                    Err(e) => log::warn!("signal: unreadable bridge output: {e}"),
                }
            }
            mgr.bridge_exited(generation).await;
        });
        Ok(())
    }

    async fn stop_bridge(&self) {
        let bridge = self.bridge.lock().await.take();
        if let Some(mut bridge) = bridge {
            let _ = bridge.child.kill().await;
        }
        for (_, waiter) in lock(&self.pending).drain() {
            let _ = waiter.send(Err("Signal disconnected.".into()));
        }
    }

    /// The bridge stopped on its own.
    async fn bridge_exited(&self, generation: u64) {
        if self.generation.compare_exchange(generation, generation + 1, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return;
        }
        self.stop_bridge().await;
        if lock(&self.status).state != AccountState::Error {
            self.fail("The Signal bridge stopped.".into());
        }
    }

    pub async fn start_linking(self: &Arc<Self>) -> Result<(), String> {
        if self.is_linked() {
            return Err("This computer is already linked.".into());
        }
        if self.bridge.lock().await.is_some() {
            return Ok(());
        }
        // Leftovers from an abandoned attempt would otherwise be reused.
        self.wipe_files();
        self.start_bridge(true).await.inspect_err(|e| self.fail(e.clone()))
    }

    pub async fn cancel_linking(self: &Arc<Self>) {
        if self.is_linked() {
            return;
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.stop_bridge().await;
        self.wipe_files();
        self.set_status(|s| *s = AccountStatus { seq: s.seq, ..AccountStatus::idle(false) });
    }

    pub async fn reconnect(self: &Arc<Self>) -> Result<(), String> {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.stop_bridge().await;
        if self.is_linked() {
            self.start_bridge(false).await.inspect_err(|e| self.fail(e.clone()))
        } else {
            self.start_linking().await
        }
    }

    /// Forgets the session on this computer. A linked device cannot remove
    /// itself from the account: that is done on the phone.
    pub async fn logout(self: &Arc<Self>) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.stop_bridge().await;
        self.wipe_files();
        self.set_status(|s| *s = AccountStatus { seq: s.seq, ..AccountStatus::idle(false) });
    }

    /// The phone removed this device.
    async fn handle_unlinked(&self, generation: u64, error: Option<String>) {
        if self.generation.compare_exchange(generation, generation + 1, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return;
        }
        self.stop_bridge().await;
        self.wipe_files();
        self.set_status(|s| {
            *s = AccountStatus {
                seq: s.seq,
                state: AccountState::LoggedOut,
                error: error.or_else(|| Some("This computer was unlinked from Signal.".into())),
                ..AccountStatus::idle(false)
            }
        });
    }

    pub fn list_chats(&self) -> Vec<ChatSummary> {
        lock(&self.store).list_chats()
    }

    pub fn messages(&self, chat_id: &str) -> Vec<MessageDto> {
        lock(&self.store).messages(chat_id)
    }

    pub fn lists(&self) -> Vec<ListDto> {
        lock(&self.store).lists()
    }

    pub fn starred(&self) -> Vec<StarredDto> {
        lock(&self.store).starred()
    }

    pub fn search(&self, query: &str, chat: Option<&str>) -> Vec<StarredDto> {
        let saved = self.history.search(query, chat, crate::store::SEARCH_LIMIT);
        lock(&self.store).search(query, chat, crate::store::SEARCH_LIMIT, saved)
    }

    pub fn contacts(&self) -> Vec<ContactDto> {
        lock(&self.store).contacts()
    }

    pub fn person(&self, id: &str) -> crate::store::PersonDto {
        lock(&self.store).person(id)
    }

    // Signal has no favorites, lists or stars of its own, so these stay on
    // this computer.

    pub fn set_favorite(&self, chat_id: &str, favorite: bool) {
        lock(&self.store).set_favorite(chat_id, favorite);
        self.mark_dirty();
    }

    /// Creates a list, or renames an existing one, and adds and removes
    /// chats. Returns its id.
    pub fn save_list(&self, id: Option<String>, name: &str, add: &[String], remove: &[String]) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("The list needs a name.".into());
        }
        let mut store = lock(&self.store);
        let (id, order) = match id {
            Some(id) => (id.clone(), store.list(&id).ok_or("That list is gone.")?.order),
            None => store.next_list(),
        };
        store.edit_list(&id, Some(name), true, Some(order), None);
        for chat in remove {
            store.set_list_chat(&id, chat, false);
        }
        for chat in add {
            store.set_list_chat(&id, chat, true);
        }
        drop(store);
        self.mark_dirty();
        Ok(id)
    }

    pub fn delete_list(&self, id: &str) {
        lock(&self.store).delete_list(id);
        self.mark_dirty();
    }

    pub fn set_starred(&self, chat_id: &str, message_id: &str, starred: bool) -> Result<MessageDto, String> {
        self.recall(chat_id, message_id);
        let dto = lock(&self.store)
            .set_starred(chat_id, message_id, starred)
            .ok_or("That message is gone.")?;
        self.mark_dirty();
        Ok(dto)
    }

    /// Clears the unread count and has the bridge pass the news on: to our
    /// other devices, and to the senders when the account sends read receipts.
    pub fn person_details(&self, id: &str) -> crate::store::PersonDetailsDto {
        crate::store::PersonDetailsDto { common_groups: lock(&self.store).common_groups(id), ..Default::default() }
    }

    /// Mutes a chat until a time, or for good without one, or unmutes it,
    /// on this computer.
    pub fn set_muted(&self, chat_id: &str, muted: bool, until_ms: Option<i64>) {
        lock(&self.store).set_muted(chat_id, muted.then(|| until_ms.unwrap_or(i64::MAX)));
        self.mark_dirty();
    }

    /// Archives a chat or brings it back, on this computer: Signal doesn't
    /// tell linked devices. Archiving unpins it.
    pub fn set_archived(&self, chat_id: &str, archived: bool) {
        let mut store = lock(&self.store);
        store.set_archived(chat_id, archived);
        if archived {
            store.set_pinned(chat_id, None);
        }
        drop(store);
        self.mark_dirty();
    }

    /// Pins a chat to the top of the list or unpins it, on this computer.
    pub fn set_pinned(&self, chat_id: &str, pinned: bool) -> Result<(), String> {
        let mut store = lock(&self.store);
        if pinned && store.pinned_count() >= MAX_PINNED {
            return Err(format!("You can only pin up to {MAX_PINNED} chats."));
        }
        store.set_pinned(chat_id, pinned.then(crate::store::now_ms));
        drop(store);
        self.mark_dirty();
        Ok(())
    }

    /// Marks a chat unread, or read. Signal keeps no such mark for linked
    /// devices, so it stays on this computer.
    pub fn set_unread(&self, chat_id: &str, unread: bool) {
        lock(&self.store).set_marked_unread(chat_id, unread);
        self.mark_dirty();
    }

    pub fn mark_read(self: &Arc<Self>, chat_id: &str) {
        // Opening a chat asks for this every time; one with nothing unread
        // changes nothing, and marking it dirty would redraw the chat list.
        let read = {
            let mut store = lock(&self.store);
            if !store.has_unread(chat_id) {
                return;
            }
            store.mark_read(chat_id)
        };
        self.mark_dirty();
        let messages: Vec<serde_json::Value> = read
            .iter()
            .filter_map(|m| Some(serde_json::json!({ "sender": m.sender.as_ref()?, "ts": m.ts_ms })))
            .collect();
        if messages.is_empty() {
            return;
        }
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            let command = serde_json::json!({ "cmd": "read", "messages": messages, "ts": now_ms() });
            if let Err(e) = mgr.request(command, SEND_TIMEOUT).await {
                log::warn!("signal: read receipts not sent: {e}");
            }
        });
    }

    /// Sends a command to the bridge and waits for it to report back.
    async fn request(
        &self,
        mut command: serde_json::Value,
        timeout: Duration,
    ) -> Result<Option<String>, String> {
        if lock(&self.status).state != AccountState::Connected {
            return Err("Signal is not connected.".into());
        }
        let id = self.next_request.fetch_add(1, Ordering::Relaxed);
        command["id"] = id.into();
        let (done, answer) = oneshot::channel();
        lock(&self.pending).insert(id, done);

        let written = match self.bridge.lock().await.as_mut() {
            Some(bridge) => bridge
                .stdin
                .write_all(format!("{command}\n").as_bytes())
                .await
                .map_err(|e| e.to_string()),
            None => Err("Signal is not connected.".into()),
        };
        let outcome = match written {
            Ok(()) => match tokio::time::timeout(timeout, answer).await {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(_)) => Err("Signal disconnected.".into()),
                Err(_) => Err("Signal did not answer in time.".into()),
            },
            Err(e) => Err(e),
        };
        lock(&self.pending).remove(&id);
        outcome
    }

    /// The id a message has here: our own are keyed by timestamp alone, which
    /// is what receipts and reactions refer to.
    fn message_id(&self, author: &str, ts: i64) -> String {
        if lock(&self.store).own_id() == Some(author) {
            ts.to_string()
        } else {
            format!("{ts}-{}", author.get(..8).unwrap_or(author))
        }
    }

    /// Downloads one of a message's attachments if needed. Returns its path
    /// in the media directory: a plain file name, except for documents,
    /// which keep the name they were sent with in a folder of their own.
    pub async fn fetch_media(&self, chat_id: &str, message_id: &str, index: usize) -> Result<String, String> {
        self.recall(chat_id, message_id);
        let media = lock(&self.store)
            .media(chat_id, message_id, index)
            .ok_or("This attachment is no longer available.")?;
        let name = media_file_name(&media);
        let dir = self.dir.join(MEDIA_DIR);
        let path = dir.join(&name);
        if path.exists() {
            return Ok(name);
        }
        if let Some(parent) = path.parent() {
            secure_dir(parent).map_err(|e| e.to_string())?;
        }
        // The bridge writes it plain into memory, and it is sealed from there.
        let incoming = crate::vault::handoff_path("signal-media").map_err(|e| e.to_string())?;
        let command = serde_json::json!({
            "cmd": "media",
            "path": incoming,
            "media": {
                "mime": media.mime,
                "cdn": media.cdn,
                "cdnKey": media.cdn_key,
                "key": media.key,
                "size": media.size,
            },
        });
        let fetched = self.request(command, MEDIA_TIMEOUT).await;
        if let Err(e) = fetched {
            let _ = fs::remove_file(&incoming);
            return Err(format!("The attachment was not downloaded: {e}"));
        }
        crate::vault::get()
            .import(&incoming, &path)
            .map_err(|e| format!("The attachment could not be saved: {e}"))?;
        Ok(name)
    }

    /// Downloads an attachment if needed and opens it with the system's app
    /// for its type.
    pub async fn open_media(&self, chat_id: &str, message_id: &str, index: usize) -> Result<(), String> {
        self.recall(chat_id, message_id);
        use tauri_plugin_opener::OpenerExt;
        let name = self.fetch_media(chat_id, message_id, index).await?;
        let file_name = Path::new(&name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let path = crate::vault::get()
            .export(&self.dir.join(MEDIA_DIR).join(&name), &file_name)
            .map_err(|e| format!("The attachment could not be opened: {e}"))?;
        self.app
            .opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| format!("The attachment could not be opened: {e}"))
    }

    /// Downloads our own profile picture and those of the listed chats that
    /// are due a check.
    fn sync_avatars(self: &Arc<Self>) {
        if self.avatars_running.swap(true, Ordering::AcqRel) {
            return;
        }
        let mgr = self.clone();
        let generation = self.generation.load(Ordering::SeqCst);
        tauri::async_runtime::spawn(async move {
            mgr.run_avatar_sync(generation).await;
            mgr.avatars_running.store(false, Ordering::Release);
        });
    }

    /// Has the bridge look a picture up and files it under `owner` when it
    /// changed. Returns the id of the picture now on disk, if there is one.
    /// `Err` means the lookups should stop for now.
    async fn fetch_avatar(
        &self,
        dir: &Path,
        owner: &str,
        target: &str,
        stored: Option<&str>,
    ) -> Result<Option<String>, String> {
        let have = stored.filter(|id| dir.join(avatar_file(owner, id)).exists());
        let incoming = crate::vault::handoff_path("signal-avatar").map_err(|e| e.to_string())?;
        let command = serde_json::json!({
            "cmd": "avatar",
            "target": target,
            "have": have,
            "path": incoming,
        });
        let picture = self.request(command, AVATAR_TIMEOUT).await;
        let picture = picture.inspect_err(|_| {
            let _ = fs::remove_file(&incoming);
        })?;
        match picture.as_deref().filter(|id| Some(*id) != have) {
            Some(id) => crate::vault::get().import(&incoming, &dir.join(avatar_file(owner, id))).map_err(|e| e.to_string())?,
            None => {
                let _ = fs::remove_file(&incoming);
            }
        }
        if let Some(old) = stored.filter(|old| Some(*old) != picture.as_deref()) {
            let _ = fs::remove_file(dir.join(avatar_file(owner, old)));
        }
        Ok(picture)
    }

    async fn run_avatar_sync(&self, generation: u64) {
        let dir = self.dir.join(AVATAR_DIR);
        if let Err(e) = secure_dir(&dir) {
            return log::warn!("signal: could not create {}: {e}", dir.display());
        }
        let own = lock(&self.store).own_avatar_due();
        if let Some((id, stored)) = own {
            match self.fetch_avatar(&dir, OWN_AVATAR, &id, stored.as_deref()).await {
                Ok(picture) => {
                    let avatar = {
                        let mut store = lock(&self.store);
                        store.set_own_avatar(picture);
                        store.own_avatar()
                    };
                    self.set_status(|s| {
                        if let Some(me) = s.me.as_mut() {
                            me.avatar = avatar;
                        }
                    });
                    self.mark_dirty();
                }
                Err(e) => return log::warn!("signal: profile picture lookups stopped: {e}"),
            }
        }
        loop {
            let due = lock(&self.store).avatars_due(20);
            if due.is_empty() {
                return;
            }
            for (chat_id, stored) in due {
                if self.generation.load(Ordering::SeqCst) != generation {
                    return;
                }
                match self.fetch_avatar(&dir, &chat_id, &chat_id, stored.as_deref()).await {
                    Ok(picture) => lock(&self.store).set_avatar(&chat_id, picture),
                    Err(e) => return log::warn!("signal: profile picture lookups stopped: {e}"),
                }
                self.mark_dirty();
                tokio::time::sleep(AVATAR_PACE).await;
            }
        }
    }

    /// Reacts to a message, or takes the reaction back when it is already there.
    pub async fn react(&self, chat_id: &str, message_id: &str, emoji: &str) -> Result<MessageDto, String> {
        self.recall(chat_id, message_id);
        let (target, me) = {
            let store = lock(&self.store);
            (
                store.reaction_target(chat_id, message_id).ok_or("That message is gone.")?,
                store.own_id().ok_or("Signal is not connected.")?.to_string(),
            )
        };
        let remove = target.mine.as_deref() == Some(emoji);
        let author = if target.from_me { me.clone() } else { target.author.ok_or("That message has no sender.")? };
        let command = serde_json::json!({
            "cmd": "react",
            "chat": chat_id,
            "targetAuthor": author,
            "targetTs": target.ts_ms,
            "emoji": emoji,
            "remove": remove,
            "ts": now_ms(),
        });
        self.request(command, SEND_TIMEOUT)
            .await
            .map_err(|e| format!("The reaction was not sent: {e}"))?;
        let dto = lock(&self.store)
            .set_reaction(chat_id, message_id, &me, (!remove).then_some(emoji))
            .ok_or("That message is gone.")?;
        self.mark_dirty();
        Ok(dto)
    }

    /// Deletes a message here, or for everyone when `for_everyone` and it is
    /// ours. Returns what is left of it, if anything is shown.
    pub async fn delete_message(&self, chat_id: &str, message_id: &str, for_everyone: bool) -> Result<Option<MessageDto>, String> {
        self.recall(chat_id, message_id);
        if for_everyone {
            let target = lock(&self.store).quote_source(chat_id, message_id).ok_or("That message is gone.")?;
            if !target.from_me {
                return Err("Only your own messages can be deleted for everyone.".into());
            }
            let command = serde_json::json!({ "cmd": "delete", "chat": chat_id, "targetTs": target.ts_ms, "ts": now_ms() });
            self.request(command, SEND_TIMEOUT)
                .await
                .map_err(|e| format!("The message was not deleted: {e}"))?;
        }
        let how = if for_everyone { Deletion::ForEveryone } else { Deletion::ForMe };
        let dto = lock(&self.store).delete_message(chat_id, message_id, how);
        self.mark_dirty();
        Ok(dto.filter(|_| for_everyone))
    }

    /// Sends a text, quoting the message `reply_to` when there is one.
    /// The message `reply_to` names, as the bridge quotes it and as it is
    /// shown here.
    fn quote(&self, chat_id: &str, reply_to: Option<&str>) -> Result<Option<(serde_json::Value, ReplyRef)>, String> {
        let Some(id) = reply_to else {
            return Ok(None);
        };
        self.recall(chat_id, id);
        let store = lock(&self.store);
        let source = store.quote_source(chat_id, id).ok_or("That message is gone.")?;
        let author = match source.sender {
            Some(sender) if !source.from_me => sender,
            _ => store.own_id().ok_or("Signal is not connected.")?.to_string(),
        };
        let quote = serde_json::json!({ "ts": source.ts_ms, "author": author, "text": source.text });
        Ok(Some((quote, ReplyRef { author_jid: Some(author), id: Some(id.to_string()), text: source.text })))
    }

    /// Files a message we just sent, keyed by its timestamp, and returns it
    /// as the UI shows it.
    fn store_sent(&self, chat_id: &str, ts: i64, content: Content) -> Result<MessageDto, String> {
        let is_group = lock(&self.store)
            .list_chats()
            .iter()
            .any(|c| c.id == chat_id && c.is_group);
        let incoming = Incoming {
            chat: chat_id.to_string(),
            is_group,
            sender: None,
            push_name: None,
            lid_pn_pairs: Vec::new(),
            message: StoredMessage {
                id: ts.to_string(),
                from_me: true,
                ts_ms: ts,
                author_jid: None,
                content,
                status: Some(Delivery::Sent),
                reactions: Vec::new(),
                wire: None,
                read_by: Vec::new(),
                starred: false,
                deleted: None,
            },
        };
        let dto = lock(&self.store)
            .ingest(incoming, false)
            .ok_or("The message was sent but could not be shown.")?;
        self.mark_dirty();
        Ok(dto)
    }

    pub async fn send_text(
        &self,
        chat_id: &str,
        text: &str,
        reply_to: Option<&str>,
        link: Option<OutgoingLink>,
        styled: Option<Styled>,
    ) -> Result<MessageDto, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("The message is empty.".into());
        }
        let (quote, reply) = self.quote(chat_id, reply_to)?.unzip();
        let ts = now_ms();
        let link = link.filter(|l| text.contains(&l.url));
        // The card's picture goes to the bridge as a file, like an attachment,
        // and is uploaded with the message.
        let picture = match link.as_ref().and_then(|l| OutgoingLink::decoded(&l.image)) {
            Some(bytes) => {
                let staged = crate::vault::handoff_path("signal-outbox").map_err(|e| e.to_string())?;
                fs::write(&staged, &bytes).map_err(|e| format!("could not hand the link picture over: {e}"))?;
                Some(staged)
            }
            None => None,
        };
        let preview = link.as_ref().map(|l| {
            serde_json::json!({
                "url": l.url,
                "title": l.title,
                "description": l.description,
                "path": picture,
                "width": l.width,
                "height": l.height,
            })
        });
        // Signal formats with ranges over the text without its markers; the
        // text kept here keeps them, as WhatsApp's does.
        let (body, ranges) = match styled.filter(|s| !s.ranges.is_empty()) {
            Some(styled) => (styled.body, styled.ranges),
            None => (text.to_string(), Vec::new()),
        };
        let command = serde_json::json!({
            "cmd": "send",
            "chat": chat_id,
            "text": body,
            "ts": ts,
            "quote": quote,
            "preview": preview,
            "ranges": ranges,
        });
        let sent = self.request(command, UPLOAD_TIMEOUT).await;
        if let Some(path) = picture {
            let _ = fs::remove_file(path);
        }
        sent.map_err(|e| format!("The message was not sent: {e}"))?;
        let mut content = Content::text(text, reply);
        content.link = link.map(|l| LinkPreview {
            thumbnail: OutgoingLink::decoded(&l.thumbnail)
                .map(|bytes| format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))),
            url: l.url,
            title: l.title,
            description: l.description,
            large: None,
        });
        self.store_sent(chat_id, ts, content)
    }

    /// Hands a file to the bridge to upload and send, and keeps it as the
    /// downloaded copy of the attachment we show.
    pub async fn send_file(&self, file: OutgoingFile, bytes: Vec<u8>) -> Result<MessageDto, String> {
        let chat_id = file.chat_id.as_str();
        let (quote, reply) = self.quote(chat_id, file.reply_to.as_deref())?.unzip();
        let mime = match file.base_mime() {
            mime if mime.is_empty() => "application/octet-stream".to_string(),
            mime => mime,
        };
        let (kind, msg_kind, label) = match mime.split('/').next().unwrap_or_default() {
            // A drawing, not a picture Signal's apps show: it goes as a file.
            "image" if mime == "image/svg+xml" => ("file", MsgKind::Media, file.name.clone()),
            "image" => ("image", MsgKind::Image, "Photo".to_string()),
            "video" => ("video", MsgKind::Media, "Video".to_string()),
            "audio" => ("audio", MsgKind::Media, "Audio".to_string()),
            _ => ("file", MsgKind::Media, file.name.clone()),
        };

        // The bridge reads the file from here rather than through the pipe,
        // which carries one JSON line per command.
        // It has to be plain for the bridge, so it waits in memory, not on disk.
        let ts = now_ms();
        let staged = crate::vault::handoff_path("signal-outbox").map_err(|e| e.to_string())?;
        fs::write(&staged, &bytes).map_err(|e| format!("could not hand the file over: {e}"))?;
        let command = serde_json::json!({
            "cmd": "send_file",
            "chat": chat_id,
            "path": staged,
            "mime": mime,
            "fileName": file.name,
            "kind": kind,
            "width": file.width,
            "height": file.height,
            "caption": file.caption(),
            "quote": quote,
            "ts": ts,
        });
        let answer = self.request(command, UPLOAD_TIMEOUT).await;
        let media = match answer {
            Ok(Some(media)) => serde_json::from_str::<MediaRef>(&media).ok(),
            Ok(None) => None,
            Err(e) => {
                let _ = fs::remove_file(&staged);
                return Err(format!("The file was not sent: {e}"));
            }
        };
        // Kept where a download of it would go, so it opens without one.
        match &media {
            Some(media) => {
                let path = self.dir.join(MEDIA_DIR).join(media_file_name(media));
                if let Some(parent) = path.parent() {
                    let _ = secure_dir(parent);
                }
                if let Err(e) = crate::vault::get().import(&staged, &path) {
                    log::warn!("signal: could not keep a copy of the sent file: {e}");
                }
            }
            None => {
                let _ = fs::remove_file(&staged);
            }
        }
        let mut content = Content::media(msg_kind, label, file.caption());
        content.reply_to = reply;
        content.attachments.extend(media);
        self.store_sent(chat_id, ts, content)
    }

    fn handle_event(self: &Arc<Self>, event: BridgeEvent) {
        match event {
            BridgeEvent::State { state, error } => match state.as_str() {
                // Waiting for the link command we already queued.
                "idle" => {}
                "qr_expired" => self.set_status(|s| {
                    s.state = AccountState::QrExpired;
                    s.qr_svg = None;
                }),
                "connecting" => self.set_status(|s| {
                    s.state = AccountState::Connecting;
                    s.qr_svg = None;
                    s.error = None;
                }),
                "connected" => {
                    self.set_status(|s| {
                        s.state = AccountState::Connected;
                        s.error = None;
                    });
                    self.sync_avatars();
                }
                _ => self.fail(error.unwrap_or_else(|| "Signal reported a problem.".into())),
            },
            BridgeEvent::Qr { url, ttl_secs } => {
                let svg = render_qr(&url);
                self.set_status(|s| {
                    s.state = AccountState::Qr;
                    s.qr_svg = svg;
                    s.qr_timeout_secs = Some(ttl_secs);
                    s.error = None;
                });
            }
            BridgeEvent::Me { id, phone, name } => {
                if let Err(e) = fs::write(self.dir.join(LINKED_MARKER), b"") {
                    log::warn!("signal: could not write link marker: {e}");
                }
                let avatar = {
                    let mut store = lock(&self.store);
                    store.set_own_ids([id]);
                    store.own_avatar()
                };
                self.set_status(|s| {
                    s.linked = true;
                    s.qr_svg = None;
                    let name = name.or_else(|| s.me.as_ref().and_then(|me| me.name.clone()));
                    s.me = Some(Me { phone: Some(phone), name, avatar });
                });
                self.mark_dirty();
            }
            BridgeEvent::Contact { id, name, saved } => {
                let mut store = lock(&self.store);
                if saved {
                    store.set_contact_name(&id, &name);
                } else {
                    store.set_push_name(&id, &name);
                }
                drop(store);
                self.mark_dirty();
            }
            BridgeEvent::Group { id, name } => {
                lock(&self.store).set_group_name(&id, &name);
                self.mark_dirty();
            }
            BridgeEvent::Message { chat, is_group, sender, from_me, ts, kind, text, media_label, quote, attachments, history, delivery } => {
                let reply_to = quote.map(|q| ReplyRef {
                    id: q.author.as_deref().zip(q.ts).map(|(author, ts)| self.message_id(author, ts)),
                    author_jid: q.author,
                    text: q.text,
                });
                let content = Content {
                    kind,
                    text,
                    media_label,
                    voice_seconds: None,
                    reply_to,
                    attachments,
                    media: None,
                    wa_media: None,
                    link: None,
                    forwarded: 0,
                };
                let id = if from_me { ts.to_string() } else { self.message_id(&sender, ts) };
                let incoming = Incoming {
                    chat,
                    is_group,
                    sender: Some(sender),
                    push_name: None,
                    lid_pn_pairs: Vec::new(),
                    message: StoredMessage {
                        id,
                        from_me,
                        ts_ms: ts,
                        author_jid: None,
                        content,
                        status: from_me.then(|| delivery.unwrap_or(Delivery::Sent)),
                        reactions: Vec::new(),
                        wire: None,
                        read_by: Vec::new(),
                        starred: false,
                        deleted: None,
                    },
                };
                // History arrives in bulk and was already seen on the phone.
                let mut store = lock(&self.store);
                if let Some(dto) = store.ingest(incoming, !history)
                    && !history
                {
                    // As on the phone, a new message brings an archived chat
                    // back, unless it is muted.
                    store.unarchive_unless_muted(&dto.chat_id);
                    let notice = store.message_notice(&dto.chat_id, &dto.id);
                    drop(store);
                    let _ = self.app.emit("signal://message", dto);
                    if let Some(notice) = notice {
                        crate::notify::deliver(&self.app, "signal", "signal://notice", notice);
                    }
                }
                self.mark_dirty();
            }
            BridgeEvent::Delete { chat, sender, target_ts, history } => {
                let id = self.message_id(&sender, target_ts);
                self.recall(&chat, &id);
                let deleted = lock(&self.store).delete_message(&chat, &id, Deletion::ForEveryone);
                if let Some(dto) = deleted {
                    if !history {
                        let _ = self.app.emit("signal://message", dto);
                    }
                    self.mark_dirty();
                }
            }
            BridgeEvent::Reaction { chat, sender, target_author, target_ts, emoji, remove, history } => {
                let id = self.message_id(&target_author, target_ts);
                self.recall(&chat, &id);
                let (changed, notice) = {
                    let mut store = lock(&self.store);
                    let changed = store.set_reaction(&chat, &id, &sender, (!remove).then_some(&emoji));
                    let notice = (changed.is_some() && !remove && !history)
                        .then(|| store.reaction_notice(&chat, &id, &sender, &emoji))
                        .flatten();
                    (changed, notice)
                };
                if let Some(dto) = changed {
                    if !history {
                        let _ = self.app.emit("signal://message", dto);
                    }
                    self.mark_dirty();
                }
                if let Some(notice) = notice {
                    crate::notify::deliver(&self.app, "signal", "signal://notice", notice);
                }
            }
            BridgeEvent::History { state, percent, error } => self.set_status(|s| {
                s.sync_progress = matches!(state.as_str(), "waiting" | "importing")
                    .then_some(percent.unwrap_or(0));
                if error.is_some() {
                    s.notice = error;
                }
            }),
            BridgeEvent::Receipt { ids, delivery } => {
                let changed = lock(&self.store).apply_receipt_anywhere(&ids, delivery);
                if !changed.is_empty() {
                    for dto in changed {
                        let _ = self.app.emit("signal://message", dto);
                    }
                    self.mark_dirty();
                }
            }
            BridgeEvent::Read { messages } => {
                let ids: Vec<String> = messages.iter().map(|m| self.message_id(&m.sender, m.ts)).collect();
                let mut store = lock(&self.store);
                for id in ids {
                    if let Some(chat) = store.chat_of(&id) {
                        store.mark_read(&chat);
                    }
                }
                drop(store);
                self.mark_dirty();
            }
            BridgeEvent::ProfileKey { id } => {
                if lock(&self.store).retry_missing_avatar(&id) {
                    self.sync_avatars();
                }
            }
            BridgeEvent::Avatar { id, picture, error } => {
                if let Some(waiter) = lock(&self.pending).remove(&id) {
                    let _ = waiter.send(error.map_or(Ok(picture), Err));
                }
            }
            BridgeEvent::Uploaded { id, media, error } => {
                if let Some(waiter) = lock(&self.pending).remove(&id) {
                    let _ = waiter.send(error.map_or(Ok(media.map(|m| m.to_string())), Err));
                }
            }
            BridgeEvent::Done { id, error } => {
                if let Some(waiter) = lock(&self.pending).remove(&id) {
                    let _ = waiter.send(error.map_or(Ok(None), Err));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines as the bridge prints them.
    #[test]
    fn bridge_output_is_understood() {
        let lines = [
            r#"{"event":"state","state":"idle","error":null}"#,
            r#"{"event":"qr","url":"sgnl://linkdevice?uuid=a&pub_key=b","ttlSecs":90}"#,
            r#"{"event":"me","id":"9d0652a3-dcc3-4d11-975f-74d61598733f","phone":"+351900000000","name":null}"#,
            r#"{"event":"contact","id":"9d0652a3-dcc3-4d11-975f-74d61598733f","name":"Maya","saved":true}"#,
            r#"{"event":"group","id":"g.00ff","name":"Crew"}"#,
            r#"{"event":"message","chat":"g.00ff","isGroup":true,"sender":"9d0652a3-dcc3-4d11-975f-74d61598733f","fromMe":false,"ts":1790000000000,"kind":"image","text":null,"mediaLabel":"Photo","quote":{"author":null,"text":"hi"}}"#,
            r#"{"event":"message","chat":"x","isGroup":false,"sender":"x","fromMe":true,"ts":1,"kind":"text","text":"hi","mediaLabel":null,"quote":null,"history":true,"delivery":"read"}"#,
            r#"{"event":"history","state":"importing","percent":40,"error":null}"#,
            r#"{"event":"receipt","ids":["1790000000000"],"delivery":"read"}"#,
            r#"{"event":"read","messages":[{"sender":"9d0652a3-dcc3-4d11-975f-74d61598733f","ts":1790000000000}]}"#,
            r#"{"event":"done","id":3,"error":null}"#,
            r#"{"event":"profile_key","id":"9d0652a3-dcc3-4d11-975f-74d61598733f"}"#,
            r#"{"event":"avatar","id":4,"picture":"profiles/AbC_-d","error":null}"#,
            r#"{"event":"avatar","id":5,"picture":null,"error":"Signal answered 429"}"#,
            r#"{"event":"reaction","chat":"x","sender":"y","targetAuthor":"x","targetTs":1,"emoji":"👍","remove":false,"history":true}"#,
            r#"{"event":"message","chat":"x","isGroup":false,"sender":"x","fromMe":false,"ts":1,"kind":"image","text":"hi","mediaLabel":"2 photos","quote":null,"attachments":[{"mime":"image/png","cdn":3,"cdnKey":"abc","key":"00","size":10,"kind":"image","fileName":null,"width":2162,"height":414},{"mime":"image/png","cdn":3,"cdnKey":"def","key":"00","size":10,"kind":"image","fileName":null,"width":0,"height":0}],"history":false,"delivery":null}"#,
        ];
        for line in lines {
            serde_json::from_str::<BridgeEvent>(line).unwrap_or_else(|e| panic!("{line}: {e}"));
        }
    }

    #[test]
    fn attachments_get_names_the_system_understands() {
        assert_eq!(extension("video/mp4", None), "mp4");
        assert_eq!(extension("application/pdf", Some("Report.PDF")), "pdf");
        assert_eq!(extension("application/octet-stream", Some("no extension")), "bin");
        assert_eq!(safe_file_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(safe_file_name(".hidden").as_deref(), Some("hidden"));
        assert_eq!(safe_file_name("..").as_deref(), None);
    }
}
