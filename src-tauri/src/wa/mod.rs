pub mod commands;
mod convert;
pub mod stickers;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use whatsapp_rust::download::{DownloadParams, MediaDecryptionError, MediaType};
use whatsapp_rust::features::{GroupDescription, GroupSubject, PreviousDescription, SyncActionMessageRange};
use whatsapp_rust::wacore::stanza::groups::{GroupNotificationAction, GroupParticipantInfo};
use whatsapp_rust::http::HttpRequest;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::store::DevicePropsOverride;
use whatsapp_rust::wacore::types::events::LazyHistorySync;
use whatsapp_rust::wacore::iq::privacy::{PrivacyCategory, PrivacyValue};
use whatsapp_rust::wacore::types::message::AddressingMode;
use whatsapp_rust::wacore::types::presence::{ChatActivity, ReceiptType};
use whatsapp_rust::waproto::buffa::MessageField;
use whatsapp_rust::waproto::whatsapp::sync_action_value::label_edit_action::ListType;
use whatsapp_rust::waproto::whatsapp::sync_action_value::{FavoritesAction, LabelEditAction, favorites_action};
use whatsapp_rust::{
    AppStateResyncMode, ErrorChainExt, MediaRetryResult, MediaReuploadRequest, PresencePolicy, PresenceStatus,
    EditRequest, MessageId, MessageRef, PinDuration, ProfilePictureLookup, ProfilePictureRequest, ProfilePictureTarget, ProfilePictureType,
    RevokeType, UploadOptions, WAPatchName, message_range, schemas,
};

use crate::account::{
    AccountState as WaState, AccountStatus as WaStatus, Backfill, BackfillState, Me, OutgoingFile, OutgoingLink, lock, render_qr, secure_dir,
};
use crate::store;
use crate::store::{
    CdnRef, ChatStore, History, ContactDto, Content, Deletion, Delivery, LinkPreview, ListDto, ReplyRef, MessageDto, Notice, PinsDto, StarredDto, StoredMessage,
    now_ms,
};
pub use crate::store::ChatSummary;

const SESSION_FILE: &str = "session.db";
const LINKED_MARKER: &str = "linked";
const CACHE_FILE: &str = "chats.json";
pub const AVATAR_DIR: &str = "avatars";
pub const LINK_THUMB_DIR: &str = "link-thumbs";
pub const MEDIA_DIR: &str = "media";
const SAVE_INTERVAL: Duration = Duration::from_secs(3);
/// `HistorySyncType::ON_DEMAND`: the phone answering a request for older messages.
const ON_DEMAND: i32 = 6;
/// Fetching all history waits this long after connecting before it begins…
const BACKFILL_START: Duration = Duration::from_secs(30);
/// …asks for this many messages at a time…
const BACKFILL_PAGE: i32 = 50;
/// …about this many chats at once, so the phone handles them in one go
/// (each time it is woken it shows a "syncing" notification)…
const BACKFILL_BURST: usize = 6;
/// …and lets this much time pass between bursts, to stay light.
const BACKFILL_PACE: Duration = Duration::from_secs(3);
/// How long the phone has to answer a request.
const BACKFILL_TIMEOUT: Duration = Duration::from_secs(40);
/// Unanswered requests after which a chat counts as complete.
const BACKFILL_STRIKES: u32 = 2;
/// Chats in a row without an answer after which the phone counts as away.
const BACKFILL_SILENT: u32 = 3;
/// How long to wait for a phone that is away before trying again.
const BACKFILL_RETRY: Duration = Duration::from_secs(5 * 60);
/// How often a fetch with nothing left to do looks again.
const BACKFILL_IDLE: Duration = Duration::from_secs(30 * 60);
/// Pause between profile picture lookups, to stay clear of the rate limit.
const AVATAR_PACE: Duration = Duration::from_millis(150);
/// How many chats the phone is asked about when old ticks are refreshed, how
/// many messages of each, and the pause between the requests: short, so the
/// phone deals with them in one go.
const TICK_CHATS: usize = 40;
const TICK_MESSAGES: usize = 50;
const TICK_PACE: Duration = Duration::from_millis(250);

/// Why a CDN fetch failed. `Rejected` means the reference itself is no longer
/// served, which a re-upload can fix.
enum CdnError {
    Rejected(String),
    Failed(String),
}

impl From<String> for CdnError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl From<&str> for CdnError {
    fn from(message: &str) -> Self {
        Self::Failed(message.to_string())
    }
}

/// Where a downloaded file is kept inside its folder: `prefix` and its hash,
/// except for documents, which keep the name they were sent with in a folder
/// of their own.
fn cdn_file_name(file: &CdnRef, prefix: &str) -> Result<String, String> {
    let sha256 = BASE64.decode(&file.sha256).map_err(|e| format!("Bad file reference: {e}"))?;
    let hash: String = sha256.iter().map(|b| format!("{b:02x}")).collect();
    let mime = file.mime.as_deref().unwrap_or("image/jpeg");
    Ok(match (file.kind.as_deref(), file.file_name.as_deref().and_then(crate::signal::safe_file_name)) {
        (Some("file"), Some(file_name)) => format!("{prefix}-{hash}/{file_name}"),
        _ => format!("{prefix}-{hash}.{}", crate::signal::extension(mime, file.file_name.as_deref())),
    })
}

impl From<CdnError> for String {
    fn from(error: CdnError) -> Self {
        match error {
            CdnError::Rejected(message) | CdnError::Failed(message) => message,
        }
    }
}

/// Which WhatsApp app a session is linked to. WhatsApp Business links the
/// same way and speaks the same protocol; each keeps its own session, chats
/// and files, in its own folder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Personal,
    Business,
}

impl Kind {
    /// The account's name in the UI and in notifications.
    pub fn account(self) -> &'static str {
        match self {
            Kind::Personal => "whatsapp",
            Kind::Business => "business",
        }
    }

    /// What the phone calls WhatsApp's own chat in this app.
    fn official_name(self) -> &'static str {
        match self {
            Kind::Personal => "WhatsApp",
            Kind::Business => "WhatsApp Business",
        }
    }

    /// The account's folder, under the app's data folder.
    pub fn dir(self, data: &std::path::Path) -> PathBuf {
        match self {
            Kind::Personal => data.to_path_buf(),
            Kind::Business => data.join("business"),
        }
    }

    /// Name of an event the UI listens to for this account.
    fn event(self, name: &str) -> String {
        let prefix = match self {
            Kind::Personal => "wa",
            Kind::Business => "wab",
        };
        format!("{prefix}://{name}")
    }
}

/// Both WhatsApp accounts. A command serves the business one when the UI
/// names it in the [`ACCOUNT_HEADER`].
pub struct WaAccounts {
    pub personal: Arc<WaManager>,
    pub business: Arc<WaManager>,
}

impl WaAccounts {
    pub fn all(&self) -> [&Arc<WaManager>; 2] {
        [&self.personal, &self.business]
    }

    /// The account whose stickers the other networks use: the personal one,
    /// or the business one when only it is linked.
    pub fn stickers(&self) -> &Arc<WaManager> {
        if !self.personal.is_linked() && self.business.is_linked() { &self.business } else { &self.personal }
    }
}

pub const ACCOUNT_HEADER: &str = "x-wa-account";

/// A file's download parameters, with the full URL of WhatsApp's static host
/// when the file is there rather than on the CDN.
struct StaticUrl {
    params: DownloadParams,
    url: Option<String>,
}

impl whatsapp_rust::download::Downloadable for StaticUrl {
    fn direct_path(&self) -> Option<&str> {
        self.params.direct_path()
    }
    fn media_key(&self) -> Option<&[u8]> {
        self.params.media_key()
    }
    fn file_enc_sha256(&self) -> Option<&[u8]> {
        self.params.file_enc_sha256()
    }
    fn file_sha256(&self) -> Option<&[u8]> {
        self.params.file_sha256()
    }
    fn file_length(&self) -> Option<u64> {
        self.params.file_length()
    }
    fn app_info(&self) -> MediaType {
        self.params.app_info()
    }
    fn static_url(&self) -> Option<&str> {
        self.url.as_deref()
    }
}

pub struct WaManager {
    app: AppHandle,
    kind: Kind,
    dir: PathBuf,
    status: Mutex<WaStatus>,
    store: Mutex<ChatStore>,
    /// Every message, and the store, on disk.
    history: History,
    bot: tokio::sync::Mutex<Option<BotHandle>>,
    /// Bumped whenever a bot is started or torn down, so events from a bot we
    /// already abandoned are ignored.
    generation: AtomicU64,
    chats_dirty: AtomicBool,
    save_dirty: AtomicBool,
    /// Wakes the refresh / save loop, which otherwise sleeps.
    wake: tokio::sync::Notify,
    avatars_running: AtomicBool,
    /// Messages each chat got from the phone on request, since last looked at.
    on_demand: Mutex<HashMap<String, u32>>,
    /// Rung when the phone answers a request for older messages.
    on_demand_answered: tokio::sync::Notify,
    /// The session whose history is being fetched, so a reconnect doesn't start a second run.
    backfill_for: AtomicU64,
    /// A chat opened while it had no messages here: fetched next.
    backfill_next: Mutex<Option<String>>,
    /// Rung when a chat is opened, for a fetch that had nothing left to do.
    backfill_wake: tokio::sync::Notify,
    /// The window is open and in front: we show as online, as WhatsApp Web
    /// does while it is looked at.
    active: AtomicBool,
    /// What the server was last told: 0 nothing since connecting, 1 offline, 2 online.
    announced: std::sync::atomic::AtomicU8,
    /// Recent and favorite stickers, sticker packs and recent GIFs.
    /// The app's sticker library, shared by both accounts.
    stickers: Arc<crate::stickers::Central>,
    /// WhatsApp's sticker store's list of packs, and when it was fetched.
    sticker_store: tokio::sync::Mutex<Option<(Instant, Vec<whatsapp_rust::sticker_pack::StickerPack>)>>,
    /// When each store pack was last fetched again for stickers whose
    /// address expired; held while one is, so a panel full of them asks once.
    pack_refreshes: tokio::sync::Mutex<HashMap<String, Instant>>,
    /// Expired files the phone did not send again, by message id, and when it
    /// last failed: not asked for again for a while.
    reupload_failed: Mutex<HashMap<String, Instant>>,
    /// Re-upload requests to the phone at once: an old chat full of expired
    /// photos would otherwise ask for all of them together.
    reuploads: tokio::sync::Semaphore,
}

/// How long a file the phone did not send again is left alone before asking again.
pub(super) const REUPLOAD_RETRY_AFTER: Duration = Duration::from_secs(60 * 60);

/// How long the window can be out of focus before we show as offline, so
/// a menu or a quick switch doesn't flicker it.
const AWAY_AFTER: Duration = Duration::from_secs(10);

/// Someone typing or recording in a chat, or stopping.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TypingDto {
    pub(crate) chat_id: String,
    /// Who, in a group.
    pub(crate) who: Option<String>,
    pub(crate) recording: bool,
    pub(crate) active: bool,
}

/// Whether someone is online, or when they last were.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PresenceDto {
    chat_id: String,
    online: bool,
    last_seen_ms: Option<i64>,
}

/// The account's privacy settings that decide what others see of it, as
/// WhatsApp names their values.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyDto {
    last_seen: Option<String>,
    online: Option<String>,
    read_receipts: Option<String>,
}

