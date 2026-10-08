pub mod commands;
mod convert;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use whatsapp_rust::download::{DownloadParams, MediaType};
use whatsapp_rust::http::HttpRequest;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::store::DevicePropsOverride;
use whatsapp_rust::wacore::types::events::LazyHistorySync;
use whatsapp_rust::wacore::types::presence::ReceiptType;
use whatsapp_rust::waproto::buffa::MessageField;
use whatsapp_rust::waproto::whatsapp::sync_action_value::label_edit_action::ListType;
use whatsapp_rust::waproto::whatsapp::sync_action_value::{FavoritesAction, LabelEditAction, favorites_action};
use whatsapp_rust::{
    AppStateResyncMode, ErrorChainExt, MediaRetryResult, MediaReuploadRequest, ProfilePictureLookup, RevokeType, WAPatchName,
    schemas,
};

use crate::account::{
    AccountState as WaState, AccountStatus as WaStatus, Me, lock, render_qr, secure_dir,
};
use crate::store;
use crate::store::{
    CdnRef, ChatStore, ContactDto, Content, Deletion, Delivery, ListDto, ReplyRef, MessageDto, Notice, StarredDto, StoredMessage,
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
/// Pause between profile picture lookups, to stay clear of the rate limit.
const AVATAR_PACE: Duration = Duration::from_millis(150);
/// How many chats the phone is asked about when old ticks are refreshed, how
/// many messages of each, and the pause between the requests.
const TICK_CHATS: usize = 40;
const TICK_MESSAGES: usize = 50;
const TICK_PACE: Duration = Duration::from_secs(1);

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

impl From<CdnError> for String {
    fn from(error: CdnError) -> Self {
        match error {
            CdnError::Rejected(message) | CdnError::Failed(message) => message,
        }
    }
}

pub struct WaManager {
    app: AppHandle,
    dir: PathBuf,
    status: Mutex<WaStatus>,
    store: Mutex<ChatStore>,
    bot: tokio::sync::Mutex<Option<BotHandle>>,
    /// Bumped whenever a bot is started or torn down, so events from a bot we
    /// already abandoned are ignored.
    generation: AtomicU64,
    chats_dirty: AtomicBool,
    save_dirty: AtomicBool,
    avatars_running: AtomicBool,
}

impl WaManager {
    pub fn new(app: AppHandle, dir: PathBuf) -> std::io::Result<Arc<Self>> {
        secure_dir(&dir)?;
        let linked = dir.join(LINKED_MARKER).exists();
        let store = if linked {
            fs::read(dir.join(CACHE_FILE))
                .ok()
                .and_then(|bytes| ChatStore::from_json(&bytes))
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
        Ok(Arc::new(Self {
            app,
            dir,
            status: Mutex::new(WaStatus::idle(linked)),
            store: Mutex::new(store),
            bot: tokio::sync::Mutex::new(None),
            generation: AtomicU64::new(0),
            chats_dirty: AtomicBool::new(false),
            save_dirty: AtomicBool::new(false),
            avatars_running: AtomicBool::new(false),
        }))
    }

    /// Starts the UI refresh / save loop and reconnects an existing session.
    pub fn start(self: &Arc<Self>) {
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut last_save = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(350)).await;
                if mgr.chats_dirty.swap(false, Ordering::AcqRel) {
                    let _ = mgr.app.emit("wa://chats", ());
                }
                if last_save.elapsed() >= SAVE_INTERVAL
                    && mgr.save_dirty.swap(false, Ordering::AcqRel)
                {
                    mgr.save_now();
                    last_save = Instant::now();
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

    fn is_linked(&self) -> bool {
        self.dir.join(LINKED_MARKER).exists()
    }

    pub fn status(&self) -> WaStatus {
        lock(&self.status).clone()
    }

    fn set_status(&self, f: impl FnOnce(&mut WaStatus)) {
        let snapshot = {
            let mut status = lock(&self.status);
            f(&mut status);
            status.seq += 1;
            status.clone()
        };
        let _ = self.app.emit("wa://status", snapshot);
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
    }

    pub fn save_now(&self) {
        if !self.is_linked() {
            return;
        }
        let bytes = match lock(&self.store).to_json() {
            Ok(bytes) => bytes,
            Err(e) => return log::warn!("whatsapp: could not serialize chats: {e}"),
        };
        let path = self.dir.join(CACHE_FILE);
        let tmp = path.with_extension("json.tmp");
        let result = fs::write(&tmp, bytes).and_then(|_| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
            }
            fs::rename(&tmp, &path)
        });
        if let Err(e) = result {
            log::warn!("whatsapp: could not save chats: {e}");
        }
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
            CACHE_FILE.to_string(),
        ];
        for name in names {
            let path = self.dir.join(name);
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
        *lock(&self.store) = store;
        self.save_dirty.store(false, Ordering::Release);
        self.chats_dirty.store(true, Ordering::Release);
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
        let backend = SqliteStore::new(&session.to_string_lossy())
            .await
            .map_err(|e| format!("Could not open the WhatsApp session store: {e}"))?;

        let mgr = self.clone();
        let bot = Bot::builder()
            .with_backend(backend)
            .with_device_props(
                DevicePropsOverride::new()
                    .with_os("Omega Zap")
                    .with_platform_type(wa::device_props::PlatformType::CHROME),
            )
            .with_event_delivery(EventDelivery::Ordered { capacity: 8192 })
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
        let read = lock(&self.store).mark_read(chat_id);
        self.mark_dirty();
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
    pub async fn send_text(&self, chat_id: &str, text: &str, reply_to: Option<&str>) -> Result<MessageDto, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("The message is empty.".into());
        }
        let client = self
            .client()
            .await
            .ok_or("WhatsApp is not connected.")?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let (outgoing, quoted) = match reply_to {
            Some(id) => {
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
                let context = wa::ContextInfo {
                    stanza_id: Some(id.to_string()),
                    participant: Some(sender.clone()),
                    quoted_message: MessageField::some(wa::Message::text(source.text.clone())),
                    ..Default::default()
                };
                let reply = ReplyRef { author_jid: Some(sender), text: source.text };
                (wa::Message::text_with_context(text, context), Some(reply))
            }
            None => (wa::Message::text(text), None),
        };
        let sent = client
            .send_message(jid, outgoing)
            .await
            .map_err(|e| format!("The message was not sent: {e}"))?;

        let message = StoredMessage {
            id: sent.message_id,
            from_me: true,
            ts_ms: now_ms(),
            author_jid: None,
            content: Content::text(text, quoted),
            status: Some(Delivery::Sent),
            reactions: Vec::new(),
            wire: None,
            read_by: Vec::new(),
            starred: false,
            deleted: None,
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

    /// Reacts to a message, or takes the reaction back when it is already there.
    /// Downloads and decrypts a file into `dir_name` under `prefix` and its
    /// hash, unless it is already there. Returns the file name.
    async fn fetch_cdn(
        &self,
        file: CdnRef,
        dir_name: &str,
        prefix: &str,
        media_type: MediaType,
    ) -> Result<String, CdnError> {
        let decode = |field: &str| BASE64.decode(field).map_err(|e| format!("Bad file reference: {e}"));
        let sha256 = decode(&file.sha256)?;
        let extension = match file.mime.as_deref() {
            Some("image/png") => "png",
            Some("image/gif") => "gif",
            Some("image/webp") => "webp",
            _ => "jpg",
        };
        let hash: String = sha256.iter().map(|b| format!("{b:02x}")).collect();
        let name = format!("{prefix}-{hash}.{extension}");
        let dir = self.dir.join(dir_name);
        if dir.join(&name).exists() {
            return Ok(name);
        }
        let client = self
            .client()
            .await
            .ok_or("WhatsApp is not connected.")?;
        let params = DownloadParams::encrypted(
            file.path,
            &decode(&file.key)?,
            &sha256,
            &decode(&file.enc_sha256)?,
            file.size,
            media_type,
        );
        let bytes = client.download_from_params(&params).await.map_err(|e| {
            let rejected = matches!(e.http_status(), Some(401 | 403 | 404 | 410));
            let message = format!("Could not download the file: {e}");
            if rejected { CdnError::Rejected(message) } else { CdnError::Failed(message) }
        })?;
        secure_dir(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        fs::write(dir.join(&name), bytes).map_err(|e| format!("could not save {name}: {e}"))?;
        Ok(name)
    }

    /// Fetches a message's photo and returns its file name inside the media directory.
    pub async fn fetch_media(&self, chat_id: &str, message_id: &str) -> Result<String, String> {
        let file = lock(&self.store)
            .wa_media(chat_id, message_id)
            .ok_or("This message has no attachment to fetch.")?;
        match self.fetch_cdn(file.clone(), MEDIA_DIR, "wa", MediaType::Image).await {
            // WhatsApp drops media from its CDN after a few weeks; the sender's
            // phone can upload it again if it still has the file.
            Err(CdnError::Rejected(_)) => {
                let path = self
                    .reupload_media(chat_id, message_id, &file)
                    .await
                    .inspect_err(|e| log::warn!("whatsapp: photo {message_id} expired and was not re-uploaded: {e}"))?;
                lock(&self.store).set_wa_media_path(chat_id, message_id, &path);
                self.mark_dirty();
                Ok(self.fetch_cdn(CdnRef { path, ..file }, MEDIA_DIR, "wa", MediaType::Image).await?)
            }
            other => Ok(other?),
        }
    }

    /// Asks the phone that sent a message to put its photo back on the CDN,
    /// returning the new path.
    async fn reupload_media(&self, chat_id: &str, message_id: &str, file: &CdnRef) -> Result<String, String> {
        let client = self.connected_client().await?;
        let (sender, from_me) = lock(&self.store).star_target(chat_id, message_id).ok_or("That message is gone.")?;
        let chat: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let sender = sender.and_then(|s| s.parse::<Jid>().ok());
        let media_key = BASE64.decode(&file.key).map_err(|e| format!("Bad file reference: {e}"))?;
        let participant = sender.filter(|_| !from_me && (chat_id.ends_with("@g.us") || chat_id == "status@broadcast"));
        let result = client
            .media_reupload()
            .request(&MediaReuploadRequest {
                msg_id: message_id,
                chat_jid: &chat,
                media_key: &media_key,
                is_from_me: from_me,
                participant: participant.as_ref(),
            })
            .await
            .map_err(|e| format!("The phone did not send the photo again: {e}"))?;
        match result {
            MediaRetryResult::Success { direct_path } => Ok(direct_path),
            MediaRetryResult::NotFound => Err("The photo is no longer on the phone that sent it.".into()),
            other => Err(format!("The phone could not send the photo again: {other:?}")),
        }
    }

    /// Fetches the full-size picture of a message's link card and returns its
    /// file name inside the thumbnail directory.
    pub async fn link_thumbnail(&self, chat_id: &str, message_id: &str) -> Result<String, String> {
        let file = lock(&self.store)
            .link_thumbnail(chat_id, message_id)
            .ok_or("This link has no larger picture.")?;
        Ok(self.fetch_cdn(file, LINK_THUMB_DIR, "link", MediaType::LinkThumbnail).await?)
    }

    pub async fn react(&self, chat_id: &str, message_id: &str, emoji: &str) -> Result<MessageDto, String> {
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
        let key = wa::MessageKey {
            remote_jid: Some(chat_id.to_string()),
            from_me: Some(target.from_me),
            id: Some(message_id.to_string()),
            // Groups need the original sender to attribute the reaction.
            participant: target.author.filter(|_| chat_id.ends_with("@g.us") && !target.from_me),
            ..Default::default()
        };
        // An empty emoji is how WhatsApp removes a reaction.
        client
            .send_reaction(jid, key, if remove { "" } else { emoji })
            .await
            .map_err(|e| format!("The reaction was not sent: {e}"))?;
        let dto = lock(&self.store)
            .set_reaction(chat_id, message_id, &me, (!remove).then_some(emoji))
            .ok_or("That message is gone.")?;
        self.mark_dirty();
        Ok(dto)
    }

    /// Deletes a message on our devices, or for everyone when `for_everyone`
    /// and it is ours. Returns what is left of it, if anything is shown.
    pub async fn delete_message(&self, chat_id: &str, message_id: &str, for_everyone: bool) -> Result<Option<MessageDto>, String> {
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
                .revoke_message(jid, message_id, RevokeType::Sender)
                .await
                .map_err(|e| format!("The message was not deleted: {e}"))?;
        } else {
            let participant = sender
                .filter(|_| chat_id.ends_with("@g.us") && !from_me)
                .and_then(|s| s.parse::<Jid>().ok());
            client
                .chat_actions()
                .delete_message_for_me(&jid, participant.as_ref(), message_id, from_me, true, Some(ts / 1000))
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

    pub fn contacts(&self) -> Vec<ContactDto> {
        lock(&self.store).contacts()
    }

    async fn connected_client(&self) -> Result<Arc<Client>, String> {
        self.client().await.ok_or_else(|| "WhatsApp is not connected.".to_string())
    }

    /// Adds a chat to the favorites or takes it out, on the phone too.
    pub async fn set_favorite(&self, chat_id: &str, favorite: bool) -> Result<(), String> {
        let client = self.connected_client().await?;
        let ids = lock(&self.store).favorites_with(chat_id, favorite);
        let value = wa::SyncActionValue {
            favorites_action: MessageField::some(FavoritesAction {
                favorites: ids
                    .into_iter()
                    .map(|id| favorites_action::Favorite { id: Some(id), ..Default::default() })
                    .collect(),
                ..Default::default()
            }),
            timestamp: Some(now_ms()),
            ..Default::default()
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
        let value = wa::SyncActionValue {
            label_edit_action: MessageField::some(LabelEditAction {
                name: Some(name.to_string()),
                color: Some(color),
                deleted: Some(false),
                order_index: Some(order),
                is_active: Some(true),
                r#type: Some(ListType::CUSTOM),
                ..Default::default()
            }),
            timestamp: Some(now_ms()),
            ..Default::default()
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
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let (sender, from_me) = lock(&self.store)
            .star_target(chat_id, message_id)
            .ok_or("That message is gone.")?;
        // Groups name who sent a message that isn't ours.
        let participant = sender
            .filter(|_| chat_id.ends_with("@g.us") && !from_me)
            .and_then(|s| s.parse::<Jid>().ok());
        let actions = client.chat_actions();
        let sent = if starred {
            actions.star_message(&jid, participant.as_ref(), message_id, from_me).await
        } else {
            actions.unstar_message(&jid, participant.as_ref(), message_id, from_me).await
        };
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
                stale
            };
            mgr.save_dirty.store(true, Ordering::Release);
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
            let members = match client.groups().fetch_metadata(&group).await {
                Ok(metadata) => metadata.participants.len() as u32,
                Err(e) => return log::warn!("whatsapp: group size lookup failed: {e}"),
            };
            if mgr.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let changed = lock(&mgr.store).set_group_members(&group.to_non_ad_string(), members);
            for dto in changed {
                let _ = mgr.app.emit("wa://message", dto);
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
        let lookup = client
            .contacts()
            .lookup_profile_picture(jid, true, on_disk)
            .await
            .map_err(|e| format!("lookup stopped: {e}"))?;
        match lookup {
            ProfilePictureLookup::Found(picture) => {
                let response = client
                    .http_client
                    .execute(HttpRequest::get(picture.url.as_str()))
                    .await
                    .map_err(|e| format!("download stopped: {e}"))?;
                if response.status_code != 200 || response.body.is_empty() {
                    log::warn!("whatsapp: profile picture download got {}", response.status_code);
                    return Ok(on_disk.map(str::to_string));
                }
                let file = dir.join(store::avatar_file(owner, &picture.id));
                fs::write(&file, &response.body)
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
                self.sync_chat_state(generation, client.clone());
                self.refresh_ticks(generation, client.clone());
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
                {
                    let mut store = lock(&self.store);
                    for m in batch.iter() {
                        if let Some(target) = convert::revoke(&m.message) {
                            let chat = m.info.source.chat.to_non_ad_string();
                            dtos.extend(store.delete_message(&chat, &target, Deletion::ForEveryone));
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
                                let Some(dto) = convert::live(&m.message, &m.info)
                                    .and_then(|incoming| store.ingest(incoming, true))
                                else {
                                    continue;
                                };
                                notices.extend(store.message_notice(&dto.chat_id, &dto.id));
                                dtos.push(dto);
                            }
                        }
                    }
                }
                for dto in dtos {
                    let _ = self.app.emit("wa://message", dto);
                }
                for notice in notices {
                    let _ = self.app.emit("wa://notice", notice);
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
                    if !changed.is_empty() {
                        for dto in changed {
                            let _ = self.app.emit("wa://message", dto);
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
            Event::ArchiveUpdate(update) => {
                lock(&self.store).set_archived(
                    &update.jid.to_non_ad_string(),
                    update.action.archived.unwrap_or(false),
                );
                self.mark_dirty();
            }
            Event::StarUpdate(update) => {
                let changed = lock(&self.store).set_starred(
                    &update.chat_jid.to_non_ad_string(),
                    &update.message_id,
                    update.action.starred.unwrap_or(false),
                );
                if let Some(dto) = changed {
                    let _ = self.app.emit("wa://message", dto);
                    self.mark_dirty();
                }
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
                if let Some(progress) = lazy.progress() {
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
                unread: c.unread_count,
                pinned_at_s: c.pinned,
                archived: c.archived,
                mute_end: c.mute_end_time,
                conversation_ts_s: c.conversation_timestamp.or(c.last_msg_timestamp),
            });
            for item in &c.messages {
                if let Some(incoming) = convert::history(&c.id, item) {
                    let id = incoming.message.id.clone();
                    store.ingest(incoming, false);
                    for (author, emoji) in convert::history_reactions(&c.id, item) {
                        let author = author.or_else(|| store.own_id().map(str::to_string));
                        if let Some(author) = author {
                            store.set_reaction(&c.id, &id, &author, Some(&emoji));
                        }
                    }
                }
            }
        }
        let rest = stream.remainder().map_err(|e| e.to_string())?;
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
        Ok(())
    }
}
