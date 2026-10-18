//! WhatsApp's stickers on Signal. The sticker library is WhatsApp's (see
//! `wa::stickers`); here its favorites and packs become Signal packs, put on
//! Signal's CDN and installed on the phone, so they can be sent from Signal
//! here and there. A Signal pack can't change once made: one whose WhatsApp
//! side changed is made again, and replaces the old one on the phone.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager as _};

use super::{MEDIA_DIR, SignalManager, UPLOAD_TIMEOUT, media_file_name, writable};
use crate::account::{lock, secure_dir};
use crate::store::{Content, MediaRef, MessageDto, MsgKind, now_ms};
use crate::wa::{WaAccounts, WaManager};

const PACKS_FILE: &str = "sticker-packs.json";
/// The most stickers Signal puts in a pack.
pub const PACK_MAX: usize = 200;
/// What a sticker goes with when WhatsApp gave it no emoji: Signal asks for one.
const DEFAULT_EMOJI: &str = "🙂";
/// Signal's own name for packs made by people, when ours has no publisher.
const AUTHOR: &str = "Omega Zap";

/// A pack made on Signal from WhatsApp's favorites or one of its packs.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SignalPack {
    /// `favorites`, or the WhatsApp pack's id.
    pub source: String,
    pub title: String,
    /// Hex, as the server named it.
    pub pack_id: String,
    /// Hex. Whoever has it can read the pack.
    pub pack_key: String,
    /// Library sticker hashes; each one's place is its id in the pack.
    pub stickers: Vec<String>,
    pub uploaded_ms: i64,
}

impl SignalPack {
    /// The link Signal's apps open to add a pack.
    pub fn url(&self) -> String {
        format!("https://signal.art/addstickers/#pack_id={}&pack_key={}", self.pack_id, self.pack_key)
    }
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Packs {
    #[serde(default)]
    packs: Vec<SignalPack>,
}

impl Packs {
    fn path(dir: &Path) -> PathBuf {
        dir.join(PACKS_FILE)
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
            Ok(packs) => packs,
            Err(e) => {
                log::warn!("signal: the sticker packs could not be read: {e}");
                Self::default()
            }
        }
    }

    fn save(&self, dir: &Path) {
        match serde_json::to_vec(self) {
            Ok(bytes) => {
                if let Err(e) = crate::vault::get().write(&Self::path(dir), &bytes) {
                    log::warn!("signal: the sticker packs were not saved: {e}");
                }
            }
            Err(e) => log::warn!("signal: the sticker packs were not saved: {e}"),
        }
    }

    pub fn remove_file(dir: &Path) {
        let _ = fs::remove_file(Self::path(dir));
    }

    /// The pack a sticker is sent from, and its id there: the newest one
    /// holding it.
    pub fn find(&self, hash: &str) -> Option<(&SignalPack, u32)> {
        self.packs
            .iter()
            .filter_map(|p| Some((p, p.stickers.iter().position(|h| h == hash)? as u32)))
            .max_by_key(|(p, _)| p.uploaded_ms)
    }

    fn by_source(&self, source: &str) -> Option<&SignalPack> {
        self.packs.iter().find(|p| p.source == source)
    }

    /// Keeps a new pack, returning the one it replaces.
    fn replace(&mut self, pack: SignalPack) -> Option<SignalPack> {
        let old = self.packs.iter().position(|p| p.source == pack.source).map(|i| self.packs.remove(i));
        self.packs.push(pack);
        old
    }
}

/// A WhatsApp collection as it stands on Signal.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SignalPackDto {
    /// `favorites` or the WhatsApp pack's id.
    pub source: String,
    pub title: String,
    /// Stickers in the collection on WhatsApp (up to what a Signal pack takes).
    pub count: usize,
    /// How many of those the Signal pack has; none when there is no pack yet.
    pub on_signal: usize,
    /// The Signal pack's link, once made.
    pub url: Option<String>,
    pub uploading: bool,
}

/// What each collection looks like on Signal, against what it holds now.
pub fn status(packs: &Packs, collections: &[(String, crate::stickers::Collection)], uploading: &HashSet<String>) -> Vec<SignalPackDto> {
    collections
        .iter()
        .map(|(source, c)| {
            let pack = packs.by_source(source);
            let count = c.stickers.len().min(PACK_MAX);
            let on_signal = pack.map_or(0, |p| c.stickers.iter().take(PACK_MAX).filter(|(h, _)| p.stickers.contains(h)).count());
            SignalPackDto {
                source: source.clone(),
                title: c.name.clone(),
                count,
                on_signal,
                url: pack.map(SignalPack::url),
                uploading: uploading.contains(source),
            }
        })
        .collect()
}

impl SignalManager {
    /// The WhatsApp account whose stickers Signal uses.
    fn wa(&self) -> Result<Arc<WaManager>, String> {
        self.app
            .try_state::<WaAccounts>()
            .map(|accounts| accounts.stickers().clone())
            .ok_or_else(|| "The sticker library is not ready yet.".to_string())
    }