impl WaManager {
    pub fn new(app: AppHandle, data: &std::path::Path, kind: Kind, stickers: Arc<crate::stickers::Central>) -> std::io::Result<Arc<Self>> {
        let dir = kind.dir(data);
        secure_dir(&dir)?;
        let linked = dir.join(LINKED_MARKER).exists();
        let history = History::new(&dir, "whatsapp history", CACHE_FILE);
        let mut store = if linked {
            history
                .load()
                .unwrap_or_else(|| {
                    // The session already applied its app state, so a rebuilt
                    // cache has to ask for pins, mutes and archives again.
                    let mut store = ChatStore::default();
                    store.set_chat_state_synced(false);
                    store
                })
        } else {
            ChatStore::default()
        };
        store.set_official_name(kind.official_name());
        Ok(Arc::new(Self {
            app,
            kind,
            status: Mutex::new(WaStatus::idle(linked)),
            store: Mutex::new(store),
            history,
            bot: tokio::sync::Mutex::new(None),
            generation: AtomicU64::new(0),
            chats_dirty: AtomicBool::new(false),
            save_dirty: AtomicBool::new(false),
            wake: tokio::sync::Notify::new(),
            avatars_running: AtomicBool::new(false),
            on_demand: Mutex::new(HashMap::new()),
            on_demand_answered: tokio::sync::Notify::new(),
            backfill_for: AtomicU64::new(u64::MAX),
            backfill_next: Mutex::new(None),
            backfill_wake: tokio::sync::Notify::new(),
            active: AtomicBool::new(false),
            announced: std::sync::atomic::AtomicU8::new(0),
            stickers,
            sticker_store: tokio::sync::Mutex::new(None),
            pack_refreshes: tokio::sync::Mutex::new(HashMap::new()),
            reupload_failed: Mutex::new(HashMap::new()),
            reuploads: tokio::sync::Semaphore::new(3),
            dir,
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
                        mgr.emit("chats", ());
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
                if let Err(e) = mgr.start_bot().await {
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

    pub fn status(&self) -> WaStatus {
        lock(&self.status).clone()
    }

    /// Sends an event to the UI, named for this account.
    fn emit<S: Serialize + Clone>(&self, name: &str, payload: S) {
        let _ = self.app.emit(&self.kind.event(name), payload);
    }

    fn set_status(&self, f: impl FnOnce(&mut WaStatus)) {
        let snapshot = {
            let mut status = lock(&self.status);
            f(&mut status);
            status.seq += 1;
            status.clone()
        };
        self.emit("status", snapshot);
    }

    fn fail(&self, error: String) {
        log::warn!("whatsapp: {error}");
        self.set_status(|s| {
            s.state = WaState::Error;
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

    /// The messages pinned in a chat, the latest pin first, and who pinned
    /// what. Pinned messages only the history database has are brought back
    /// into memory.
    pub fn pinned_messages(&self, chat_id: &str) -> PinsDto {
        let ids = lock(&self.store).pinned_ids(chat_id);
        for id in &ids {
            self.recall(chat_id, id);
        }
        let store = lock(&self.store);
        PinsDto {
            messages: ids.iter().filter_map(|id| store.message_dto(chat_id, id)).collect(),
            notes: store.pin_notes(chat_id),
        }
    }

    /// Pins a message for everyone for `secs` (24 hours, 7 days or 30
    /// days), or unpins it when `secs` is `None`.
    pub async fn pin_message(&self, chat_id: &str, message_id: &str, secs: Option<u32>) -> Result<(), String> {
        self.recall(chat_id, message_id);
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let target = lock(&self.store).reaction_target(chat_id, message_id).ok_or("That message is gone.")?;
        let key = {
            let mut key = wa::MessageKey::default();
            key.remote_jid = Some(chat_id.to_string());
            key.from_me = Some(target.from_me);
            key.id = Some(message_id.to_string());
            key.participant = target.author.filter(|_| chat_id.ends_with("@g.us") && !target.from_me);
            key
        };
        let at_ms = now_ms();
        let until_ms = match secs {
            Some(secs) => {
                let duration = match secs {
                    0..=86_400 => PinDuration::Hours24,
                    86_401..=604_800 => PinDuration::Days7,
                    _ => PinDuration::Days30,
                };
                client.pin_message_raw(jid, key, duration).await.map_err(|e| format!("The message was not pinned: {e}"))?;
                Some(at_ms + secs as i64 * 1000)
            }
            None => {
                client.unpin_message_raw(jid, key).await.map_err(|e| format!("The message was not unpinned: {e}"))?;
                None
            }
        };
        let chat = {
            let mut store = lock(&self.store);
            store.set_message_pin(chat_id, message_id, at_ms, until_ms, None);
            store.chat_key(chat_id)
        };
        self.emit("pins", chat);
        self.mark_dirty();
        Ok(())
    }

    /// Messages of a chat from before `before_ms`, for scrolling back past
    /// what memory keeps.
    pub fn older_messages(&self, chat_id: &str, before_ms: i64, limit: usize) -> Vec<MessageDto> {
        let older = self.history.older(chat_id, before_ms, limit);
        lock(&self.store).dtos(chat_id, &older)
    }

    fn write_marker(&self) {
        if let Err(e) = fs::write(self.dir.join(LINKED_MARKER), b"") {
            log::warn!("whatsapp: could not write link marker: {e}");
        }
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
                log::warn!("whatsapp: could not remove {}: {e}", path.display());
            }
        }
        let avatars = self.dir.join(AVATAR_DIR);
        if avatars.exists()
            && let Err(e) = fs::remove_dir_all(&avatars)
        {
            log::warn!("whatsapp: could not remove {}: {e}", avatars.display());
        }
        for name in [LINK_THUMB_DIR, MEDIA_DIR] {
            let downloads = self.dir.join(name);
            if downloads.exists()
                && let Err(e) = fs::remove_dir_all(&downloads)
            {
                log::warn!("whatsapp: could not remove {}: {e}", downloads.display());
            }
        }
        // A new link replays app state while history is still arriving, so
        // ask for it again once connected.
        let mut store = ChatStore::default();
        store.set_chat_state_synced(false);
        store.set_lists_synced(false);
        store.set_official_name(self.kind.official_name());
        *lock(&self.store) = store;
        self.save_dirty.store(false, Ordering::Release);
        self.chats_dirty.store(true, Ordering::Release);
        self.wake.notify_one();
    }

    async fn client(&self) -> Option<Arc<Client>> {
        self.bot.lock().await.as_ref().map(|h| h.client())
    }

    async fn start_bot(self: &Arc<Self>) -> Result<(), String> {
        let mut slot = self.bot.lock().await;
        if slot.is_some() {
            return Ok(());
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let linked = self.is_linked();
        self.set_status(|s| {
            s.state = if linked { WaState::Connecting } else { WaState::Starting };
            s.linked = linked;
            s.error = None;
            s.qr_svg = None;
        });

        let session = self.dir.join(SESSION_FILE);
        let key = crate::sqlcipher::key("whatsapp session");
        {
            let (session, key) = (session.clone(), key.clone());
            tauri::async_runtime::spawn_blocking(move || crate::sqlcipher::encrypt_in_place(&session, &key))
                .await
                .map_err(|e| e.to_string())??;
        }
        let config = whatsapp_rust::store::SqliteDatabaseConfig::default()
            .with_connection_init(move |conn| crate::sqlcipher::unlock(conn, &key));
        // One account, device 1, as the store has always been opened.
        let backend = whatsapp_rust::store::SqliteDatabase::open(&session.to_string_lossy(), config)
            .await
            .map_err(|e| format!("Could not open the WhatsApp session store: {e}"))?
            .store(1);

        let mgr = self.clone();
        let bot = Bot::builder()
            .with_backend(backend)
            .with_device_props(
                DevicePropsOverride::new()
                    .with_os("Omega Zap")
                    .with_platform_type(wa::device_props::PlatformType::CHROME),
            )
            .with_event_delivery(EventDelivery::Ordered { capacity: 8192 })
            // Online only while the window is in front, not whenever the app
            // runs in the tray: that would also keep the phone from ringing.
            .with_presence_policy(PresencePolicy::Manual)
            .on_event(move |event, client| {
                let mgr = mgr.clone();
                async move { mgr.handle_event(generation, event, client).await }
            })
            .build()
            .await
            .map_err(|e| format!("Could not start WhatsApp: {e}"))?;

        *slot = Some(bot.spawn());
        Ok(())
    }

    async fn stop_bot(&self) {
        let handle = self.bot.lock().await.take();
        if let Some(handle) = handle
            && tokio::time::timeout(Duration::from_secs(8), handle.shutdown())
                .await
                .is_err()
        {
            log::warn!("whatsapp: shutdown timed out; the connection was dropped");
        }
    }

    pub async fn start_linking(self: &Arc<Self>) -> Result<(), String> {
        if self.is_linked() {
            return Err("This computer is already linked.".into());
        }
        if self.bot.lock().await.is_some() {
            return Ok(());
        }
        // Leftovers from an abandoned attempt would otherwise be reused.
        self.wipe_files();
        self.start_bot().await.inspect_err(|e| self.fail(e.clone()))
    }

    pub async fn cancel_linking(self: &Arc<Self>) {
        if self.is_linked() {
            return;
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.stop_bot().await;
        self.wipe_files();
        self.set_status(|s| *s = WaStatus { seq: s.seq, ..WaStatus::idle(false) });
    }

    pub async fn reconnect(self: &Arc<Self>) -> Result<(), String> {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.stop_bot().await;
        if self.is_linked() {
            self.start_bot().await.inspect_err(|e| self.fail(e.clone()))
        } else {
            self.start_linking().await
        }
    }

    pub async fn logout(self: &Arc<Self>) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(client) = self.client().await
            && tokio::time::timeout(Duration::from_secs(10), client.logout())
                .await
                .is_err()
        {
            log::warn!("whatsapp: logout request timed out");
        }
        self.stop_bot().await;
        self.wipe_files();
        self.set_status(|s| *s = WaStatus { seq: s.seq, ..WaStatus::idle(false) });
    }

    /// The phone removed this device, or the server refused the session.
    async fn handle_unlinked(self: Arc<Self>, generation: u64) {
        if self.generation.compare_exchange(generation, generation + 1, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return;
        }
        self.stop_bot().await;
        self.wipe_files();
        self.set_status(|s| {
            *s = WaStatus {
                seq: s.seq,
                state: WaState::LoggedOut,
                error: Some("This computer was unlinked from WhatsApp.".into()),
                ..WaStatus::idle(false)
            }
        });
    }

    pub fn list_chats(&self) -> Vec<ChatSummary> {
        lock(&self.store).list_chats()
    }

    pub fn messages(&self, chat_id: &str) -> Vec<MessageDto> {
        lock(&self.store).messages(chat_id)
    }

    /// Clears the unread count and tells the senders their messages were read.
    /// WhatsApp keeps that from them when the account has read receipts off.
    pub fn mark_read(self: &Arc<Self>, chat_id: &str) {
        // Opening a chat asks for this every time; one with nothing unread
        // changes nothing, and marking it dirty would redraw the chat list.
        let (read, was_marked) = {
            let mut store = lock(&self.store);
            if !store.has_unread(chat_id) {
                return;
            }
            let was_marked = store.is_marked_unread(chat_id);
            (store.mark_read(chat_id), was_marked)
        };
        self.mark_dirty();
        // A chat marked unread, here or on the phone, is read everywhere.
        if was_marked {
            let mgr = self.clone();
            let chat_id = chat_id.to_string();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = mgr.send_unread_mark(&chat_id, false).await {
                    log::warn!("whatsapp: {e}");
                }
            });
        }
        if read.is_empty() {
            return;
        }
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            let Some(client) = mgr.client().await else {
                return;
            };
            // One receipt per chat and, in a group, per sender.
            let mut receipts: Vec<(&str, Option<&str>, Vec<&str>)> = Vec::new();
            for message in &read {
                let is_group = message.chat.ends_with("@g.us");
                let sender = message.sender.as_deref().filter(|_| is_group);
                if is_group && sender.is_none() {
                    continue;
                }
                match receipts.iter_mut().find(|(c, s, _)| *c == message.chat && *s == sender) {
                    Some((_, _, ids)) => ids.push(&message.id),
                    None => receipts.push((&message.chat, sender, vec![&message.id])),
                }
            }
            for (chat, sender, ids) in receipts {
                let Ok(chat) = chat.parse::<Jid>() else {
                    continue;
                };
                let sender = sender.and_then(|s| s.parse::<Jid>().ok());
                if let Err(e) = client.mark_as_read(&chat, sender.as_ref(), &ids).await {
                    log::warn!("whatsapp: read receipt not sent: {e}");
                }
            }
        });
    }

    /// Sends a text, quoting the message `reply_to` when there is one.
    /// The context that quotes `reply_to`, and the quote as it is shown here.
    fn quote(&self, chat_id: &str, reply_to: Option<&str>) -> Result<Option<(wa::ContextInfo, ReplyRef)>, String> {
        let Some(id) = reply_to else {
            return Ok(None);
        };
        self.recall(chat_id, id);
        let (source, me) = {
            let store = lock(&self.store);
            (
                store.quote_source(chat_id, id).ok_or("That message is gone.")?,
                store.own_id().ok_or("WhatsApp is not connected.")?.to_string(),
            )
        };
        // The quote names who wrote the message: in a chat with one
        // person, the chat itself when it isn't ours.
        let sender = match (&source.sender, source.from_me) {
            (_, true) => me,
            (Some(sender), _) => sender.clone(),
            (None, _) => chat_id.to_string(),
        };
        let context = {
            let mut context = wa::ContextInfo::default();
            context.stanza_id = Some(id.to_string());
            context.participant = Some(sender.clone());
            context.quoted_message = MessageField::some(wa::Message::text(source.text.clone()));
            context
        };
        Ok(Some((context, ReplyRef { author_jid: Some(sender), id: Some(id.to_string()), text: source.text })))
    }

    /// Files a message we just sent and returns it as the UI shows it.
    fn store_sent(&self, chat_id: &str, id: String, content: Content) -> Result<MessageDto, String> {
        let message = StoredMessage {
            id,
            from_me: true,
            ts_ms: now_ms(),
            author_jid: None,
            content,
            status: Some(Delivery::Sent),
            reactions: Vec::new(),
            wire: None,
            read_by: Vec::new(),
            starred: false,
            deleted: None,
            edited_ms: None,
            expires_in: None,
            expires_at_ms: None,
        };
        let incoming = store::Incoming {
            chat: chat_id.to_string(),
            is_group: chat_id.ends_with("@g.us"),
            sender: None,
            push_name: None,
            lid_pn_pairs: Vec::new(),
            message,
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
        forwarded: u32,
        mentions: &[String],
        everyone: bool,
    ) -> Result<MessageDto, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("The message is empty.".into());
        }
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let (text, mentioned) = mentioning(&client, &jid, text, mentions).await;
        let text = text.as_str();
        let (context, quoted) = self.quote(chat_id, reply_to)?.unzip();
        let everyone = everyone && mentions_everyone(text);
        let context = with_mentions(forwarding(context, forwarded), mentioned, everyone);
        let (outgoing, card) = match link.filter(|l| text.contains(&l.url)) {
            Some(link) => {
                let (ext, card) = self.link_message(&client, text, link).await;
                let mut ext = ext;
                ext.context_info = context.map(MessageField::some).unwrap_or_default();
                ({
                    let mut message = wa::Message::default();
                    message.extended_text_message = MessageField::some(ext);
                    message
                }, Some(card))
            }
            None => match context {
                Some(context) => (wa::Message::text_with_context(text, context), None),
                None => (wa::Message::text(text), None),
            },
        };
        let sent = client
            .send_message(jid, outgoing)
            .await
            .map_err(|e| format!("The message was not sent: {e}"))?;
        let mut content = Content::text(text, quoted);
        content.link = card;
        content.forwarded = forwarded;
        content.mentions_all = everyone;
        self.store_sent(chat_id, sent.message_id.into_string(), content)
    }

    /// A text with its link card, as WhatsApp's apps send it: the small
    /// picture inside the message, the full one uploaded beside it. A
    /// picture that fails to upload leaves the small one.
    async fn link_message(
        &self,
        client: &Arc<Client>,
        text: &str,
        link: OutgoingLink,
    ) -> (wa::message::ExtendedTextMessage, LinkPreview) {
        let thumbnail = OutgoingLink::decoded(&link.thumbnail);
        let mut ext = {
            let mut ext = wa::message::ExtendedTextMessage::default();
            ext.text = Some(text.to_string());
            ext.matched_text = Some(link.url.clone());
            ext.title = link.title.clone();
            ext.description = link.description.clone();
            ext.jpeg_thumbnail = thumbnail.clone();
            ext
        };
        let mut large = None;
        if let Some(image) = OutgoingLink::decoded(&link.image) {
            match client.upload(image, MediaType::LinkThumbnail, UploadOptions::default()).await {
                Ok(upload) => {
                    ext.thumbnail_direct_path = Some(upload.direct_path.clone());
                    ext.thumbnail_sha256 = Some(upload.file_sha256.to_vec());
                    ext.thumbnail_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
                    ext.media_key = Some(upload.media_key.to_vec());
                    ext.media_key_timestamp = Some(upload.media_key_timestamp);
                    ext.thumbnail_width = link.width;
                    ext.thumbnail_height = link.height;
                    large = Some(CdnRef {
                        path: upload.direct_path.clone(),
                        key: BASE64.encode(upload.media_key),
                        sha256: BASE64.encode(upload.file_sha256),
                        enc_sha256: BASE64.encode(upload.file_enc_sha256),
                        size: 0,
                        mime: None,
                        width: link.width.unwrap_or_default(),
                        height: link.height.unwrap_or_default(),
                        kind: None,
                        file_name: None,
                    });
                }
                Err(e) => log::warn!("the link picture was not uploaded: {e}"),
            }
        }
        let card = LinkPreview {
            url: link.url,
            title: link.title,
            description: link.description,
            thumbnail: thumbnail.map(|bytes| format!("data:image/jpeg;base64,{}", BASE64.encode(bytes))),
            large,
        };
        (ext, card)
    }

    /// Uploads a file and sends it: a photo, video or audio when WhatsApp
    /// shows that type as one, otherwise a document.
    pub async fn send_file(&self, file: OutgoingFile, bytes: Vec<u8>) -> Result<MessageDto, String> {
        let client = self.connected_client().await?;
        let chat_id = file.chat_id.as_str();
        let jid: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let context = self.quote(chat_id, file.reply_to.as_deref())?.map(|(context, _)| context);
        let mime = file.base_mime();
        let media_type = match mime.as_str() {
            "image/jpeg" | "image/png" | "image/webp" => MediaType::Image,
            "video/mp4" | "video/3gpp" => MediaType::Video,
            "audio/ogg" | "audio/mpeg" | "audio/mp4" | "audio/aac" | "audio/amr" => MediaType::Audio,
            _ => MediaType::Document,
        };

        // Kept on disk while it uploads, and named after its hash once that is
        // known, so the message we show opens without downloading it back.
        let dir = self.dir.join(MEDIA_DIR);
        secure_dir(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        let staged = dir.join(format!(".outgoing-{}", now_ms()));
        crate::vault::get()
            .write(&staged, &bytes)
            .map_err(|e| format!("could not keep a copy of the file: {e}"))?;
        let uploaded = client.upload(bytes, media_type, UploadOptions::default()).await;
        let upload = match uploaded {
            Ok(upload) => upload,
            Err(e) => {
                let _ = fs::remove_file(&staged);
                return Err(format!("The file was not uploaded: {e}"));
            }
        };

        let (caption, mentioned) = match file.caption() {
            Some(caption) => {
                let (caption, mentioned) = mentioning(&client, &jid, &caption, &file.mentions).await;
                (Some(caption), mentioned)
            }
            None => (None, Vec::new()),
        };
        let thumbnail = file.thumbnail.as_deref().and_then(|t| BASE64.decode(t).ok());
        let everyone = file.mention_all && caption.as_deref().is_some_and(mentions_everyone);
        let context_info = with_mentions(forwarding(context, file.forwarded), mentioned, everyone)
            .map(MessageField::some)
            .unwrap_or_default();
        let mut outgoing = wa::Message::default();
        match media_type {
            MediaType::Image => {
                outgoing.image_message = MessageField::some({
                    let mut image = wa::message::ImageMessage::default();
                    image.url = Some(upload.url.clone());
                    image.direct_path = Some(upload.direct_path.clone());
                    image.media_key = Some(upload.media_key.to_vec());
                    image.file_sha256 = Some(upload.file_sha256.to_vec());
                    image.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
                    image.file_length = Some(upload.file_length);
                    image.media_key_timestamp = Some(upload.media_key_timestamp);
                    image.mimetype = Some(mime.clone());
                    image.caption = caption;
                    image.width = file.width;
                    image.height = file.height;
                    image.jpeg_thumbnail = thumbnail;
                    image.context_info = context_info;
                    image
                })
            }
            MediaType::Video => {
                outgoing.video_message = MessageField::some({
                    let mut video = wa::message::VideoMessage::default();
                    video.url = Some(upload.url.clone());
                    video.direct_path = Some(upload.direct_path.clone());
                    video.media_key = Some(upload.media_key.to_vec());
                    video.file_sha256 = Some(upload.file_sha256.to_vec());
                    video.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
                    video.file_length = Some(upload.file_length);
                    video.media_key_timestamp = Some(upload.media_key_timestamp);
                    video.streaming_sidecar = upload.streaming_sidecar.clone();
                    video.mimetype = Some(mime.clone());
                    video.caption = caption;
                    video.seconds = file.seconds;
                    video.width = file.width;
                    video.height = file.height;
                    video.jpeg_thumbnail = thumbnail;
                    video.context_info = context_info;
                    video
                })
            }
            MediaType::Audio => {
                outgoing.audio_message = MessageField::some({
                    let mut audio = wa::message::AudioMessage::default();
                    audio.url = Some(upload.url.clone());
                    audio.direct_path = Some(upload.direct_path.clone());
                    audio.media_key = Some(upload.media_key.to_vec());
                    audio.file_sha256 = Some(upload.file_sha256.to_vec());
                    audio.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
                    audio.file_length = Some(upload.file_length);
                    audio.media_key_timestamp = Some(upload.media_key_timestamp);
                    audio.streaming_sidecar = upload.streaming_sidecar.clone();
                    // A voice message keeps its codec, as the phone sends it.
                    audio.mimetype = Some(if file.voice { file.mime.clone() } else { mime.clone() });
                    audio.seconds = file.seconds;
                    audio.ptt = Some(file.voice);
                    audio.waveform = (file.voice && !file.waveform.is_empty()).then(|| file.waveform.clone());
                    audio.context_info = context_info;
                    audio
                })
            }
            _ => {
                outgoing.document_message = MessageField::some({
                    let mut document = wa::message::DocumentMessage::default();
                    document.url = Some(upload.url.clone());
                    document.direct_path = Some(upload.direct_path.clone());
                    document.media_key = Some(upload.media_key.to_vec());
                    document.file_sha256 = Some(upload.file_sha256.to_vec());
                    document.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
                    document.file_length = Some(upload.file_length);
                    document.media_key_timestamp = Some(upload.media_key_timestamp);
                    document.mimetype = Some(if mime.is_empty() { "application/octet-stream".into() } else { mime.clone() });
                    document.file_name = Some(file.name.clone());
                    document.title = Some(file.name.clone());
                    document.caption = caption;
                    document.jpeg_thumbnail = thumbnail;
                    document.context_info = context_info;
                    document
                })
            }
        }
        // What we show is read back from the message, as one that arrived is.
        let content = convert::content(&outgoing);
        let sent = client.send_message(jid, outgoing).await;
        let sent = match sent {
            Ok(sent) => sent,
            Err(e) => {
                let _ = fs::remove_file(&staged);
                return Err(format!("The file was uploaded but not sent: {e}"));
            }
        };
        let content = content.ok_or("The file was sent but could not be shown.")?;
        if let Some(name) = content.wa_media.as_ref().and_then(|r| cdn_file_name(r, "wa").ok()) {
            let path = dir.join(&name);
            if let Some(parent) = path.parent() {
                let _ = secure_dir(parent);
            }
            if fs::rename(&staged, &path).is_err() {
                let _ = fs::remove_file(&staged);
            }
        } else {
            let _ = fs::remove_file(&staged);
        }
        self.store_sent(chat_id, sent.message_id.into_string(), content)
    }

    /// Downloads and decrypts a file into `dir_name` under `prefix` and its
    /// hash, unless it is already there. Returns its path inside `dir_name`:
    /// a plain file name, except for documents, which keep the name they were
    /// sent with in a folder of their own.
    async fn fetch_cdn(
        &self,
        file: CdnRef,
        dir_name: &str,
        prefix: &str,
        media_type: MediaType,
    ) -> Result<String, CdnError> {
        let decode = |field: &str| BASE64.decode(field).map_err(|e| format!("Bad file reference: {e}"));
        let sha256 = decode(&file.sha256)?;
        let name = cdn_file_name(&file, prefix)?;
        let path = self.dir.join(dir_name).join(&name);
        if path.exists() {
            return Ok(name);
        }
        let client = self
            .client()
            .await
            .ok_or("WhatsApp is not connected.")?;
        let static_url = file.path.starts_with("https://").then(|| file.path.clone());
        let params = DownloadParams::encrypted(
            file.path,
            &decode(&file.key)?,
            &sha256,
            &decode(&file.enc_sha256)?,
            file.size,
            media_type,
        )
        .map_err(|e| format!("Bad file reference: {e}"))?;
        // WhatsApp's own messages keep their file on its static host, at a
        // full URL, encrypted as any other.
        let params = StaticUrl { params, url: static_url };
        // The folder first, so a document's own folder is not what creates it
        // with the default permissions.
        for dir in [self.dir.join(dir_name), path.parent().unwrap_or(&self.dir).to_path_buf()] {
            secure_dir(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        }
        // Written beside the file and moved into place once it is complete, so
        // a download cut short is never served as the file. Streamed rather
        // than held in memory: a video or a document can be hundreds of MB.
        // Numbered, because the same file can be asked for twice at once (a
        // video preloading and then pressed), and two downloads into one file
        // would interleave.
        static DOWNLOADS: AtomicU64 = AtomicU64::new(0);
        let mut partial = path.clone().into_os_string();
        partial.push(format!(".{}.part", DOWNLOADS.fetch_add(1, Ordering::Relaxed)));
        let partial = PathBuf::from(partial);
        // Encrypted as it streams in; nothing plain reaches the disk.
        let sink = crate::vault::Writer::create(crate::vault::get(), &partial)
            .map_err(|e| format!("could not save {name}: {e}"))?;
        let sink = match client.download_to_writer(&params, sink).await {
            Ok(sink) => sink,
            Err(e) => {
                let _ = fs::remove_file(&partial);
                // Every host served bytes that fail the file's MAC or hash:
                // an expired address the CDN answers with something else.
                let rejected = matches!(e.http_status(), Some(401 | 403 | 404 | 410))
                    || e.sources().any(|cause| cause.is::<MediaDecryptionError>());
                let message = format!("Could not download the file: {e}");
                return Err(if rejected { CdnError::Rejected(message) } else { CdnError::Failed(message) });
            }
        };
        if let Err(e) = sink.finish() {
            let _ = fs::remove_file(&partial);
            return Err(format!("could not save {name}: {e}").into());
        }
        fs::rename(&partial, &path).map_err(|e| format!("could not save {name}: {e}"))?;
        Ok(name)
    }

    /// Fetches a message's file and returns its path inside the media directory.
    pub async fn fetch_media(&self, chat_id: &str, message_id: &str) -> Result<String, String> {
        self.recall(chat_id, message_id);
        let file = lock(&self.store)
            .wa_media(chat_id, message_id)
            .ok_or("This message has no attachment to fetch.")?;
        let media_type = match file.kind.as_deref() {
            None => MediaType::Image,
            Some("video" | "gif") => MediaType::Video,
            Some("voice" | "audio") => MediaType::Audio,
            Some("sticker") => MediaType::Sticker,
            Some(_) => MediaType::Document,
        };
        match self.fetch_cdn(file.clone(), MEDIA_DIR, "wa", media_type).await {
            // WhatsApp drops media from its CDN after a few weeks; the sender's
            // phone can upload it again if it still has the file.
            Err(CdnError::Rejected(_)) => {
                let failed_lately = lock(&self.reupload_failed)
                    .get(message_id)
                    .is_some_and(|at| at.elapsed() < REUPLOAD_RETRY_AFTER);
                if failed_lately {
                    return Err("The file expired and the phone did not send it again.".into());
                }
                let path = {
                    let _turn = self.reuploads.acquire().await.map_err(|e| e.to_string())?;
                    self.reupload_media(chat_id, message_id, &file).await
                }
                .inspect_err(|e| {
                    lock(&self.reupload_failed).insert(message_id.to_string(), Instant::now());
                    log::warn!("whatsapp: file {message_id} expired and was not re-uploaded: {e}");
                })?;
                lock(&self.store).set_wa_media_path(chat_id, message_id, &path);
                self.mark_dirty();
                Ok(self.fetch_cdn(CdnRef { path, ..file }, MEDIA_DIR, "wa", media_type).await?)
            }
            other => Ok(other?),
        }
    }

    /// Downloads a message's file if needed and opens it with the system's app
    /// for its type.
    pub async fn open_media(&self, chat_id: &str, message_id: &str) -> Result<(), String> {
        self.recall(chat_id, message_id);
        use tauri_plugin_opener::OpenerExt;
        let name = self.fetch_media(chat_id, message_id).await?;
        let file_name = Path::new(&name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let path = crate::vault::get()
            .export(&self.dir.join(MEDIA_DIR).join(&name), &file_name)
            .map_err(|e| format!("The file could not be opened: {e}"))?;
        self.app
            .opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| format!("The file could not be opened: {e}"))
    }

    /// Asks the phone that sent a message to put its file back on the CDN,
    /// returning the new path.
    async fn reupload_media(&self, chat_id: &str, message_id: &str, file: &CdnRef) -> Result<String, String> {
        let client = self.connected_client().await?;
        let (sender, from_me) = lock(&self.store).star_target(chat_id, message_id).ok_or("That message is gone.")?;
        let chat: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let sender = sender.and_then(|s| s.parse::<Jid>().ok());
        let media_key: [u8; 32] = BASE64
            .decode(&file.key)
            .ok()
            .and_then(|key| key.try_into().ok())
            .ok_or("Bad file reference.")?;
        let participant = sender.filter(|_| !from_me && (chat_id.ends_with("@g.us") || chat_id == "status@broadcast"));
        let id = MessageId::new(message_id).map_err(|e| format!("Bad message: {e}"))?;
        let target = MessageRef::new(&chat, id, participant.as_ref(), from_me).map_err(|e| format!("Bad message: {e}"))?;
        let result = client
            .media_reupload()
            .request(&MediaReuploadRequest { target, media_key: &media_key })
            .await
            .map_err(|e| format!("The phone did not send the file again: {e}"))?;
        match result {
            MediaRetryResult::Success { direct_path } => Ok(direct_path),
            MediaRetryResult::NotFound => Err("The file is no longer on the phone that sent it.".into()),
            other => Err(format!("The phone could not send the file again: {other:?}")),
        }
    }

    /// Fetches the full-size picture of a message's link card and returns its
    /// file name inside the thumbnail directory.
    pub async fn link_thumbnail(&self, chat_id: &str, message_id: &str) -> Result<String, String> {
        self.recall(chat_id, message_id);
        let file = lock(&self.store)
            .link_thumbnail(chat_id, message_id)
            .ok_or("This link has no larger picture.")?;
        Ok(self.fetch_cdn(file, LINK_THUMB_DIR, "link", MediaType::LinkThumbnail).await?)
    }

    /// Reacts to a message, or takes the reaction back when it is already there.
    pub async fn react(&self, chat_id: &str, message_id: &str, emoji: &str) -> Result<MessageDto, String> {
        self.recall(chat_id, message_id);
        let client = self
            .client()
            .await
            .ok_or("WhatsApp is not connected.")?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let (target, me) = {
            let store = lock(&self.store);
            (
                store.reaction_target(chat_id, message_id).ok_or("That message is gone.")?,
                store.own_id().ok_or("WhatsApp is not connected.")?.to_string(),
            )
        };
        let remove = target.mine.as_deref() == Some(emoji);
        let key = {
            let mut key = wa::MessageKey::default();
            key.remote_jid = Some(chat_id.to_string());
            key.from_me = Some(target.from_me);
            key.id = Some(message_id.to_string());
            // Groups need the original sender to attribute the reaction.
            key.participant = target.author.filter(|_| chat_id.ends_with("@g.us") && !target.from_me);
            key
        };
        // An empty emoji is how WhatsApp removes a reaction.
        client
            .send_reaction_raw(jid, key, if remove { "" } else { emoji })
            .await
            .map_err(|e| format!("The reaction was not sent: {e}"))?;
        let dto = lock(&self.store)
            .set_reaction(chat_id, message_id, &me, (!remove).then_some(emoji))
            .ok_or("That message is gone.")?;
        self.mark_dirty();
        Ok(dto)
    }

    /// Opens a vote on a poll with the poll's secret and records it.
    /// Returns the poll's message when it changed.
    async fn open_vote(&self, client: &Client, chat: &str, voter: &str, vote: convert::SealedVote) -> Option<MessageDto> {
        let Some((options, _, secret, _)) = lock(&self.store).poll_of(chat, &vote.target) else {
            log::info!("whatsapp: a vote on a poll not kept here was skipped");
            return None;
        };
        let Some(secret) = secret.and_then(|s| BASE64.decode(s).ok()) else {
            log::info!("whatsapp: a vote on a poll without its secret was skipped");
            return None;
        };
        let creator = match &vote.creator {
            Some(creator) => creator.parse::<Jid>().ok()?,
            None => client.pn()?.to_non_ad(),
        };
        let voter_jid: Jid = voter.parse().ok()?;
        let ciphertext = whatsapp_rust::wacore::poll::PollVoteCiphertext { enc_payload: &vote.payload, enc_iv: &vote.iv };
        let hashes = match client.polls().decrypt_vote_raw(ciphertext, &secret, &vote.target, &creator, &voter_jid).await {
            Ok(hashes) => hashes,
            Err(e) => {
                log::warn!("whatsapp: a poll vote could not be opened: {e}");
                return None;
            }
        };
        lock(&self.store).set_poll_vote(chat, &vote.target, voter, convert::picked(&options, &hashes), vote.at_ms)
    }

    /// Votes on a poll for the options at `picked`; none takes our vote back.
    pub async fn vote_poll(&self, chat_id: &str, message_id: &str, picked: Vec<usize>) -> Result<MessageDto, String> {
        self.recall(chat_id, message_id);
        let (options, selectable, secret, creator) =
            lock(&self.store).poll_of(chat_id, message_id).ok_or("That poll is gone.")?;
        let secret = secret
            .and_then(|s| BASE64.decode(s).ok())
            .ok_or("This poll can't be voted on from here: WhatsApp didn't send its key.")?;
        if selectable > 0 && picked.len() > selectable as usize {
            return Err(format!("Pick up to {selectable}."));
        }
        let names: Vec<String> = picked.iter().filter_map(|&i| options.get(i).cloned()).collect();
        let client = self.connected_client().await?;
        let creator: Jid = match creator {
            Some(creator) => creator.parse().map_err(|e| format!("Unknown poll author {creator}: {e}"))?,
            None => client.pn().ok_or("WhatsApp is not connected.")?.to_non_ad(),
        };
        let chat: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        client
            .polls()
            .vote_raw(chat, message_id, &creator, &secret, &names)
            .await
            .map_err(|e| format!("The vote was not sent: {e}"))?;
        let mut store = lock(&self.store);
        let me = store.own_id().ok_or("WhatsApp is not connected.")?.to_string();
        let dto = store.set_poll_vote(chat_id, message_id, &me, picked, store::now_ms());
        drop(store);
        self.mark_dirty();
        dto.ok_or_else(|| "That poll is gone.".into())
    }

    /// Changes what one of our texts says, for everyone, within WhatsApp's
    /// edit window.
    pub async fn edit_message(&self, chat_id: &str, message_id: &str, text: &str) -> Result<MessageDto, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("The message is empty.".into());
        }
        self.recall(chat_id, message_id);
        if !lock(&self.store).can_edit(chat_id, message_id, store::EDIT_WINDOW_MS) {
            return Err("Only your own texts can be edited, up to 15 minutes after sending.".into());
        }
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let id = MessageId::new(message_id).map_err(|e| format!("The message was not edited: {e}"))?;
        let target = MessageRef::new(&jid, id, None, true).map_err(|e| format!("The message was not edited: {e}"))?;
        client
            .edit_message(EditRequest::new(target, wa::Message::text(text)))
            .await
            .map_err(|e| format!("The message was not edited: {e}"))?;
        let dto = lock(&self.store).edit_message(chat_id, message_id, None, Content::text(text, None), store::now_ms());
        self.mark_dirty();
        dto.ok_or_else(|| "The message is no longer here.".into())
    }

    /// Deletes a message on our devices, or for everyone when `for_everyone`
    /// and it is ours. Returns what is left of it, if anything is shown.
    pub async fn delete_message(&self, chat_id: &str, message_id: &str, for_everyone: bool) -> Result<Option<MessageDto>, String> {
        self.recall(chat_id, message_id);
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let (source, ts) = {
            let store = lock(&self.store);
            let target = store.reaction_target(chat_id, message_id).ok_or("That message is gone.")?;
            (store.star_target(chat_id, message_id).ok_or("That message is gone.")?, target.ts_ms)
        };
        let (sender, from_me) = source;
        if for_everyone {
            if !from_me {
                return Err("Only your own messages can be deleted for everyone.".into());
            }
            client
                .revoke_message_raw(&jid, message_id, RevokeType::Sender)
                .await
                .map_err(|e| format!("The message was not deleted: {e}"))?;
        } else {
            let participant = sender
                .filter(|_| chat_id.ends_with("@g.us") && !from_me)
                .and_then(|s| s.parse::<Jid>().ok());
            let id = MessageId::new(message_id).map_err(|e| format!("The message was not deleted: {e}"))?;
            let target =
                MessageRef::new(&jid, id, participant.as_ref(), from_me).map_err(|e| format!("The message was not deleted: {e}"))?;
            client
                .chat_actions()
                .delete_message_for_me(&target, true, Some(ts / 1000))
                .await
                .map_err(|e| format!("The message was not deleted: {e}"))?;
        }
        let how = if for_everyone { Deletion::ForEveryone } else { Deletion::ForMe };
        let dto = lock(&self.store).delete_message(chat_id, message_id, how);
        self.mark_dirty();
        Ok(dto.filter(|_| for_everyone))
    }

    pub fn lists(&self) -> Vec<ListDto> {
        lock(&self.store).lists()
    }

    pub fn starred(&self) -> Vec<StarredDto> {
        lock(&self.store).starred()
    }

    pub fn search(&self, query: &str, chat: Option<&str>) -> Vec<StarredDto> {
        let limit = if chat.is_some() { crate::store::CHAT_SEARCH_LIMIT } else { crate::store::SEARCH_LIMIT };
        let terms = lock(&self.store).search_terms(query);
        let saved = self.history.search(&terms, chat, limit);
        lock(&self.store).search(query, chat, limit, saved)
    }

    pub fn contacts(&self) -> Vec<ContactDto> {
        lock(&self.store).contacts()
    }

    pub fn person(&self, id: &str) -> crate::store::PersonDto {
        lock(&self.store).person(id)
    }

    async fn connected_client(&self) -> Result<Arc<Client>, String> {
        self.client().await.ok_or_else(|| "WhatsApp is not connected.".to_string())
    }

    /// Mutes a chat until a time, or for good without one, or unmutes it,
    /// here and on the phone.
    pub async fn set_muted(&self, chat_id: &str, muted: bool, until_ms: Option<i64>) -> Result<(), String> {
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Not a chat: {e}"))?;
        let actions = client.chat_actions();
        match (muted, until_ms) {
            (true, Some(until)) => actions.mute_chat_until(&jid, until).await,
            (true, None) => actions.mute_chat(&jid).await,
            (false, _) => actions.unmute_chat(&jid).await,
        }
        .map_err(|e| format!("The chat was not {}: {e}", if muted { "muted" } else { "unmuted" }))?;
        lock(&self.store).set_muted(chat_id, muted.then(|| until_ms.unwrap_or(i64::MAX)));
        self.mark_dirty();
        Ok(())
    }

    /// Shows us online while the window is in front, and offline otherwise.
    pub fn set_active(self: &Arc<Self>, active: bool) {
        if self.active.swap(active, Ordering::AcqRel) == active {
            return;
        }
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            if !active {
                tokio::time::sleep(AWAY_AFTER).await;
            }
            // Back in front meanwhile, or gone again: a later call decides.
            if mgr.active.load(Ordering::Acquire) == active {
                mgr.announce_presence().await;
            }
        });
    }

