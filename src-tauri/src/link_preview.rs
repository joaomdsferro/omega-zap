//! Cards for links that came without one: the page's title, description and
//! picture, read from its Open Graph tags. Each link is fetched once; the
//! card and its picture are kept, encrypted, so a chat reopens without
//! going back to the site.

use std::io::Read;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::vault;

/// Where the cards and their pictures are kept, under the app's data folder.
pub const PREVIEW_DIR: &str = "link-previews";

/// Many sites only describe a page in its tags to the apps that make
/// cards, and WhatsApp's is the one they all know.
const USER_AGENT: &str = "WhatsApp/2.24.20 (Omega Zap)";
/// Enough of a page to reach the end of its `<head>`.
const MAX_PAGE: u64 = 768 * 1024;
const MAX_PICTURE: u64 = 5 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// The picture's file name, served by the media protocol.
    pub image: Option<String>,
}

/// The card for `url`, from what was kept or from the site. `None` when the
/// page has nothing to show.
#[tauri::command]
pub async fn link_preview(app: AppHandle, url: String) -> Result<Option<Preview>, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join(PREVIEW_DIR);
    tauri::async_runtime::spawn_blocking(move || cached_or_fetched(&dir, &url))
        .await
        .map_err(|e| format!("Could not read the link: {e}"))?
}

fn cached_or_fetched(dir: &Path, url: &str) -> Result<Option<Preview>, String> {
    let key = key_of(url);
    let card = dir.join(format!("{key}.json"));
    if let Ok(bytes) = vault::get().read(&card) {
        let kept: Option<Preview> = serde_json::from_slice(&bytes).unwrap_or_default();
        return Ok(kept.filter(|p| p.image.as_ref().is_none_or(|name| dir.join(name).is_file())));
    }
    let preview = fetch(dir, &key, url)?;
    crate::account::secure_dir(dir).map_err(|e| e.to_string())?;
    // A page with nothing to show is remembered too, so it is not asked again.
    let json = serde_json::to_vec(&preview).map_err(|e| e.to_string())?;
    vault::get().write(&card, &json).map_err(|e| format!("Could not keep the link card: {e}"))?;
    Ok(preview)
}

fn key_of(url: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, url.as_bytes());
    digest.as_ref()[..16].iter().map(|b| format!("{b:02x}")).collect()
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        // Each hop is checked before it is followed.
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(TIMEOUT))
        .user_agent(USER_AGENT)
        .build()
        .new_agent()
}

/// Fetches `url`, following redirects only to public addresses.
fn get(agent: &ureq::Agent, url: &str, accept: &str) -> Result<(String, ureq::http::Response<ureq::Body>), String> {
    let mut url = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        public(&url)?;
        let response = agent
            .get(&url)
            .header("Accept", accept)
            .call()
            .map_err(|e| format!("Could not open {url}: {e}"))?;
        let status = response.status();
        if status.is_redirection() {
            let to = response
                .headers()
                .get("location")
                .and_then(|l| l.to_str().ok())
                .ok_or_else(|| format!("{url} redirects nowhere"))?;
            url = resolve(&url, to).ok_or_else(|| format!("{url} redirects to {to}"))?;
            continue;
        }
        if !status.is_success() {
            return Err(format!("{url} answered {status}"));
        }
        return Ok((url, response));
    }
    Err(format!("{url} redirects too many times"))
}

/// Links in messages are other people's: they must not reach into this
/// computer or the network it is on.
fn public(url: &str) -> Result<(), String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| format!("Not a web link: {url}"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let (host, port) = match host_port.strip_prefix('[') {
        Some(v6) => {
            let (host, after) = v6.split_once(']').ok_or("Bad address")?;
            (host, after.strip_prefix(':').and_then(|p| p.parse().ok()))
        }
        None => match host_port.rsplit_once(':') {
            Some((host, port)) => (host, port.parse().ok()),
            None => (host_port, None),
        },
    };
    let port = port.unwrap_or(if url.starts_with("https") { 443 } else { 80 });
    let addrs: Vec<IpAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("Could not find {host}: {e}"))?
        .map(|a| a.ip())
        .collect();
    if addrs.is_empty() || addrs.iter().any(|ip| !is_public(ip)) {
        return Err(format!("{host} is not on the public internet"));
    }
    Ok(())
}

