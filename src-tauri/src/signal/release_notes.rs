//! The "Signal" chat. Signal sends no messages for it: each of its apps reads
//! the release notes Signal publishes and writes them into the chat itself.
//! We take the ones meant for the desktop app.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::store::{Content, MediaRef, MsgKind};

/// The chat's id: not a Signal account, so never sent to the bridge.
pub const CHAT_ID: &str = "signal-release-notes";
pub const CHAT_NAME: &str = "Signal";
/// The notes already shown, so a cleared or deleted chat stays that way.
pub const SEEN_FILE: &str = "release-notes.json";
/// How often the list is read again.
pub const CHECK_EVERY_MS: i64 = 12 * 60 * 60 * 1000;

const HOST: &str = "https://updates.signal.org";
const MANIFEST: &str = "/dynamic/release-notes/release-notes-v2.json";
/// Where a note's pictures live; anything else is refused.
const MEDIA_PREFIX: &str = "/static/release-notes/";
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_IMAGE: u64 = 10 * 1024 * 1024;

#[derive(Deserialize)]
struct Manifest {
    announcements: Vec<Announcement>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Announcement {
    uuid: String,
    /// Only the notes for the desktop app have one.
    desktop_min_version: Option<String>,
    /// `<calling code>:<parts per million>,…`: who gets it.
    countries: Option<String>,
    link: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Note {
    title: Option<String>,
    body: Option<String>,
    media: Option<String>,
    media_content_type: Option<String>,
    media_width: Option<String>,
    media_height: Option<String>,
}

/// A note to add to the chat.
pub struct ReleaseNote {
    pub id: String,
    pub content: Content,
}

pub fn load_seen(dir: &Path) -> HashSet<String> {
    fs::read(dir.join(SEEN_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_seen(dir: &Path, seen: &HashSet<String>) {
    let mut ids: Vec<&String> = seen.iter().collect();
    ids.sort();
    if let Err(e) = serde_json::to_vec(&ids).map(|json| fs::write(dir.join(SEEN_FILE), json)) {
        log::warn!("signal: could not remember the release notes: {e}");
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).build().new_agent()
}

fn get_text(agent: &ureq::Agent, path: &str) -> Result<Option<String>, String> {
    let mut response = match agent.get(format!("{HOST}{path}")).call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    response.body_mut().read_to_string().map(Some).map_err(|e| e.to_string())
}

/// The notes not seen yet that are meant for this account, in the order
/// Signal lists them. Blocks: run it off the async threads.
pub fn fetch_new(seen: &HashSet<String>, own_id: &str, phone: &str, locales: &[String]) -> Result<Vec<ReleaseNote>, String> {
    let agent = agent();
    let manifest = get_text(&agent, MANIFEST)?.ok_or("Signal has no release notes list")?;
    let manifest: Manifest = serde_json::from_str(&manifest).map_err(|e| e.to_string())?;
    let mut notes = Vec::new();
    for a in manifest.announcements {
        let id = a.uuid.to_ascii_lowercase();
        if a.desktop_min_version.is_none() || seen.contains(&id) || !for_us(a.countries.as_deref(), &id, own_id, phone) {
            continue;
        }
        let mut note = None;
        for locale in locales {
            if let Some(text) = get_text(&agent, &format!("{MEDIA_PREFIX}{}/{locale}.json", a.uuid))? {
                note = Some(serde_json::from_str::<Note>(&text).map_err(|e| e.to_string())?);
                break;
            }
        }
        let Some(note) = note else { continue };
        notes.push(ReleaseNote { id, content: content(note, a.link) });
    }
    Ok(notes)
}

fn content(note: Note, link: Option<String>) -> Content {
    let title = note.title.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let body = note.body.as_deref().map(str::trim).filter(|b| !b.is_empty());
    let link = link.as_deref().map(str::trim).filter(|l| l.starts_with("https://"));
    let text = [title.map(|t| format!("*{t}*")), body.map(str::to_string), link.map(str::to_string)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n");
    let mime = note.media_content_type.unwrap_or_else(|| "image/png".into());
    // Their videos are short silent loops, played like a GIF.
    let (kind, msg_kind, label) = match mime.split('/').next() {
        Some("video") => ("gif", MsgKind::Media, "Video"),
        _ => ("image", MsgKind::Image, "Photo"),
    };
    let picture = note.media.filter(|m| m.starts_with(MEDIA_PREFIX) && !m.contains("..")).map(|path| MediaRef {
        mime,
        cdn: 0,
        cdn_key: path,
        key: String::new(),
        size: 0,
        kind: kind.into(),
        file_name: None,
        width: note.media_width.and_then(|w| w.parse().ok()).unwrap_or(0),
        height: note.media_height.and_then(|h| h.parse().ok()).unwrap_or(0),
    });
    let mut content = Content::text(text, None);
    if let Some(picture) = picture {
        content.kind = msg_kind;
        content.media_label = Some(label.into());
        content.attachments.push(picture);
    }
    content
}

/// Whether an attachment is one of these notes', fetched with [`fetch_image`]
/// rather than from Signal's attachment servers.
pub fn is_note_image(media: &MediaRef) -> bool {
    media.key.is_empty() && media.cdn_key.starts_with(MEDIA_PREFIX)
}

/// Downloads a note's picture or video. Blocks.
pub fn fetch_image(media: &MediaRef) -> Result<Vec<u8>, String> {
    if !is_note_image(media) || media.cdn_key.contains("..") {
        return Err("Not a release note's picture.".into());
    }
    agent()
        .get(format!("{HOST}{}", media.cdn_key))
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .with_config()
        .limit(MAX_IMAGE)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

/// Whether a note limited to some countries reaches this account. Signal
/// gives each country a share in parts per million; the account's place in
/// that share stays the same for a note.
fn for_us(countries: Option<&str>, uuid: &str, own_id: &str, phone: &str) -> bool {
    let Some(countries) = countries.filter(|c| !c.trim().is_empty()) else { return true };
    let digits: String = phone.chars().filter(char::is_ascii_digit).collect();
    let mut share = None;
    let mut fallback = None;
    for entry in countries.split(',') {
        let Some((code, ppm)) = entry.trim().split_once(':') else { continue };
        let Ok(ppm) = ppm.trim().parse::<u64>() else { continue };
        let code = code.trim();
        if code == "*" {
            fallback = Some(ppm);
        } else if !code.is_empty() && digits.starts_with(code) && share.is_none_or(|(len, _)| code.len() > len) {
            share = Some((code.len(), ppm));
        }
    }
    let ppm = share.map(|(_, ppm)| ppm).or(fallback).unwrap_or(0);
    ppm >= 1_000_000 || ppm > 0 && bucket(uuid, own_id) < ppm
}

/// FNV-1a over the note and the account, in parts per million.
fn bucket(uuid: &str, own_id: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in uuid.bytes().chain(own_id.bytes()) {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash % 1_000_000
}

/// The languages to ask for, most specific first, as Signal names them
/// (`pt_PT`, then `pt`), ending in English.
pub fn locales() -> Vec<String> {
    let raw = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .unwrap_or_default();
    let tag = raw.split(['.', '@']).next().unwrap_or_default().replace('-', "_");
    let mut out = Vec::new();
    if tag.contains('_') {
        out.push(tag.clone());
    }
    if let Some(lang) = tag.split('_').next().filter(|l| !l.is_empty()) {
        out.push(lang.to_ascii_lowercase());
    }
    if !out.iter().any(|l| l == "en") {
        out.push("en".into());
    }
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countries_pick_the_longest_calling_code() {
        assert!(for_us(None, "a", "b", "+351900000000"));
        assert!(for_us(Some("351:1000000,1:1000000"), "a", "b", "+351900000000"));
        assert!(!for_us(Some("35:1000000,351:0"), "a", "b", "+351900000000"));
        assert!(!for_us(Some("44:1000000"), "a", "b", "+351900000000"));
        assert!(for_us(Some("44:0,*:1000000"), "a", "b", "+351900000000"));
    }

    #[test]
    fn a_note_reads_as_a_message() {
        let note: Note = serde_json::from_str(
            r#"{"mediaHeight":"884","mediaWidth":"1560","media":"/static/release-notes/backups.png","mediaContentType":"image/png","title":"Backups","body":"Now you can. ","callToActionText":"Enable"}"#,
        )
        .unwrap();
        let message = content(note, None);
        assert_eq!(message.text.as_deref(), Some("*Backups*\n\nNow you can."));
        assert!(matches!(message.kind, MsgKind::Image));
        assert_eq!(message.attachments[0].width, 1560);
        assert!(is_note_image(&message.attachments[0]));

        let video: Note = serde_json::from_str(r#"{"media":"/static/release-notes/water-plant.mp4","mediaContentType":"video/mp4","title":"Nonprofit"}"#).unwrap();
        let video = content(video, None);
        assert!(matches!(video.kind, MsgKind::Media));
        assert_eq!(video.attachments[0].kind, "gif");
    }
}
