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
