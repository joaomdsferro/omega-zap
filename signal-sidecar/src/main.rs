//! Signal bridge for Omega Zap.
//!
//! Usage: `omega-zap-signal <database path>`. Commands arrive as JSON lines on
//! stdin, events leave as JSON lines on stdout. Closing stdin stops the bridge.

mod avatars;
mod history;
mod linking;

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures::{AsyncReadExt, StreamExt, pin_mut};
use presage::Manager;
use presage::libsignal_service::attachment_cipher::decrypt_in_place;
use presage::libsignal_service::configuration::SignalServers;
use presage::libsignal_service::content::{Content, ContentBody, DataMessage, GroupContextV2};
use presage::libsignal_service::proto::{AttachmentPointer, BodyRange, EditMessage, Preview, body_range};
use presage::libsignal_service::proto::attachment_pointer::AttachmentIdentifier;
use presage::libsignal_service::proto::data_message::{Delete, Quote as QuoteProto, Reaction};
use presage::libsignal_service::push_service::PushService;
use presage::libsignal_service::sender::AttachmentSpec;
use presage::libsignal_service::utils::parse_service_id_with_fallback;
use presage::libsignal_service::prelude::{ProfileKey, Uuid};
use presage::libsignal_service::proto::sync_message::{Content as SyncContent, Sent};
use presage::libsignal_service::proto::{ReceiptMessage, SyncMessage, receipt_message, sync_message};
use presage::libsignal_service::protocol::ServiceId;
use presage::libsignal_service::push_service::ServiceError;
use presage::manager::Registered;
use presage::model::identity::OnNewIdentity;
use presage::model::messages::Received;
use presage::store::{ContentExt, ContentsStore, StateStore, Thread};
use presage_store_sqlite::SqliteStore;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

type Signal = Manager<SqliteStore, Registered>;

const DEVICE_NAME: &str = "Omega Zap";
const USER_AGENT: &str = concat!("omega-zap-signal/", env!("CARGO_PKG_VERSION"));
const GROUP_PREFIX: &str = "g.";
const SEND_TIMEOUT: Duration = Duration::from_secs(45);
const MEDIA_TIMEOUT: Duration = Duration::from_secs(120);
/// Uploading a file of up to 100 MB.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(540);
const PROFILE_TIMEOUT: Duration = Duration::from_secs(10);
const RETRY_DELAY: Duration = Duration::from_secs(5);
const KEY_SYNC_ATTEMPTS: u32 = 5;
/// How long Signal's servers keep one linking code alive.
const QR_TTL_SECS: u64 = 90;
/// How long linking keeps replacing expired codes before it gives up.
const LINK_WINDOW: Duration = Duration::from_secs(10 * 60);

/// DataMessage flags that mark protocol traffic instead of a chat message.
const FLAG_END_SESSION: u32 = 1;
const FLAG_EXPIRATION_TIMER_UPDATE: u32 = 2;
const FLAG_PROFILE_KEY_UPDATE: u32 = 4;
const ATTACHMENT_FLAG_VOICE: u32 = 1;
const ATTACHMENT_FLAG_GIF: u32 = 8;
/// How far back the stored messages are offered to the app again when the
/// bridge starts, for any it missed while it was not listening.
const REPLAY_WINDOW: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Whether the account sends read receipts. Off until the phone's setting is
/// known, so nobody is told by mistake.
pub(crate) static READ_RECEIPTS: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
#[serde(rename_all_fields = "camelCase")]
enum Command {
    Link,
    Send {
        id: u64,
        chat: String,
        text: String,
        ts: u64,
        quote: Option<SendQuote>,
        #[serde(default)]
        preview: Option<SendPreview>,
        /// Formatting over `text`, which comes without markers.
        #[serde(default)]
        ranges: Vec<SendRange>,
    },
    /// Uploads the file at `path` and sends it. Answered with `uploaded`.
    SendFile {
        id: u64,
        chat: String,
        path: PathBuf,
        mime: String,
        file_name: Option<String>,
        /// `image`, `video`, `audio` or `file`.
        kind: String,
        width: Option<u32>,
        height: Option<u32>,
        caption: Option<String>,
        quote: Option<SendQuote>,
        ts: u64,
    },
    /// Takes back one of our messages for everyone.
    Delete { id: u64, chat: String, target_ts: u64, ts: u64 },
    /// Changes what one of our texts says, for everyone. `target_ts` is
    /// when the message was first sent, however often it was edited since.
    Edit {
        id: u64,
        chat: String,
        target_ts: u64,
        text: String,
        #[serde(default)]
        ranges: Vec<SendRange>,
        /// The quote the message was sent with, which an edit has to repeat.
        quote: Option<SendQuote>,
        ts: u64,
    },
    /// Sets or removes our reaction on a message.
    React {
        id: u64,
        chat: String,
        target_author: String,
        target_ts: u64,
        emoji: String,
        remove: bool,
        ts: u64,
    },
    /// Marks messages read on our other devices and, when the account sends
    /// read receipts, tells the people who wrote them.
    Read { id: u64, messages: Vec<ReadRef>, ts: u64 },
    /// Downloads an attachment and writes it, decrypted, to `path`.
    Media { id: u64, media: Media, path: PathBuf },
    /// Writes the picture of a chat, or our own, to `path` unless it is still
    /// the one the app `have`s. Answered with an `avatar` event.
    Avatar { id: u64, target: String, have: Option<String>, path: PathBuf },
}

/// Where an attachment lives, the key that decrypts it, and what it is.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Media {
    mime: String,
    cdn: u32,
    cdn_key: String,
    /// Hex.
    key: String,
    size: u32,
    /// `image`, `gif`, `video`, `voice`, `audio`, `sticker` or `file`.
    #[serde(default)]
    kind: String,
    #[serde(default)]
    file_name: Option<String>,
    /// Pixels, when the sender said.
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
}

/// The card for a link in a message we send. Its picture, a JPEG, waits at `path`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendPreview {
    url: String,
    title: Option<String>,
    description: Option<String>,
    path: Option<PathBuf>,
    width: Option<u32>,
    height: Option<u32>,
}