    /// Tells the server whether we are online, when that changed since it
    /// was last told.
    async fn announce_presence(&self) {
        let Some(client) = self.client().await else {
            return;
        };
        // Offline meanwhile: the next connection announces it.
        if !client.is_socket_connected() || !client.is_logged_in() {
            return;
        }
        let active = self.active.load(Ordering::Acquire);
        if self.announced.swap(if active { 2 } else { 1 }, Ordering::AcqRel) == if active { 2 } else { 1 } {
            return;
        }
        let status = if active { PresenceStatus::Available } else { PresenceStatus::Unavailable };
        if let Err(e) = client.presence().set(status).await {
            self.announced.store(0, Ordering::Release);
            log::warn!("whatsapp: presence not sent: {e}");
        }
    }

    /// Tells the chat we are typing, recording, or have stopped.
    pub async fn set_typing(&self, chat_id: &str, state: &str) -> Result<(), String> {
        let state = match state {
            "typing" => ChatActivity::Typing,
            "recording" => ChatActivity::RecordingAudio,
            _ => ChatActivity::Idle,
        };
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Not a chat: {e}"))?;
        client.chatstate().send(&jid, state).await.map_err(|e| format!("Typing not sent: {e}"))
    }

    /// Asks to be told when someone comes online or goes away. Groups have
    /// no presence of their own.
    pub async fn watch_presence(&self, chat_id: &str) -> Result<(), String> {
        if chat_id.ends_with("@g.us") || chat_id.ends_with("@broadcast") || chat_id.ends_with("@newsletter") {
            return Ok(());
        }
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Not a chat: {e}"))?;
        client.presence().subscribe(jid).await.map_err(|e| format!("Presence not asked for: {e}"))
    }

