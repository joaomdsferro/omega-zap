//! GIF and sticker search through KLIPY. The app's key is built in (`KLIPY_API_KEY` at
//! build time; the same variable at run time overrides it for development),
//! so nobody installing the app needs one. Without a key there is no search.
//! What is searched goes to KLIPY with an anonymous id for this install.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const API: &str = "https://api.klipy.com/api/v1";
const PER_PAGE: u32 = 24;
/// A GIF or sticker bigger than this is not downloaded to send.
const MAX_BYTES: u64 = 16 * 1024 * 1024;
const ID_FILE: &str = "klipy-id";

pub fn key() -> Option<String> {
    std::env::var("KLIPY_API_KEY")
        .ok()
        .or_else(|| option_env!("KLIPY_API_KEY").map(str::to_string))
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

/// One result: what the picker shows, and what is sent.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GifResult {
    pub slug: String,
    pub title: String,
    /// A small animated picture for the grid.
    pub preview: String,
    pub width: u32,
    pub height: u32,
    pub send: GifPick,
}

/// The GIF to send: an MP4, as WhatsApp sends GIFs, and a still for its thumbnail.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GifPick {
    pub slug: String,
    pub mp4: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub still: Option<String>,
}

/// A sticker found: what the picker shows, and the picture made into a sticker to send.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StickerResult {
    pub slug: String,
    pub title: String,
    pub preview: String,
    pub width: u32,
    pub height: u32,
    pub send: StickerPick,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StickerPick {
    pub slug: String,
    /// A WebP, GIF or PNG, animated or not.
    pub url: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResultPage<T> {
    pub items: Vec<T>,
    pub next: Option<u32>,
}

pub type GifPage = ResultPage<GifResult>;
pub type StickerPage = ResultPage<StickerResult>;

/// What KLIPY calls its kinds of content in its addresses.
#[derive(Clone, Copy)]
pub enum Kind {
    Gifs,
    Stickers,
}

impl Kind {
    fn path(self) -> &'static str {
        match self {
            Kind::Gifs => "gifs",
            Kind::Stickers => "stickers",
        }
    }
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    data: Option<Page>,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    data: Vec<Item>,
    #[serde(default)]
    current_page: u32,
    #[serde(default)]
    has_next: bool,
}

#[derive(Deserialize)]
struct Item {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    file: Option<Sizes>,
}

#[derive(Deserialize, Default)]
struct Sizes {
    hd: Option<Formats>,
    md: Option<Formats>,
    sm: Option<Formats>,
    xs: Option<Formats>,
}

#[derive(Deserialize, Default)]
struct Formats {
    gif: Option<Format>,
    webp: Option<Format>,
    jpg: Option<Format>,
    png: Option<Format>,
    mp4: Option<Format>,
}

#[derive(Deserialize, Clone)]
struct Format {
    url: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    size: u64,
}

impl Item {
    fn result(self) -> Option<GifResult> {
        // Ads and anything that isn't a GIF are left out.
        if self.kind.as_deref().is_some_and(|k| k != "gif") || self.slug.is_empty() {
            return None;
        }
        let sizes = self.file?;
        let order = [&sizes.sm, &sizes.xs, &sizes.md, &sizes.hd];
        let preview = order.iter().find_map(|s| s.as_ref().and_then(|f| f.webp.clone().or_else(|| f.gif.clone())))?;
        // Medium is what WhatsApp sends GIFs at; HD only when there is no other.
        let mp4 = [&sizes.md, &sizes.hd, &sizes.sm]
            .iter()
            .find_map(|s| s.as_ref().and_then(|f| f.mp4.clone()).filter(|f| f.size <= MAX_BYTES))?;
        let still = order.iter().find_map(|s| s.as_ref().and_then(|f| f.jpg.clone())).map(|f| f.url);
        Some(GifResult {
            slug: self.slug.clone(),
            title: self.title,
            preview: preview.url,
            width: preview.width,
            height: preview.height,
            send: GifPick { slug: self.slug, mp4: mp4.url, width: mp4.width, height: mp4.height, still },
        })
    }

    fn sticker(self) -> Option<StickerResult> {
        // Ads are left out.
        if self.kind.as_deref() == Some("ad") || self.slug.is_empty() {
            return None;
        }
        let sizes = self.file?;
        let picture = |f: &Formats| f.webp.clone().or_else(|| f.gif.clone()).or_else(|| f.png.clone());
        let preview = [&sizes.sm, &sizes.xs, &sizes.md, &sizes.hd].iter().find_map(|s| s.as_ref().and_then(picture))?;
        // Stickers are 512 pixels a side: the largest picture that is not too big.
        let send = [&sizes.hd, &sizes.md, &sizes.sm]
            .iter()
            .find_map(|s| s.as_ref().and_then(picture).filter(|f| f.size <= MAX_BYTES))?;
        Some(StickerResult {
            slug: self.slug.clone(),
            title: self.title,
            preview: preview.url,
            width: preview.width,
            height: preview.height,
            send: StickerPick { slug: self.slug, url: send.url },
        })
    }
}

