//! The stickers and GIFs offered in the composer, as the phone keeps them:
//! the ones we sent last, favorite stickers (synced with the phone), and
//! sticker packs, from WhatsApp's own store or made here. Kept encrypted in a
//! file of its own, by each sticker's hash (WhatsApp's `filehash`: the base64
//! SHA-256 of the plain file).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use tauri::Emitter as _;
use whatsapp_rust::download::MediaType;
use whatsapp_rust::prelude::*;
use whatsapp_rust::waproto::buffa::MessageField;
use whatsapp_rust::{AppStateResyncMode, UploadOptions, WAPatchName, schemas, sticker_pack};

use super::{CdnError, MEDIA_DIR, REUPLOAD_RETRY_AFTER, WaManager, convert};
use crate::account::lock;
use crate::store::{CdnRef, MessageDto, StoredMessage, now_ms};
use crate::stickers::{Collection, Library, LibraryDto, Pack, unzip};

/// Stickers of a shared pack uploaded at once.
const UPLOADS_AT_ONCE: usize = 4;

/// One of the packs in WhatsApp's sticker store, as the store lists it.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StorePackDto {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub description: String,
    pub animated: bool,
    pub tray: Option<String>,
    pub previews: Vec<String>,
    /// Bytes to download.
    pub size: u64,
    pub installed: bool,
}

/// WhatsApp's sticker store: its list of packs and each pack's stickers.
const STORE: &str = "https://static.whatsapp.net/sticker";
/// The store's list is fetched again after this long.
const STORE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

fn store_image(id: &str) -> String {
    format!("{STORE}?img={id}")
}

/// The language the system is set to, for the store's pack names.
fn locale() -> String {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .and_then(|lang| lang.split(['_', '.', '@']).next().map(str::to_lowercase))
        .filter(|l| l.len() == 2)
        .unwrap_or_else(|| "en".into())
}

/// A store pack's sticker as the library keeps it.
fn store_sticker(item: &sticker_pack::StickerPackItem) -> Option<CdnRef> {
    Some(CdnRef {
        path: item.direct_path.clone().filter(|p| !p.is_empty())?,
        key: BASE64.encode(item.media_key.as_ref()?),
        sha256: BASE64.encode(item.file_hash.as_ref()?),
        enc_sha256: BASE64.encode(item.enc_file_hash.as_ref()?),
        size: item.file_size.unwrap_or_default(),
        mime: Some(item.mimetype.clone().unwrap_or_else(|| "image/webp".into())),
        width: item.width.unwrap_or(512),
        height: item.height.unwrap_or(512),
        kind: Some("sticker".into()),
        file_name: None,
    })
}

impl WaManager {
    /// Changes the library; when `f` says something changed, saves it and
    /// tells the UI.
    fn with_stickers<R>(&self, f: impl FnOnce(&mut Library) -> (R, bool)) -> R {
        let mut library = lock(&self.stickers.library);
        let (result, changed) = f(&mut library);
        if changed {
            self.stickers.save(&mut library);
            drop(library);
            self.emit("stickers", ());
            let _ = self.app.emit("stickers://changed", ());
        }
        result
    }

    pub fn sticker_library(&self) -> LibraryDto {
        lock(&self.stickers.library).to_dto()
    }