    pub async fn privacy(&self) -> Result<PrivacyDto, String> {
        let client = self.connected_client().await?;
        let settings = client.fetch_privacy_settings().await.map_err(|e| format!("Privacy settings not loaded: {e}"))?;
        let value = |category| settings.get_value(&category).map(|v: &PrivacyValue| v.to_string());
        Ok(PrivacyDto {
            last_seen: value(PrivacyCategory::Last),
            online: value(PrivacyCategory::Online),
            read_receipts: value(PrivacyCategory::ReadReceipts),
        })
    }

    /// Changes who sees our last seen (`last`), online (`online`) or read
    /// receipts (`readreceipts`), on the account itself.
    pub async fn set_privacy(&self, category: &str, value: &str) -> Result<(), String> {
        let category = PrivacyCategory::from(category);
        let value = PrivacyValue::from(value);
        if matches!(category, PrivacyCategory::Other(_)) || !category.is_valid_value(&value) {
            return Err("That privacy setting can't be changed here.".into());
        }
        let client = self.connected_client().await?;
        client
            .set_privacy_setting(category, value)
            .await
            .map_err(|e| format!("The privacy setting was not changed: {e}"))?;
        Ok(())
    }

    /// Archives a chat or brings it back, here and on the phone. Archiving
    /// unpins it, as the phone does.
    pub async fn set_archived(&self, chat_id: &str, archived: bool) -> Result<(), String> {
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Not a chat: {e}"))?;
        let pinned = lock(&self.store).is_pinned(chat_id);
        let range = self.last_range(chat_id);
        let actions = client.chat_actions();
        if archived { actions.archive_chat(&jid, range).await } else { actions.unarchive_chat(&jid, range).await }
            .map_err(|e| format!("The chat was not {}: {e}", if archived { "archived" } else { "unarchived" }))?;
        if archived && pinned {
            match actions.unpin_chat(&jid).await {
                Ok(()) => lock(&self.store).set_pinned(chat_id, None),
                Err(e) => log::warn!("whatsapp: unpin on archive failed: {e}"),
            }
        }
        lock(&self.store).set_archived(chat_id, archived);
        self.mark_dirty();
        Ok(())
    }

    /// The range the phone weighs a change to a chat against: its newest
    /// message.
    fn last_range(&self, chat_id: &str) -> Option<SyncActionMessageRange> {
        let last = lock(&self.store).last_message_key(chat_id)?;
        let chat: Jid = last.chat.parse().ok()?;
        let sender = last.sender.and_then(|s| s.parse::<Jid>().ok());
        let key = MessageRef::new(&chat, MessageId::new(last.id).ok()?, sender.as_ref(), last.from_me).ok()?;
        Some(message_range(last.ts_s, None, vec![(key, last.ts_s)]))
    }

    /// Deletes a chat and its messages, here and on the phone.
    pub async fn delete_chat(&self, chat_id: &str) -> Result<(), String> {
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Not a chat: {e}"))?;
        let before = lock(&self.store).clear_point(chat_id);
        client
            .chat_actions()
            .delete_chat(&jid, false, self.last_range(chat_id))
            .await
            .map_err(|e| format!("The chat was not deleted: {e}"))?;
        lock(&self.store).delete_chat(chat_id, before);
        self.chat_emptied(chat_id);
        Ok(())
    }

