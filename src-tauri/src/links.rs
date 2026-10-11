//! What the UI needs to know about a video link before it plays it, and cannot
//! find out itself because browsers keep other sites' responses from it.
//!
//! Short share links (`vm.tiktok.com/…`) only say which video they are for
//! once followed.

use ureq::ResponseExt;

/// Hosts whose short links we are willing to follow.
const SHORT_LINK_HOSTS: [&str; 4] = ["vm.tiktok.com", "vt.tiktok.com", "www.tiktok.com", "tiktok.com"];

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) Gecko/20100101 Firefox/140.0";

/// Whether Instagram's player for a post (`https://www.instagram.com/reel/<id>/embed/`)
/// plays the video. For many, mostly over the music in them, it only shows a
/// picture that links to the site.
#[tauri::command]
pub async fn instagram_embed_plays(url: String) -> Result<bool, String> {
    if !url.starts_with("https://www.instagram.com/") {
        return Err(format!("Not an Instagram player: {url}"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let page = ureq::get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Sec-Fetch-Dest", "iframe")
            .call()
            .and_then(|mut response| response.body_mut().read_to_string())
            .map_err(|e| format!("Could not ask Instagram about {url}: {e}"))?;
        Ok(!page.contains("WatchOnInstagram"))
    })
    .await
    .map_err(|e| format!("Could not ask Instagram: {e}"))?
}

/// Follows a short share link and returns the address it leads to.
#[tauri::command]
pub async fn resolve_short_link(url: String) -> Result<String, String> {
    let host = url
        .strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .unwrap_or_default();
    if !SHORT_LINK_HOSTS.contains(&host) {
        return Err(format!("Not a short link we follow: {url}"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let response = ureq::get(&url)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|e| format!("Could not follow {url}: {e}"))?;
        Ok(response.get_uri().to_string())
    })
    .await
    .map_err(|e| format!("Could not follow the link: {e}"))?
}

/// A post's video on X, as the page that embeds posts gets it.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct XVideo {
    /// An MP4 of at most 720p: enough for a chat, and light on the connection.
    pub url: String,
    pub poster: Option<String>,
    pub width: u32,
    pub height: u32,
}

/// Each post asked about, so a card drawn again asks nobody.
static X_VIDEOS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, Option<XVideo>>>> =
    std::sync::LazyLock::new(Default::default);

/// The video in a post on X (`id` is the number in `/status/<id>`), or
/// `None` when it has none. Asks X's embedding service, which needs no account.
#[tauri::command]
pub async fn x_video(id: String) -> Result<Option<XVideo>, String> {
    if id.is_empty() || id.len() > 25 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("Not a post on X: {id}"));
    }
    if let Some(known) = crate::account::lock(&X_VIDEOS).get(&id) {
        return Ok(known.clone());
    }
    let asked = id.clone();
    let video = tauri::async_runtime::spawn_blocking(move || {
        let url = format!("https://cdn.syndication.twimg.com/tweet-result?id={asked}&token={}&lang=en", x_token(&asked));
        let body = ureq::get(&url)
            .header("User-Agent", USER_AGENT)
            .call()
            .and_then(|mut response| response.body_mut().read_to_string())
            .map_err(|e| format!("Could not ask X about the post: {e}"))?;
        let post: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("X answered something unexpected: {e}"))?;
        Ok::<_, String>(x_video_in(&post))
    })
    .await
    .map_err(|e| format!("Could not ask X: {e}"))??;
    crate::account::lock(&X_VIDEOS).insert(id, video.clone());
    Ok(video)
}

/// The best MP4 up to 720p among a post's videos (a quoted post's too).
fn x_video_in(post: &serde_json::Value) -> Option<XVideo> {
    let media = post["mediaDetails"].as_array().into_iter().flatten().chain(post["quoted_tweet"]["mediaDetails"].as_array().into_iter().flatten());
    for item in media {
        if !matches!(item["type"].as_str(), Some("video" | "animated_gif")) {
            continue;
        }
        let width = item["original_info"]["width"].as_u64().unwrap_or(16) as u32;
        let height = item["original_info"]["height"].as_u64().unwrap_or(9) as u32;
        let mut variants: Vec<(u64, u32, &str)> = item["video_info"]["variants"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v["content_type"] == "video/mp4")
            .filter_map(|v| {
                let url = v["url"].as_str()?;
                // The size is in the address: …/vid/avc1/1280x720/…
                let short = url
                    .split('/')
                    .find_map(|part| part.split_once('x').and_then(|(w, h)| Some(w.parse::<u32>().ok()?.min(h.parse::<u32>().ok()?))))
                    .unwrap_or(0);
                Some((v["bitrate"].as_u64().unwrap_or(0), short, url))
            })
            .collect();
        variants.sort_by_key(|(bitrate, _, _)| *bitrate);
        let pick = variants.iter().rev().find(|(_, short, _)| *short <= 720).or(variants.first())?;
        return Some(XVideo {
            url: pick.2.to_string(),
            poster: item["media_url_https"].as_str().map(str::to_string),
            width,
            height,
        });
    }
    None
}

/// The token X's embed script sends with a post's id: id / 1e15 * π in
/// base 36, without its zeros and point.
fn x_token(id: &str) -> String {
    let value = id.parse::<f64>().unwrap_or(0.0) / 1e15 * std::f64::consts::PI;
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut whole = value.trunc() as u64;
    let mut int = Vec::new();
    loop {
        int.push(DIGITS[(whole % 36) as usize]);
        whole /= 36;
        if whole == 0 {
            break;
        }
    }
    int.reverse();
    let mut fraction = value.fract();
    let mut out: String = int.into_iter().map(char::from).collect();
    for _ in 0..11 {
        fraction *= 36.0;
        out.push(char::from(DIGITS[fraction.trunc() as usize]));
        fraction = fraction.fract();
    }
    out.replace('0', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x_token_starts_like_the_embed_scripts() {
        // JS: ((2103576084499358051 / 1e15) * Math.PI).toString(36).replace(/(0+|\.)/g, "")
        assert!(x_token("2103576084499358051").starts_with("53kkulws"));
    }

    #[test]
    fn x_video_picks_720p_at_most() {
        let post = serde_json::json!({"mediaDetails": [{"type": "video", "media_url_https": "p.jpg", "original_info": {"width": 1920, "height": 1080},
            "video_info": {"variants": [
                {"content_type": "application/x-mpegURL", "url": "a.m3u8"},
                {"bitrate": 256000, "content_type": "video/mp4", "url": "https://v/vid/avc1/480x270/a.mp4"},
                {"bitrate": 2176000, "content_type": "video/mp4", "url": "https://v/vid/avc1/1280x720/b.mp4"},
                {"bitrate": 10368000, "content_type": "video/mp4", "url": "https://v/vid/avc1/1920x1080/c.mp4"}]}}]});
        let video = x_video_in(&post).unwrap();
        assert_eq!(video.url, "https://v/vid/avc1/1280x720/b.mp4");
        assert_eq!((video.width, video.height), (1920, 1080));
    }
}
