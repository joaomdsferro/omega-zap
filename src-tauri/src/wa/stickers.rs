//! The stickers and GIFs offered in the composer, as the phone keeps them:
//! the ones we sent last, favorite stickers (synced with the phone), and
//! sticker packs, from WhatsApp's own store or made here. Kept encrypted in a
//! file of its own, by each sticker's hash (WhatsApp's `filehash`: the base64
//! SHA-256 of the plain file).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use whatsapp_rust::download::MediaType;
use whatsapp_rust::prelude::*;
use whatsapp_rust::waproto::buffa::MessageField;
use whatsapp_rust::{AppStateResyncMode, UploadOptions, WAPatchName, schemas, sticker_pack};

use super::{MEDIA_DIR, WaManager, cdn_file_name, convert};
use crate::account::{lock, secure_dir};
use crate::store::{CdnRef, MessageDto, StoredMessage, now_ms};

/// Stickers and GIFs shown under Recent.
pub const RECENT: usize = 32;
/// The most stickers a pack holds, as on the phone.
pub const PACK_MAX: usize = 120;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Sticker {
    pub file: CdnRef,
    /// When we last sent it, from here or from the phone; 0 never.
    #[serde(default)]
    pub sent_ms: i64,
    /// When it was made a favorite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favorite_ms: Option<i64>,
    /// `None` until the file has been looked at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animated: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emojis: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Pack {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub publisher: String,
    /// One of WhatsApp's own packs, from its sticker store; otherwise made here.
    #[serde(default)]
    pub store: bool,
    /// Its tray picture's address, for a store pack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tray: Option<String>,
    /// Sticker hashes, in the pack's order.
    pub stickers: Vec<String>,
    pub added_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Gif {
    pub file: CdnRef,
    pub sent_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<u32>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Library {
    #[serde(default)]
    stickers: HashMap<String, Sticker>,
    #[serde(default)]
    packs: Vec<Pack>,
    #[serde(default)]
    gifs: HashMap<String, Gif>,
    /// The phone's favorites have been asked for once.
    #[serde(default)]
    pub favorites_synced: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StickerDto {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub animated: Option<bool>,
    pub favorite: bool,
    pub emojis: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackDto {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub store: bool,
    pub tray: Option<String>,
    pub stickers: Vec<StickerDto>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GifDto {
    pub id: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDto {
    pub recent: Vec<StickerDto>,
    pub favorites: Vec<StickerDto>,
    pub packs: Vec<PackDto>,
    pub gifs: Vec<GifDto>,
}

/// A sticker file as the library keeps it: always a WebP sticker.
fn as_sticker(mut file: CdnRef) -> CdnRef {
    file.kind = Some("sticker".into());
    if file.mime.as_deref().is_none_or(str::is_empty) {
        file.mime = Some("image/webp".into());
    }
    file
}

impl Library {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join("stickers.json")
    }

    pub fn load(dir: &Path) -> Self {
        let path = Self::path(dir);
        if !path.exists() {
            return Self::default();
        }
        match crate::vault::get()
            .read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
        {
            Ok(library) => library,
            Err(e) => {
                log::warn!("whatsapp: the sticker library could not be read, starting again: {e}");
                Self::default()
            }
        }
    }

    pub fn save(&mut self, dir: &Path) {
        self.prune();
        match serde_json::to_vec(self) {
            Ok(bytes) => {
                if let Err(e) = crate::vault::get().write(&Self::path(dir), &bytes) {
                    log::warn!("whatsapp: the sticker library was not saved: {e}");
                }
            }
            Err(e) => log::warn!("whatsapp: the sticker library was not saved: {e}"),
        }
    }

    /// Drops what is no longer shown anywhere: neither recent, favorite nor in a pack.
    fn prune(&mut self) {
        let mut keep: HashSet<String> = self.packs.iter().flat_map(|p| p.stickers.iter().cloned()).collect();
        keep.extend(self.stickers.iter().filter(|(_, s)| s.favorite_ms.is_some()).map(|(h, _)| h.clone()));
        keep.extend(self.recent_hashes());
        self.stickers.retain(|hash, _| keep.contains(hash));
        let gifs: HashSet<String> = self.recent_gif_hashes().into_iter().collect();
        self.gifs.retain(|hash, _| gifs.contains(hash));
    }

    fn recent_hashes(&self) -> Vec<String> {
        let mut sent: Vec<(&String, i64)> = self.stickers.iter().filter(|(_, s)| s.sent_ms > 0).map(|(h, s)| (h, s.sent_ms)).collect();
        sent.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        sent.into_iter().take(RECENT).map(|(h, _)| h.clone()).collect()
    }

    fn recent_gif_hashes(&self) -> Vec<String> {
        let mut sent: Vec<(&String, i64)> = self.gifs.iter().map(|(h, g)| (h, g.sent_ms)).collect();
        sent.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        sent.into_iter().take(RECENT).map(|(h, _)| h.clone()).collect()
    }

    /// The entry for a sticker file, made when new; a newer address replaces an older one.
    fn entry(&mut self, file: CdnRef) -> &mut Sticker {
        let file = as_sticker(file);
        let hash = file.sha256.clone();
        let entry = self.stickers.entry(hash).or_insert_with(|| Sticker {
            file: file.clone(),
            sent_ms: 0,
            favorite_ms: None,
            animated: None,
            emojis: Vec::new(),
        });
        if entry.file.path != file.path && !file.path.is_empty() {
            entry.file = CdnRef {
                width: file.width.max(entry.file.width),
                height: file.height.max(entry.file.height),
                ..file
            };
        }
        entry
    }

    /// We sent this sticker at `at_ms` (here, or from the phone). Returns whether Recent changed.
    pub fn note_sent(&mut self, file: CdnRef, at_ms: i64) -> bool {
        let entry = self.entry(file);
        if at_ms <= entry.sent_ms {
            return false;
        }
        entry.sent_ms = at_ms;
        true
    }

    /// The phone took a sticker out of Recent, unless it was sent again after `sent_before_ms`.
    pub fn remove_recent(&mut self, hash: &str, sent_before_ms: Option<i64>) -> bool {
        match self.stickers.get_mut(hash) {
            Some(s) if s.sent_ms > 0 && sent_before_ms.is_none_or(|before| s.sent_ms <= before) => {
                s.sent_ms = 0;
                true
            }
            _ => false,
        }
    }

    /// Makes a sticker a favorite or not. A favorite not known here needs its file.
    pub fn set_favorite(&mut self, hash: &str, file: Option<CdnRef>, favorite: bool, at_ms: i64) -> bool {
        let entry = match (self.stickers.contains_key(hash), file) {
            (true, _) => self.stickers.get_mut(hash),
            (false, Some(file)) if favorite => Some(self.entry(file)),
            _ => None,
        };
        let Some(entry) = entry else { return false };
        let was = entry.favorite_ms.is_some();
        entry.favorite_ms = favorite.then(|| entry.favorite_ms.unwrap_or(at_ms));
        was != favorite
    }

    pub fn note_animated(&mut self, hash: &str, animated: bool) -> bool {
        match self.stickers.get_mut(hash) {
            Some(s) if s.animated != Some(animated) => {
                s.animated = Some(animated);
                true
            }
            _ => false,
        }
    }

    pub fn sticker(&self, hash: &str) -> Option<&Sticker> {
        self.stickers.get(hash)
    }

    pub fn gif(&self, hash: &str) -> Option<&Gif> {
        self.gifs.get(hash)
    }

    /// We sent this GIF at `at_ms`.
    pub fn note_gif(&mut self, mut file: CdnRef, seconds: Option<u32>, at_ms: i64) -> bool {
        file.kind = Some("gif".into());
        let hash = file.sha256.clone();
        match self.gifs.get_mut(&hash) {
            Some(gif) if gif.sent_ms >= at_ms => false,
            Some(gif) => {
                gif.sent_ms = at_ms;
                gif.file = file;
                true
            }
            None => {
                self.gifs.insert(
                    hash,
                    Gif {
                        file,
                        sent_ms: at_ms,
                        seconds,
                    },
                );
                true
            }
        }
    }

    pub fn has_pack(&self, id: &str) -> bool {
        self.packs.iter().any(|p| p.id == id)
    }

    /// Adds one of the store's packs with its stickers.
    pub fn install_pack(&mut self, mut pack: Pack, stickers: Vec<(CdnRef, Vec<String>, bool)>, at_ms: i64) {
        pack.stickers = Vec::new();
        for (file, emojis, animated) in stickers {
            let hash = file.sha256.clone();
            let entry = self.entry(file);
            entry.emojis = emojis;
            entry.animated = Some(animated);
            if !pack.stickers.contains(&hash) {
                pack.stickers.push(hash);
            }
        }
        pack.added_ms = at_ms;
        self.packs.retain(|p| p.id != pack.id);
        self.packs.push(pack);
    }

    pub fn remove_pack(&mut self, id: &str) -> bool {
        let before = self.packs.len();
        self.packs.retain(|p| p.id != id);
        before != self.packs.len()
    }

    /// A pack of our own, empty. Returns its id.
    pub fn create_pack(&mut self, name: &str, at_ms: i64) -> String {
        let id = format!("own-{at_ms}");
        self.packs.push(Pack {
            id: id.clone(),
            name: name.trim().to_string(),
            publisher: String::new(),
            store: false,
            tray: None,
            stickers: Vec::new(),
            added_ms: at_ms,
        });
        id
    }

    pub fn rename_pack(&mut self, id: &str, name: &str) -> Result<(), String> {
        let pack = self.own_pack(id)?;
        pack.name = name.trim().to_string();
        Ok(())
    }

    fn own_pack(&mut self, id: &str) -> Result<&mut Pack, String> {
        match self.packs.iter_mut().find(|p| p.id == id) {
            Some(pack) if !pack.store => Ok(pack),
            Some(_) => Err("WhatsApp's packs can't be changed.".into()),
            None => Err("That pack is gone.".into()),
        }
    }

    /// Puts a sticker in one of our packs, first; one already there moves first.
    pub fn add_to_pack(&mut self, id: &str, file: CdnRef) -> Result<(), String> {
        let hash = file.sha256.clone();
        let pack = self.own_pack(id)?;
        if pack.stickers.len() >= PACK_MAX && !pack.stickers.contains(&hash) {
            return Err(format!("A pack holds {PACK_MAX} stickers at most."));
        }
        pack.stickers.retain(|h| *h != hash);
        pack.stickers.insert(0, hash);
        self.entry(file);
        Ok(())
    }

    pub fn remove_from_pack(&mut self, id: &str, hash: &str) -> Result<(), String> {
        self.own_pack(id)?.stickers.retain(|h| h != hash);
        Ok(())
    }

    fn dto(&self, hash: &str) -> Option<StickerDto> {
        let s = self.stickers.get(hash)?;
        Some(StickerDto {
            id: hash.to_string(),
            width: s.file.width,
            height: s.file.height,
            animated: s.animated,
            favorite: s.favorite_ms.is_some(),
            emojis: s.emojis.clone(),
        })
    }

    pub fn to_dto(&self) -> LibraryDto {
        let mut favorites: Vec<(&String, i64)> = self.stickers.iter().filter_map(|(h, s)| s.favorite_ms.map(|at| (h, at))).collect();
        // The latest favorite first, as on the phone.
        favorites.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let mut packs: Vec<&Pack> = self.packs.iter().collect();
        packs.sort_by_key(|p| p.added_ms);
        LibraryDto {
            recent: self.recent_hashes().iter().filter_map(|h| self.dto(h)).collect(),
            favorites: favorites.into_iter().filter_map(|(h, _)| self.dto(h)).collect(),
            packs: packs
                .into_iter()
                .map(|p| PackDto {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    publisher: p.publisher.clone(),
                    store: p.store,
                    tray: p.tray.clone(),
                    stickers: p.stickers.iter().filter_map(|h| self.dto(h)).collect(),
                })
                .collect(),
            gifs: self
                .recent_gif_hashes()
                .iter()
                .filter_map(|h| {
                    let g = self.gifs.get(h)?;
                    Some(GifDto {
                        id: h.clone(),
                        width: g.file.width,
                        height: g.file.height,
                    })
                })
                .collect(),
        }
    }
}

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
        let mut library = lock(&self.stickers);
        let (result, changed) = f(&mut library);
        if changed {
            library.save(&self.dir);
            drop(library);
            let _ = self.app.emit("wa://stickers", ());
        }
        result
    }

    pub fn sticker_library(&self) -> LibraryDto {
        lock(&self.stickers).to_dto()
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
        if lock(&self.stickers).favorites_synced {
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
                    lib.favorites_synced = true;
                    ((), true)
                });
            }
        });
    }

    fn library_file(&self, id: &str) -> Option<(CdnRef, MediaType)> {
        let library = lock(&self.stickers);
        library
            .sticker(id)
            .map(|s| (s.file.clone(), MediaType::Sticker))
            .or_else(|| library.gif(id).map(|g| (g.file.clone(), MediaType::Video)))
    }

    /// A library sticker's or GIF's file, downloaded if needed: its name in the media folder.
    pub async fn library_media(&self, id: &str) -> Result<String, String> {
        let (file, media_type) = self.library_file(id).ok_or("That sticker is gone.")?;
        let name = self.fetch_cdn(file, MEDIA_DIR, "wa", media_type).await?;
        if media_type == MediaType::Sticker
            && lock(&self.stickers).sticker(id).is_some_and(|s| s.animated.is_none())
            && let Ok(bytes) = crate::vault::get().read(&self.dir.join(MEDIA_DIR).join(&name))
        {
            let animated = whatsapp_rust::webp::is_animated(&bytes);
            self.with_stickers(|lib| ((), lib.note_animated(id, animated)));
        }
        Ok(name)
    }

    async fn library_bytes(&self, id: &str) -> Result<Vec<u8>, String> {
        let name = self.library_media(id).await?;
        crate::vault::get()
            .read(&self.dir.join(MEDIA_DIR).join(name))
            .map_err(|e| format!("The file could not be read: {e}"))
    }

    /// Sends a sticker from the library.
    pub async fn send_library_sticker(&self, chat_id: &str, id: &str, reply_to: Option<&str>) -> Result<MessageDto, String> {
        let bytes = self.library_bytes(id).await?;
        let emojis = lock(&self.stickers).sticker(id).map(|s| s.emojis.clone()).unwrap_or_default();
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
        if let Ok(name) = cdn_file_name(&file, "wa") {
            let path = self.dir.join(MEDIA_DIR).join(name);
            if !path.exists() {
                let _ = secure_dir(&self.dir.join(MEDIA_DIR));
                let _ = crate::vault::get().write(&path, &plain);
            }
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
            (Some(id), _) => lock(&self.stickers).sticker(id).map(|s| s.file.clone()),
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
        let library = lock(&self.stickers);
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
            (Some(id), _) => lock(&self.stickers).sticker(id).map(|s| s.file.clone()),
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
            let library = lock(&self.stickers);
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
            if let Ok(name) = cdn_file_name(&file, "wa") {
                let path = self.dir.join(MEDIA_DIR).join(name);
                if !path.exists() {
                    let _ = secure_dir(&self.dir.join(MEDIA_DIR));
                    let _ = crate::vault::get().write(&path, &plain);
                }
            }
            self.with_stickers(|lib| ((), lib.note_gif(file, seconds, now_ms())));
        }
        self.store_sent(chat_id, sent.message_id.into_string(), content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(hash: &str) -> CdnRef {
        CdnRef {
            path: format!("/v/{hash}"),
            key: "a2V5".into(),
            sha256: hash.into(),
            enc_sha256: "ZW5j".into(),
            size: 100,
            mime: Some("image/webp".into()),
            width: 512,
            height: 512,
            kind: Some("sticker".into()),
            file_name: None,
        }
    }

    #[test]
    fn recent_is_newest_sent_first_and_capped() {
        let mut lib = Library::default();
        for i in 0..40 {
            lib.note_sent(file(&format!("h{i:02}")), 1000 + i);
        }
        // An older replay doesn't move it back.
        assert!(!lib.note_sent(file("h39"), 5));
        let dto = lib.to_dto();
        assert_eq!(dto.recent.len(), RECENT);
        assert_eq!(dto.recent[0].id, "h39");
        lib.prune();
        assert_eq!(lib.stickers.len(), RECENT);
    }

    #[test]
    fn removing_from_recent_respects_a_later_send() {
        let mut lib = Library::default();
        lib.note_sent(file("a"), 2000);
        assert!(!lib.remove_recent("a", Some(1000)));
        assert!(lib.remove_recent("a", Some(2000)));
        assert!(lib.to_dto().recent.is_empty());
    }

    #[test]
    fn favorites_need_the_file_only_when_new() {
        let mut lib = Library::default();
        assert!(!lib.set_favorite("x", None, true, 1));
        assert!(lib.set_favorite("x", Some(file("x")), true, 1));
        assert!(lib.set_favorite("x", None, false, 2));
        assert!(lib.to_dto().favorites.is_empty());
        // Still known as long as something shows it.
        lib.note_sent(file("x"), 3);
        assert!(lib.set_favorite("x", None, true, 4));
        assert_eq!(lib.to_dto().favorites[0].id, "x");
    }

    #[test]
    fn own_packs_take_stickers_store_packs_do_not() {
        let mut lib = Library::default();
        let id = lib.create_pack(" Mine ", 10);
        lib.add_to_pack(&id, file("a")).unwrap();
        lib.add_to_pack(&id, file("b")).unwrap();
        lib.add_to_pack(&id, file("a")).unwrap();
        let store = Pack {
            id: "Cuppy".into(),
            name: "Cuppy".into(),
            publisher: "WhatsApp".into(),
            store: true,
            tray: None,
            stickers: vec![],
            added_ms: 0,
        };
        lib.install_pack(store, vec![(file("c"), vec!["😀".into()], false)], 20);
        assert!(lib.add_to_pack("Cuppy", file("d")).is_err());
        let dto = lib.to_dto();
        assert_eq!(dto.packs[0].name, "Mine");
        assert_eq!(dto.packs[0].stickers.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(dto.packs[1].stickers[0].emojis, ["😀"]);
        lib.remove_pack("Cuppy");
        lib.prune();
        assert!(lib.sticker("c").is_none());
        assert!(lib.sticker("a").is_some());
    }

    #[test]
    fn gifs_keep_the_latest_send() {
        let mut lib = Library::default();
        let mut gif = file("g");
        gif.kind = Some("gif".into());
        assert!(lib.note_gif(gif.clone(), Some(3), 5));
        assert!(!lib.note_gif(gif.clone(), None, 4));
        assert_eq!(lib.to_dto().gifs.len(), 1);
    }
}
