//! The sticker library: stickers, packs and GIFs, one for the whole app.
//! WhatsApp's recent and favorite stickers (synced with the phone) and its
//! packs live here, as do packs made here; the other networks send from it
//! (see `signal::stickers`). Each sticker is known by its hash (WhatsApp's
//! `filehash`: the base64 SHA-256 of the plain file) and kept, encrypted, in
//! `stickers/files`, so it outlives WhatsApp's CDN and any one account.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::store::CdnRef;

/// The most a sticker in a shared pack's zip may take once unpacked.
const ZIP_ENTRY_MAX: usize = 4 * 1024 * 1024;

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
    /// The personal account's phone was asked for its favorites once.
    #[serde(default)]
    pub favorites_synced: bool,
    /// The same, for the business account's phone.
    #[serde(default)]
    pub business_favorites_synced: bool,
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
    fn read(path: &Path) -> Option<Self> {
        if !path.exists() {
            return None;
        }
        match crate::vault::get()
            .read(path)
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
        {
            Ok(library) => Some(library),
            Err(e) => {
                log::warn!("stickers: {} could not be read: {e}", path.display());
                None
            }
        }
    }

    /// Takes in another library's stickers, packs and GIFs: what one has
    /// and the other doesn't, the newer send and favorite of the two.
    fn merge(&mut self, other: Library) {
        for (hash, theirs) in other.stickers {
            match self.stickers.get_mut(&hash) {
                Some(ours) => {
                    ours.sent_ms = ours.sent_ms.max(theirs.sent_ms);
                    ours.favorite_ms = ours.favorite_ms.or(theirs.favorite_ms);
                    ours.animated = ours.animated.or(theirs.animated);
                    if ours.emojis.is_empty() {
                        ours.emojis = theirs.emojis;
                    }
                }
                None => {
                    self.stickers.insert(hash, theirs);
                }
            }
        }
        for pack in other.packs {
            if !self.has_pack(&pack.id) {
                self.packs.push(pack);
            }
        }
        for (hash, gif) in other.gifs {
            self.gifs.entry(hash).or_insert(gif);
        }
    }

    /// Drops what is no longer shown anywhere: neither recent, favorite nor
    /// in a pack. Returns the files that went.
    fn prune(&mut self) -> Vec<CdnRef> {
        let mut keep: HashSet<String> = self.packs.iter().flat_map(|p| p.stickers.iter().cloned()).collect();
        keep.extend(self.stickers.iter().filter(|(_, s)| s.favorite_ms.is_some()).map(|(h, _)| h.clone()));
        keep.extend(self.recent_hashes());
        let mut gone = Vec::new();
        self.stickers.retain(|hash, s| {
            let kept = keep.contains(hash);
            if !kept {
                gone.push(s.file.clone());
            }
            kept
        });
        let gifs: HashSet<String> = self.recent_gif_hashes().into_iter().collect();
        self.gifs.retain(|hash, g| {
            let kept = gifs.contains(hash);
            if !kept {
                gone.push(g.file.clone());
            }
            kept
        });
        gone
    }

    /// Every file the library shows, stickers and GIFs.
    pub fn files(&self) -> Vec<CdnRef> {
        self.stickers.values().map(|s| s.file.clone()).chain(self.gifs.values().map(|g| g.file.clone())).collect()
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

    /// The store pack a sticker came with, if any.
    pub fn store_pack_of(&self, hash: &str) -> Option<String> {
        self.packs.iter().find(|p| p.store && p.stickers.iter().any(|h| h == hash)).map(|p| p.id.clone())
    }

    /// Newer addresses for stickers already kept; others are left out.
    /// Returns whether any changed.
    pub fn refresh_files(&mut self, files: impl IntoIterator<Item = CdnRef>) -> bool {
        let mut changed = false;
        for file in files {
            if let Some(entry) = self.stickers.get(&file.sha256)
                && entry.file.path != file.path
            {
                self.entry(file);
                changed = true;
            }
        }
        changed
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

    /// Moves a sticker of one of our packs to place `to` in it.
    pub fn move_in_pack(&mut self, id: &str, hash: &str, to: usize) -> Result<(), String> {
        let pack = self.own_pack(id)?;
        let from = pack.stickers.iter().position(|h| h == hash).ok_or("That sticker isn't in the pack.")?;
        let sticker = pack.stickers.remove(from);
        pack.stickers.insert(to.min(pack.stickers.len()), sticker);
        Ok(())
    }

    /// How much the library holds.
    pub fn counts(&self) -> (usize, usize, usize) {
        (self.stickers.len(), self.packs.len(), self.gifs.len())
    }

    /// A collection to take elsewhere: `favorites` or a pack, as its name,
    /// publisher, and stickers (hash and emojis) in the order shown.
    pub fn collection(&self, source: &str) -> Option<Collection> {
        let (name, publisher, hashes) = if source == "favorites" {
            let mut favorites: Vec<(&String, i64)> = self.stickers.iter().filter_map(|(h, s)| s.favorite_ms.map(|at| (h, at))).collect();
            favorites.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
            ("Favorites".to_string(), String::new(), favorites.into_iter().map(|(h, _)| h.clone()).collect::<Vec<_>>())
        } else {
            let pack = self.packs.iter().find(|p| p.id == source)?;
            (pack.name.clone(), pack.publisher.clone(), pack.stickers.clone())
        };
        let stickers = hashes
            .into_iter()
            .filter_map(|h| {
                let emojis = self.stickers.get(&h)?.emojis.clone();
                Some((h, emojis))
            })
            .collect();
        Some(Collection { name, publisher, stickers })
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

/// The files in a zip, by name: stored or deflated, as WhatsApp packs them.
/// No zip64: a sticker pack is far from needing it.
pub fn unzip(bytes: &[u8]) -> Result<HashMap<String, Vec<u8>>, String> {
    use std::io::Read as _;
    let bad = || "The pack's file could not be read.".to_string();
    let u16_at = |at: usize| bytes.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).ok_or_else(bad);
    let u32_at = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or_else(bad);
    // The end record is last, after a comment of up to 64 KB.
    let floor = bytes.len().saturating_sub(22 + 65_535);
    let end = (floor..bytes.len().saturating_sub(21))
        .rev()
        .find(|&at| bytes[at..].starts_with(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or_else(bad)?;
    let count = u16_at(end + 10)?;
    let mut at = u32_at(end + 16)?;
    let mut files = HashMap::new();
    for _ in 0..count {
        if u32_at(at)? != 0x0201_4b50 {
            return Err(bad());
        }
        let method = u16_at(at + 10)?;
        let packed = u32_at(at + 20)?;
        let size = u32_at(at + 24)?;
        let (name_len, extra_len, comment_len) = (u16_at(at + 28)?, u16_at(at + 30)?, u16_at(at + 32)?);
        let local = u32_at(at + 42)?;
        let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + name_len).ok_or_else(bad)?).into_owned();
        at += 46 + name_len + extra_len + comment_len;
        if name.ends_with('/') || size > ZIP_ENTRY_MAX {
            continue;
        }
        if u32_at(local)? != 0x0403_4b50 {
            return Err(bad());
        }
        let start = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
        let data = bytes.get(start..start + packed).ok_or_else(bad)?;
        let plain = match method {
            0 => data.to_vec(),
            8 => {
                let mut out = Vec::with_capacity(size);
                flate2::read::DeflateDecoder::new(data)
                    .take(ZIP_ENTRY_MAX as u64)
                    .read_to_end(&mut out)
                    .map_err(|_| bad())?;
                out
            }
            _ => continue,
        };
        files.insert(name, plain);
    }
    Ok(files)
}

/// Favorites or a pack, to take to another network.
pub struct Collection {
    pub name: String,
    pub publisher: String,
    /// Hash and emojis of each sticker.
    pub stickers: Vec<(String, Vec<String>)>,
}

/// Where the library and its files are kept: `stickers/` in the app's
/// folder, apart from any account, so unlinking one keeps them.
pub struct Central {
    dir: PathBuf,
    pub library: std::sync::Mutex<Library>,
    /// The files of the library are being fetched, once per start.
    pub filling: std::sync::atomic::AtomicBool,
}

const LIBRARY_FILE: &str = "library.json";
const FILES_DIR: &str = "files";
/// The library each WhatsApp account kept before there was one for the app.
const OLD_LIBRARY: &str = "stickers.json";

impl Central {
    pub fn dir(data: &Path) -> PathBuf {
        data.join("stickers")
    }

    /// Opens the library, made the first time from what the WhatsApp
    /// accounts kept (`account_dirs`), whose old files are then set aside.
    pub fn open(data: &Path, account_dirs: &[PathBuf]) -> std::io::Result<Self> {
        let dir = Self::dir(data);
        crate::account::secure_dir(&dir)?;
        crate::account::secure_dir(&dir.join(FILES_DIR))?;
        let path = dir.join(LIBRARY_FILE);
        let library = match Library::read(&path) {
            Some(library) => library,
            None => {
                let mut library = Library::default();
                for (n, account) in account_dirs.iter().enumerate() {
                    let old = account.join(OLD_LIBRARY);
                    if let Some(theirs) = Library::read(&old) {
                        let synced = theirs.favorites_synced;
                        library.merge(theirs);
                        if n == 0 {
                            library.favorites_synced = synced;
                        } else {
                            library.business_favorites_synced = synced;
                        }
                    }
                }
                let central = Self { dir: dir.clone(), library: std::sync::Mutex::new(library), filling: Default::default() };
                central.save(&mut crate::account::lock(&central.library));
                for account in account_dirs {
                    let old = account.join(OLD_LIBRARY);
                    if old.exists() {
                        let _ = std::fs::rename(&old, account.join(format!("{OLD_LIBRARY}.migrated")));
                    }
                }
                return Ok(central);
            }
        };
        Ok(Self { dir, library: std::sync::Mutex::new(library), filling: Default::default() })
    }

    /// Writes the library, after dropping what nothing shows any more, and
    /// the files of what was dropped.
    pub fn save(&self, library: &mut Library) {
        for gone in library.prune() {
            if let Some(name) = file_name(&gone) {
                let _ = std::fs::remove_file(self.files().join(name));
            }
        }
        match serde_json::to_vec(library) {
            Ok(bytes) => {
                if let Err(e) = crate::vault::get().write(&self.dir.join(LIBRARY_FILE), &bytes) {
                    log::warn!("stickers: the library was not saved: {e}");
                }
            }
            Err(e) => log::warn!("stickers: the library was not saved: {e}"),
        }
    }

    /// The folder the files are in, which the `media` scheme serves.
    pub fn files(&self) -> PathBuf {
        self.dir.join(FILES_DIR)
    }

    /// A file's name in the library when it is kept there.
    pub fn kept(&self, file: &CdnRef) -> Option<String> {
        let name = file_name(file)?;
        self.files().join(&name).exists().then_some(name)
    }

    /// Keeps a file's plain bytes, encrypted; returns its name.
    pub fn keep(&self, file: &CdnRef, bytes: &[u8]) -> Result<String, String> {
        let name = file_name(file).ok_or("Bad file reference.")?;
        let path = self.files().join(&name);
        if !path.exists() {
            crate::vault::get().write(&path, bytes).map_err(|e| format!("The sticker could not be kept: {e}"))?;
        }
        Ok(name)
    }

    /// What the library holds, and the bytes its kept files take.
    pub fn info(&self) -> InfoDto {
        let (stickers, packs, gifs) = crate::account::lock(&self.library).counts();
        let bytes = std::fs::read_dir(self.files())
            .map(|entries| entries.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
            .unwrap_or(0);
        InfoDto { stickers, packs, gifs, bytes }
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        crate::vault::get().read(&self.files().join(name)).map_err(|e| format!("The file could not be read: {e}"))
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InfoDto {
    pub stickers: usize,
    pub packs: usize,
    pub gifs: usize,
    /// On disk, encrypted.
    pub bytes: u64,
}

#[tauri::command]
pub fn stickers_info(central: tauri::State<'_, std::sync::Arc<Central>>) -> InfoDto {
    central.info()
}

/// A library file's name: `st-` and its hash in hex, with its type's extension.
pub fn file_name(file: &CdnRef) -> Option<String> {
    use base64::Engine as _;
    let hash = base64::engine::general_purpose::STANDARD.decode(&file.sha256).ok()?;
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    let mime = file.mime.as_deref().unwrap_or("image/webp");
    Some(format!("st-{hex}.{}", crate::signal::extension(mime, None)))
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
    fn own_packs_reorder() {
        let mut lib = Library::default();
        let id = lib.create_pack("Mine", 1);
        for h in ["a", "b", "c"] {
            lib.add_to_pack(&id, file(h)).unwrap();
        }
        // Each lands first: c b a.
        lib.move_in_pack(&id, "a", 0).unwrap();
        lib.move_in_pack(&id, "c", 9).unwrap();
        let order: Vec<String> = lib.to_dto().packs[0].stickers.iter().map(|s| s.id.clone()).collect();
        assert_eq!(order, ["a", "b", "c"]);
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

    /// A zip of one file, stored (method 0) or deflated (8).
    fn zip_of(name: &str, plain: &[u8], method: u16) -> Vec<u8> {
        use std::io::Write as _;
        let data = if method == 8 {
            let mut deflate = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            deflate.write_all(plain).unwrap();
            deflate.finish().unwrap()
        } else {
            plain.to_vec()
        };
        let header = |zip: &mut Vec<u8>, central: bool| {
            zip.extend(if central { 0x0201_4b50u32 } else { 0x0403_4b50 }.to_le_bytes());
            if central {
                zip.extend([20, 0]);
            }
            zip.extend([20, 0, 0, 0]);
            zip.extend(method.to_le_bytes());
            zip.extend([0u8; 8]);
            zip.extend((data.len() as u32).to_le_bytes());
            zip.extend((plain.len() as u32).to_le_bytes());
            zip.extend((name.len() as u16).to_le_bytes());
            zip.extend([0, 0]);
            if central {
                zip.extend([0u8; 10]);
                zip.extend(0u32.to_le_bytes());
            }
            zip.extend(name.as_bytes());
        };
        let mut zip = Vec::new();
        header(&mut zip, false);
        zip.extend(&data);
        let central = zip.len() as u32;
        header(&mut zip, true);
        let size = zip.len() as u32 - central;
        zip.extend(0x0605_4b50u32.to_le_bytes());
        zip.extend([0, 0, 0, 0, 1, 0, 1, 0]);
        zip.extend(size.to_le_bytes());
        zip.extend(central.to_le_bytes());
        zip.extend([0, 0]);
        zip
    }

    #[test]
    fn a_shared_packs_zip_is_unpacked() {
        let plain = b"RIFF deflated deflated deflated WEBP";
        assert_eq!(unzip(&zip_of("a.webp", plain, 0)).unwrap()["a.webp"], plain);
        assert_eq!(unzip(&zip_of("b.webp", plain, 8)).unwrap()["b.webp"], plain);
        assert!(unzip(b"not a zip").is_err());
    }

    #[test]
    fn two_accounts_libraries_become_one() {
        let mut personal = Library::default();
        personal.note_sent(file("a"), 10);
        personal.set_favorite("b", Some(file("b")), true, 5);
        let mut business = Library::default();
        business.note_sent(file("a"), 20);
        let id = business.create_pack("Work", 1);
        business.add_to_pack(&id, file("c")).unwrap();
        personal.merge(business);
        let dto = personal.to_dto();
        assert_eq!(dto.recent[0].id, "a");
        assert_eq!(personal.sticker("a").unwrap().sent_ms, 20);
        assert_eq!(dto.favorites[0].id, "b");
        assert_eq!(dto.packs[0].stickers[0].id, "c");
    }

    #[test]
    fn pruning_names_the_files_to_delete() {
        let mut lib = Library::default();
        let id = lib.create_pack("Mine", 1);
        lib.add_to_pack(&id, file("a")).unwrap();
        lib.add_to_pack(&id, file("b")).unwrap();
        lib.remove_from_pack(&id, "b").unwrap();
        let gone: Vec<String> = lib.prune().into_iter().map(|f| f.sha256).collect();
        assert_eq!(gone, ["b"]);
        assert_eq!(file_name(&file("AAEC")).as_deref(), Some("st-000102.webp"));
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