fn is_public(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                // Carrier-grade NAT.
                || (a == 100 && (64..128).contains(&b)))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(&IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                // Unique local and link-local.
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

fn fetch(dir: &Path, key: &str, url: &str) -> Result<Option<Preview>, String> {
    let agent = agent();
    let (page_url, mut response) = get(&agent, url, "text/html,application/xhtml+xml")?;
    if !response.body().mime_type().is_some_and(|m| m.contains("html")) {
        return Ok(None);
    }
    let mut page = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_PAGE)
        .read_to_end(&mut page)
        .map_err(|e| format!("Could not read {url}: {e}"))?;
    let tags = read_tags(&String::from_utf8_lossy(&page));
    if tags.title.is_none() && tags.image.is_none() {
        return Ok(None);
    }
    // A card without its picture is still worth showing.
    let image = tags
        .image
        .as_deref()
        .and_then(|src| resolve(&page_url, src))
        .and_then(|src| save_picture(&agent, dir, key, &src).inspect_err(|e| log::debug!("{e}")).ok());
    Ok(Some(Preview { url: url.to_string(), title: tags.title, description: tags.description, image }))
}

fn save_picture(agent: &ureq::Agent, dir: &Path, key: &str, src: &str) -> Result<String, String> {
    let (_, mut response) = get(agent, src, "image/avif,image/webp,image/png,image/jpeg,image/*")?;
    let ext = match response.body().mime_type() {
        Some("image/jpeg" | "image/jpg") => "jpg",
        Some("image/png") => "png",
        Some("image/webp") => "webp",
        Some("image/gif") => "gif",
        Some("image/avif") => "avif",
        other => return Err(format!("{src} is not a picture we show ({other:?})")),
    };
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_PICTURE)
        .read_to_vec()
        .map_err(|e| format!("Could not download {src}: {e}"))?;
    crate::account::secure_dir(dir).map_err(|e| e.to_string())?;
    let name = format!("{key}.{ext}");
    vault::get().write(&dir.join(&name), &bytes).map_err(|e| format!("Could not keep {src}: {e}"))?;
    Ok(name)
}

/// `to` as an absolute address, read from the page at `base`.
fn resolve(base: &str, to: &str) -> Option<String> {
    let to = to.trim();
    if to.starts_with("https://") || to.starts_with("http://") {
        return Some(to.to_string());
    }
    let (scheme, rest) = base.split_once("://")?;
    if let Some(net) = to.strip_prefix("//") {
        return Some(format!("{scheme}://{net}"));
    }
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let origin = format!("{scheme}://{}", &rest[..host_end]);
    if to.starts_with('/') {
        return Some(format!("{origin}{to}"));
    }
    if to.contains(':') {
        // data:, javascript: and the like.
        return None;
    }
    let path = rest[host_end..].split(['?', '#']).next().unwrap_or("");
    let folder = path.rsplit_once('/').map_or("", |(folder, _)| folder);
    Some(format!("{origin}{folder}/{to}"))
}

#[derive(Default, Debug, PartialEq)]
struct Tags {
    title: Option<String>,
    description: Option<String>,
    image: Option<String>,
}

/// The title, description and picture a page gives for its card, in the
/// order card makers look for them: Open Graph, then Twitter's, then plain HTML.
fn read_tags(html: &str) -> Tags {
    let head = match html.find("</head") {
        Some(end) => &html[..end],
        None => html,
    };
    let mut found: Vec<(String, String)> = Vec::new();
    let mut title = None;
    let lower = head.to_ascii_lowercase();
    let mut at = 0;
    while let Some(start) = lower[at..].find('<').map(|i| at + i) {
        let Some(end) = lower[start..].find('>').map(|i| start + i) else { break };
        let tag = &head[start + 1..end];
        let name = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("").to_ascii_lowercase();
        if name == "meta" {
            let attrs = attributes(tag);
            let key = attrs
                .iter()
                .find(|(k, _)| k == "property" || k == "name")
                .map(|(_, v)| v.to_ascii_lowercase());
            let content = attrs.iter().find(|(k, _)| k == "content").map(|(_, v)| v.clone());
            if let (Some(key), Some(content)) = (key, content) {
                found.push((key, content));
            }
        } else if name == "title" && title.is_none() {
            if let Some(close) = lower[end..].find("</title").map(|i| end + i) {
                title = Some(decode(&head[end + 1..close]));
            }
        } else if name == "script" || name == "style" {
            // Their text can hold anything, `<meta` included.
            if let Some(close) = lower[end..].find(&format!("</{name}")).map(|i| end + i) {
                at = close;
                continue;
            }
        }
        at = end + 1;
    }
    let first = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            found
                .iter()
                .find(|(k, v)| k == key && !v.trim().is_empty())
                .map(|(_, v)| clean(v))
        })
    };
    Tags {
        title: first(&["og:title", "twitter:title"]).or(title.map(|t| clean(&t)).filter(|t| !t.is_empty())),
        description: first(&["og:description", "twitter:description", "description"]),
        image: first(&["og:image:secure_url", "og:image", "og:image:url", "twitter:image", "twitter:image:src"]),
    }
}