/// A link card as Signal's apps send it. A picture that fails to upload
/// leaves the card without one.
async fn link_preview(manager: &mut Signal, preview: SendPreview, ts: u64) -> Preview {
    let mut image = None;
    if let Some(bytes) = match &preview.path {
        Some(path) => tokio::fs::read(path).await.ok(),
        None => None,
    } {
        let spec = AttachmentSpec {
            content_type: "image/jpeg".into(),
            length: bytes.len(),
            width: preview.width,
            height: preview.height,
            ..Default::default()
        };
        match tokio::time::timeout(SEND_TIMEOUT, manager.upload_attachments(vec![(spec, bytes)])).await {
            Ok(Ok(mut uploaded)) => image = uploaded.pop().and_then(Result::ok),
            Ok(Err(e)) => eprintln!("the link picture was not uploaded: {e}"),
            Err(_) => eprintln!("the link picture took too long to upload"),
        }
    }
    Preview {
        url: Some(preview.url),
        title: preview.title,
        description: preview.description,
        image,
        date: Some(ts),
    }
}

/// The message a reply quotes.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendQuote {
    ts: u64,
    author: String,
    text: String,
}

/// A message as read receipts name it: who wrote it and when.
#[derive(Serialize, Deserialize)]
struct ReadRef {
    sender: String,
    ts: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Quote {
    author: Option<String>,
    /// When the quoted message was sent, which names it with its author.
    ts: Option<u64>,
    text: String,
}

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case", rename_all_fields = "camelCase")]
enum Event {
    /// `idle` (not linked), `qr_expired`, `connecting`, `connected`, `logged_out`
    /// or `error`.
    State { state: &'static str, error: Option<String> },
    /// A linking code, and how long the phone has to finish linking with it.
    Qr { url: String, ttl_secs: u64 },
    Me { id: String, phone: String, name: Option<String> },
    /// `saved` names come from the phone's contact list, the others from profiles.
    Contact { id: String, name: String, saved: bool },
    Group { id: String, name: String },
    Message {
        chat: String,
        is_group: bool,
        sender: String,
        from_me: bool,
        ts: u64,
        kind: &'static str,
        text: Option<String>,
        media_label: Option<String>,
        quote: Option<Quote>,
        /// In the order the sender attached them.
        attachments: Vec<Media>,
        /// From the imported history or the stored messages, not the live
        /// stream.
        history: bool,
        /// How far one of our own messages got, when the history says.
        delivery: Option<&'static str>,
    },
    /// Someone took back a message they sent.
    Delete { chat: String, sender: String, target_ts: u64, history: bool },
    /// Someone changed what a message they sent says. `target_ts` names the
    /// message as first sent, `ts` is when it was edited.
    Edit { chat: String, sender: String, target_ts: u64, ts: u64, text: Option<String>, history: bool },
    Reaction {
        chat: String,
        sender: String,
        target_author: String,
        target_ts: u64,
        emoji: String,
        remove: bool,
        history: bool,
    },
    /// Progress of the history import: `waiting` for the phone's upload,
    /// `importing`, `done` or `failed`.
    History { state: &'static str, percent: Option<u32>, error: Option<String> },
    Receipt { ids: Vec<String>, delivery: &'static str },
    /// Messages that were read on another of our devices.
    Read { messages: Vec<ReadRef> },
    /// Someone's profile key arrived, so their picture can be fetched now.
    ProfileKey { id: String },
    /// The answer to an `avatar` command: the id of the current picture.
    Avatar { id: u64, picture: Option<String>, error: Option<String> },
    /// The answer to `send_file`: where the file now is.
    Uploaded { id: u64, media: Option<Media>, error: Option<String> },
    /// The answer to any other command that carried this `id`.
    Done { id: u64, error: Option<String> },
}

fn emit(event: Event) {
    let Ok(line) = serde_json::to_string(&event) else { return };
    let mut out = std::io::stdout().lock();
    // A closed pipe means the app is gone.
    if writeln!(out, "{line}").and_then(|_| out.flush()).is_err() {
        std::process::exit(0);
    }
}

fn state(state: &'static str, error: Option<String>) {
    emit(Event::State { state, error });
}

fn chat_id(thread: &Thread) -> String {
    match thread {
        Thread::Contact(id) => id.raw_uuid().to_string(),
        Thread::Group(key) => format!("{GROUP_PREFIX}{}", hex::encode(key)),
    }
}

/// The chat a message belongs in. A message we sent from another device
/// can name its recipient only in the delivery list, and presage then files
/// it under the sender, us, as if it were a note to self.
fn thread_of(content: &Content) -> Option<Thread> {
    let thread = Thread::try_from(content).ok()?;
    let ContentBody::SynchronizeMessage(SyncMessage { content: Some(SyncContent::Sent(sent)), .. }) = &content.body
    else {
        return Some(thread);
    };
    if matches!(thread, Thread::Group(_)) || sent.parse_destination_service_id().is_some() {
        return Some(thread);
    }
    let [only] = sent.unidentified_status.as_slice() else { return None };
    let id = parse_service_id_with_fallback(
        only.destination_service_id_binary.as_deref(),
        only.destination_service_id.as_deref(),
    )?;
    Some(Thread::Contact(id))
}

/// A look over part of a message's text, as Signal sends it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Look {
    Bold,
    Italic,
    Strike,
    Mono,
}

impl Look {
    /// From the number the protocol gives each style; spoilers have no marker.
    pub(crate) fn from_style(style: i32) -> Option<Look> {
        match style {
            1 => Some(Look::Bold),
            2 => Some(Look::Italic),
            4 => Some(Look::Strike),
            5 => Some(Look::Mono),
            _ => None,
        }
    }
}

/// The looks of a live message's text: (start, length, look), in UTF-16 units.
fn looks(ranges: &[BodyRange]) -> Vec<(u32, u32, Look)> {
    ranges
        .iter()
        .filter_map(|range| match range.associated_value {
            Some(body_range::AssociatedValue::Style(style)) => {
                Some((range.start?, range.length?, Look::from_style(style)?))
            }
            _ => None,
        })
        .collect()
}

/// Who a live message mentions, in the order the mentions appear.
fn mentions(ranges: &[BodyRange]) -> Vec<String> {
    let mut found: Vec<(u32, String)> = ranges
        .iter()
        .filter_map(|range| {
            let aci = match &range.associated_value {
                Some(body_range::AssociatedValue::MentionAci(aci)) => aci.clone(),
                Some(body_range::AssociatedValue::MentionAciBinary(bytes)) => Uuid::from_slice(bytes).ok()?.to_string(),
                _ => return None,
            };
            Some((range.start?, aci))
        })
        .collect();
    found.sort_by_key(|(start, _)| *start);
    found.into_iter().map(|(_, aci)| aci).collect()
}

/// Signal writes each mention as U+FFFC and names the person in a range;
/// the app gets "@<ACI>" there, which it turns into the person's name.
pub(crate) fn with_mentions(text: &str, acis: &[String]) -> String {
    if acis.is_empty() {
        return text.to_string();
    }
    let mut acis = acis.iter();
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match (c == '\u{fffc}').then(|| acis.next()).flatten() {
            Some(aci) => {
                out.push('@');
                out.push_str(aci);
            }
            None => out.push(c),
        }
    }
    out
}

/// Signal's looks written into the text with WhatsApp's markers, which is
/// how the app keeps and shows formatting.
pub(crate) fn with_markers(text: &str, looks: &[(u32, u32, Look)]) -> String {
    if looks.is_empty() {
        return text.to_string();
    }
    // UTF-16 offsets to char positions.
    let mut unit_to_char = Vec::with_capacity(text.len() + 1);
    let chars: Vec<char> = text.chars().collect();
    for (index, c) in chars.iter().enumerate() {
        for _ in 0..c.len_utf16() {
            unit_to_char.push(index);
        }
    }
    unit_to_char.push(chars.len());
    let at = |unit: u32| unit_to_char.get(unit as usize).copied().unwrap_or(chars.len());
    // (opens at, closes at, marker), trimmed: a marker beside a space doesn't count.
    let mut spans: Vec<(usize, usize, &str)> = looks
        .iter()
        .filter_map(|&(start, length, look)| {
            let (mut from, mut to) = (at(start), at(start.saturating_add(length)));
            while from < to && chars[from].is_whitespace() {
                from += 1;
            }
            while to > from && chars[to - 1].is_whitespace() {
                to -= 1;
            }
            let marker = match look {
                Look::Bold => "*",
                Look::Italic => "_",
                Look::Strike => "~",
                Look::Mono if chars[from..to].contains(&'\n') => "```",
                Look::Mono => "`",
            };
            (from < to).then_some((from, to, marker))
        })
        .collect();
    // Outer ones first, so they open before and close after the ones inside.
    spans.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut out = String::with_capacity(text.len() + spans.len() * 2);
    for (index, c) in chars.iter().enumerate().chain(std::iter::once((chars.len(), &'\0'))) {
        for span in spans.iter().rev().filter(|s| s.1 == index) {
            out.push_str(span.2);
        }
        for span in spans.iter().filter(|s| s.0 == index) {
            out.push_str(span.2);
        }
        if index < chars.len() {
            out.push(*c);
        }
    }
    out
}

/// What the app sends: a look over part of the text, with markers taken out.
#[derive(Deserialize)]
struct SendRange {
    start: u32,
    length: u32,
    style: String,
}

fn body_ranges(ranges: Vec<SendRange>) -> Vec<BodyRange> {
    ranges
        .into_iter()
        .filter_map(|range| {
            let style = match range.style.as_str() {
                "bold" => body_range::Style::Bold,
                "italic" => body_range::Style::Italic,
                "strikethrough" => body_range::Style::Strikethrough,
                "monospace" => body_range::Style::Monospace,
                _ => return None,
            };
            Some(BodyRange {
                start: Some(range.start),
                length: Some(range.length),
                associated_value: Some(body_range::AssociatedValue::Style(style as i32)),
            })
        })
        .collect()
}

fn non_empty(text: &Option<String>) -> Option<String> {
    text.as_ref().filter(|t| !t.trim().is_empty()).cloned()
}

/// A UUID that a message names as a string or, from newer clients, as bytes.
fn aci(text: &Option<String>, binary: &Option<Vec<u8>>) -> Option<String> {
    text.clone()
        .or_else(|| Some(Uuid::from_slice(binary.as_deref()?).ok()?.to_string()))
}

/// What an attachment is, as the chat view draws it.
fn attachment_kind(mime: &str, voice: bool, gif: bool) -> &'static str {
    match mime.split('/').next().unwrap_or("") {
        _ if gif || mime == "image/gif" => "gif",
        "image" => "image",
        "audio" if voice => "voice",
        "audio" => "audio",
        "video" => "video",
        _ => "file",
    }
}

/// Kind, text and media label for a message, from the kind and file name of
/// each attachment.
fn summarize(
    text: Option<String>,
    attachments: &[(&'static str, Option<String>)],
) -> Option<(&'static str, Option<String>, Option<String>)> {
    let Some((kind, file_name)) = attachments.first() else {
        return text.map(|text| ("text", Some(text), None));
    };
    let photos = attachments.iter().filter(|(kind, _)| *kind == "image").count();
    Some(match *kind {
        _ if photos > 1 && photos == attachments.len() => ("image", text, Some(format!("{photos} photos"))),
        _ if attachments.len() > 1 => ("media", text, Some("Album".into())),
        "image" => ("image", text, Some("Photo".into())),
        "voice" => ("voice", text, None),
        "gif" => ("media", text, Some("GIF".into())),
        "video" => ("media", text, Some("Video".into())),
        "audio" => ("media", text, Some("Audio".into())),
        "sticker" => ("media", text, Some("Sticker".into())),
        _ => ("media", text, Some(file_name.clone().unwrap_or_else(|| "Document".into()))),
    })
}

/// A message's attachments with their kinds; a sticker counts as one.
fn pointers(message: &DataMessage) -> Vec<(&AttachmentPointer, &'static str)> {
    if let Some(sticker) = message.sticker.as_ref().and_then(|s| s.data.as_ref()) {
        return vec![(sticker, "sticker")];
    }
    message
        .attachments
        .iter()
        .map(|a| {
            let flags = a.flags.unwrap_or(0);
            let mime = a.content_type.as_deref().unwrap_or("");
            (a, attachment_kind(mime, flags & ATTACHMENT_FLAG_VOICE != 0, flags & ATTACHMENT_FLAG_GIF != 0))
        })
        .collect()
}

/// What the chat view shows for a message: kind, text and a media label.
fn describe(message: &DataMessage) -> Option<(&'static str, Option<String>, Option<String>)> {
    let protocol_flags = FLAG_END_SESSION | FLAG_EXPIRATION_TIMER_UPDATE | FLAG_PROFILE_KEY_UPDATE;
    if message.flags.unwrap_or(0) & protocol_flags != 0
        || message.delete.is_some()
    {
        return None;
    }
    if message.sticker.is_some() && message.sticker.as_ref().and_then(|s| s.data.as_ref()).is_none() {
        return Some(("media", non_empty(&message.body), Some("Sticker".into())));
    }
    let kinds: Vec<_> = pointers(message)
        .into_iter()
        .map(|(pointer, kind)| (kind, pointer.file_name.clone()))
        .collect();
    summarize(non_empty(&message.body), &kinds)
}

fn media_of(attachment: &AttachmentPointer, kind: &'static str) -> Option<Media> {
    let cdn_key = match attachment.attachment_identifier.as_ref()? {
        AttachmentIdentifier::CdnKey(key) => key.clone(),
        AttachmentIdentifier::CdnId(id) => id.to_string(),
    };
    Some(Media {
        mime: attachment.content_type.clone().unwrap_or_default(),
        cdn: attachment.cdn_number.unwrap_or(0),
        cdn_key,
        key: hex::encode(attachment.key.as_ref()?),
        size: attachment.size.unwrap_or(0),
        kind: kind.into(),
        file_name: attachment.file_name.clone().filter(|n| !n.trim().is_empty()),
        width: attachment.width.unwrap_or(0),
        height: attachment.height.unwrap_or(0),
    })
}

/// Downloads an attachment and decrypts it. The cipher authenticates the
/// file with the key from the message, so a tampered download is rejected.
async fn fetch_media(media: Media, path: PathBuf) -> Result<(), String> {
    let key: [u8; 64] = hex::decode(&media.key)
        .ok()
        .and_then(|k| k.try_into().ok())
        .ok_or("The attachment key is not valid.")?;
    let pointer = AttachmentPointer {
        cdn_number: Some(media.cdn),
        attachment_identifier: Some(AttachmentIdentifier::CdnKey(media.cdn_key)),
        ..Default::default()
    };
    let fetching = async {
        let mut service = PushService::new(SignalServers::Production, None, USER_AGENT);
        let mut bytes = Vec::new();
        let mut download = service.get_attachment(&pointer).await.map_err(|e| e.to_string())?;
        download.stream.read_to_end(&mut bytes).await.map_err(|e| e.to_string())?;
        decrypt_in_place(key, &mut bytes).map_err(|e| e.to_string())?;
        if media.size > 0 {
            bytes.truncate(media.size as usize);
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, bytes).map_err(|e| e.to_string())
    };
    tokio::time::timeout(MEDIA_TIMEOUT, fetching)
        .await
        .map_err(|_| "Signal did not answer in time.".to_string())?
}

struct Receiver {
    manager: Signal,
    me: Uuid,
    /// Replaying stored messages: what they report was already seen.
    history: bool,
    named: HashSet<Uuid>,
    groups: HashSet<[u8; 32]>,
    /// Senders whose profile key was looked at in this session.
    keyed: HashSet<Uuid>,
}

impl Receiver {
    async fn announce_contacts(&mut self) {
        let Ok(contacts) = self.manager.store().contacts().await else { return };
        for contact in contacts.flatten().filter(|c| !c.name.trim().is_empty()) {
            self.named.insert(contact.uuid);
            emit(Event::Contact { id: contact.uuid.to_string(), name: contact.name, saved: true });
        }
    }

    async fn announce_groups(&mut self) {
        let Ok(groups) = self.manager.store().groups().await else { return };
        let mut keys = Vec::new();
        for (key, group) in groups.flatten() {
            self.groups.insert(key);
            keys.extend(group.members.iter().map(|m| (Uuid::from(m.aci), m.profile_key)));
            emit(Event::Group { id: chat_id(&Thread::Group(key)), name: group.title });
        }
        self.fill_keys(keys).await;
    }

    /// Keeps the profile keys a group lists for its members, for the people
    /// who have not sent theirs in a message.
    async fn fill_keys(&self, keys: Vec<(Uuid, ProfileKey)>) {
        let mut store = self.manager.store().clone();
        for (uuid, key) in keys {
            if let Ok(None) = store.profile_key(&ServiceId::Aci(uuid.into())).await {
                let _ = store.upsert_profile_key(&uuid, key).await;
            }
        }
    }

    /// Keeps the profile key a message came with. presage does too, but only
    /// after a network round trip, and the app is told right away.
    async fn learn_key(&mut self, sender: &ServiceId, key: Option<&[u8]>) {
        let uuid = sender.raw_uuid();
        let Some(key) = key.and_then(|k| <[u8; 32]>::try_from(k).ok()).map(ProfileKey::create) else {
            return;
        };
        if !self.keyed.insert(uuid) {
            return;
        }
        let mut store = self.manager.store().clone();
        if !matches!(store.profile_key(sender).await, Ok(Some(known)) if known.bytes == key.bytes) {
            let _ = store.upsert_profile_key(&uuid, key).await;
        }
        emit(Event::ProfileKey { id: uuid.to_string() });
    }

    async fn announce_group(&mut self, key: [u8; 32]) {
        if self.groups.contains(&key) {
            return;
        }
        if let Ok(Some(group)) = self.manager.store().group(key).await {
            self.groups.insert(key);
            let keys = group.members.iter().map(|m| (Uuid::from(m.aci), m.profile_key)).collect();
            emit(Event::Group { id: chat_id(&Thread::Group(key)), name: group.title });
            self.fill_keys(keys).await;
        }
    }

    /// Finds a name for someone who is not in the phone's contact list.
    async fn announce_sender(&mut self, sender: &ServiceId, profile_key: Option<&[u8]>) {
        let uuid = sender.raw_uuid();
        if uuid == self.me || !self.named.insert(uuid) {
            return;
        }
        if let Ok(Some(contact)) = self.manager.store().contact_by_id(sender).await
            && !contact.name.trim().is_empty()
        {
            return emit(Event::Contact { id: uuid.to_string(), name: contact.name, saved: true });
        }
        let Some(key) = profile_key.and_then(|k| <[u8; 32]>::try_from(k).ok()) else {
            // Try again when a message brings the profile key.
            self.named.remove(&uuid);
            return;
        };
        let lookup = self.manager.retrieve_profile_by_uuid(uuid, ProfileKey::create(key));
        if let Ok(Ok(profile)) = tokio::time::timeout(PROFILE_TIMEOUT, lookup).await
            && let Some(name) = profile.name.map(|n| n.to_string()).filter(|n| !n.trim().is_empty())
        {
            emit(Event::Contact { id: uuid.to_string(), name, saved: false });
        }
    }

    /// Our own profile name, once the connection can fetch it.
    async fn announce_me(&mut self) {
        let lookup = self.manager.retrieve_profile();
        if let Ok(Ok(profile)) = tokio::time::timeout(PROFILE_TIMEOUT, lookup).await {
            emit(Event::Me {
                id: self.me.to_string(),
                phone: self.manager.registration_data().phone_number.to_string(),
                name: profile.name.map(|n| n.to_string()).filter(|n| !n.trim().is_empty()),
            });
        }
    }

    async fn data_message(&mut self, thread: &Thread, sender: ServiceId, from_me: bool, ts: u64, message: &DataMessage) {
        if let Some(reaction) = &message.reaction {
            let (Some(target_author), Some(target_ts), Some(emoji)) = (
                aci(&reaction.target_author_aci, &reaction.target_author_aci_binary),
                reaction.target_sent_timestamp,
                reaction.emoji.clone(),
            ) else {
                return;
            };
            return emit(Event::Reaction {
                chat: chat_id(thread),
                sender: sender.raw_uuid().to_string(),
                target_author,
                target_ts,
                emoji,
                remove: reaction.remove.unwrap_or(false),
                history: self.history,
            });
        }
        if let Some(target_ts) = message.delete.as_ref().and_then(|d| d.target_sent_timestamp) {
            return emit(Event::Delete {
                chat: chat_id(thread),
                sender: sender.raw_uuid().to_string(),
                target_ts,
                history: self.history,
            });
        }
        let Some((kind, text, media_label)) = describe(message) else { return };
        let text = text.map(|text| with_mentions(&with_markers(&text, &looks(&message.body_ranges)), &mentions(&message.body_ranges)));
        if let Thread::Group(key) = thread {
            self.announce_group(*key).await;
        }
        // A replay must not wait on profile lookups before the live stream.
        if !from_me && !self.history {
            self.announce_sender(&sender, message.profile_key.as_deref()).await;
        }
        let quote = message.quote.as_ref().map(|q| Quote {
            author: aci(&q.author_aci, &q.author_aci_binary),
            ts: q.id,
            text: non_empty(&q.text).unwrap_or_else(|| "Message".into()),
        });
        emit(Event::Message {
            chat: chat_id(thread),
            is_group: matches!(thread, Thread::Group(_)),
            sender: sender.raw_uuid().to_string(),
            from_me,
            ts,
            kind,
            text,
            media_label,
            quote,
            attachments: pointers(message).into_iter().filter_map(|(a, kind)| media_of(a, kind)).collect(),
            history: self.history,
            delivery: None,
        });
        // After the message, so the chat it belongs to exists by then.
        if !from_me && !self.history {
            self.learn_key(&sender, message.profile_key.as_deref()).await;
        }
    }

    /// An edit: the message as first sent, and what it says now. Edits
    /// carry only the new text; anything else in them is ignored.
    fn edit_message(&self, thread: &Thread, sender: ServiceId, edit: &EditMessage) {
        let (Some(target_ts), Some(message)) = (edit.target_sent_timestamp, edit.data_message.as_ref()) else {
            return;
        };
        if message.reaction.is_some() || message.delete.is_some() {
            return;
        }
        let Some(ts) = message.timestamp else { return };
        let text = non_empty(&message.body)
            .map(|text| with_mentions(&with_markers(&text, &looks(&message.body_ranges)), &mentions(&message.body_ranges)));
        emit(Event::Edit {
            chat: chat_id(thread),
            sender: sender.raw_uuid().to_string(),
            target_ts,
            ts,
            text,
            history: self.history,
        });
    }

    /// What our other devices report: the read receipt setting, and what was
    /// read there.
    fn synced(&self, sync: &SyncMessage) {
        if let Some(SyncContent::Configuration(configuration)) = &sync.content
            && let Some(on) = configuration.read_receipts
        {
            READ_RECEIPTS.store(on, Ordering::Relaxed);
        }
        let messages: Vec<ReadRef> = sync
            .read
            .iter()
            .filter_map(|read| {
                let sender = read.sender_aci.clone().or_else(|| {
                    Some(Uuid::from_slice(read.sender_aci_binary.as_deref()?).ok()?.to_string())
                })?;
                Some(ReadRef { sender, ts: read.timestamp? })
            })
            .collect();
        if !messages.is_empty() {
            emit(Event::Read { messages });
        }
    }

    /// `stored_in` is the chat presage filed a stored message under.
    async fn content(&mut self, content: &Content, stored_in: Option<&Thread>) {
        if let ContentBody::SynchronizeMessage(sync) = &content.body {
            self.synced(sync);
        }
        // A message we sent from here is stored as our own data message, which
        // names no recipient: only the chat it was filed under says who.
        let ours = matches!(content.body, ContentBody::DataMessage(_) | ContentBody::EditMessage(_))
            && content.metadata.sender.raw_uuid() == self.me;
        let thread = match stored_in {
            Some(thread) if ours => Some(thread.clone()),
            _ => thread_of(content),
        };
        let Some(thread) = thread else { return };
        let ts = content.timestamp();
        match &content.body {
            // Only stored messages we sent from here come as our own data messages.
            ContentBody::DataMessage(message) => {
                let sender = content.metadata.sender;
                let from_me = sender.raw_uuid() == self.me;
                self.data_message(&thread, sender, from_me, ts, message).await;
            }
            ContentBody::SynchronizeMessage(SyncMessage {
                content: Some(SyncContent::Sent(Sent { message: Some(message), .. })),
                ..
            }) => {
                self.data_message(&thread, content.metadata.sender, true, ts, message).await;
            }
            ContentBody::EditMessage(edit) => self.edit_message(&thread, content.metadata.sender, edit),
            ContentBody::SynchronizeMessage(SyncMessage {
                content: Some(SyncContent::Sent(Sent { edit_message: Some(edit), .. })),
                ..
            }) => self.edit_message(&thread, content.metadata.sender, edit),
            ContentBody::ReceiptMessage(ReceiptMessage { r#type, timestamp }) => {
                let delivery = match receipt_message::Type::try_from(r#type.unwrap_or_default()) {
                    Ok(receipt_message::Type::Delivery) => "delivered",
                    Ok(receipt_message::Type::Read | receipt_message::Type::Viewed) => "read",
                    Err(_) => return,
                };
                emit(Event::Receipt {
                    ids: timestamp.iter().map(u64::to_string).collect(),
                    delivery,
                });
            }
            _ => {}
        }
    }

    /// Offers the app the recent messages presage stored, for any it missed:
    /// one that arrives while the app is closing is stored and acknowledged
    /// here but never reaches it. The app skips those it already has.
    async fn replay(&mut self) {
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .saturating_sub(REPLAY_WINDOW)
            .as_millis() as u64;
        let mut threads = vec![Thread::Contact(ServiceId::Aci(self.me.into()))];
        if let Ok(contacts) = self.manager.store().contacts().await {
            threads.extend(contacts.flatten().map(|c| Thread::Contact(ServiceId::Aci(c.uuid.into()))));
        }
        if let Ok(groups) = self.manager.store().groups().await {
            threads.extend(groups.flatten().map(|(key, _)| Thread::Group(key)));
        }
        let mut stored = Vec::new();
        for thread in &threads {
            if let Ok(messages) = self.manager.store().messages(thread, since..).await {
                stored.extend(messages.flatten().map(|content| (thread.clone(), content)));
            }
        }
        stored.sort_by_key(|(_, content)| content.timestamp());
        self.history = true;
        for (thread, content) in &stored {
            self.content(content, Some(thread)).await;
        }
        self.history = false;
    }

    /// Runs until the connection drops. `Err` means the account was unlinked.
    async fn run(mut self) -> Result<(), ()> {
        self.announce_contacts().await;
        self.announce_groups().await;
        self.replay().await;

        let mut receiving = self.manager.clone();
        let messages = match receiving.receive_messages().await {
            Ok(messages) => messages,
            Err(presage::Error::ServiceError(ServiceError::Unauthorized)) => return Err(()),
            Err(e) => {
                state("error", Some(format!("Can't reach Signal: {e}")));
                return Ok(());
            }
        };
        pin_mut!(messages);

        let mut asked_for_contacts = !self.named.is_empty();
        let mut caught_up = false;
        while let Some(received) = messages.next().await {
            match received {
                Received::QueueEmpty => {
                    state("connected", None);
                    if !caught_up {
                        caught_up = true;
                        self.announce_me().await;
                    }
                    if !asked_for_contacts {
                        asked_for_contacts = true;
                        if let Err(e) = self.manager.request_contacts().await {
                            eprintln!("signal: could not request contacts: {e}");
                        }
                    }
                }
                Received::Contacts => self.announce_contacts().await,
                Received::Content(content) => self.content(&content, None).await,
                Received::DecryptionError(_) => {}
            }
        }
        state("connecting", None);
        Ok(())
    }
}

async fn send(manager: &mut Signal, chat: &str, message: DataMessage, ts: u64) -> Result<(), String> {
    send_as(manager, chat, message, ts, ContentBody::DataMessage).await
}

/// Sends `message` wrapped by `wrap`, as an edit wraps the message it
/// replaces.
async fn send_as(
    manager: &mut Signal,
    chat: &str,
    mut message: DataMessage,
    ts: u64,
    wrap: impl FnOnce(DataMessage) -> ContentBody,
) -> Result<(), String> {
    message.timestamp = Some(ts);
    let sending = async {
        if let Some(key) = chat.strip_prefix(GROUP_PREFIX) {
            let key: [u8; 32] = hex::decode(key)
                .ok()
                .and_then(|k| k.try_into().ok())
                .ok_or("Unknown group.")?;
            message.group_v2 = Some(GroupContextV2 {
                master_key: Some(key.to_vec()),
                revision: Some(0),
                ..Default::default()
            });
            manager.send_message_to_group(&key, wrap(message), ts).await.map_err(|e| e.to_string())
        } else {
            let uuid: Uuid = chat.parse().map_err(|_| "Unknown chat.")?;
            manager
                .send_message(ServiceId::Aci(uuid.into()), wrap(message), ts)
                .await
                .map_err(|e| e.to_string())
        }
    };
    tokio::time::timeout(SEND_TIMEOUT, sending)
        .await
        .map_err(|_| "Signal did not answer in time.".to_string())?
}

/// A file the app asked to send.
struct OutgoingFile {
    path: PathBuf,
    mime: String,
    file_name: Option<String>,
    kind: String,
    width: Option<u32>,
    height: Option<u32>,
    caption: Option<String>,
    quote: Option<SendQuote>,
}

/// Uploads a file and sends it as a message's one attachment, returning
/// where it now lives so the app can show it as received ones are.
async fn send_file(manager: &mut Signal, chat: &str, file: OutgoingFile, ts: u64) -> Result<Media, String> {
    let bytes = tokio::fs::read(&file.path).await.map_err(|e| format!("could not read the file: {e}"))?;
    let kind = match file.kind.as_str() {
        "image" => "image",
        "video" => "video",
        "audio" => "audio",
        _ => "file",
    };
    let spec = AttachmentSpec {
        content_type: file.mime,
        length: bytes.len(),
        file_name: file.file_name,
        width: file.width,
        height: file.height,
        ..Default::default()
    };
    let uploading = manager.upload_attachments(vec![(spec, bytes)]);
    let pointer = tokio::time::timeout(UPLOAD_TIMEOUT, uploading)
        .await
        .map_err(|_| "Signal did not take the file in time.".to_string())?
        .map_err(|e| e.to_string())?
        .pop()
        .ok_or("Nothing was uploaded.")?
        .map_err(|e| e.to_string())?;
    let media = media_of(&pointer, kind).ok_or("The upload came back without a reference.")?;
    let quote = file.quote.map(|q| QuoteProto {
        id: Some(q.ts),
        author_aci: Some(q.author),
        text: Some(q.text),
        ..Default::default()
    });
    let message = DataMessage {
        body: file.caption,
        attachments: vec![pointer],
        quote,
        ..Default::default()
    };
    send(manager, chat, message, ts).await?;
    Ok(media)
}

async fn mark_read(manager: &mut Signal, me: Uuid, messages: Vec<ReadRef>, ts: u64) -> Result<(), String> {
    let sending = async {
        let sync = SyncMessage {
            read: messages
                .iter()
                .map(|m| sync_message::Read {
                    sender_aci: Some(m.sender.clone()),
                    timestamp: Some(m.ts),
                    ..Default::default()
                })
                .collect(),
            ..SyncMessage::with_padding(&mut rand::rng())
        };
        manager
            .send_message(ServiceId::Aci(me.into()), sync, ts)
            .await
            .map_err(|e| e.to_string())?;
        if !READ_RECEIPTS.load(Ordering::Relaxed) {
            return Ok(());
        }
        let mut by_sender: HashMap<Uuid, Vec<u64>> = HashMap::new();
        for message in &messages {
            if let Ok(uuid) = message.sender.parse::<Uuid>()
                && uuid != me
            {
                by_sender.entry(uuid).or_default().push(message.ts);
            }
        }
        for (uuid, timestamp) in by_sender {
            let receipt = ReceiptMessage {
                r#type: Some(receipt_message::Type::Read as i32),
                timestamp,
            };
            manager
                .send_message(ServiceId::Aci(uuid.into()), receipt, ts)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    tokio::time::timeout(SEND_TIMEOUT, sending)
        .await
        .map_err(|_| "Signal did not answer in time.".to_string())?
}

/// Links as a new device. Signal drops a linking code that the phone has not
/// confirmed in time, so a fresh one replaces it until the window closes.
/// `Err(None)` means the window closed.
async fn link(store: &SqliteStore) -> Result<linking::Linked, Option<String>> {
    let deadline = Instant::now() + LINK_WINDOW;
    loop {
        match linking::link_once(store.clone()).await {
            Ok(linked) => return Ok(linked),
            Err(linking::LinkError::Expired) => {
                if Instant::now() >= deadline {
                    return Err(None);
                }
            }
            Err(linking::LinkError::Failed(e)) => return Err(Some(e)),
        }
    }
}

/// Serves one linked session: receives in the background, sends on request.
async fn session(
    manager: Signal,
    history: Option<(history::HistoryAccess, &Path)>,
    commands: &mut mpsc::UnboundedReceiver<Command>,
) -> Result<(), ()> {
    state("connecting", None);
    let data = manager.registration_data();
    let me = data.service_ids.aci;
    emit(Event::Me { id: me.to_string(), phone: data.phone_number.to_string(), name: None });
    // Before the live stream, so older messages land first.
    if let Some((access, dir)) = history {
        history::import(access, dir, manager.store().clone()).await;
    }

    let service = avatars::service(data);
    if let Some(service) = service.clone() {
        let store = manager.store().clone();
        tokio::task::spawn_local(async move {
            // The network is often not up yet when the app starts.
            for attempt in 1..=KEY_SYNC_ATTEMPTS {
                match avatars::sync_keys(store.clone(), service.clone()).await {
                    Ok(()) => return,
                    Err(e) => eprintln!("signal: could not read the profile keys (try {attempt}): {e}"),
                }
                tokio::time::sleep(RETRY_DELAY * attempt).await;
            }
        });
    }
    let receiver = Receiver {
        manager: manager.clone(),
        me,
        history: false,
        named: HashSet::new(),
        groups: HashSet::new(),
        keyed: HashSet::new(),
    };
    let mut receiving = tokio::task::spawn_local(receiver.run());
    let mut sender = manager;

    loop {
        tokio::select! {
            ended = &mut receiving => return ended.unwrap_or(Ok(())),
            command = commands.recv() => match command {
                Some(Command::Send { id, chat, text, ts, quote, preview, ranges }) => {
                    let quote = quote.map(|q| QuoteProto {
                        id: Some(q.ts),
                        author_aci: Some(q.author),
                        text: Some(q.text),
                        ..Default::default()
                    });
                    let preview = match preview {
                        Some(preview) => vec![link_preview(&mut sender, preview, ts).await],
                        None => Vec::new(),
                    };
                    let body_ranges = body_ranges(ranges);
                    let message = DataMessage { body: Some(text), quote, preview, body_ranges, ..Default::default() };
                    let error = send(&mut sender, &chat, message, ts).await.err();
                    emit(Event::Done { id, error });
                }
                // On its own task: an upload can take minutes, and the
                // commands behind it should not wait for it.
                Some(Command::SendFile { id, chat, path, mime, file_name, kind, width, height, caption, quote, ts }) => {
                    let mut manager = sender.clone();
                    tokio::task::spawn_local(async move {
                        let file = OutgoingFile { path, mime, file_name, kind, width, height, caption, quote };
                        let (media, error) = match send_file(&mut manager, &chat, file, ts).await {
                            Ok(media) => (Some(media), None),
                            Err(e) => (None, Some(e)),
                        };
                        emit(Event::Uploaded { id, media, error });
                    });
                }
                Some(Command::Delete { id, chat, target_ts, ts }) => {
                    let message = DataMessage {
                        delete: Some(Delete { target_sent_timestamp: Some(target_ts) }),
                        ..Default::default()
                    };
                    let error = send(&mut sender, &chat, message, ts).await.err();
                    emit(Event::Done { id, error });
                }
                Some(Command::Edit { id, chat, target_ts, text, ranges, quote, ts }) => {
                    let quote = quote.map(|q| QuoteProto {
                        id: Some(q.ts),
                        author_aci: Some(q.author),
                        text: Some(q.text),
                        ..Default::default()
                    });
                    // The whole message again, as it now is, naming the one first sent.
                    let message = DataMessage { body: Some(text), body_ranges: body_ranges(ranges), quote, ..Default::default() };
                    let wrap = |message| {
                        ContentBody::EditMessage(EditMessage { target_sent_timestamp: Some(target_ts), data_message: Some(message) })
                    };
                    let error = send_as(&mut sender, &chat, message, ts, wrap).await.err();
                    emit(Event::Done { id, error });
                }
                Some(Command::React { id, chat, target_author, target_ts, emoji, remove, ts }) => {
                    let message = DataMessage {
                        reaction: Some(Reaction {
                            emoji: Some(emoji),
                            remove: Some(remove),
                            target_author_aci: Some(target_author),
                            target_sent_timestamp: Some(target_ts),
                            ..Default::default()
                        }),
                        ..Default::default()
                    };
                    let error = send(&mut sender, &chat, message, ts).await.err();
                    emit(Event::Done { id, error });
                }
                // On its own task: a busy group has many people to tell.
                Some(Command::Read { id, messages, ts }) => {
                    let mut manager = sender.clone();
                    tokio::task::spawn_local(async move {
                        let error = mark_read(&mut manager, me, messages, ts).await.err();
                        emit(Event::Done { id, error });
                    });
                }
                // On its own task: a large download must not hold up sending.
                Some(Command::Media { id, media, path }) => {
                    tokio::task::spawn_local(async move {
                        let error = fetch_media(media, path).await.err();
                        emit(Event::Done { id, error });
                    });
                }
                Some(Command::Avatar { id, target, have, path }) => {
                    let (mut manager, service) = (sender.clone(), service.clone());
                    tokio::task::spawn_local(async move {
                        let fetched = match &service {
                            Some(service) => {
                                avatars::fetch(&mut manager, service, &target, have.as_deref(), &path).await
                            }
                            None => Err("The Signal session could not be read.".into()),
                        };
                        let (picture, error) = match fetched {
                            Ok(picture) => (picture, None),
                            Err(e) => (None, Some(e)),
                        };
                        emit(Event::Avatar { id, picture, error });
                    });
                }
                Some(Command::Link) => {}
                None => std::process::exit(0),
            },
        }
    }
}

async fn run(path: String) {
    // The app keeps the store encrypted (SQLCipher) under a key it passes here.
    let key = std::env::var("OMEGA_ZAP_DB_KEY").ok();
    let store = match SqliteStore::open_with_passphrase(&path, key.as_deref(), OnNewIdentity::Trust).await {
        Ok(store) => store,
        Err(e) => return state("error", Some(format!("Could not open the Signal store: {e}"))),
    };

    let (tx, mut commands) = mpsc::unbounded_channel();
    tokio::task::spawn_local(async move {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            match serde_json::from_str::<Command>(&line) {
                Ok(command) => {
                    if tx.send(command).is_err() {
                        break;
                    }
                }
                Err(e) => eprintln!("signal: ignored command: {e}"),
            }
        }
        // Dropping `tx` tells the main loop the app closed the pipe.
    });

    let dir = PathBuf::from(&path).parent().map(Path::to_path_buf).unwrap_or_default();
    loop {
        let (manager, history) = if store.is_registered().await {
            match Manager::load_registered(store.clone()).await {
                Ok(manager) => (manager, None),
                Err(e) => return state("error", Some(format!("Could not load the Signal session: {e}"))),
            }
        } else {
            state("idle", None);
            loop {
                match commands.recv().await {
                    Some(Command::Link) => break,
                    Some(
                        Command::Send { id, .. }
                        | Command::SendFile { id, .. }
                        | Command::Delete { id, .. }
                        | Command::Edit { id, .. }
                        | Command::React { id, .. }
                        | Command::Read { id, .. }
                        | Command::Media { id, .. }
                        | Command::Avatar { id, .. },
                    ) => emit(Event::Done { id, error: Some("Signal is not linked.".into()) }),
                    None => return,
                }
            }
            match link(&store).await {
                Ok(linked) => (linked.manager, linked.history),
                Err(Some(e)) => {
                    state("error", Some(format!("Linking failed: {e}")));
                    continue;
                }
                Err(None) => {
                    state("qr_expired", None);
                    continue;
                }
            }
        };

        let history = history.map(|access| (access, dir.as_path()));
        if session(manager, history, &mut commands).await.is_err() {
            return state("logged_out", Some("This computer was unlinked from Signal.".into()));
        }
        tokio::time::sleep(RETRY_DELAY).await;
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: omega-zap-signal <database path>");
        std::process::exit(2);
    };
    tokio::task::LocalSet::new().run_until(run(path)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn albums_and_files_are_summarized() {
        let photos = [("image", None), ("image", None)];
        assert_eq!(summarize(Some("hi".into()), &photos), Some(("image", Some("hi".into()), Some("2 photos".into()))));
        let mixed = [("image", None), ("video", None)];
        assert_eq!(summarize(None, &mixed).unwrap().2.as_deref(), Some("Album"));
        let file = [("file", Some("notes.pdf".to_string()))];
        assert_eq!(summarize(None, &file).unwrap().2.as_deref(), Some("notes.pdf"));
        assert_eq!(summarize(None, &[("voice", None)]).unwrap().0, "voice");
        assert_eq!(summarize(Some("hi".into()), &[]), Some(("text", Some("hi".into()), None)));
        assert_eq!(attachment_kind("video/mp4", false, true), "gif");
        assert_eq!(attachment_kind("audio/aac", true, false), "voice");
    }

    #[test]
    fn authors_are_read_from_either_field() {
        let uuid = Uuid::from_bytes([7; 16]);
        assert_eq!(aci(&None, &Some(uuid.as_bytes().to_vec())), Some(uuid.to_string()));
        assert_eq!(aci(&Some("x".into()), &None).as_deref(), Some("x"));
        assert_eq!(aci(&None, &Some(vec![1, 2])), None);
    }
}

#[cfg(test)]
mod marker_tests {
    use super::*;

    #[test]
    fn writes_looks_as_markers() {
        assert_eq!(with_markers("hi bold both x", &[(3, 9, Look::Bold), (8, 4, Look::Italic), (13, 1, Look::Strike)]), "hi *bold _both_* ~x~");
        // A look that starts or ends on a space keeps its marker beside the word.
        assert_eq!(with_markers("a big one", &[(1, 5, Look::Bold)]), "a *big* one");
        assert_eq!(with_markers("x = 1", &[(0, 5, Look::Mono)]), "`x = 1`");
        // Offsets count UTF-16 units: the emoji takes two.
        assert_eq!(with_markers("😅 ok", &[(3, 2, Look::Italic)]), "😅 _ok_");
    }
}