    /// Empties a chat of its messages, here and on the phone, keeping it in
    /// the list; starred messages stay when `keep_starred`.
    pub async fn clear_chat(&self, chat_id: &str, keep_starred: bool) -> Result<(), String> {
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Not a chat: {e}"))?;
        let before = lock(&self.store).clear_point(chat_id);
        client
            .chat_actions()
            .clear_chat(&jid, !keep_starred, false, self.last_range(chat_id))
            .await
            .map_err(|e| format!("The chat was not cleared: {e}"))?;
        lock(&self.store).clear_chat(chat_id, before, keep_starred);
        self.chat_emptied(chat_id);
        Ok(())
    }

    /// Tells the UI a chat lost its messages, so an open one shows it.
    fn chat_emptied(&self, chat_id: &str) {
        let chat = lock(&self.store).chat_key(chat_id);
        self.emit("cleared", chat);
        self.mark_dirty();
    }

    /// Pins a chat to the top of the list or unpins it, here and on the
    /// phone, which holds three at most.
    pub async fn set_pinned(&self, chat_id: &str, pinned: bool) -> Result<(), String> {
        if pinned && lock(&self.store).pinned_count() >= store::MAX_PINNED {
            return Err(format!("You can only pin up to {} chats.", store::MAX_PINNED));
        }
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Not a chat: {e}"))?;
        let actions = client.chat_actions();
        if pinned { actions.pin_chat(&jid).await } else { actions.unpin_chat(&jid).await }
            .map_err(|e| format!("The chat was not {}: {e}", if pinned { "pinned" } else { "unpinned" }))?;
        lock(&self.store).set_pinned(chat_id, pinned.then(now_ms));
        self.mark_dirty();
        Ok(())
    }

    /// What someone's contact info needs from the network: their about, their
    /// picture at full size, and the groups we share. Each part that fails is
    /// left out.
    pub async fn person_details(&self, id: &str) -> Result<store::PersonDetailsDto, String> {
        let client = self.connected_client().await?;
        let jid: Jid = id.parse().map_err(|e| format!("Not a person: {e}"))?;
        let about = match client.contacts().get_user_info(std::slice::from_ref(&jid)).await {
            Ok(info) => info.into_values().next().and_then(|i| i.status).filter(|s| !s.trim().is_empty()),
            Err(e) => {
                log::warn!("whatsapp: about lookup failed: {e}");
                None
            }
        };
        let photo = self.full_picture(&client, id, &jid).await;
        lock(&self.store).remember_person(id, photo.as_deref(), about.as_deref());
        self.mark_dirty();
        let common_groups = lock(&self.store).common_groups(id);
        Ok(store::PersonDetailsDto { about, photo, common_groups })
    }

    pub fn group_info(&self, id: &str) -> store::GroupInfoDto {
        lock(&self.store).group_info(id)
    }

    /// A group's info fresh from the network: members and their roles,
    /// description, and its picture at full size.
    pub async fn group_details(&self, id: &str) -> Result<store::GroupInfoDto, String> {
        let client = self.connected_client().await?;
        let jid: Jid = id.parse().map_err(|e| format!("Not a group: {e}"))?;
        let groups = client.groups();
        let mut metadata = groups.fetch_metadata(&jid).await.map_err(|e| format!("The group could not be looked up: {e}"))?;
        groups.resolve_participant_addresses(&mut metadata).await;
        let photo = self.full_picture(&client, id, &jid).await;
        let members = members_of(&metadata);
        {
            let mut store = lock(&self.store);
            store.set_group_participants(id, &members);
            store.set_group_facts(id, facts_of(&metadata));
            store.remember_group_picture(id, photo.as_deref());
        }
        self.mark_dirty();
        Ok(self.group_info(id))
    }

    /// Leaves a group. Its chat stays, read-only, until it is deleted.
    pub async fn leave_group(&self, id: &str) -> Result<(), String> {
        let client = self.connected_client().await?;
        let jid: Jid = id.parse().map_err(|e| format!("Not a group: {e}"))?;
        client.groups().leave(jid).await.map_err(|e| format!("You did not leave the group: {e}"))?;
        lock(&self.store).set_left(id, true);
        self.mark_dirty();
        Ok(())
    }

    /// Renames a group, or changes or removes its description.
    pub async fn edit_group(&self, id: &str, name: Option<String>, description: Option<String>) -> Result<store::GroupInfoDto, String> {
        let client = self.connected_client().await?;
        let jid: Jid = id.parse().map_err(|e| format!("Not a group: {e}"))?;
        let groups = client.groups();
        if let Some(name) = name {
            let subject = GroupSubject::new(name.trim()).map_err(|e| format!("That name can't be used: {e}"))?;
            groups.set_subject(jid.clone(), subject).await.map_err(|e| format!("The group was not renamed: {e}"))?;
            lock(&self.store).apply_group_change(id, store::GroupChange::Subject(name));
        }
        if let Some(text) = description {
            let text = text.trim();
            let body = if text.is_empty() {
                None
            } else {
                Some(GroupDescription::new(text).map_err(|e| format!("That description can't be used: {e}"))?)
            };
            groups
                .set_description(jid, body, PreviousDescription::Resolve)
                .await
                .map_err(|e| format!("The description was not changed: {e}"))?;
            lock(&self.store).apply_group_change(id, store::GroupChange::Description(Some(text.to_string())));
        }
        self.mark_dirty();
        Ok(self.group_info(id))
    }

    /// Adds people to a group, removes one, or makes one an admin or not.
    /// Says who could not be changed, and why when the server says.
    pub async fn change_members(&self, id: &str, change: &str, members: Vec<String>) -> Result<store::GroupInfoDto, String> {
        let client = self.connected_client().await?;
        let jid: Jid = id.parse().map_err(|e| format!("Not a group: {e}"))?;
        let people: Vec<Jid> = members.iter().map(|m| m.parse::<Jid>()).collect::<Result<_, _>>().map_err(|e| format!("Not a person: {e}"))?;
        let groups = client.groups();
        let result = match change {
            "add" => groups.add_participants(jid.clone(), &people).await,
            "remove" => groups.remove_participants(jid.clone(), &people).await,
            "promote" => groups.promote_participants(jid.clone(), &people).await,
            "demote" => groups.demote_participants(jid.clone(), &people).await,
            other => return Err(format!("Unknown change: {other}")),
        }
        .map_err(|e| format!("The group was not changed: {e}"))?;
        let failed: Vec<String> = result
            .iter()
            .filter(|r| !r.is_ok())
            .map(|r| {
                let who = lock(&self.store).display_name(&r.jid.to_non_ad_string());
                match r.error.as_deref() {
                    // Their privacy settings only let contacts add them: WhatsApp invites them instead.
                    Some("403") => format!("{who} can only be added by an invite link"),
                    Some("409") if change == "add" => format!("{who} is already in the group"),
                    Some("408") => format!("{who} left recently and can't be added yet"),
                    Some(code) => format!("{who} ({code})"),
                    None => who,
                }
            })
            .collect();
        // The server's own answer: who is in it and who is admin.
        let info = self.group_details(id).await.unwrap_or_else(|_| self.group_info(id));
        if failed.is_empty() {
            Ok(info)
        } else {
            Err(format!("Not changed: {}.", failed.join("; ")))
        }
    }