/// Whitespace folded, as a card shows it.
fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A tag's attributes, names lowercased and values decoded.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = tag.char_indices().peekable();
    // Past the tag's name.
    while chars.next_if(|(_, c)| !c.is_whitespace()).is_some() {}
    loop {
        while chars.next_if(|(_, c)| c.is_whitespace() || *c == '/').is_some() {}
        let Some(&(start, _)) = chars.peek() else { break };
        let mut end = start;
        while let Some((i, c)) = chars.next_if(|(_, c)| !c.is_whitespace() && *c != '=' && *c != '/') {
            end = i + c.len_utf8();
        }
        let name = tag[start..end].to_ascii_lowercase();
        while chars.next_if(|(_, c)| c.is_whitespace()).is_some() {}
        if chars.next_if(|(_, c)| *c == '=').is_none() {
            if name.is_empty() {
                chars.next();
            }
            continue;
        }
        while chars.next_if(|(_, c)| c.is_whitespace()).is_some() {}
        let value = match chars.peek().map(|&(_, c)| c) {
            Some(quote @ ('"' | '\'')) => {
                chars.next();
                let mut value = String::new();
                for (_, c) in chars.by_ref() {
                    if c == quote {
                        break;
                    }
                    value.push(c);
                }
                value
            }
            _ => {
                let mut value = String::new();
                while let Some((_, c)) = chars.next_if(|(_, c)| !c.is_whitespace()) {
                    value.push(c);
                }
                value
            }
        };
        out.push((name, decode(&value)));
    }
    out
}

/// HTML's character references, the ones pages use in their tags.
fn decode(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let ch = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_open_graph_first() {
        let page = r#"<html><head><title>Plain &amp; simple</title>
            <meta name="description" content="plain description">
            <meta property="og:title" content="The &quot;real&quot; title" />
            <meta content='https://cdn.example.com/a.jpg' property='og:image'>
            <script>var s = '<meta property="og:title" content="fake">';</script>
            </head><body><meta property="og:description" content="too late"></body>"#;
        assert_eq!(
            read_tags(page),
            Tags {
                title: Some("The \"real\" title".into()),
                description: Some("plain description".into()),
                image: Some("https://cdn.example.com/a.jpg".into()),
            }
        );
    }

    #[test]
    fn falls_back_to_the_title() {
        let tags = read_tags("<head><TITLE>\n  Only   this\n</TITLE></head>");
        assert_eq!(tags.title.as_deref(), Some("Only this"));
        assert_eq!(tags.image, None);
    }

    #[test]
    fn resolves_relative_addresses() {
        let base = "https://example.com/a/b/page.html?x=1";
        assert_eq!(resolve(base, "/img.png").as_deref(), Some("https://example.com/img.png"));
        assert_eq!(resolve(base, "img.png").as_deref(), Some("https://example.com/a/b/img.png"));
        assert_eq!(resolve(base, "//cdn.example.com/i.png").as_deref(), Some("https://cdn.example.com/i.png"));
        assert_eq!(resolve(base, "data:image/png;base64,xx"), None);
    }

    #[test]
    fn keeps_to_the_public_internet() {
        assert!(public("http://127.0.0.1/").is_err());
        assert!(public("http://192.168.1.1:8080/admin").is_err());
        assert!(public("http://[::1]/").is_err());
        assert!(public("http://localhost/").is_err());
        assert!(public("file:///etc/passwd").is_err());
    }

    #[test]
    fn decodes_references() {
        assert_eq!(decode("a &amp; b &#39;c&#x27; &bogus; &"), "a & b 'c' &bogus; &");
    }

    /// Reads real pages: `cargo test live_cards -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_cards() {
        for url in [
            "https://github.com/tauri-apps/tauri",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://apps.apple.com/us/app/whatsapp-messenger/id310633997",
            "https://x.com/rustlang",
            "https://www.publico.pt/",
        ] {
            let (final_url, mut response) = get(&agent(), url, "text/html").unwrap();
            let mut page = Vec::new();
            response.body_mut().as_reader().take(MAX_PAGE).read_to_end(&mut page).unwrap();
            println!("{url} -> {final_url}\n  {:?}", read_tags(&String::from_utf8_lossy(&page)));
        }
    }
}