    /// Keeps the stickers and GIFs in messages of ours as recent: sent from
    /// here, or from the phone and synced. One save for the lot.
    pub(super) fn note_own_media<'a>(&self, messages: impl IntoIterator<Item = &'a StoredMessage>) {
        let files: Vec<(CdnRef, i64)> = messages
            .into_iter()
            .filter(|m| m.from_me && m.deleted.is_none())
            .filter_map(|m| m.content.wa_media.clone().map(|f| (f, m.ts_ms)))
            .filter(|(f, _)| matches!(f.kind.as_deref(), Some("sticker" | "gif")))
            .collect();
        if files.is_empty() {
            return;
        }
        self.with_stickers(|lib| {
            let mut changed = false;
            for (file, at_ms) in files {
                changed |= if file.kind.as_deref() == Some("gif") {
                    lib.note_gif(file, None, at_ms)
                } else {
                    lib.note_sent(file, at_ms)
                };
            }
            ((), changed)
        });
    }

    /// The stickers the phone sent at linking as recently used.
    pub(super) fn note_phone_recents(&self, recents: &[wa::StickerMetadata]) {
        if recents.is_empty() {
            return;
        }
        self.with_stickers(|lib| {
            let mut changed = false;
            for s in recents {
                let file = (|| {
                    Some(CdnRef {
                        path: s.direct_path.clone().filter(|p| !p.is_empty())?,
                        key: BASE64.encode(s.media_key.as_ref()?),
                        sha256: BASE64.encode(s.file_sha256.as_ref()?),
                        enc_sha256: BASE64.encode(s.file_enc_sha256.as_ref()?),
                        size: s.file_length.unwrap_or_default(),
                        mime: s.mimetype.clone(),
                        width: s.width.unwrap_or_default(),
                        height: s.height.unwrap_or_default(),
                        kind: Some("sticker".into()),
                        file_name: None,
                    })
                })();
                // Lottie stickers can't be drawn here.
                if let (Some(file), false) = (file, s.is_lottie == Some(true)) {
                    changed |= lib.note_sent(file, s.last_sticker_sent_ts.unwrap_or(1));
                }
            }
            ((), changed)
        });
    }

    /// A favorite added or taken away on the phone.
    pub(super) fn phone_favorite(&self, filehash: &str, action: &wa::sync_action_value::StickerAction, at_ms: i64) {
        let favorite = action.is_favorite == Some(true);
        let file = (|| {
            Some(CdnRef {
                path: action.direct_path.clone().filter(|p| !p.is_empty())?,
                key: BASE64.encode(action.media_key.as_ref()?),
                sha256: filehash.to_string(),
                enc_sha256: BASE64.encode(action.file_enc_sha256.as_ref()?),
                size: action.file_length.unwrap_or_default(),
                mime: action.mimetype.clone(),
                width: action.width.unwrap_or_default(),
                height: action.height.unwrap_or_default(),
                kind: Some("sticker".into()),
                file_name: None,
            })
        })()
        .filter(|_| action.is_lottie != Some(true));
        self.with_stickers(|lib| ((), lib.set_favorite(filehash, file, favorite, at_ms)));
    }

    pub(super) fn phone_removed_recent(&self, filehash: &str, sent_before_ms: Option<i64>) {
        self.with_stickers(|lib| ((), lib.remove_recent(filehash, sent_before_ms)));
    }

    /// Asks the phone once for the collection favorites live in, so the ones
    /// made before this app listened arrive.
    pub(super) fn sync_sticker_favorites(self: &Arc<Self>, generation: u64, client: Arc<Client>) {
        let business = self.kind == super::Kind::Business;
        let synced = |lib: &Library| if business { lib.business_favorites_synced } else { lib.favorites_synced };
        if synced(&lock(&self.stickers.library)) {
            return;
        }
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            // After the chat state's own resync, if one runs.
            tokio::time::sleep(Duration::from_secs(20)).await;
            if mgr.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let resync = client.resync_app_state(vec![WAPatchName::RegularLow], AppStateResyncMode::Snapshot);
            let done = match tokio::time::timeout(Duration::from_secs(120), resync).await {
                Ok(Ok(_)) => true,
                // The phone is asked to rebuild that collection in the
                // background; asking again on every start would download the
                // whole snapshot each time and fail the same way. Favorites
                // made from now on still arrive as they change.
                Ok(Err(e)) if e.to_string().contains("MAC mismatch") => {
                    log::info!("whatsapp: favorite stickers snapshot does not validate; only new favorites will arrive");
                    true
                }
                Ok(Err(e)) => {
                    log::warn!("whatsapp: favorite stickers not synced: {e}");
                    false
                }
                Err(_) => {
                    log::warn!("whatsapp: favorite stickers sync timed out");
                    false
                }
            };
            if done {
                mgr.with_stickers(|lib| {
                    if business {
                        lib.business_favorites_synced = true;
                    } else {
                        lib.favorites_synced = true;
                    }
                    ((), true)
                });
            }
        });
    }

    fn library_file(&self, id: &str) -> Option<(CdnRef, MediaType)> {
        let library = lock(&self.stickers.library);
        library
            .sticker(id)
            .map(|s| (s.file.clone(), MediaType::Sticker))
            .or_else(|| library.gif(id).map(|g| (g.file.clone(), MediaType::Video)))
    }

    /// A library sticker's or GIF's file, downloaded if needed: its name in the media folder.
    /// Kept in the library's own folder once fetched, so it no longer needs
    /// WhatsApp, nor its address on the CDN to last.
    pub async fn library_media(&self, id: &str) -> Result<String, String> {
        let (file, media_type) = self.library_file(id).ok_or("That sticker is gone.")?;
        if let Some(kept) = self.stickers.kept(&file) {
            return Ok(kept);
        }
        let name = match self.fetch_cdn(file.clone(), MEDIA_DIR, "wa", media_type).await {
            // Gone from WhatsApp's CDN: favorites and recents keep the address
            // they had when first seen, which expires after a few weeks.
            Err(CdnError::Rejected(e)) if media_type == MediaType::Sticker => self.recover_sticker(id, &file).await.map_err(|why| {
                log::info!("whatsapp: expired sticker not recovered ({e}): {why}");
                why
            })?,
            other => other?,
        };
        let bytes = crate::vault::get().read(&self.dir.join(MEDIA_DIR).join(&name)).map_err(|e| format!("The file could not be read: {e}"))?;
        if media_type == MediaType::Sticker && lock(&self.stickers.library).sticker(id).is_some_and(|s| s.animated.is_none()) {
            let animated = whatsapp_rust::webp::is_animated(&bytes);
            self.with_stickers(|lib| ((), lib.note_animated(id, animated)));
        }
        match self.stickers.keep(&file, &bytes) {
            Ok(kept) => Ok(kept),
            Err(e) => {
                log::warn!("{e}");
                Ok(name)
            }
        }
    }

    /// Fetches every library file not kept yet, one at a time, so packs
    /// outlive WhatsApp's CDN. Once per start, by whichever account
    /// connects first.
    pub(super) fn fill_library(self: &Arc<Self>, generation: u64) {
        if self.stickers.filling.swap(true, Ordering::SeqCst) {
            return;
        }
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            // After the start's own traffic.
            tokio::time::sleep(Duration::from_secs(30)).await;
            let wanted: Vec<String> = {
                let library = lock(&mgr.stickers.library);
                library.files().into_iter().filter(|f| mgr.stickers.kept(f).is_none()).map(|f| f.sha256).collect()
            };
            let mut missing = 0;
            for id in wanted {
                if mgr.generation.load(Ordering::SeqCst) != generation {
                    // This account went away: another may take it up.
                    mgr.stickers.filling.store(false, Ordering::SeqCst);
                    return;
                }
                if mgr.library_media(&id).await.is_err() {
                    missing += 1;
                }
            }
            if missing > 0 {
                log::info!("stickers: {missing} library files could not be fetched");
            }
        });
    }

    pub fn sticker_collection(&self, source: &str) -> Option<Collection> {
        lock(&self.stickers.library).collection(source)
    }

    /// The emojis a library sticker goes with.
    pub fn sticker_emojis(&self, id: &str) -> Vec<String> {
        lock(&self.stickers.library).sticker(id).map(|s| s.emojis.clone()).unwrap_or_default()
    }

    /// A sticker whose address expired, fetched from a fresh one: a store
    /// pack's own list has them, and otherwise the phone that sent a message
    /// with it can put it back on the CDN.
    async fn recover_sticker(&self, id: &str, file: &CdnRef) -> Result<String, String> {
        let pack = lock(&self.stickers.library).store_pack_of(id);
        if let Some(pack) = pack {
            self.refresh_store_pack(&pack).await;
            let fresh = lock(&self.stickers.library).sticker(id).map(|s| s.file.clone());
            if let Some(fresh) = fresh.filter(|f| f.path != file.path)
                && let Ok(name) = self.fetch_cdn(fresh, MEDIA_DIR, "wa", MediaType::Sticker).await
            {
                return Ok(name);
            }
        }
        for (chat, message) in self.history.with_file(id, 3) {
            if let Ok(name) = self.fetch_media(&chat, &message).await {
                let path = lock(&self.store).wa_media(&chat, &message).map(|f| f.path);
                if let Some(path) = path {
                    self.with_stickers(|lib| {
                        let changed = lib.refresh_files([CdnRef { path, ..file.clone() }]);
                        ((), changed)
                    });
                }
                return Ok(name);
            }
        }
        Err("This sticker expired on WhatsApp, and no phone sent it again.".into())
    }

    /// Fetches a store pack's list again, for its stickers' current
    /// addresses; at most once an hour.
    async fn refresh_store_pack(&self, pack_id: &str) {
        let mut refreshed = self.pack_refreshes.lock().await;
        if refreshed.get(pack_id).is_some_and(|at| at.elapsed() < REUPLOAD_RETRY_AFTER) {
            return;
        }
        refreshed.insert(pack_id.to_string(), Instant::now());
        let Ok(client) = self.connected_client().await else { return };
        match client.fetch_sticker_pack(pack_id, &locale()).await {
            Ok(pack) => {
                let files: Vec<CdnRef> = pack.stickers.iter().filter_map(store_sticker).collect();
                self.with_stickers(|lib| ((), lib.refresh_files(files)));
            }
            Err(e) => log::info!("whatsapp: sticker pack {pack_id} could not be fetched again: {e}"),
        }
    }

    /// A library sticker's or GIF's plain bytes, downloaded if needed.
    pub(crate) async fn library_bytes(&self, id: &str) -> Result<Vec<u8>, String> {
        let name = self.library_media(id).await?;
        if name.starts_with("st-") {
            return self.stickers.read(&name);
        }
        crate::vault::get()
            .read(&self.dir.join(MEDIA_DIR).join(name))
            .map_err(|e| format!("The file could not be read: {e}"))
    }

    /// Sends a sticker from the library.
    pub async fn send_library_sticker(&self, chat_id: &str, id: &str, reply_to: Option<&str>) -> Result<MessageDto, String> {
        let bytes = self.library_bytes(id).await?;
        let emojis = lock(&self.stickers.library).sticker(id).map(|s| s.emojis.clone()).unwrap_or_default();
        self.send_sticker_bytes(chat_id, bytes, emojis, reply_to).await
    }

    /// Makes a sticker from a picture and sends it; it then shows under Recent.
    pub async fn send_new_sticker(&self, chat_id: &str, picture: Vec<u8>, reply_to: Option<&str>) -> Result<MessageDto, String> {
        let made = tauri::async_runtime::spawn_blocking(move || crate::sticker_image::prepare(picture))
            .await
            .map_err(|e| format!("The sticker could not be made: {e}"))??;
        self.send_sticker_bytes(chat_id, made.bytes, Vec::new(), reply_to).await
    }

    /// Makes a sticker from a picture and puts it in one of our packs, without sending it.
    pub async fn add_new_sticker(&self, pack_id: &str, picture: Vec<u8>) -> Result<(), String> {
        let made = tauri::async_runtime::spawn_blocking(move || crate::sticker_image::prepare(picture))
            .await
            .map_err(|e| format!("The sticker could not be made: {e}"))??;
        let animated = made.animated;
        let file = self.upload_sticker(made.bytes).await?.1;
        let hash = file.sha256.clone();
        self.with_stickers(|lib| {
            let added = lib.add_to_pack(pack_id, file);
            lib.note_animated(&hash, animated);
            let ok = added.is_ok();
            (added, ok)
        })
    }

    /// Makes a sticker as the editor set it up and puts it first in a pack
    /// of ours, or in a new one named `new_pack`. Returns the pack's id.
    pub async fn make_sticker(&self, made: crate::sticker_image::Sticker, pack_id: Option<&str>, new_pack: Option<&str>, emojis: Vec<String>) -> Result<String, String> {
        let animated = made.animated;
        let file = self.upload_sticker(made.bytes).await?.1;
        let hash = file.sha256.clone();
        let pack_id = match (pack_id, new_pack) {
            (Some(id), _) => id.to_string(),
            (None, Some(name)) => self.create_pack(name)?,
            (None, None) => self.create_pack("My stickers")?,
        };
        self.with_stickers(|lib| {
            let added = lib.add_to_pack(&pack_id, file);
            lib.note_animated(&hash, animated);
            lib.set_emojis(&hash, emojis);
            let ok = added.is_ok();
            (added.map(|_| pack_id.clone()), ok)
        })
    }

    /// Uploads a GIF made in the editor and keeps it with the GIFs, first
    /// under Recent, to send from there.
    pub async fn make_gif(&self, video: crate::gif_video::Video) -> Result<(), String> {
        let client = self.connected_client().await?;
        let seconds = crate::klipy::mp4_seconds(&video.mp4);
        let plain = video.mp4.clone();
        let upload = client
            .upload(video.mp4, MediaType::Video, UploadOptions::default())
            .await
            .map_err(|e| format!("The GIF was not uploaded: {e}"))?;
        let file = CdnRef {
            path: upload.direct_path.clone(),
            key: BASE64.encode(upload.media_key),
            sha256: BASE64.encode(upload.file_sha256),
            enc_sha256: BASE64.encode(upload.file_enc_sha256),
            size: upload.file_length,
            mime: Some("video/mp4".into()),
            width: video.width,
            height: video.height,
            kind: Some("gif".into()),
            file_name: None,
        };
        self.stickers.keep(&file, &plain)?;
        self.with_stickers(|lib| ((), lib.note_gif(file, seconds, now_ms())));
        Ok(())
    }

    /// Uploads a sticker's bytes and keeps a copy in the media folder, so it
    /// shows without downloading it back. Returns the upload and the file.
    async fn upload_sticker(&self, bytes: Vec<u8>) -> Result<(whatsapp_rust::upload::UploadResponse, CdnRef), String> {
        let client = self.connected_client().await?;
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
            return Err("A sticker has to be a WebP picture.".into());
        }
        let (width, height) = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
            .map(|p| (p.width(), p.height()))
            .unwrap_or((crate::sticker_image::SIDE, crate::sticker_image::SIDE));
        let plain = bytes.clone();
        let upload = client
            .upload(bytes, MediaType::Sticker, UploadOptions::default())
            .await
            .map_err(|e| format!("The sticker was not uploaded: {e}"))?;
        let file = CdnRef {
            path: upload.direct_path.clone(),
            key: BASE64.encode(upload.media_key),
            sha256: BASE64.encode(upload.file_sha256),
            enc_sha256: BASE64.encode(upload.file_enc_sha256),
            size: upload.file_length,
            mime: Some("image/webp".into()),
            width,
            height,
            kind: Some("sticker".into()),
            file_name: None,
        };
        if let Err(e) = self.stickers.keep(&file, &plain) {
            log::warn!("{e}");
        }
        Ok((upload, file))
    }

    /// Uploads a WebP and sends it as a sticker. Uploaded each time, so a
    /// sticker whose old address expired still reaches everyone.
    async fn send_sticker_bytes(&self, chat_id: &str, bytes: Vec<u8>, emojis: Vec<String>, reply_to: Option<&str>) -> Result<MessageDto, String> {
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let context = self.quote(chat_id, reply_to)?.map(|(context, _)| context);
        let animated = whatsapp_rust::webp::is_animated(&bytes);
        let (upload, file) = self.upload_sticker(bytes).await?;
        let outgoing = {
            let mut message = wa::Message::default();
            message.sticker_message = MessageField::some({
                let mut sticker = wa::message::StickerMessage::default();
                sticker.url = Some(upload.url.clone());
                sticker.direct_path = Some(upload.direct_path.clone());
                sticker.media_key = Some(upload.media_key.to_vec());
                sticker.file_sha256 = Some(upload.file_sha256.to_vec());
                sticker.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
                sticker.file_length = Some(upload.file_length);
                sticker.media_key_timestamp = Some(upload.media_key_timestamp);
                sticker.mimetype = Some("image/webp".into());
                sticker.width = Some(file.width);
                sticker.height = Some(file.height);
                sticker.is_animated = Some(animated);
                sticker.sticker_sent_ts = Some(now_ms());
                sticker.emojis = (!emojis.is_empty()).then(|| emojis.join(""));
                sticker.context_info = context.map(MessageField::some).unwrap_or_default();
                sticker
            });
            message
        };
        let content = convert::content(&outgoing).ok_or("The sticker could not be shown.")?;
        let sent = client
            .send_message(jid, outgoing)
            .await
            .map_err(|e| format!("The sticker was uploaded but not sent: {e}"))?;
        let hash = file.sha256.clone();
        self.with_stickers(|lib| {
            let changed = lib.note_sent(file, now_ms());
            lib.note_animated(&hash, animated);
            ((), changed)
        });
        self.store_sent(chat_id, sent.message_id.into_string(), content)
    }

    /// Makes a sticker a favorite, or not, here and on the phone. `id` is a
    /// library sticker, or the sticker in message `message` of `chat`.
    pub async fn set_sticker_favorite(&self, id: Option<&str>, message: Option<(&str, &str)>, favorite: bool) -> Result<(), String> {
        let client = self.connected_client().await?;
        let file = match (id, message) {
            (Some(id), _) => lock(&self.stickers.library).sticker(id).map(|s| s.file.clone()),
            (None, Some((chat, message))) => {
                self.recall(chat, message);
                lock(&self.store).wa_media(chat, message).filter(|f| f.kind.as_deref() == Some("sticker"))
            }
            _ => None,
        }
        .ok_or("That sticker is gone.")?;
        let decode = |field: &str| BASE64.decode(field).ok();
        let value = {
            let mut value = wa::SyncActionValue::default();
            value.sticker_action = MessageField::some({
                let mut sticker = wa::sync_action_value::StickerAction::default();
                sticker.direct_path = Some(file.path.clone());
                sticker.media_key = decode(&file.key);
                sticker.file_enc_sha256 = decode(&file.enc_sha256);
                sticker.file_length = Some(file.size);
                sticker.mimetype = Some(file.mime.clone().unwrap_or_else(|| "image/webp".into()));
                sticker.width = Some(file.width);
                sticker.height = Some(file.height);
                sticker.is_favorite = Some(favorite);
                sticker.is_lottie = Some(false);
                sticker
            });
            value.timestamp = Some(now_ms());
            value
        };
        // Unfavoriting is a set with `isFavorite` false, as on WhatsApp Web.
        client
            .send_app_state_action(&schemas::FAVORITE_STICKER, &[file.sha256.as_str()], &value)
            .await
            .map_err(|e| format!("The favorite was not saved: {e}"))?;
        let hash = file.sha256.clone();
        self.with_stickers(|lib| ((), lib.set_favorite(&hash, Some(file), favorite, now_ms())));
        Ok(())
    }

    /// The library id of the sticker in a message, if it has one.
    pub fn message_sticker(&self, chat_id: &str, message_id: &str) -> Option<String> {
        self.recall(chat_id, message_id);
        lock(&self.store)
            .wa_media(chat_id, message_id)
            .filter(|f| f.kind.as_deref() == Some("sticker"))
            .map(|f| f.sha256)
    }

    /// The packs in WhatsApp's sticker store, fetched at most every few hours.
    pub async fn sticker_store(&self) -> Result<Vec<StorePackDto>, String> {
        let packs = {
            let mut cached = self.sticker_store.lock().await;
            match cached.as_ref() {
                Some((at, packs)) if at.elapsed() < STORE_TTL => packs.clone(),
                _ => {
                    let url = format!("{STORE}?cat=all&lg={}&ver=2", locale());
                    let body = tauri::async_runtime::spawn_blocking(move || -> Result<Vec<u8>, String> {
                        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().into();
                        agent
                            .get(&url)
                            .call()
                            .map_err(|e| e.to_string())?
                            .body_mut()
                            .with_config()
                            .limit(8 * 1024 * 1024)
                            .read_to_vec()
                            .map_err(|e| e.to_string())
                    })
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| format!("WhatsApp's sticker store did not answer: {e}"))?;
                    let packs: Vec<sticker_pack::StickerPack> =
                        serde_json::from_slice(&body).map_err(|e| format!("WhatsApp's sticker store could not be read: {e}"))?;
                    // Lottie packs need a player this app doesn't have.
                    let packs: Vec<sticker_pack::StickerPack> = packs.into_iter().filter(|p| p.lottie == 0).collect();
                    *cached = Some((Instant::now(), packs.clone()));
                    packs
                }
            }
        };
        let library = lock(&self.stickers.library);
        Ok(packs
            .into_iter()
            .filter_map(|p| {
                let id = p.sticker_pack_id?;
                Some(StorePackDto {
                    installed: library.has_pack(&id),
                    name: p.name.unwrap_or_else(|| id.clone()),
                    id,
                    publisher: p.publisher.unwrap_or_default(),
                    description: p.description.unwrap_or_default(),
                    animated: p.animated != 0,
                    tray: p.tray_image_id.as_deref().map(store_image),
                    previews: p.preview_image_ids.iter().map(|i| store_image(i)).collect(),
                    size: p.file_size.and_then(|s| s.parse().ok()).unwrap_or_default(),
                })
            })
            .collect())
    }

    /// Adds one of the store's packs.
    pub async fn install_store_pack(&self, id: &str) -> Result<(), String> {
        let client = self.connected_client().await?;
        let pack = client
            .fetch_sticker_pack(id, &locale())
            .await
            .map_err(|e| format!("The pack could not be fetched: {e}"))?;
        let animated = pack.animated != 0;
        let stickers: Vec<(CdnRef, Vec<String>, bool)> = pack
            .stickers
            .iter()
            .filter_map(|item| Some((store_sticker(item)?, item.emojis.clone(), animated)))
            .collect();
        if stickers.is_empty() {
            return Err("That pack has no stickers this app can show.".into());
        }
        let entry = Pack {
            id: pack.sticker_pack_id.clone().unwrap_or_else(|| id.to_string()),
            name: pack.name.clone().unwrap_or_else(|| id.to_string()),
            publisher: pack.publisher.clone().unwrap_or_default(),
            store: true,
            tray: pack.tray_image_id.as_deref().map(store_image),
            stickers: Vec::new(),
            added_ms: 0,
        };
        self.with_stickers(|lib| {
            lib.install_pack(entry, stickers, now_ms());
            ((), true)
        });
        Ok(())
    }

    pub fn remove_pack(&self, id: &str) {
        self.with_stickers(|lib| ((), lib.remove_pack(id)));
    }

    /// Adds the sticker pack shared in a message as a pack of ours: its zip
    /// is unpacked and each sticker uploaded, so it can be sent, starred and
    /// taken to Signal like any other. Returns the pack's id; adding it
    /// again replaces it.
    pub async fn add_shared_pack(&self, chat_id: &str, message_id: &str) -> Result<String, String> {
        self.recall(chat_id, message_id);
        let pack = lock(&self.store).sticker_pack(chat_id, message_id).ok_or("That message has no sticker pack.")?;
        let zip = pack.zip.clone().ok_or("The pack came without its stickers.")?;
        let name = match self.fetch_cdn(zip.clone(), MEDIA_DIR, "wa", MediaType::StickerPack).await {
            // Gone from the CDN: the phone that shared it can put it back.
            Err(CdnError::Rejected(_)) => {
                let path = self.reupload_media(chat_id, message_id, &zip).await?;
                self.fetch_cdn(CdnRef { path, ..zip.clone() }, MEDIA_DIR, "wa", MediaType::StickerPack).await?
            }
            other => other?,
        };
        let file = self.dir.join(MEDIA_DIR).join(&name);
        let bytes = crate::vault::get().read(&file).map_err(|e| format!("The pack could not be read: {e}"))?;
        // Only its stickers are kept, in the library.
        let _ = std::fs::remove_file(&file);
        let mut files = tauri::async_runtime::spawn_blocking(move || unzip(&bytes)).await.map_err(|e| e.to_string())??;
        let wanted: Vec<(Vec<u8>, Vec<String>, bool)> = pack
            .stickers
            .iter()
            .filter(|s| !s.lottie)
            .filter_map(|s| Some((files.remove(&s.file)?, s.emojis.clone(), s.animated)))
            .collect();
        if wanted.is_empty() {
            return Err("That pack has no stickers this app can show.".into());
        }
        use futures::StreamExt as _;
        let uploaded: Vec<_> = futures::stream::iter(wanted)
            .map(|(bytes, emojis, animated)| async move { self.upload_sticker(bytes).await.map(|(_, file)| (file, emojis, animated)) })
            .buffered(UPLOADS_AT_ONCE)
            .collect()
            .await;
        let failed = uploaded.iter().filter(|r| r.is_err()).count();
        let stickers: Vec<(CdnRef, Vec<String>, bool)> = uploaded.into_iter().filter_map(Result::ok).collect();
        if stickers.is_empty() {
            return Err("The pack's stickers could not be uploaded.".into());
        }
        let id = format!("shared-{}", if pack.id.is_empty() { &zip.sha256 } else { &pack.id });
        let entry = Pack {
            id: id.clone(),
            name: pack.name.clone(),
            publisher: pack.publisher.clone(),
            store: false,
            tray: None,
            stickers: Vec::new(),
            added_ms: 0,
        };
        self.with_stickers(|lib| {
            lib.install_pack(entry, stickers, now_ms());
            ((), true)
        });
        if failed > 0 {
            log::warn!("whatsapp: {failed} stickers of a shared pack were not uploaded");
        }
        Ok(id)
    }

    pub fn create_pack(&self, name: &str) -> Result<String, String> {
        if name.trim().is_empty() {
            return Err("A pack needs a name.".into());
        }
        Ok(self.with_stickers(|lib| (lib.create_pack(name, now_ms()), true)))
    }

    pub fn rename_pack(&self, id: &str, name: &str) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("A pack needs a name.".into());
        }
        self.with_stickers(|lib| {
            let result = lib.rename_pack(id, name);
            let ok = result.is_ok();
            (result, ok)
        })
    }

    /// Puts a library sticker, or the sticker in a message, in one of our packs.
    pub fn add_to_pack(&self, pack_id: &str, id: Option<&str>, message: Option<(&str, &str)>) -> Result<(), String> {
        let file = match (id, message) {
            (Some(id), _) => lock(&self.stickers.library).sticker(id).map(|s| s.file.clone()),
            (None, Some((chat, message))) => {
                self.recall(chat, message);
                lock(&self.store).wa_media(chat, message).filter(|f| f.kind.as_deref() == Some("sticker"))
            }
            _ => None,
        }
        .ok_or("That sticker is gone.")?;
        self.with_stickers(|lib| {
            let result = lib.add_to_pack(pack_id, file);
            let ok = result.is_ok();
            (result, ok)
        })
    }

    pub fn move_in_pack(&self, pack_id: &str, id: &str, to: usize) -> Result<(), String> {
        self.with_stickers(|lib| {
            let result = lib.move_in_pack(pack_id, id, to);
            let ok = result.is_ok();
            (result, ok)
        })
    }

    pub fn remove_from_pack(&self, pack_id: &str, id: &str) -> Result<(), String> {
        self.with_stickers(|lib| {
            let result = lib.remove_from_pack(pack_id, id);
            let ok = result.is_ok();
            (result, ok)
        })
    }

    /// Sends a GIF found on KLIPY.
    pub async fn send_klipy_gif(&self, chat_id: &str, pick: crate::klipy::GifPick, reply_to: Option<&str>) -> Result<MessageDto, String> {
        let (mp4, still) = {
            let pick = pick.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let mp4 = crate::klipy::download(&pick.mp4)?;
                let still = pick.still.as_deref().and_then(|url| crate::klipy::download(url).ok());
                Ok::<_, String>((mp4, still))
            })
            .await
            .map_err(|e| e.to_string())??
        };
        let thumbnail = still.as_deref().and_then(crate::klipy::thumbnail);
        let dto = self
            .send_gif_bytes(chat_id, mp4, pick.width, pick.height, thumbnail, true, reply_to)
            .await?;
        let dir = self.dir.clone();
        let slug = pick.slug.clone();
        tauri::async_runtime::spawn_blocking(move || crate::klipy::shared(&dir, crate::klipy::Kind::Gifs, &slug));
        Ok(dto)
    }

    async fn klipy_picture(pick: &crate::klipy::StickerPick) -> Result<Vec<u8>, String> {
        let url = pick.url.clone();
        tauri::async_runtime::spawn_blocking(move || crate::klipy::download(&url))
            .await
            .map_err(|e| e.to_string())?
    }

    /// Sends a sticker found on KLIPY, made into a WhatsApp sticker; it then shows under Recent.
    pub async fn send_klipy_sticker(&self, chat_id: &str, pick: crate::klipy::StickerPick, reply_to: Option<&str>) -> Result<MessageDto, String> {
        let picture = Self::klipy_picture(&pick).await?;
        let dto = self.send_new_sticker(chat_id, picture, reply_to).await?;
        let dir = self.dir.clone();
        tauri::async_runtime::spawn_blocking(move || crate::klipy::shared(&dir, crate::klipy::Kind::Stickers, &pick.slug));
        Ok(dto)
    }

    /// Puts a sticker found on KLIPY in one of our packs.
    pub async fn add_klipy_sticker(&self, pack_id: &str, pick: crate::klipy::StickerPick) -> Result<(), String> {
        let picture = Self::klipy_picture(&pick).await?;
        self.add_new_sticker(pack_id, picture).await
    }

    /// Sends one of the GIFs we sent before again.
    pub async fn send_recent_gif(&self, chat_id: &str, id: &str, reply_to: Option<&str>) -> Result<MessageDto, String> {
        let (width, height) = {
            let library = lock(&self.stickers.library);
            let gif = library.gif(id).ok_or("That GIF is gone.")?;
            (gif.file.width, gif.file.height)
        };
        let bytes = self.library_bytes(id).await?;
        self.send_gif_bytes(chat_id, bytes, width, height, None, false, reply_to).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_gif_bytes(
        &self,
        chat_id: &str,
        mp4: Vec<u8>,
        width: u32,
        height: u32,
        thumbnail: Option<Vec<u8>>,
        klipy: bool,
        reply_to: Option<&str>,
    ) -> Result<MessageDto, String> {
        use wa::message::video_message::Attribution;
        let client = self.connected_client().await?;
        let jid: Jid = chat_id.parse().map_err(|e| format!("Invalid chat {chat_id}: {e}"))?;
        let context = self.quote(chat_id, reply_to)?.map(|(context, _)| context);
        let seconds = crate::klipy::mp4_seconds(&mp4);
        let plain = mp4.clone();
        let upload = client
            .upload(mp4, MediaType::Video, UploadOptions::default())
            .await
            .map_err(|e| format!("The GIF was not uploaded: {e}"))?;
        let outgoing = {
            let mut message = wa::Message::default();
            message.video_message = MessageField::some({
                let mut video = wa::message::VideoMessage::default();
                video.url = Some(upload.url.clone());
                video.direct_path = Some(upload.direct_path.clone());
                video.media_key = Some(upload.media_key.to_vec());
                video.file_sha256 = Some(upload.file_sha256.to_vec());
                video.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
                video.file_length = Some(upload.file_length);
                video.media_key_timestamp = Some(upload.media_key_timestamp);
                video.streaming_sidecar = upload.streaming_sidecar.clone();
                video.mimetype = Some("video/mp4".into());
                video.seconds = seconds;
                video.width = Some(width).filter(|w| *w > 0);
                video.height = Some(height).filter(|h| *h > 0);
                video.gif_playback = Some(true);
                video.gif_attribution = Some(if klipy { Attribution::KLIPY } else { Attribution::NONE });
                video.jpeg_thumbnail = thumbnail;
                video.context_info = context.map(MessageField::some).unwrap_or_default();
                video
            });
            message
        };
        let content = convert::content(&outgoing).ok_or("The GIF could not be shown.")?;
        let sent = client
            .send_message(jid, outgoing)
            .await
            .map_err(|e| format!("The GIF was uploaded but not sent: {e}"))?;
        // Kept so it plays without downloading it back.
        if let Some(file) = content.wa_media.clone() {
            if let Err(e) = self.stickers.keep(&file, &plain) {
                log::warn!("{e}");
            }
            self.with_stickers(|lib| ((), lib.note_gif(file, seconds, now_ms())));
        }
        self.store_sent(chat_id, sent.message_id.into_string(), content)
    }
}