    /// Looks up who is in every group, in one request, at most once a day:
    /// contact info finds the groups in common from it.
    fn sync_participants(self: &Arc<Self>, generation: u64, client: Arc<Client>) {
        if !lock(&self.store).participants_due() {
            return;
        }
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            let groups = match client.execute(whatsapp_rust::wacore::iq::groups::GroupParticipatingIq::new()).await {
                Ok(response) => response.groups,
                Err(e) => return log::warn!("whatsapp: group members lookup failed: {e}"),
            };
            if mgr.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            {
                let mut store = lock(&mgr.store);
                for group in groups {
                    let metadata = whatsapp_rust::features::GroupMetadata::from(group);
                    let id = metadata.id.to_non_ad_string();
                    if let Some(subject) = &metadata.subject {
                        store.name_listed_group(&id, subject);
                    }
                    store.set_group_participants(&id, &members_of(&metadata));
                }
                store.set_participants_synced();
            }
            mgr.mark_dirty();
        });
    }

    /// Looks up who we blocked: the phone can change it without telling us.
    fn sync_blocklist(self: &Arc<Self>, generation: u64, client: Arc<Client>) {
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            let entries = match client.blocking().get_blocklist().await {
                Ok(entries) => entries,
                Err(e) => return log::warn!("whatsapp: blocklist lookup failed: {e}"),
            };
            if mgr.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            if lock(&mgr.store).set_blocklist(entries.into_iter().map(|e| e.jid.to_non_ad_string())) {
                mgr.mark_dirty();
            }
        });
    }

    /// Blocks someone or unblocks them, here and on the phone.
    pub async fn set_blocked(&self, id: &str, blocked: bool) -> Result<(), String> {
        let client = self.connected_client().await?;
        let jid: Jid = id.parse().map_err(|e| format!("Not a person: {e}"))?;
        let blocking = client.blocking();
        if blocked { blocking.block(&jid).await } else { blocking.unblock(&jid).await }
            .map_err(|e| format!("They were not {}: {e}", if blocked { "blocked" } else { "unblocked" }))?;
        lock(&self.store).set_blocked(id, blocked);
        self.mark_dirty();
        Ok(())
    }

    /// Someone's profile picture at full size, saved encrypted beside the
    /// small ones. Its file name, when they have one we may see.
    async fn full_picture(&self, client: &Client, id: &str, jid: &Jid) -> Option<String> {
        let dir = self.dir.join(AVATAR_DIR);
        let owner = format!("{id}-full");
        let known = lock(&self.store).full_picture(id).map(str::to_string);
        // The picture id is what follows the owner in the file name.
        let known_id = known.as_deref().and_then(|file| file.strip_suffix(".jpg")?.rsplit('-').next().map(str::to_string));
        let request = ProfilePictureRequest::new(picture_target(jid), ProfilePictureType::Full).existing_id(known_id.as_deref());
        let lookup = client.pictures().lookup(request).await.ok()?;
        let picture = match lookup {
            ProfilePictureLookup::Found(picture) => picture,
            ProfilePictureLookup::Unchanged => return known.filter(|file| dir.join(file).exists()),
            _ => return None,
        };
        let file = store::avatar_file(&owner, &picture.id);
        if dir.join(&file).exists() {
            return Some(file);
        }
        let response = client.http_client().execute(HttpRequest::get(picture.url.as_str())).await.ok()?;
        if response.status_code != 200 || response.body.is_empty() {
            return None;
        }
        crate::vault::get().write(&dir.join(&file), &response.body).ok()?;
        Some(file)
    }

    /// Marks a chat unread, or read, here and on the phone.
    pub async fn set_unread(&self, chat_id: &str, unread: bool) -> Result<(), String> {
        self.send_unread_mark(chat_id, unread).await?;
        lock(&self.store).set_marked_unread(chat_id, unread);
        self.mark_dirty();
        Ok(())
    }

    async fn send_unread_mark(&self, chat_id: &str, unread: bool) -> Result<(), String> {
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Not a chat: {e}"))?;
        client
            .chat_actions()
            .mark_chat_as_read(&jid, !unread, None)
            .await
            .map_err(|e| format!("The chat was not marked {}: {e}", if unread { "unread" } else { "read" }))
    }

    /// Adds a chat to the favorites or takes it out, on the phone too.
    pub async fn set_favorite(&self, chat_id: &str, favorite: bool) -> Result<(), String> {
        let client = self.connected_client().await?;
        let ids = lock(&self.store).favorites_with(chat_id, favorite);
        let value = {
            let mut value = wa::SyncActionValue::default();
            value.favorites_action = MessageField::some({
                let mut action = FavoritesAction::default();
                action.favorites = ids
                    .into_iter()
                    .map(|id| {
                        let mut entry = favorites_action::Favorite::default();
                        entry.id = Some(id);
                        entry
                    })
                    .collect();
                action
            });
            value.timestamp = Some(now_ms());
            value
        };
        // The phone keeps one list, so every change sends all of it.
        client
            .send_app_state_action(&schemas::FAVORITES, &[], &value)
            .await
            .map_err(|e| format!("The favorites were not updated: {e}"))?;
        lock(&self.store).set_favorite(chat_id, favorite);
        self.mark_dirty();
        Ok(())
    }

    /// Creates a list, or renames an existing one, and adds and removes
    /// chats. Returns its id.
    pub async fn save_list(
        &self,
        id: Option<String>,
        name: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("The list needs a name.".into());
        }
        let client = self.connected_client().await?;
        let (id, order, color, before) = {
            let store = lock(&self.store);
            match id {
                Some(id) => {
                    let list = store.list(&id).ok_or("That list is gone.")?;
                    let before = store.list_chat_ids(&id);
                    (id, list.order, list.color, before)
                }
                None => {
                    let (id, order) = store.next_list();
                    // WhatsApp offers 20 list colours.
                    let color = (id.parse::<i32>().unwrap_or(0) - 1).rem_euclid(20);
                    (id, order, color, Vec::new())
                }
            }
        };
        let value = {
            let mut value = wa::SyncActionValue::default();
            value.label_edit_action = MessageField::some({
                let mut label = LabelEditAction::default();
                label.name = Some(name.to_string());
                label.color = Some(color);
                label.deleted = Some(false);
                label.order_index = Some(order);
                label.is_active = Some(true);
                label.r#type = Some(ListType::CUSTOM);
                label
            });
            value.timestamp = Some(now_ms());
            value
        };
        client
            .send_app_state_action(&schemas::LABEL_EDIT, &[id.as_str()], &value)
            .await
            .map_err(|e| format!("The list was not saved: {e}"))?;
        lock(&self.store).edit_list(&id, Some(name), true, Some(order), Some(color));
        self.mark_dirty();

        let labels = client.labels();
        let changes = add
            .iter()
            .filter(|c| !before.contains(c))
            .map(|c| (c, true))
            .chain(remove.iter().filter(|c| before.contains(c)).map(|c| (c, false)));
        for (chat, member) in changes {
            let jid: Jid = chat.parse().map_err(|e| format!("Invalid chat {chat}: {e}"))?;
            let sent = if member {
                labels.add_chat_label(&id, &jid).await
            } else {
                labels.remove_chat_label(&id, &jid).await
            };
            sent.map_err(|e| format!("The list was saved, but not all of its chats: {e}"))?;
            lock(&self.store).set_list_chat(&id, chat, member);
            self.mark_dirty();
        }
        Ok(id)
    }

    pub async fn delete_list(&self, id: &str) -> Result<(), String> {
        let client = self.connected_client().await?;
        client
            .labels()
            .delete_label(id)
            .await
            .map_err(|e| format!("The list was not deleted: {e}"))?;
        lock(&self.store).delete_list(id);
        self.mark_dirty();
        Ok(())
    }

    /// Stars a message or takes the star away, on the phone too.
    pub async fn set_starred(&self, chat_id: &str, message_id: &str, starred: bool) -> Result<MessageDto, String> {
        self.recall(chat_id, message_id);
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let (sender, from_me) = lock(&self.store)
            .star_target(chat_id, message_id)
            .ok_or("That message is gone.")?;
        // Groups name who sent a message that isn't ours.
        let participant = sender
            .filter(|_| chat_id.ends_with("@g.us") && !from_me)
            .and_then(|s| s.parse::<Jid>().ok());
        let id = MessageId::new(message_id).map_err(|e| format!("The star was not saved: {e}"))?;
        let target = MessageRef::new(&jid, id, participant.as_ref(), from_me).map_err(|e| format!("The star was not saved: {e}"))?;
        let actions = client.chat_actions();
        let sent = if starred { actions.star_message(&target).await } else { actions.unstar_message(&target).await };
        sent.map_err(|e| format!("The star was not saved: {e}"))?;
        let dto = lock(&self.store)
            .set_starred(chat_id, message_id, starred)
            .ok_or("That message is gone.")?;
        self.mark_dirty();
        Ok(dto)
    }

    /// The chat for a phone number, when the number is on WhatsApp.
    pub async fn lookup_number(&self, phone: &str) -> Result<Option<String>, String> {
        let digits: String = phone.chars().filter(char::is_ascii_digit).collect();
        if digits.len() < 7 || digits.len() > 15 {
            return Err("Enter the full number, with its country code.".into());
        }
        let client = self.connected_client().await?;
        let results = client
            .contacts()
            .is_on_whatsapp(&[Jid::pn(&digits)])
            .await
            .map_err(|e| format!("The number could not be checked: {e}"))?;
        let Some(found) = results.into_iter().find(|r| r.is_registered) else {
            return Ok(None);
        };
        let chat = format!("{digits}@s.whatsapp.net");
        if let Some(lid) = found.lid.as_ref().or(Some(&found.jid).filter(|j| j.is_lid())) {
            lock(&self.store).learn_mapping(&lid.to_non_ad_string(), &chat);
        }
        Ok(Some(chat))
    }

    fn remember_identity(&self, client: &Client) {
        let pn = client.pn();
        let lid = client.lid();
        let name = Some(client.push_name()).filter(|n| !n.is_empty());
        {
            let mut store = lock(&self.store);
            store.set_own_ids(
                [pn.as_ref(), lid.as_ref()]
                    .into_iter()
                    .flatten()
                    .map(|j| j.to_non_ad_string()),
            );
            if let (Some(pn), Some(lid)) = (&pn, &lid) {
                store.learn_mapping(&lid.to_non_ad_string(), &pn.to_non_ad_string());
            }
        }
        let avatar = lock(&self.store).own_avatar();
        self.set_status(|s| {
            s.me = Some(Me {
                phone: pn.map(|j| format!("+{}", j.user_base())),
                name,
                avatar,
            })
        });
    }

    /// Replays pins, mutes and archives, or favorites, lists and stars, for a
    /// cache that never recorded them.
    fn sync_chat_state(self: &Arc<Self>, generation: u64, client: Arc<Client>) {
        let (chat_state, lists) = {
            let store = lock(&self.store);
            (store.needs_chat_state_sync(), store.needs_lists_sync())
        };
        let mut patches = Vec::new();
        if chat_state {
            patches.push(WAPatchName::RegularLow);
        }
        if lists {
            patches.push(WAPatchName::Regular);
        }
        if patches.is_empty() {
            return;
        }
        // Pins, favorites and stars all live here.
        patches.push(WAPatchName::RegularHigh);
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            let resync = client.resync_app_state(patches, AppStateResyncMode::Snapshot);
            let done = match tokio::time::timeout(Duration::from_secs(120), resync).await {
                Ok(Ok(report)) if report.all_synced() => true,
                Ok(Ok(report)) => {
                    log::warn!("whatsapp: chat state still stale: {:?}", report.unsynced().collect::<Vec<_>>());
                    false
                }
                // The phone is asked to rebuild that collection in the
                // background. Asking again on every start would only download
                // every snapshot again and fail the same way.
                Ok(Err(whatsapp_rust::AppStateError::Internal(e)))
                    if e.chain().any(|cause| {
                        matches!(
                            cause.downcast_ref::<whatsapp_rust::wacore::appstate::AppStateError>(),
                            Some(whatsapp_rust::wacore::appstate::AppStateError::SnapshotMACMismatch)
                        )
                    }) =>
                {
                    log::warn!("whatsapp: a chat state snapshot does not validate; left to the phone's recovery");
                    true
                }
                Ok(Err(e)) => {
                    log::warn!("whatsapp: chat state sync failed: {e}");
                    false
                }
                Err(_) => {
                    log::warn!("whatsapp: chat state sync timed out");
                    false
                }
            };
            if done && mgr.generation.load(Ordering::SeqCst) == generation {
                let mut store = lock(&mgr.store);
                store.set_chat_state_synced(true);
                store.set_lists_synced(true);
                drop(store);
                mgr.mark_dirty();
            }
        });
    }

    /// Asks the phone once for the recent messages of chats where ours are
    /// not marked read, or where a photo cannot be fetched: a cache from
    /// before ticks and photos were tracked properly has them wrong. The
    /// answers arrive as history.
    fn refresh_ticks(self: &Arc<Self>, generation: u64, client: Arc<Client>) {
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            // Let the messages missed while offline land first.
            tokio::time::sleep(Duration::from_secs(8)).await;
            if mgr.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let stale = {
                let mut store = lock(&mgr.store);
                let mut stale = store.stale_ticks(TICK_CHATS, TICK_MESSAGES);
                for chat in store.stale_media(TICK_CHATS, TICK_MESSAGES) {
                    if !stale.iter().any(|known| known.0 == chat.0) {
                        stale.push(chat);
                    }
                }
                // Each request wakes the phone, so a start asks about a few
                // dozen chats at most, each once a day.
                stale.truncate(TICK_CHATS);
                for (chat, ..) in &stale {
                    store.mark_refreshed(chat);
                }
                stale
            };
            mgr.save_dirty.store(true, Ordering::Release);
            mgr.wake.notify_one();
            for (chat, id, from_me, ts_ms) in stale {
                if mgr.generation.load(Ordering::SeqCst) != generation {
                    return;
                }
                let Ok(jid) = chat.parse::<Jid>() else {
                    continue;
                };
                let asked = client
                    .fetch_message_history(&jid, &id, from_me, ts_ms, TICK_MESSAGES as i32)
                    .await;
                if let Err(e) = asked {
                    return log::warn!("whatsapp: tick refresh stopped: {e}");
                }
                tokio::time::sleep(TICK_PACE).await;
            }
        });
    }

    /// Looks up how many people are in a group: a message there only counts
    /// as read once all of them have read it.
    fn count_members(self: &Arc<Self>, generation: u64, client: Arc<Client>, group: Jid) {
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            let metadata = match client.groups().fetch_metadata(&group).await {
                Ok(metadata) => metadata,
                Err(e) => return log::warn!("whatsapp: group size lookup failed: {e}"),
            };
            if mgr.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let members = members_of(&metadata);
            let changed = {
                let mut store = lock(&mgr.store);
                store.set_group_participants(&group.to_non_ad_string(), &members);
                store.set_group_members(&group.to_non_ad_string(), members.len() as u32)
            };
            for dto in changed {
                mgr.emit("message", dto);
            }
            mgr.mark_dirty();
        });
    }

    /// Downloads our own profile picture and those of the listed chats that
    /// are due a check.
    fn sync_avatars(self: &Arc<Self>, generation: u64, client: Arc<Client>) {
        if self.avatars_running.swap(true, Ordering::AcqRel) {
            return;
        }
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            mgr.run_avatar_sync(generation, &client).await;
            mgr.avatars_running.store(false, Ordering::Release);
        });
    }

    /// Looks a profile picture up and saves it under `owner` when it changed.
    /// Returns the id of the picture now on disk, if there is one. `Err`
    /// means the lookups should stop for now.
    async fn fetch_avatar(
        &self,
        client: &Client,
        dir: &Path,
        owner: &str,
        jid: &Jid,
        stored: Option<&str>,
    ) -> Result<Option<String>, String> {
        let on_disk = stored.filter(|id| dir.join(store::avatar_file(owner, id)).exists());
        let request = ProfilePictureRequest::new(picture_target(jid), ProfilePictureType::Preview).existing_id(on_disk);
        let lookup = client
            .pictures()
            .lookup(request)
            .await
            .map_err(|e| format!("lookup stopped: {e}"))?;
        match lookup {
            ProfilePictureLookup::Found(picture) => {
                let response = client
                    .http_client()
                    .execute(HttpRequest::get(picture.url.as_str()))
                    .await
                    .map_err(|e| format!("download stopped: {e}"))?;
                if response.status_code != 200 || response.body.is_empty() {
                    log::warn!("whatsapp: profile picture download got {}", response.status_code);
                    return Ok(on_disk.map(str::to_string));
                }
                let file = dir.join(store::avatar_file(owner, &picture.id));
                crate::vault::get()
                    .write(&file, &response.body)
                    .map_err(|e| format!("could not save {}: {e}", file.display()))?;
                if let Some(old) = stored.filter(|old| *old != picture.id) {
                    let _ = fs::remove_file(dir.join(store::avatar_file(owner, old)));
                }
                Ok(Some(picture.id))
            }
            ProfilePictureLookup::Unchanged => Ok(on_disk.map(str::to_string)),
            ProfilePictureLookup::NotFound | ProfilePictureLookup::NotAuthorized => {
                if let Some(old) = stored {
                    let _ = fs::remove_file(dir.join(store::avatar_file(owner, old)));
                }
                Ok(None)
            }
            // Rate limited (or an outcome added later): try again next time.
            _ => Err("lookups paused".into()),
        }
    }

    async fn run_avatar_sync(&self, generation: u64, client: &Client) {
        let dir = self.dir.join(AVATAR_DIR);
        if let Err(e) = secure_dir(&dir) {
            return log::warn!("whatsapp: could not create {}: {e}", dir.display());
        }
        let own = lock(&self.store).own_avatar_due();
        if let Some((id, stored)) = own
            && let Ok(jid) = id.parse::<Jid>()
        {
            match self.fetch_avatar(client, &dir, store::OWN_AVATAR, &jid, stored.as_deref()).await {
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
                Err(e) => return log::warn!("whatsapp: profile picture {e}"),
            }
        }
        loop {
            let due = lock(&self.store).avatars_due(20);
            if due.is_empty() {
                return;
            }
            for (chat_id, stored_id) in due {
                if self.generation.load(Ordering::SeqCst) != generation {
                    return;
                }
                let Ok(jid) = chat_id.parse::<Jid>() else {
                    lock(&self.store).avatar_checked(&chat_id);
                    continue;
                };
                match self.fetch_avatar(client, &dir, &chat_id, &jid, stored_id.as_deref()).await {
                    Ok(picture) => lock(&self.store).set_avatar(&chat_id, picture),
                    Err(e) => return log::warn!("whatsapp: profile picture {e}"),
                }
                self.mark_dirty();
                tokio::time::sleep(AVATAR_PACE).await;
            }
        }
    }

    async fn handle_event(self: Arc<Self>, generation: u64, event: Arc<Event>, client: Arc<Client>) {
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        match &*event {
            Event::PairingQrCode(qr) => {
                let svg = render_qr(&qr.code);
                let timeout = qr.timeout.as_secs();
                self.set_status(|s| {
                    s.state = WaState::Qr;
                    s.qr_svg = svg;
                    s.qr_timeout_secs = Some(timeout);
                    s.error = None;
                });
            }
            Event::PairingQrCodesExhausted(_) => self.set_status(|s| {
                s.state = WaState::QrExpired;
                s.qr_svg = None;
            }),
            Event::QrScannedWithoutMultidevice(_) => {
                self.fail("Update WhatsApp on your phone, then try linking again.".into())
            }
            Event::PairSuccess(_) => {
                self.write_marker();
                self.set_status(|s| {
                    s.linked = true;
                    s.state = WaState::Connecting;
                    s.qr_svg = None;
                    s.error = None;
                });
            }
            Event::PairError(_) => self.fail("Linking failed. Try again.".into()),
            Event::Connected(_) => {
                self.write_marker();
                self.remember_identity(&client);
                self.set_status(|s| {
                    s.linked = true;
                    s.state = WaState::Connected;
                    s.qr_svg = None;
                    s.error = None;
                });
                self.mark_dirty();
                // A new connection starts offline.
                self.announced.store(1, Ordering::Release);
                if self.active.load(Ordering::Acquire) {
                    let mgr = self.clone();
                    tauri::async_runtime::spawn(async move { mgr.announce_presence().await });
                }
                self.sync_chat_state(generation, client.clone());
                self.sync_sticker_favorites(generation, client.clone());
                self.fill_library(generation);
                self.refresh_ticks(generation, client.clone());
                self.fetch_all_history(generation, client.clone());
                self.sync_participants(generation, client.clone());
                self.sync_blocklist(generation, client.clone());
                self.sync_avatars(generation, client);
            }
            Event::Disconnected(_) if self.is_linked() => {
                self.set_status(|s| s.state = WaState::Connecting);
            }
            Event::LoggedOut(_) => {
                tauri::async_runtime::spawn(self.clone().handle_unlinked(generation));
            }
            Event::StreamReplaced(_) => self.fail(
                "This session was opened somewhere else. Reconnect to use it here.".into(),
            ),
            Event::TemporaryBan(_) => {
                self.fail("WhatsApp temporarily banned this account.".into())
            }
            Event::ClientOutdated(_) => {
                self.fail("WhatsApp rejected this client version. Update Omega Zap.".into())
            }
            Event::ConnectFailure(_) => self.fail("WhatsApp refused the connection.".into()),
            Event::Messages(batch) => {
                let mut dtos: Vec<MessageDto> = Vec::new();
                let mut notices: Vec<Notice> = Vec::new();
                let mut pinned_in: Vec<String> = Vec::new();
                // Opened once the store is free: (chat, voter, vote).
                let mut votes: Vec<(String, String, convert::SealedVote)> = Vec::new();
                // Reactions and deletions can be for messages only the history has.
                for m in batch.iter() {
                    let target = convert::revoke(&m.message)
                        .or_else(|| convert::reaction(&m.message).map(|(target, _)| target))
                        .or_else(|| convert::edit(&m.message, 0).map(|edit| edit.target))
                        .or_else(|| convert::poll_vote(&m.message, 0).map(|vote| vote.target));
                    if let Some(target) = target {
                        self.recall(&m.info.source.chat.to_non_ad_string(), &target);
                    }
                }
                {
                    let mut store = lock(&self.store);
                    for m in batch.iter() {
                        if let Some(target) = convert::revoke(&m.message) {
                            let chat = m.info.source.chat.to_non_ad_string();
                            dtos.extend(store.delete_message(&chat, &target, Deletion::ForEveryone));
                            continue;
                        }
                        if let Some(edit) = convert::edit(&m.message, m.info.timestamp.timestamp_millis()) {
                            let chat = m.info.source.chat.to_non_ad_string();
                            let sender = m.info.source.sender.to_non_ad_string();
                            if let Some(alt) = &m.info.source.sender_alt {
                                store.learn_mapping(&sender, &alt.to_non_ad_string());
                            }
                            let editor = (!m.info.source.is_from_me).then_some(sender);
                            dtos.extend(store.edit_message(&chat, &edit.target, editor.as_deref(), edit.content, edit.at_ms));
                            continue;
                        }
                        if let Some(vote) = convert::poll_vote(&m.message, m.info.timestamp.timestamp_millis()) {
                            let sender = m.info.source.sender.to_non_ad_string();
                            if let Some(alt) = &m.info.source.sender_alt {
                                store.learn_mapping(&sender, &alt.to_non_ad_string());
                            }
                            votes.push((m.info.source.chat.to_non_ad_string(), sender, vote));
                            continue;
                        }
                        if convert::is_sealed_edit(&m.message) {
                            log::info!("whatsapp: an edit sealed with the original message's secret was skipped");
                            continue;
                        }
                        if let Some(pin) = convert::pin(&m.message, m.info.timestamp.timestamp_millis()) {
                            let chat = m.info.source.chat.to_non_ad_string();
                            let by = (!m.info.source.is_from_me).then(|| m.info.source.sender.to_non_ad_string());
                            let stored = store.set_message_pin(&chat, &pin.target, pin.at_ms, pin.until_ms, by.as_deref());
                            log::info!(
                                "whatsapp: {} of {} in {chat} (stored: {stored})",
                                if pin.until_ms.is_some() { "pin" } else { "unpin" },
                                pin.target
                            );
                            pinned_in.push(store.chat_key(&chat));
                            continue;
                        }
                        match convert::reaction(&m.message) {
                            Some((target, emoji)) => {
                                let chat = m.info.source.chat.to_non_ad_string();
                                let sender = m.info.source.sender.to_non_ad_string();
                                let Some(dto) = store.set_reaction(&chat, &target, &sender, emoji.as_deref()) else {
                                    continue;
                                };
                                if let Some(emoji) = &emoji {
                                    notices.extend(store.reaction_notice(&chat, &target, &sender, emoji));
                                }
                                dtos.push(dto);
                            }
                            None => {
                                let Some(incoming) = convert::live(&m.message, &m.info) else {
                                    continue;
                                };
                                self.note_own_media([&incoming.message]);
                                let Some(dto) = store.ingest(incoming, true) else {
                                    continue;
                                };
                                notices.extend(store.message_notice(&dto.chat_id, &dto.id));
                                dtos.push(dto);
                            }
                        }
                    }
                }
                for (chat, voter, vote) in votes {
                    dtos.extend(self.open_vote(&client, &chat, &voter, vote).await);
                }
                for dto in dtos {
                    self.emit("message", dto);
                }
                pinned_in.sort();
                pinned_in.dedup();
                for chat in pinned_in {
                    self.emit("pins", chat);
                }
                for notice in notices {
                    crate::notify::deliver(&self.app, self.kind.account(), &self.kind.event("notice"), notice);
                }
                self.mark_dirty();
            }
            Event::Receipt(receipt) => {
                let delivery = match receipt.r#type {
                    ReceiptType::Delivered => Some(Delivery::Delivered),
                    ReceiptType::Read | ReceiptType::Played => Some(Delivery::Read),
                    ReceiptType::ReadSelf => {
                        lock(&self.store).mark_read(&receipt.source.chat.to_non_ad_string());
                        self.mark_dirty();
                        None
                    }
                    _ => None,
                };
                if let Some(delivery) = delivery {
                    let ids: Vec<String> = receipt.message_ids.iter().map(|id| id.to_string()).collect();
                    let chat = receipt.source.chat.to_non_ad_string();
                    let reader = receipt.source.sender.to_non_ad_string();
                    let (changed, count_members) = {
                        let mut store = lock(&self.store);
                        if let Some(alt) = &receipt.source.sender_alt {
                            store.learn_mapping(&reader, &alt.to_non_ad_string());
                        }
                        let changed = store.apply_receipt(&chat, &ids, delivery, Some(&reader));
                        (changed, delivery == Delivery::Read && store.claim_member_check(&chat))
                    };
                    // Who has read so far is worth keeping even when no tick changed.
                    self.save_dirty.store(true, Ordering::Release);
                    self.wake.notify_one();
                    if !changed.is_empty() {
                        for dto in changed {
                            self.emit("message", dto);
                        }
                        self.mark_dirty();
                    }
                    if count_members {
                        self.count_members(generation, client, receipt.source.chat.clone());
                    }
                }
            }
            Event::ContactUpdate(update) => {
                let jid = update.jid.to_non_ad_string();
                {
                    let mut store = lock(&self.store);
                    if let (Some(lid), Some(pn)) = (&update.action.lid_jid, &update.action.pn_jid) {
                        store.learn_mapping(lid, pn);
                    }
                    let name = update
                        .action
                        .full_name
                        .as_deref()
                        .or(update.action.first_name.as_deref());
                    if let Some(name) = name {
                        store.set_contact_name(&jid, name);
                    }
                }
                self.mark_dirty();
            }
            Event::PinUpdate(update) => {
                let pinned = update.action.pinned.unwrap_or(false).then(|| {
                    update.action_timestamp.unwrap_or(update.timestamp).timestamp_millis()
                });
                lock(&self.store).set_pinned(&update.jid.to_non_ad_string(), pinned);
                self.mark_dirty();
            }
            Event::MuteUpdate(update) => {
                let until = update.action.muted.unwrap_or(false).then(|| {
                    // No end, or a negative one, means muted for good.
                    match update.action.mute_end_timestamp {
                        Some(end) if end > 0 => end,
                        _ => i64::MAX,
                    }
                });
                lock(&self.store).set_muted(&update.jid.to_non_ad_string(), until);
                self.mark_dirty();
            }
            Event::MarkChatAsReadUpdate(update) => {
                lock(&self.store).set_read_elsewhere(
                    &update.jid.to_non_ad_string(),
                    update.action.read.unwrap_or(true),
                    update.action_timestamp.unwrap_or(update.timestamp).timestamp_millis(),
                );
                self.mark_dirty();
            }
            Event::ChatPresence(update) => {
                let source = &update.source;
                let store = lock(&self.store);
                let sender = source.sender.to_non_ad_string();
                if source.is_from_me || store.is_own(&sender) {
                    return;
                }
                let typing = TypingDto {
                    chat_id: store.canonical(&source.chat.to_non_ad_string()),
                    who: source.is_group.then(|| store.display_name(&sender)),
                    recording: update.state == ChatActivity::RecordingAudio,
                    active: update.state != ChatActivity::Idle,
                };
                drop(store);
                self.emit("typing", typing);
            }
            Event::Presence(update) => {
                let presence = PresenceDto {
                    chat_id: lock(&self.store).canonical(&update.from.to_non_ad_string()),
                    online: update.status == PresenceStatus::Available,
                    last_seen_ms: update.last_seen.map(|t| t.timestamp_millis()),
                };
                self.emit("presence", presence);
            }
            Event::ArchiveUpdate(update) => {
                lock(&self.store).set_archived(
                    &update.jid.to_non_ad_string(),
                    update.action.archived.unwrap_or(false),
                );
                self.mark_dirty();
            }
            Event::GroupUpdate(update) => {
                let Some(change) = group_change(&update.action) else { return };
                let group = update.group_jid.to_non_ad_string();
                if lock(&self.store).apply_group_change(&group, change) {
                    self.mark_dirty();
                    self.emit("group", lock(&self.store).chat_key(&group));
                }
            }
            Event::DeleteChatUpdate(update) => {
                let chat = update.jid.to_non_ad_string();
                let before = range_end(update.action.message_range.as_option(), update.action_timestamp.unwrap_or(update.timestamp).timestamp_millis());
                if lock(&self.store).delete_chat(&chat, before) {
                    self.chat_emptied(&chat);
                }
            }
            Event::ClearChatUpdate(update) => {
                let chat = update.jid.to_non_ad_string();
                let before = range_end(update.action.message_range.as_option(), update.action_timestamp.unwrap_or(update.timestamp).timestamp_millis());
                if lock(&self.store).clear_chat(&chat, before, !update.delete_starred) {
                    self.chat_emptied(&chat);
                }
            }
            Event::StarUpdate(update) => {
                let changed = lock(&self.store).set_starred(
                    &update.chat_jid.to_non_ad_string(),
                    &update.message_id,
                    update.action.starred.unwrap_or(false),
                );
                if let Some(dto) = changed {
                    self.emit("message", dto);
                    self.mark_dirty();
                }
            }
            Event::FavoriteStickerUpdate(update) => {
                self.phone_favorite(&update.filehash, &update.action, update.timestamp.timestamp_millis());
            }
            Event::RemoveRecentStickerUpdate(update) => {
                self.phone_removed_recent(&update.filehash, update.action.last_sticker_sent_ts);
            }
            Event::FavoritesUpdate(update) => {
                let ids = update.action.favorites.iter().filter_map(|f| f.id.clone()).collect();
                lock(&self.store).set_favorites(ids);
                self.mark_dirty();
            }
            Event::LabelEditUpdate(update) => {
                let action = &update.action;
                let mut store = lock(&self.store);
                if action.deleted == Some(true) {
                    store.delete_list(&update.label_id);
                } else {
                    // Lists the user made; WhatsApp Business labels carry no type.
                    let custom = matches!(action.r#type, None | Some(ListType::CUSTOM | ListType::NONE))
                        && action.is_active != Some(false);
                    store.edit_list(&update.label_id, action.name.as_deref(), custom, action.order_index, action.color);
                }
                drop(store);
                self.mark_dirty();
            }
            Event::LabelAssociationUpdate(update) => {
                lock(&self.store).set_list_chat(
                    &update.label_id,
                    &update.chat_jid.to_non_ad_string(),
                    update.action.labeled.unwrap_or(false),
                );
                self.mark_dirty();
            }
            Event::PictureUpdate(update) => {
                lock(&self.store).invalidate_avatar(&update.jid.to_non_ad_string());
                self.sync_avatars(generation, client);
            }
            Event::HistorySync(lazy) => {
                // Answers to our own requests for older messages aren't the first sync.
                if let Some(progress) = lazy.progress().filter(|_| lazy.sync_type() != ON_DEMAND) {
                    self.set_status(|s| {
                        s.sync_progress = (progress < 100).then_some(progress);
                    });
                }
                let mgr = self.clone();
                let event = event.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Event::HistorySync(lazy) = &*event
                        && let Err(e) = mgr.ingest_history(lazy)
                    {
                        log::warn!("whatsapp: history sync chunk skipped: {e}");
                    }
                    mgr.mark_dirty();
                    // History brings chats the first pass did not know about.
                    mgr.sync_avatars(generation, client);
                });
            }
            _ => {}
        }
    }

    fn ingest_history(&self, lazy: &LazyHistorySync) -> Result<(), String> {
        let on_demand = lazy.sync_type() == ON_DEMAND;
        let mut answered: HashMap<String, u32> = HashMap::new();
        // Our stickers and GIFs, kept as recent once the chunk is in.
        let mut own_media: Vec<StoredMessage> = Vec::new();
        let mut stream = lazy.stream();
        let mut conversation = wa::Conversation::default();
        while stream
            .next_conversation_into(&mut conversation)
            .map_err(|e| e.to_string())?
        {
            let c = &conversation;
            let mut store = lock(&self.store);
            store.ingest_conversation(store::ConversationMeta {
                id: &c.id,
                name: c.name.as_deref().or(c.display_name.as_deref()),
                pn_jid: c.pn_jid.as_deref(),
                lid_jid: c.lid_jid.as_deref(),
                // A page asked for later carries the read state of when the
                // phone kept it, not of now.
                unread: c.unread_count.filter(|_| !on_demand),
                marked_unread: c.marked_as_unread.filter(|_| !on_demand),
                pinned_at_s: c.pinned,
                archived: c.archived,
                mute_end: c.mute_end_time,
                conversation_ts_s: c.conversation_timestamp.or(c.last_msg_timestamp),
            });
            if on_demand {
                *answered.entry(store.chat_key(&c.id)).or_default() += c.messages.len() as u32;
            }
            // Edits are applied once the messages they change are in.
            let mut edits = Vec::new();
            for item in &c.messages {
                edits.extend(convert::history_edit(&c.id, item));
                for pin in convert::history_pins(&c.id, item) {
                    store.set_message_pin(&c.id, &pin.target, pin.at_ms, pin.until_ms, pin.by.as_deref());
                }
                if let Some(incoming) = convert::history(&c.id, item) {
                    let id = incoming.message.id.clone();
                    let options = incoming.message.content.poll.as_ref().map(|p| p.options.clone());
                    if incoming.message.from_me && incoming.message.content.wa_media.is_some() {
                        own_media.push(incoming.message.clone());
                    }
                    store.ingest(incoming, false);
                    // The phone sends a poll's votes already opened.
                    for (voter, hashes, at_ms) in options.as_ref().map(|_| convert::history_votes(&c.id, item)).unwrap_or_default() {
                        let voter = voter.or_else(|| store.own_id().map(str::to_string));
                        if let (Some(voter), Some(options)) = (voter, &options) {
                            store.set_poll_vote(&c.id, &id, &voter, convert::picked(options, &hashes), at_ms);
                        }
                    }
                    for (author, emoji) in convert::history_reactions(&c.id, item) {
                        let author = author.or_else(|| store.own_id().map(str::to_string));
                        if let Some(author) = author {
                            store.set_reaction(&c.id, &id, &author, Some(&emoji));
                        }
                    }
                }
            }
            for (editor, edit) in edits {
                store.edit_message(&c.id, &edit.target, editor.as_deref(), edit.content, edit.at_ms);
            }
        }
        self.note_own_media(&own_media);
        let rest = stream.remainder().map_err(|e| e.to_string())?;
        self.note_phone_recents(&rest.recent_stickers);
        let mut store = lock(&self.store);
        for mapping in &rest.phone_number_to_lid_mappings {
            if let (Some(pn), Some(lid)) = (&mapping.pn_jid, &mapping.lid_jid) {
                store.learn_mapping(lid, pn);
            }
        }
        for entry in &rest.pushnames {
            if let (Some(id), Some(name)) = (&entry.id, &entry.pushname) {
                store.set_push_name(id, name);
            }
        }
        drop(store);
        if on_demand {
            let mut got = lock(&self.on_demand);
            for (chat, count) in answered {
                *got.entry(chat).or_default() += count;
            }
            drop(got);
            self.on_demand_answered.notify_one();
        }
        Ok(())
    }

    /// Asks the phone for every chat's older messages, a page at a time,
    /// a few chats per request burst, newest chats first, until it has
    /// nothing older. Chats with no message here are left until they are
    /// opened: there is nothing to count back from, and the phone is woken
    /// for each one. Chats found complete stay so across restarts. Shows
    /// where it is in the status.
    fn fetch_all_history(self: &Arc<Self>, generation: u64, client: Arc<Client>) {
        if self.backfill_for.swap(generation, Ordering::SeqCst) == generation {
            return;
        }
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            let current = || mgr.generation.load(Ordering::SeqCst) == generation;
            // Let the first sync and the messages missed while offline land.
            tokio::time::sleep(BACKFILL_START).await;
            let mut fetched = 0u32;
            let mut silent = 0u32;
            // Chats the phone did not answer for, this run.
            let mut strikes: HashMap<String, u32> = HashMap::new();
            // Chats with no message here, left until opened.
            let mut empty: std::collections::HashSet<String> = std::collections::HashSet::new();
            loop {
                if !current() {
                    return;
                }
                if lock(&mgr.status).sync_progress.is_some() {
                    tokio::time::sleep(BACKFILL_PACE).await;
                    continue;
                }
                // What came in is written first, so the oldest message is known.
                mgr.save_now();
                let queue = lock(&mgr.store).history_queue();
                let opened = lock(&mgr.backfill_next).take();
                let mut queue: Vec<(String, String)> = queue
                    .into_iter()
                    .filter(|(chat, _)| strikes.get(chat).is_none_or(|n| *n < BACKFILL_STRIKES))
                    .filter(|(chat, _)| !empty.contains(chat) || opened.as_ref() == Some(chat))
                    .collect();
                if let Some(at) = opened.as_ref().and_then(|first| queue.iter().position(|(chat, _)| chat == first)) {
                    let chat = queue.remove(at);
                    queue.insert(0, chat);
                }
                // The burst: each chat with where to count back from.
                let mut burst: Vec<(String, String, Jid, (String, bool, i64))> = Vec::new();
                for (chat, name) in queue {
                    if burst.len() >= BACKFILL_BURST {
                        break;
                    }
                    let Ok(jid) = chat.parse::<Jid>() else {
                        lock(&mgr.store).mark_history_complete(&chat);
                        continue;
                    };
                    let in_memory = lock(&mgr.store).oldest_message(&chat).cloned();
                    let oldest = match (in_memory, mgr.history.oldest(&chat)) {
                        (Some(a), Some(b)) => Some(if b.ts_ms < a.ts_ms { b } else { a }),
                        (a, b) => a.or(b),
                    };
                    let anchor = match &oldest {
                        Some(m) => (m.id.clone(), m.from_me, m.ts_ms),
                        // Opened with no message here: asked for what came before
                        // the last time it was active, with nothing to count from.
                        None if opened.as_ref() == Some(&chat) => {
                            let last = lock(&mgr.store).chat_last_ts(&chat);
                            (String::new(), false, if last > 0 { last + 1 } else { now_ms() })
                        }
                        None => {
                            empty.insert(chat);
                            continue;
                        }
                    };
                    burst.push((chat, name, jid, anchor));
                }
                if burst.is_empty() {
                    mgr.set_backfill_counts(BackfillState::Done, None, fetched, empty.len() as u32);
                    // Opening a chat brings it back to work; now and then, so
                    // does a look at which empty chats have messages by now.
                    tokio::select! {
                        _ = mgr.backfill_wake.notified() => {}
                        _ = tokio::time::sleep(BACKFILL_IDLE) => empty.clear(),
                    }
                    continue;
                }
                mgr.set_backfill_counts(BackfillState::Fetching, Some(burst[0].1.clone()), fetched, empty.len() as u32);
                let mut sent = Vec::new();
                let mut failed = false;
                for (chat, name, jid, anchor) in burst {
                    lock(&mgr.on_demand).remove(&chat);
                    match client.fetch_message_history(&jid, &anchor.0, anchor.1, anchor.2, BACKFILL_PAGE).await {
                        Ok(_) => sent.push((chat, name, anchor.2)),
                        Err(e) => {
                            log::warn!("whatsapp: could not ask for older messages of {chat}: {e}");
                            failed = true;
                            break;
                        }
                    }
                }
                let deadline = Instant::now() + BACKFILL_TIMEOUT;
                for (chat, _, oldest_ts) in sent {
                    match mgr.wait_for_answer(&chat, deadline).await {
                        // The phone answered with nothing older: this chat is all here.
                        Some(0) => {
                            lock(&mgr.store).mark_history_complete(&chat);
                            silent = 0;
                        }
                        Some(count) => {
                            fetched += count;
                            silent = 0;
                            mgr.save_now();
                            mgr.emit("history", &chat);
                            // Anything already known means the phone has no more before it.
                            let older = mgr.history.oldest(&chat).is_some_and(|now| now.ts_ms < oldest_ts)
                                || lock(&mgr.store).oldest_message(&chat).is_some_and(|now| now.ts_ms < oldest_ts);
                            if !older {
                                lock(&mgr.store).mark_history_complete(&chat);
                            }
                            mgr.mark_dirty();
                        }
                        None => {
                            let strike = strikes.entry(chat.clone()).or_default();
                            *strike += 1;
                            // A phone that answers nothing for a chat it was asked
                            // about twice has nothing older for it.
                            if *strike >= BACKFILL_STRIKES {
                                lock(&mgr.store).mark_history_complete(&chat);
                            }
                            silent += 1;
                        }
                    }
                }
                if failed || silent >= BACKFILL_SILENT {
                    // Several chats in a row without an answer: the phone is away.
                    mgr.set_backfill_counts(BackfillState::WaitingForPhone, None, fetched, empty.len() as u32);
                    tokio::time::sleep(BACKFILL_RETRY).await;
                    silent = 0;
                    strikes.clear();
                    continue;
                }
                tokio::time::sleep(BACKFILL_PACE).await;
            }
        });
    }

    /// Whether all of a chat's history is here; if not, it is fetched
    /// before the others', as for a search in it.
    pub fn complete_history(&self, chat_id: &str) -> bool {
        let complete = lock(&self.store).history_complete(chat_id);
        if !complete {
            self.fetch_history_first(chat_id);
        }
        complete
    }

    /// Fetches this chat's history before the others'.
    pub fn fetch_history_first(&self, chat_id: &str) {
        let chat = lock(&self.store).chat_key(chat_id);
        *lock(&self.backfill_next) = Some(chat);
        self.backfill_wake.notify_one();
    }

    /// Messages the phone sent for `chat` after being asked, or `None` if it
    /// didn't answer in time.
    async fn wait_for_answer(&self, chat: &str, deadline: Instant) -> Option<u32> {
        loop {
            if let Some(count) = lock(&self.on_demand).remove(chat) {
                return Some(count);
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            let _ = tokio::time::timeout(left, self.on_demand_answered.notified()).await;
        }
    }

    /// Shows where fetching all history is; chats left until opened count
    /// as done, since nothing more happens to them in the background.
    fn set_backfill_counts(&self, state: BackfillState, chat: Option<String>, messages: u32, waiting: u32) {
        let (done, total) = lock(&self.store).history_counts();
        let done = (done + waiting).min(total);
        self.set_status(|s| s.backfill = Some(Backfill { state, chat, done, total, messages }));
    }
}