    fn stickers_changed(&self) {
        let _ = self.app.emit("signal://stickers", ());
    }

    /// Favorites and every pack of the library, as they stand on Signal.
    pub fn sticker_packs(&self) -> Result<Vec<SignalPackDto>, String> {
        let wa = self.wa()?;
        let library = wa.sticker_library();
        let sources = std::iter::once("favorites".to_string()).chain(library.packs.iter().map(|p| p.id.clone()));
        let collections: Vec<_> = sources
            .filter_map(|source| Some((source.clone(), wa.sticker_collection(&source)?)))
            .filter(|(_, c)| !c.stickers.is_empty())
            .collect();
        Ok(status(&lock(&self.sticker_packs), &collections, &lock(&self.stickers_uploading)))
    }

    /// Makes WhatsApp's favorites, or one of its packs, a Signal pack, and
    /// adds it on the phone. A pack made before for the same collection is
    /// taken off the phone: Signal packs can't change.
    pub async fn upload_stickers(&self, source: &str) -> Result<(), String> {
        if !lock(&self.stickers_uploading).insert(source.to_string()) {
            return Err("That pack is already being added to Signal.".into());
        }
        self.stickers_changed();
        let result = self.upload_collection(source).await;
        lock(&self.stickers_uploading).remove(source);
        self.stickers_changed();
        result
    }