/// This install's anonymous id for KLIPY, made the first time.
fn customer_id(dir: &Path) -> String {
    let path = dir.join(ID_FILE);
    if let Ok(id) = std::fs::read_to_string(&path)
        && !id.trim().is_empty()
    {
        return id.trim().to_string();
    }
    let mut bytes = [0u8; 16];
    let _ = ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes);
    let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let _ = std::fs::write(&path, &id);
    id
}

/// The country the system is set to, as KLIPY localises by it.
fn country() -> String {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .and_then(|lang| lang.split(['.', '@']).next()?.split('_').nth(1).map(str::to_uppercase))
        .filter(|c| c.len() == 2)
        .unwrap_or_else(|| "US".into())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into()
}

/// A page of trending items without a query, otherwise a search. Blocking.
fn page(dir: &Path, kind: Kind, query: &str, page: u32) -> Result<(Vec<Item>, Option<u32>), String> {
    let key = key().ok_or("Search isn't set up in this build.")?;
    let customer = customer_id(dir);
    let page = page.max(1);
    let query = query.trim();
    let path = kind.path();
    let url = if query.is_empty() { format!("{API}/{key}/{path}/trending") } else { format!("{API}/{key}/{path}/search") };
    let mut request = agent()
        .get(&url)
        .query("page", page.to_string())
        .query("per_page", PER_PAGE.to_string())
        .query("customer_id", &customer)
        .query("locale", country());
    if !query.is_empty() {
        request = request.query("q", query);
    }
    let body = request
        .call()
        .map_err(|e| format!("KLIPY did not answer: {e}"))?
        .body_mut()
        .read_to_vec()
        .map_err(|e| format!("KLIPY did not answer: {e}"))?;
    let response: Response = serde_json::from_slice(&body).map_err(|e| format!("KLIPY's answer could not be read: {e}"))?;
    let data = response.data.ok_or("KLIPY's answer could not be read.")?;
    Ok((data.data, data.has_next.then_some(data.current_page.max(page) + 1)))
}

/// Trending GIFs without a query, otherwise a search. Blocking.
pub fn gifs(dir: &Path, query: &str, at: u32) -> Result<GifPage, String> {
    let (items, next) = page(dir, Kind::Gifs, query, at)?;
    Ok(ResultPage { items: items.into_iter().filter_map(Item::result).collect(), next })
}

/// Trending stickers without a query, otherwise a search. Blocking.
pub fn stickers(dir: &Path, query: &str, at: u32) -> Result<StickerPage, String> {
    let (items, next) = page(dir, Kind::Stickers, query, at)?;
    Ok(ResultPage { items: items.into_iter().filter_map(Item::sticker).collect(), next })
}

/// Tells KLIPY a GIF or sticker was sent, as it asks, so its ranking learns. Blocking; failures don't matter.
pub fn shared(dir: &Path, kind: Kind, slug: &str) {
    let Some(key) = key() else { return };
    let customer = customer_id(dir);
    let url = format!("{API}/{key}/{}/share/{slug}", kind.path());
    if let Err(e) = agent()
        .post(&url)
        .header("Content-Type", "application/json")
        .send(serde_json::json!({ "customer_id": customer }).to_string()) {
        log::info!("klipy: share not counted: {e}");
    }
}

/// Downloads a GIF's or sticker's file to send it. Only KLIPY's own servers. Blocking.
pub fn download(url: &str) -> Result<Vec<u8>, String> {
    let host_ok = url
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|host| host == "klipy.com" || host.ends_with(".klipy.com"));
    if !host_ok {
        return Err("That file is not from KLIPY.".into());
    }
    let mut response = agent().get(url).call().map_err(|e| format!("The file could not be downloaded: {e}"))?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| format!("The file could not be downloaded: {e}"))
}

/// An MP4's length in whole seconds (at least 1), from its movie header.
pub fn mp4_seconds(bytes: &[u8]) -> Option<u32> {
    fn boxes(data: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
        let mut at = 0usize;
        std::iter::from_fn(move || {
            let header = data.get(at..at + 8)?;
            let size = u32::from_be_bytes(header[0..4].try_into().ok()?) as usize;
            let kind = &header[4..8];
            let (start, end) = match size {
                1 => {
                    let big = u64::from_be_bytes(data.get(at + 8..at + 16)?.try_into().ok()?) as usize;
                    (at + 16, at.checked_add(big)?)
                }
                0 => (at + 8, data.len()),
                n if n >= 8 => (at + 8, at.checked_add(n)?),
                _ => return None,
            };
            let body = data.get(start..end.min(data.len()))?;
            at = end;
            Some((kind, body))
        })
    }
    let moov = boxes(bytes).find(|(k, _)| *k == b"moov")?.1;
    let mvhd = boxes(moov).find(|(k, _)| *k == b"mvhd")?.1;
    let (scale, duration) = if mvhd.first()? == &1 {
        (u32::from_be_bytes(mvhd.get(20..24)?.try_into().ok()?) as u64, u64::from_be_bytes(mvhd.get(24..32)?.try_into().ok()?))
    } else {
        (u32::from_be_bytes(mvhd.get(12..16)?.try_into().ok()?) as u64, u32::from_be_bytes(mvhd.get(16..20)?.try_into().ok()?) as u64)
    };
    (scale > 0).then(|| (duration.div_ceil(scale)).max(1) as u32)
}