/// Marks a message as forwarded, as the phones do: `score` is how many
/// times it has been sent on, 0 for one written here.
fn forwarding(context: Option<wa::ContextInfo>, score: u32) -> Option<wa::ContextInfo> {
    if score == 0 {
        return context;
    }
    Some({
        let mut context = context.unwrap_or_default();
        context.is_forwarded = Some(true);
        context.forwarding_score = Some(score);
        context
    })
}

/// `context` naming the people a text mentions, so they are told; with
/// `everyone`, its "@all" tells the whole group.
fn with_mentions(context: Option<wa::ContextInfo>, mentioned: Vec<String>, everyone: bool) -> Option<wa::ContextInfo> {
    if mentioned.is_empty() && !everyone {
        return context;
    }
    Some({
        let mut context = context.unwrap_or_default();
        context.mentioned_jid = mentioned;
        if everyone {
            context.non_jid_mentions = Some(1);
        }
        context
    })
}

/// Whether `text` still has "@all" as a word of its own.
fn mentions_everyone(text: &str) -> bool {
    text.match_indices(store::EVERYONE).any(|(at, word)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// `text` and the people it mentions as the group names them. The app
/// writes "@<number>" for each of `mentions`, by the id it knows them by;
/// a group that addresses its members by LID gets their LIDs instead, one
/// by phone number gets numbers. A mention no longer in the text is dropped.
async fn mentioning(client: &Arc<Client>, chat: &Jid, text: &str, mentions: &[String]) -> (String, Vec<String>) {
    if mentions.is_empty() {
        return (text.to_string(), Vec::new());
    }
    let routing = if whatsapp_rust::wacore_binary::JidExt::is_group(chat) { client.groups().routing_info(chat).await.ok() } else { None };
    let mut text = text.to_string();
    let mut mentioned = Vec::new();
    for id in mentions {
        let Ok(person) = id.parse::<Jid>() else { continue };
        let written = format!("@{}", person.user);
        if find_mention(&text, &written).is_none() {
            continue;
        }
        let addressed = match routing.as_deref() {
            Some(r) if matches!(r.addressing_mode, AddressingMode::Lid) && person.is_pn() => {
                r.lid_user_for_phone_user(&person.user).map(|lid| Jid::lid(lid.clone()))
            }
            Some(r) if matches!(r.addressing_mode, AddressingMode::Pn) && person.is_lid() => {
                r.phone_jid_for_lid_user(&person.user).cloned()
            }
            _ => None,
        }
        .unwrap_or_else(|| person.clone());
        let now = format!("@{}", addressed.user);
        while let Some(at) = find_mention(&text, &written) {
            text.replace_range(at..at + written.len(), &now);
        }
        mentioned.push(addressed.to_non_ad_string());
    }
    (text, mentioned)
}

/// Where "@<number>" is in `text` as a whole word, not the start of a longer number.
fn find_mention(text: &str, written: &str) -> Option<usize> {
    text.match_indices(written)
        .find(|(at, _)| !text[at + written.len()..].starts_with(|c: char| c.is_ascii_digit()))
        .map(|(at, _)| at)
}

/// A group's members as the store keeps them: (id, phone number id, LID).
/// The time of the last message a delete or clear from another device
/// covers: its range says, or else when it was made.
fn range_end(range: Option<&SyncActionMessageRange>, made_ms: i64) -> i64 {
    range
        .and_then(|r| r.last_message_timestamp)
        .filter(|ts| *ts > 0)
        .map_or(made_ms, |ts| ts * 1000)
}

fn members_of(metadata: &whatsapp_rust::features::GroupMetadata) -> Vec<store::GroupMember> {
    metadata
        .participants
        .iter()
        .map(|p| store::GroupMember {
            id: p.jid.to_non_ad_string(),
            pn: p.phone_number.as_ref().map(|j| j.to_non_ad_string()),
            lid: p.lid.as_ref().map(|j| j.to_non_ad_string()),
            admin: p.is_admin(),
            super_admin: p.is_super_admin(),
        })
        .collect()
}

/// What a group lookup says about a group, besides who is in it.
fn facts_of(metadata: &whatsapp_rust::features::GroupMetadata) -> store::GroupFacts {
    store::GroupFacts {
        description: metadata.description.clone(),
        created_ms: metadata.creation_time.map(|t| t as i64 * 1000),
        creator: metadata.creator_pn.as_ref().or(metadata.creator.as_ref()).map(|j| j.to_non_ad_string()),
        locked: metadata.is_locked,
    }
}

/// What a group notification changes that the store keeps.
fn group_change(action: &GroupNotificationAction) -> Option<store::GroupChange> {
    let ids = |list: &[GroupParticipantInfo]| list.iter().map(|p| p.jid.to_non_ad_string()).collect::<Vec<_>>();
    Some(match action {
        GroupNotificationAction::Subject { subject, .. } => store::GroupChange::Subject(subject.clone()),
        GroupNotificationAction::Description { description, .. } => store::GroupChange::Description(description.clone()),
        GroupNotificationAction::Add { participants, .. } => store::GroupChange::Added(
            participants
                .iter()
                .map(|p| {
                    let id = p.jid.to_non_ad_string();
                    let pn = p.phone_number.as_ref().map(|j| j.to_non_ad_string());
                    let lid = p.jid.is_lid().then(|| id.clone());
                    store::GroupMember { id, pn, lid, admin: false, super_admin: false }
                })
                .collect(),
        ),
        GroupNotificationAction::Remove { participants, .. } => store::GroupChange::Removed(ids(participants)),
        GroupNotificationAction::Promote { participants } => store::GroupChange::Promoted(ids(participants)),
        GroupNotificationAction::Demote { participants } => store::GroupChange::Demoted(ids(participants)),
        GroupNotificationAction::Locked { .. } => store::GroupChange::Locked(true),
        GroupNotificationAction::Unlocked => store::GroupChange::Locked(false),
        _ => return None,
    })
}

/// Where a chat's profile picture is asked for: groups have their own route.
fn picture_target(jid: &Jid) -> ProfilePictureTarget<'_> {
    if whatsapp_rust::wacore_binary::JidExt::is_group(jid) { ProfilePictureTarget::Group(jid) } else { ProfilePictureTarget::Contact(jid) }
}