    async fn upload_collection(&self, source: &str) -> Result<(), String> {
        let wa = self.wa()?;
        let collection = wa.sticker_collection(source).ok_or("That pack is gone.")?;
        // Each file as Signal takes it; one that can't be had is left out.
        let mut files: Vec<(String, String, PathBuf, &'static str)> = Vec::new();
        let mut missing = 0usize;
        for (hash, emojis) in collection.stickers.iter().take(PACK_MAX) {
            let made = match wa.library_bytes(hash).await {
                Ok(bytes) => tauri::async_runtime::spawn_blocking(move || crate::sticker_image::for_signal(&bytes))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
                Err(e) => Err(e),
            };
            let made = match made {
                Ok(made) => made,
                Err(e) => {
                    log::warn!("signal: a sticker was left out of the pack: {e}");
                    missing += 1;
                    continue;
                }
            };
            let staged = crate::vault::handoff_path("signal-sticker").map_err(|e| e.to_string())?;
            if let Err(e) = fs::write(&staged, &made.bytes) {
                cleanup(&files);
                return Err(format!("could not hand a sticker over: {e}"));
            }
            let emoji = emojis.first().cloned().unwrap_or_else(|| DEFAULT_EMOJI.to_string());
            files.push((hash.clone(), emoji, staged, made.mime));
        }
        if files.is_empty() {
            return Err(if missing > 0 {
                "None of the pack's stickers could be downloaded from WhatsApp.".into()
            } else {
                "That pack has no stickers.".into()
            });
        }
        let title = if source == "favorites" { "WhatsApp favorites".to_string() } else { collection.name.clone() };
        let author = if collection.publisher.trim().is_empty() { AUTHOR.to_string() } else { collection.publisher.clone() };
        let command = serde_json::json!({
            "cmd": "upload_pack",
            "title": title,
            "author": author,
            "stickers": files.iter().map(|(_, emoji, path, mime)| serde_json::json!({ "path": path, "emoji": emoji, "mime": mime })).collect::<Vec<_>>(),
            "install": true,
            "ts": now_ms(),
        });
        let answer = self.request(command, UPLOAD_TIMEOUT).await;
        cleanup(&files);
        let answer = answer.map_err(|e| format!("The pack was not added to Signal: {e}"))?.ok_or("Signal did not name the pack.")?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Made {
            pack_id: String,
            pack_key: String,
        }
        let made: Made = serde_json::from_str(&answer).map_err(|e| e.to_string())?;
        let pack = SignalPack {
            source: source.to_string(),
            title,
            pack_id: made.pack_id,
            pack_key: made.pack_key,
            stickers: files.into_iter().map(|(hash, ..)| hash).collect(),
            uploaded_ms: now_ms(),
        };
        let old = {
            let mut packs = lock(&self.sticker_packs);
            let old = packs.replace(pack);
            packs.save(&self.dir);
            old
        };
        if let Some(old) = old {
            let command = serde_json::json!({ "cmd": "install_pack", "packId": old.pack_id, "packKey": old.pack_key, "install": false, "ts": now_ms() });
            if let Err(e) = self.request(command, super::SEND_TIMEOUT).await {
                log::warn!("signal: the older pack was not taken off the phone: {e}");
            }
        }
        if missing > 0 {
            return Err(format!(
                "The pack is on Signal without {missing} sticker{} that could not be downloaded from WhatsApp.",
                if missing == 1 { "" } else { "s" }
            ));
        }
        Ok(())
    }

    /// Sends a library sticker. One in a pack on Signal goes as that pack's
    /// sticker; any other as a picture, as Signal's stickers have to come
    /// from a pack.
    pub async fn send_sticker(&self, chat_id: &str, id: &str, reply_to: Option<&str>) -> Result<MessageDto, String> {
        writable(chat_id)?;
        let wa = self.wa()?;
        let bytes = wa.library_bytes(id).await?;
        let made = tauri::async_runtime::spawn_blocking(move || crate::sticker_image::for_signal(&bytes))
            .await
            .map_err(|e| e.to_string())??;
        let pack = lock(&self.sticker_packs).find(id).map(|(p, n)| (p.pack_id.clone(), p.pack_key.clone(), n));
        let (quote, reply) = self.quote(chat_id, reply_to)?.unzip();
        let ts = now_ms();
        let staged = crate::vault::handoff_path("signal-outbox").map_err(|e| e.to_string())?;
        fs::write(&staged, &made.bytes).map_err(|e| format!("could not hand the sticker over: {e}"))?;
        let command = match &pack {
            Some((pack_id, pack_key, sticker_id)) => serde_json::json!({
                "cmd": "send_sticker",
                "chat": chat_id,
                "packId": pack_id,
                "packKey": pack_key,
                "stickerId": sticker_id,
                "emoji": wa.sticker_emojis(id).into_iter().next().unwrap_or_else(|| DEFAULT_EMOJI.to_string()),
                "path": staged,
                "mime": made.mime,
                "quote": quote,
                "ts": ts,
            }),
            None => serde_json::json!({
                "cmd": "send_file",
                "chat": chat_id,
                "path": staged,
                "mime": made.mime,
                "fileName": null,
                "kind": "image",
                "width": crate::sticker_image::SIDE,
                "height": crate::sticker_image::SIDE,
                "caption": null,
                "quote": quote,
                "ts": ts,
            }),
        };
        let answer = self.request(command, UPLOAD_TIMEOUT).await;
        let media = match answer {
            Ok(Some(media)) => serde_json::from_str::<MediaRef>(&media).ok(),
            Ok(None) => None,
            Err(e) => {
                let _ = fs::remove_file(&staged);
                return Err(format!("The sticker was not sent: {e}"));
            }
        };
        // Kept where a download of it would go, so it shows without one.
        match &media {
            Some(media) => {
                let path = self.dir.join(MEDIA_DIR).join(media_file_name(media));
                if let Some(parent) = path.parent() {
                    let _ = secure_dir(parent);
                }
                if let Err(e) = crate::vault::get().import(&staged, &path) {
                    log::warn!("signal: could not keep a copy of the sent sticker: {e}");
                }
            }
            None => {
                let _ = fs::remove_file(&staged);
            }
        }
        let mut content = match pack {
            Some(_) => Content::media(MsgKind::Media, "Sticker", None),
            None => Content::media(MsgKind::Image, "Photo", None),
        };
        content.reply_to = reply;
        content.attachments.extend(media);
        self.store_sent(chat_id, ts, content)
    }
}

fn cleanup(files: &[(String, String, PathBuf, &'static str)]) {
    for (_, _, path, _) in files {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stickers::Collection;

    fn pack(source: &str, stickers: &[&str], at: i64) -> SignalPack {
        SignalPack {
            source: source.into(),
            title: source.into(),
            pack_id: "ab".into(),
            pack_key: "cd".into(),
            stickers: stickers.iter().map(|s| s.to_string()).collect(),
            uploaded_ms: at,
        }
    }

    #[test]
    fn a_sticker_is_sent_from_the_newest_pack_holding_it() {
        let mut packs = Packs::default();
        packs.replace(pack("favorites", &["a", "b"], 1));
        packs.replace(pack("cats", &["c", "b"], 2));
        assert_eq!(packs.find("b").map(|(p, n)| (p.source.as_str(), n)), Some(("cats", 1)));
        assert_eq!(packs.find("a").map(|(_, n)| n), Some(0));
        assert!(packs.find("z").is_none());
        // Making a collection's pack again replaces the old one.
        let old = packs.replace(pack("cats", &["c"], 3)).unwrap();
        assert_eq!(old.uploaded_ms, 2);
        assert_eq!(packs.find("b").map(|(p, _)| p.source.as_str()), Some("favorites"));
    }

    #[test]
    fn a_collection_that_grew_shows_what_signal_is_missing() {
        let mut packs = Packs::default();
        packs.replace(pack("favorites", &["a", "b"], 1));
        let collection = |hashes: &[&str]| Collection {
            name: "Favorites".into(),
            publisher: String::new(),
            stickers: hashes.iter().map(|h| (h.to_string(), Vec::new())).collect(),
        };
        let collections = vec![("favorites".to_string(), collection(&["c", "a", "b"])), ("cats".to_string(), collection(&["x"]))];
        let uploading = HashSet::from(["cats".to_string()]);
        let status = status(&packs, &collections, &uploading);
        assert_eq!((status[0].count, status[0].on_signal), (3, 2));
        assert!(status[0].url.as_deref().is_some_and(|u| u.contains("pack_id=ab&pack_key=cd")));
        assert_eq!((status[1].on_signal, status[1].uploading, status[1].url.is_none()), (0, true, true));
    }
}