/// A small JPEG for a message's thumbnail, from a still picture.
pub fn thumbnail(still: &[u8]) -> Option<Vec<u8>> {
    let picture = image::load_from_memory(still).ok()?.thumbnail(96, 96).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 60).encode_image(&picture).ok()?;
    Some(out)
}

/// Whether this build can search GIFs.
#[tauri::command]
pub fn gif_search_available() -> bool {
    key().is_some()
}

/// A page of GIFs: trending without a query.
#[tauri::command]
pub async fn gif_search(app: tauri::AppHandle, query: String, page: Option<u32>) -> Result<GifPage, String> {
    use tauri::Manager;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || gifs(&dir, &query, page.unwrap_or(1)))
        .await
        .map_err(|e| e.to_string())?
}

/// A page of stickers: trending without a query.
#[tauri::command]
pub async fn sticker_search(app: tauri::AppHandle, query: String, page: Option<u32>) -> Result<StickerPage, String> {
    use tauri::Manager;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || stickers(&dir, &query, page.unwrap_or(1)))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_skip_ads_and_prefer_medium_mp4() {
        let body = r#"{"result":true,"data":{"data":[
            {"id":1,"slug":"hi-1","title":"Hi","type":"gif","file":{
                "hd":{"mp4":{"url":"https://static.klipy.com/hd.mp4","width":498,"height":498,"size":900000}},
                "md":{"mp4":{"url":"https://static.klipy.com/md.mp4","width":320,"height":320,"size":200000}},
                "sm":{"webp":{"url":"https://static.klipy.com/sm.webp","width":220,"height":220,"size":1000},
                      "jpg":{"url":"https://static.klipy.com/sm.jpg","width":220,"height":220,"size":1000}}}},
            {"id":2,"slug":"ad","type":"ad"}
        ],"current_page":1,"per_page":24,"has_next":true}}"#;
        let response: Response = serde_json::from_str(body).unwrap();
        let page = response.data.unwrap();
        let items: Vec<GifResult> = page.data.into_iter().filter_map(Item::result).collect();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].preview, "https://static.klipy.com/sm.webp");
        assert_eq!(items[0].send.mp4, "https://static.klipy.com/md.mp4");
        assert_eq!(items[0].send.still.as_deref(), Some("https://static.klipy.com/sm.jpg"));
    }

    #[test]
    fn sticker_results_send_the_largest_picture() {
        let body = r#"{"result":true,"data":{"data":[
            {"id":1,"slug":"cat-1","title":"Cat","type":"sticker","file":{
                "hd":{"webp":{"url":"https://static.klipy.com/hd.webp","width":512,"height":512,"size":90000}},
                "sm":{"webp":{"url":"https://static.klipy.com/sm.webp","width":150,"height":150,"size":9000}}}},
            {"id":2,"slug":"ad","type":"ad"},
            {"id":3,"slug":"dog-2","title":"Dog","file":{"md":{"png":{"url":"https://static.klipy.com/md.png","width":300,"height":300,"size":5000}}}}
        ],"current_page":1,"per_page":24,"has_next":false}}"#;
        let response: Response = serde_json::from_str(body).unwrap();
        let items: Vec<StickerResult> = response.data.unwrap().data.into_iter().filter_map(Item::sticker).collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].preview, "https://static.klipy.com/sm.webp");
        assert_eq!(items[0].send.url, "https://static.klipy.com/hd.webp");
        assert_eq!(items[1].send.url, "https://static.klipy.com/md.png");
    }

    #[test]
    fn downloads_only_from_klipy() {
        assert!(download("https://example.com/a.mp4").is_err());
        assert!(download("http://static.klipy.com/a.mp4").is_err());
        assert!(download("https://static.klipy.com.evil.net/a.mp4").is_err());
    }

    #[test]
    fn reads_an_mp4_length() {
        // ftyp, then moov > mvhd (version 0, timescale 1000, duration 2500).
        let mut mvhd = vec![0u8; 100];
        mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
        mvhd[16..20].copy_from_slice(&2500u32.to_be_bytes());
        let mut moov = ((8 + 8 + mvhd.len()) as u32).to_be_bytes().to_vec();
        moov.extend_from_slice(b"moov");
        moov.extend_from_slice(&((8 + mvhd.len()) as u32).to_be_bytes());
        moov.extend_from_slice(b"mvhd");
        moov.extend_from_slice(&mvhd);
        let mut file = 16u32.to_be_bytes().to_vec();
        file.extend_from_slice(b"ftypisom\0\0\0\0");
        file.extend_from_slice(&moov);
        assert_eq!(mp4_seconds(&file), Some(3));
        assert_eq!(mp4_seconds(b"nonsense"), None);
    }
}
