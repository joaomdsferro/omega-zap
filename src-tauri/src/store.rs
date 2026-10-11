use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

mod history;
pub use history::History;

/// Messages kept in memory per chat; the history database has the rest.
const MAX_MESSAGES_PER_CHAT: usize = 400;
const STORE_VERSION: u32 = 1;
const AVATAR_TTL_MS: i64 = 24 * 60 * 60 * 1000;
const MEMBERS_TTL_MS: i64 = 6 * 60 * 60 * 1000;
/// What our own profile picture is filed under, in place of a chat id.
pub const OWN_AVATAR: &str = "me";
/// Most messages a search returns, per account.
pub const SEARCH_LIMIT: usize = 200;
/// Messages read from the history database per scroll back.
pub const OLDER_PAGE: usize = 100;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum MsgKind {
    Text,
    Image,
    Voice,
    Media,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Delivery {
    Sent,
    Delivered,
    Read,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReplyRef {
    pub author_jid: Option<String>,
    /// The quoted message's id here, when the network named it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub text: String,
}

/// Where an attachment can be downloaded from, and the key that decrypts it.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaRef {
    pub mime: String,
    pub cdn: u32,
    #[serde(alias = "cdnKey")]
    pub cdn_key: String,
    /// Hex.
    pub key: String,
    pub size: u32,
    /// `image`, `gif`, `video`, `voice`, `audio`, `sticker` or `file`; empty
    /// for attachments stored before kinds were kept.
    #[serde(default)]
    pub kind: String,
    #[serde(default, alias = "fileName", skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// Pixels, when the sender said.
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

impl MediaRef {
    pub fn kind(&self) -> &str {
        match (self.kind.as_str(), self.mime.split('/').next().unwrap_or("")) {
            ("", _) if self.mime == "image/gif" => "gif",
            ("", "image") => "image",
            ("", "video") => "video",
            ("", "audio") => "audio",
            ("", _) => "file",
            (kind, _) => kind,
        }
    }
}

/// The card a network attaches to a message for the link in it.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LinkPreview {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// A small JPEG as a `data:` URL.
    pub thumbnail: Option<String>,
    /// Where the full-size picture can be downloaded from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub large: Option<CdnRef>,
}

/// An encrypted file on WhatsApp's servers. The byte fields are base64.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CdnRef {
    pub path: String,
    pub key: String,
    pub sha256: String,
    pub enc_sha256: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    /// Pixels, when the sender said.
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// `gif`, `video`, `voice`, `audio`, `sticker` or `file`, as
    /// [`MediaRef::kind`] names them; `None` for a photo, which is all that
    /// was kept before the other kinds were.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

impl CdnRef {
    /// Shown as an attachment rather than as the message's photo.
    pub fn attachment(&self) -> Option<AttachmentDto> {
        let kind = self.kind.clone()?;
        Some(AttachmentDto {
            kind,
            mime: self.mime.clone().unwrap_or_default(),
            file_name: self.file_name.clone(),
            size: u32::try_from(self.size).unwrap_or(u32::MAX),
            width: Some(self.width).filter(|w| *w > 0),
            height: Some(self.height).filter(|h| *h > 0),
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Reaction {
    pub author: String,
    pub emoji: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Content {
    pub kind: MsgKind,
    pub text: Option<String>,
    pub media_label: Option<String>,
    pub voice_seconds: Option<u32>,
    pub reply_to: Option<ReplyRef>,
    /// Signal's attachments, in the order they were sent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<MediaRef>,
    /// The one Signal attachment kept before there were several; read from
    /// older files and moved into `attachments`.
    #[serde(default, skip_serializing)]
    pub media: Option<MediaRef>,
    /// A WhatsApp message's file; `attachments` are Signal's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wa_media: Option<CdnRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkPreview>,
    /// How many times it was forwarded on the way here (WhatsApp's
    /// forwarding score); 0 when it was written by its sender.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub forwarded: u32,
}

/// What search looks at in a message: its text and, for a file, its name.
/// `None` for a photo, video or voice message without a caption.
fn search_text(m: &StoredMessage) -> Option<String> {
    let has_file = m.content.wa_media.is_some() || !m.content.attachments.is_empty();
    match (&m.content.text, &m.content.media_label) {
        (Some(text), Some(label)) if has_file => Some(format!("{text} {label}")),
        (Some(text), _) => Some(text.clone()),
        (None, Some(label)) if !matches!(label.as_str(), "Photo" | "Video" | "Voice message" | "GIF" | "Sticker" | "Audio") => Some(label.clone()),
        _ => None,
    }
}

/// The words of a search, folded.
pub fn search_words(query: &str) -> Vec<String> {
    fold(query).split_whitespace().map(str::to_string).collect()
}

/// Text as search compares it: lower case, without accents.
pub fn fold(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).map(unaccented).collect()
}

/// The letter under a Latin accent, as in "ã" → "a"; anything else as it is.
fn unaccented(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'ď' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' => 'i',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ý' | 'ÿ' => 'y',
        'ś' | 'š' | 'ş' => 's',
        'ţ' | 'ť' => 't',
        'ź' | 'ż' | 'ž' => 'z',
        _ => c,
    }
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Content {
    pub fn text(text: impl Into<String>, reply_to: Option<ReplyRef>) -> Self {
        Self {
            kind: MsgKind::Text,
            text: Some(text.into()),
            media_label: None,
            voice_seconds: None,
            reply_to,
            attachments: Vec::new(),
            media: None,
            wa_media: None,
            link: None,
            forwarded: 0,
        }
    }

    pub fn media(kind: MsgKind, label: impl Into<String>, caption: Option<String>) -> Self {
        Self {
            kind,
            text: caption.filter(|c| !c.trim().is_empty()),
            media_label: Some(label.into()),
            voice_seconds: None,
            reply_to: None,
            attachments: Vec::new(),
            media: None,
            wa_media: None,
            link: None,
            forwarded: 0,
        }
    }

    /// A WhatsApp photo or file stored before its download reference was
    /// kept, so it cannot be fetched until history sends the message again.
    pub fn lacks_wa_file(&self) -> bool {
        const NO_FILE: [&str; 5] = ["Location", "Live location", "Contact", "Contacts", "Poll"];
        self.wa_media.is_none()
            && self.attachments.is_empty()
            && match self.kind {
                MsgKind::Image | MsgKind::Voice => true,
                MsgKind::Media => self.media_label.as_deref().is_some_and(|l| !NO_FILE.contains(&l)),
                MsgKind::Text => false,
            }
    }

    /// One-line summary used for chat previews and quotes.
    pub fn preview(&self) -> String {
        match (self.kind, &self.text, &self.media_label) {
            (MsgKind::Text, Some(text), _) => text.clone(),
            (MsgKind::Voice, _, _) => "Voice message".into(),
            (_, Some(caption), Some(label)) => format!("{label}: {caption}"),
            (_, None, Some(label)) => label.clone(),
            (_, Some(text), None) => text.clone(),
            _ => String::new(),
        }
    }
}

/// The ids a message arrived under, when they are not the ones it is filed
/// under. A read receipt has to name these.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WireIds {
    pub chat: String,
    pub sender: Option<String>,
}

/// An incoming message that was just read, as a read receipt names it.
pub struct ReadTarget {
    pub id: String,
    pub chat: String,
    pub sender: Option<String>,
    pub ts_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StoredMessage {
    pub id: String,
    pub from_me: bool,
    pub ts_ms: i64,
    pub author_jid: Option<String>,
    pub content: Content,
    pub status: Option<Delivery>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    /// Filled in when the message is stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire: Option<WireIds>,
    /// Who has read a message of ours in a group, until everyone has.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_by: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub starred: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted: Option<Deletion>,
}

/// How a message was deleted. Kept rather than dropped, so a history replay
/// does not bring it back.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Deletion {
    /// Gone from this account only: not shown at all.
    ForMe,
    /// Taken back by its sender: shown as a placeholder.
    ForEveryone,
}

/// What a reply needs to quote a message.
pub struct QuoteSource {
    /// The sender, as the message arrived; `None` for our own.
    pub sender: Option<String>,
    pub from_me: bool,
    pub ts_ms: i64,
    pub text: String,
}

/// What is needed to react to a message.
pub struct ReactionTarget {
    pub author: Option<String>,
    pub ts_ms: i64,
    pub from_me: bool,
    /// The reaction we already have on it.
    pub mine: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
struct StoredChat {
    name: Option<String>,
    is_group: bool,
    pinned_at: Option<i64>,
    archived: bool,
    muted_until_ms: Option<i64>,
    unread: u32,
    last_ts_ms: i64,
    messages: Vec<StoredMessage>,
    /// Server id of the profile picture saved on disk, if the chat has one.
    avatar_id: Option<String>,
    avatar_checked_ms: i64,
    /// How many people are in the group, us included, once looked up.
    members: Option<u32>,
    members_checked_ms: i64,
}

/// A group message counts as read once everyone else has read it.
fn everyone_read(message: &StoredMessage, members: Option<u32>) -> bool {
    members.is_some_and(|members| message.read_by.len() as u32 + 1 >= members)
}

/// A message ready for the chat pipeline: where it goes, who sent it, and what
/// identity hints came with it.
pub struct Incoming {
    pub chat: String,
    pub is_group: bool,
    pub sender: Option<String>,
    pub push_name: Option<String>,
    pub lid_pn_pairs: Vec<(String, String)>,
    pub message: StoredMessage,
}

/// Chat-level fields from a history-sync conversation.
pub struct ConversationMeta<'a> {
    pub id: &'a str,
    pub name: Option<&'a str>,
    pub pn_jid: Option<&'a str>,
    pub lid_jid: Option<&'a str>,
    pub unread: Option<u32>,
    pub marked_unread: Option<bool>,
    pub pinned_at_s: Option<u32>,
    pub archived: Option<bool>,
    pub mute_end: Option<u64>,
    pub conversation_ts_s: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChatSummary {
    pub id: String,
    pub name: String,
    pub last_message: String,
    pub last_at: i64,
    pub last_from_me: bool,
    pub last_status: Option<Delivery>,
    pub unread: u32,
    pub pinned: bool,
    pub muted: bool,
    pub is_group: bool,
    /// File name of the profile picture inside the avatars directory.
    pub avatar: Option<String>,
    pub favorite: bool,
    /// The ids of the lists the chat is in.
    pub lists: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentDto {
    pub kind: String,
    pub mime: String,
    pub file_name: Option<String>,
    /// Bytes.
    pub size: u32,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReplyDto {
    pub author: String,
    /// The quoted message is one of ours.
    pub from_me: bool,
    pub id: Option<String>,
    pub text: String,
}

/// One emoji on a message, with everyone who picked it counted together.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReactionDto {
    pub emoji: String,
    pub count: u32,
    pub mine: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreviewDto {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub thumbnail: Option<String>,
    /// A full-size picture can be fetched.
    pub has_large: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MessageDto {
    pub id: String,
    pub chat_id: String,
    pub kind: MsgKind,
    pub sent_at: i64,
    pub from_me: bool,
    pub author: Option<String>,
    pub text: Option<String>,
    pub image_label: Option<String>,
    pub media_label: Option<String>,
    pub voice_seconds: Option<u32>,
    pub reply_to: Option<ReplyDto>,
    pub status: Option<Delivery>,
    /// The attachment can be fetched.
    pub has_media: bool,
    /// Every attachment but a WhatsApp photo, each fetched by its index.
    pub attachments: Vec<AttachmentDto>,
    /// The attachment's size in pixels, when known before it is fetched.
    pub media_width: Option<u32>,
    pub media_height: Option<u32>,
    pub link_preview: Option<LinkPreviewDto>,
    /// Forwarding score: 0 when not forwarded.
    pub forwarded: u32,
    /// Names `text` mentions as "@Name", for the UI to mark.
    pub mentions: Vec<String>,
    pub reactions: Vec<ReactionDto>,
    pub starred: bool,
    /// Taken back by its sender: only a placeholder is left.
    pub deleted: bool,
}

/// A chat list the user made, such as "Family" or "Work".
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct StoredList {
    pub id: String,
    pub name: String,
    /// Offered as a filter. WhatsApp also keeps its built-in filters
    /// (unread, groups…) as lists, and those are not.
    pub custom: bool,
    pub order: i32,
    pub color: i32,
    /// Chat ids as the phone named them.
    pub chats: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ListDto {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StarredDto {
    pub chat_name: String,
    pub message: MessageDto,
}

/// Someone a new chat can be started with.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContactDto {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    /// File name of the profile picture inside the avatars directory.
    pub avatar: Option<String>,
    pub is_me: bool,
}

/// What a desktop notification about a chat says.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub chat_id: String,
    pub title: String,
    pub body: String,
    pub muted: bool,
    /// File name of the chat's picture inside the avatars directory.
    pub avatar: Option<String>,
}

/// Longest quote of a message a reaction notification carries.
const NOTICE_QUOTE_CHARS: usize = 60;

fn shorten(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    if line.chars().count() <= max && !text.trim().contains('\n') {
        return line.to_string();
    }
    let cut: String = line.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

#[derive(Serialize, Deserialize)]
pub struct ChatStore {
    version: u32,
    chats: HashMap<String, StoredChat>,
    contact_names: HashMap<String, String>,
    push_names: HashMap<String, String>,
    lid_to_pn: HashMap<String, String>,
    own_ids: Vec<String>,
    /// Whether pins, mutes and archives were replayed from app state. Caches
    /// written before that was handled load as `false`.
    #[serde(default)]
    chat_state_synced: bool,
    /// Server id of our own profile picture saved on disk.
    #[serde(default)]
    own_avatar_id: Option<String>,
    /// Not saved, so every run looks again.
    #[serde(skip)]
    own_avatar_checked_ms: i64,
    /// Whether the phone was asked again for the ticks this cache got wrong
    /// before it kept track of them properly.
    #[serde(default)]
    ticks_refreshed: bool,
    /// Whether the phone was asked again for the photos this cache stored
    /// before it kept where to download them from.
    #[serde(default)]
    media_refreshed: bool,
    /// Favorite chats, in the phone's order and as the phone names them.
    #[serde(default)]
    favorites: Vec<String>,
    #[serde(default)]
    lists: Vec<StoredList>,
    /// Whether favorites, lists and starred messages were replayed from app
    /// state. Caches written before they were handled load as `false`.
    #[serde(default)]
    lists_synced: bool,
    /// Messages starred in app state before they arrived.
    #[serde(default)]
    pending_stars: HashSet<String>,
    /// Messages pushed out of memory by newer ones, by chat: the history
    /// database keeps them.
    #[serde(skip)]
    evicted: Vec<(String, StoredMessage)>,
    /// Chats whose whole history is here: the phone had nothing older.
    #[serde(default)]
    history_complete: HashSet<String>,
}

impl Default for ChatStore {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            chats: HashMap::new(),
            contact_names: HashMap::new(),
            push_names: HashMap::new(),
            lid_to_pn: HashMap::new(),
            own_ids: Vec::new(),
            chat_state_synced: true,
            own_avatar_id: None,
            own_avatar_checked_ms: 0,
            ticks_refreshed: false,
            media_refreshed: false,
            favorites: Vec::new(),
            lists: Vec::new(),
            lists_synced: true,
            pending_stars: HashSet::new(),
            evicted: Vec::new(),
            history_complete: HashSet::new(),
        }
    }
}

/// File name a chat's profile picture is saved under.
pub fn avatar_file(chat_id: &str, picture_id: &str) -> String {
    let safe = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect()
    };
    format!("{}-{}.jpg", safe(chat_id), safe(picture_id))
}

/// `123:4@s.whatsapp.net` -> `123@s.whatsapp.net`, `c.us` -> `s.whatsapp.net`.
fn strip_device(jid: &str) -> String {
    let Some((user, server)) = jid.split_once('@') else {
        return jid.to_string();
    };
    let user = user.split_once(':').map_or(user, |(base, _)| base);
    let server = if server == "c.us" { "s.whatsapp.net" } else { server };
    format!("{user}@{server}")
}

fn is_lid(jid: &str) -> bool {
    jid.ends_with("@lid")
}

fn is_pn(jid: &str) -> bool {
    jid.ends_with("@s.whatsapp.net")
}

fn is_hidden_chat(jid: &str) -> bool {
    jid == "status@broadcast"
        || jid.ends_with("@broadcast")
        || jid.ends_with("@newsletter")
        || jid.is_empty()
}

/// A Signal ACI as the bridge writes it: 8-4-4-4-12 lowercase hex digits.
fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
        })
}

fn format_phone(jid: &str) -> Option<String> {
    let (user, server) = jid.split_once('@')?;
    (server == "s.whatsapp.net" && user.chars().all(|c| c.is_ascii_digit()))
        .then(|| format!("+{user}"))
}

fn normalize_mute(raw: u64) -> i64 {
    // History sync mixes second and millisecond precision, and uses u64::MAX
    // for "always".
    if raw > i64::MAX as u64 {
        i64::MAX
    } else if raw < 100_000_000_000 {
        (raw as i64).saturating_mul(1000)
    } else {
        raw as i64
    }
}

impl ChatStore {
    pub fn from_json(bytes: &[u8]) -> Option<Self> {
        let mut store: Self = serde_json::from_slice(bytes).ok()?;
        store.migrate_attachments();
        (store.version == STORE_VERSION).then_some(store)
    }

    /// Moves the one attachment older stores kept into the list of them.
    fn migrate_attachments(&mut self) {
        for message in self.chats.values_mut().flat_map(|c| c.messages.iter_mut()) {
            if let Some(media) = message.content.media.take() {
                message.content.attachments.push(media);
            }
        }
    }

    /// The id a chat is filed under, as the history database has it too.
    pub fn chat_key(&self, jid: &str) -> String {
        self.canonical(jid)
    }

    /// Chats whose older history is still to be asked for, most recent
    /// first, with their names.
    pub fn history_queue(&self) -> Vec<(String, String)> {
        let mut chats: Vec<(&String, &StoredChat)> = self
            .chats
            .iter()
            .filter(|(id, _)| !is_hidden_chat(id) && !self.history_complete.contains(*id))
            .collect();
        chats.sort_by_key(|(_, chat)| std::cmp::Reverse(chat.last_ts_ms));
        chats.into_iter().map(|(id, chat)| (id.clone(), self.chat_name(id, chat))).collect()
    }

    /// How many chats have all their history here, of how many there are.
    pub fn history_counts(&self) -> (u32, u32) {
        let chats = self.chats.iter().filter(|(id, _)| !is_hidden_chat(id));
        let (mut done, mut total) = (0, 0);
        for (id, _) in chats {
            total += 1;
            done += self.history_complete.contains(id) as u32;
        }
        (done, total)
    }

    pub fn mark_history_complete(&mut self, chat: &str) {
        let chat = self.canonical(chat);
        self.history_complete.insert(chat);
    }

    /// When a chat last had something happen, as far as is known.
    pub fn chat_last_ts(&self, chat: &str) -> i64 {
        self.chats.get(&self.canonical(chat)).map_or(0, |c| c.last_ts_ms)
    }

    /// The oldest message of a chat in memory.
    pub fn oldest_message(&self, chat: &str) -> Option<&StoredMessage> {
        self.chats.get(&self.canonical(chat))?.messages.first()
    }

    /// Whether a chat has a message in memory.
    pub fn has_message(&self, chat: &str, id: &str) -> bool {
        self.chats.get(&self.canonical(chat)).is_some_and(|c| c.messages.iter().any(|m| m.id == id))
    }

    /// Brings a message from the history database back into memory, so it
    /// can be reacted to, starred or deleted like a recent one. It leaves
    /// again, saved, once newer messages push it out.
    pub fn restore(&mut self, chat: &str, message: StoredMessage) {
        let Some(chat) = self.chats.get_mut(&self.canonical(chat)) else { return };
        if chat.messages.iter().any(|m| m.id == message.id) {
            return;
        }
        let at = chat.messages.partition_point(|m| m.ts_ms <= message.ts_ms);
        chat.messages.insert(at, message);
    }

    /// Older messages of a chat, read from the history database, as the UI shows them.
    pub fn dtos(&self, chat: &str, messages: &[StoredMessage]) -> Vec<MessageDto> {
        let chat = self.canonical(chat);
        messages.iter().filter(|m| m.deleted != Some(Deletion::ForMe)).map(|m| self.to_dto(&chat, m)).collect()
    }

    pub fn to_json(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(self)
    }

    pub fn set_own_ids(&mut self, ids: impl IntoIterator<Item = String>) {
        for id in ids {
            let id = strip_device(&id);
            if !self.own_ids.contains(&id) {
                self.own_ids.push(id);
            }
        }
    }

    fn canonical(&self, jid: &str) -> String {
        let jid = strip_device(jid);
        if is_lid(&jid)
            && let Some(pn) = self.lid_to_pn.get(&jid)
        {
            return pn.clone();
        }
        jid
    }

    /// Records that a LID and a phone-number JID are the same person, merging
    /// any chat that was stored under the LID into the phone-number chat.
    pub fn learn_mapping(&mut self, a: &str, b: &str) {
        let (a, b) = (strip_device(a), strip_device(b));
        let (lid, pn) = match (is_lid(&a), is_pn(&b), is_lid(&b), is_pn(&a)) {
            (true, true, _, _) => (a, b),
            (_, _, true, true) => (b, a),
            _ => return,
        };
        if self.lid_to_pn.get(&lid) == Some(&pn) {
            return;
        }
        self.lid_to_pn.insert(lid.clone(), pn.clone());

        if let Some(name) = self.contact_names.remove(&lid) {
            self.contact_names.entry(pn.clone()).or_insert(name);
        }
        if let Some(name) = self.push_names.remove(&lid) {
            self.push_names.entry(pn.clone()).or_insert(name);
        }
        let Some(from) = self.chats.remove(&lid) else {
            return;
        };
        let into = self.chats.entry(pn.clone()).or_default();
        into.name = into.name.take().or(from.name);
        into.pinned_at = into.pinned_at.or(from.pinned_at);
        into.archived |= from.archived;
        into.muted_until_ms = into.muted_until_ms.max(from.muted_until_ms);
        into.unread += from.unread;
        into.last_ts_ms = into.last_ts_ms.max(from.last_ts_ms);
        let mut evicted = Vec::new();
        for message in from.messages {
            evicted.extend(insert_message(into, message));
        }
        self.evicted.extend(evicted.into_iter().map(|m| (pn.clone(), m)));
    }

    pub fn set_contact_name(&mut self, jid: &str, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let jid = self.canonical(jid);
        self.contact_names.insert(jid, name.to_string());
    }

    pub fn set_push_name(&mut self, jid: &str, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let jid = self.canonical(jid);
        self.push_names.insert(jid, name.to_string());
    }

    fn display_name(&self, jid: &str) -> String {
        let jid = self.canonical(jid);
        if self.own_ids.contains(&jid) {
            return "You".into();
        }
        self.contact_names
            .get(&jid)
            .or_else(|| self.push_names.get(&jid))
            .cloned()
            .or_else(|| format_phone(&jid))
            .unwrap_or_else(|| "Unknown contact".into())
    }

    /// `text` with each mention swapped for "@Name", and the names swapped
    /// in. WhatsApp writes "@<number>" (a phone number or a LID), kept as it
    /// is when the person has no name here; the Signal bridge writes
    /// "@<ACI>", always replaced.
    fn with_mentions(&self, text: &str) -> (String, Vec<String>) {
        let mut out = String::with_capacity(text.len());
        let mut names = Vec::new();
        let mut rest = text;
        while let Some(at) = rest.find('@') {
            let (before, after) = rest.split_at(at);
            out.push_str(before);
            let starts_word = !before.chars().next_back().is_some_and(char::is_alphanumeric);
            let aci = after.get(1..37).filter(|id| is_uuid(id));
            let digits = if aci.is_some() { 36 } else { after[1..].bytes().take_while(u8::is_ascii_digit).count() };
            let ends_word = !after[1 + digits..].chars().next().is_some_and(char::is_alphanumeric);
            let name = match aci {
                Some(aci) if starts_word && ends_word => Some(self.display_name(aci)),
                _ => (starts_word && ends_word && digits >= 5)
                    .then(|| self.mention_name(&after[1..1 + digits]))
                    .flatten(),
            };
            match name {
                Some(name) => {
                    out.push('@');
                    out.push_str(&name);
                    names.push(format!("@{name}"));
                    rest = &after[1 + digits..];
                }
                None => {
                    out.push('@');
                    rest = &after[1..];
                }
            }
        }
        out.push_str(rest);
        (out, names)
    }

    /// The name of whoever "@<number>" mentions, if known.
    fn mention_name(&self, number: &str) -> Option<String> {
        for server in ["lid", "s.whatsapp.net"] {
            let jid = self.canonical(&format!("{number}@{server}"));
            if self.own_ids.contains(&jid) {
                return Some("You".into());
            }
            if let Some(name) = self.contact_names.get(&jid).or_else(|| self.push_names.get(&jid)) {
                return Some(name.clone());
            }
        }
        None
    }

    fn chat_name(&self, id: &str, chat: &StoredChat) -> String {
        if chat.is_group {
            return chat.name.clone().unwrap_or_else(|| "Group".into());
        }
        if self.own_ids.contains(&id.to_string()) {
            return "You".into();
        }
        self.contact_names
            .get(id)
            .or(chat.name.as_ref())
            .or_else(|| self.push_names.get(id))
            .cloned()
            .or_else(|| format_phone(id))
            .unwrap_or_else(|| "Unknown contact".into())
    }

    /// Adds a message from the live stream or history. Returns the message as
    /// the UI sees it when it was new.
    pub fn ingest(&mut self, incoming: Incoming, count_unread: bool) -> Option<MessageDto> {
        for (a, b) in &incoming.lid_pn_pairs {
            self.learn_mapping(a, b);
        }
        if let (Some(sender), Some(name)) = (&incoming.sender, &incoming.push_name)
            && !incoming.message.from_me
        {
            self.set_push_name(sender, name);
        }

        let chat_id = self.canonical(&incoming.chat);
        if is_hidden_chat(&chat_id) {
            return None;
        }
        let mut message = incoming.message;
        message.starred |= self.pending_stars.remove(&message.id);
        message.author_jid = incoming.sender.as_deref().map(|s| self.canonical(s));
        message.wire = (!message.from_me
            && (incoming.chat != chat_id || incoming.sender != message.author_jid))
            .then(|| WireIds { chat: incoming.chat, sender: incoming.sender });
        if let Some(reply) = message.content.reply_to.as_mut() {
            reply.author_jid = reply.author_jid.as_deref().map(|j| self.canonical(j));
        }

        let chat = self.chats.entry(chat_id.clone()).or_default();
        chat.is_group |= incoming.is_group;
        if let Some(known) = chat.messages.iter_mut().find(|m| m.id == message.id) {
            if known.deleted.is_some() {
                return None;
            }
            // History can know better than we do how far a message got.
            if known.from_me && known.status < message.status {
                known.status = message.status;
                known.read_by.clear();
            }
            // Stars are only ever added here: app state is the authority.
            known.starred |= message.starred;
            // Messages stored before attachments and link cards were kept.
            if known.content.wa_media.is_none() && known.content.kind == message.content.kind {
                known.content.wa_media = message.content.wa_media;
            }
            // Before every attachment, their kinds and quote authors were.
            if known.content.attachments.len() < message.content.attachments.len()
                || known.content.attachments.iter().any(|a| a.kind.is_empty())
                    && known.content.attachments.len() == message.content.attachments.len()
            {
                known.content.attachments = message.content.attachments;
                known.content.media_label = message.content.media_label;
            }
            if let (Some(known), Some(reply)) = (known.content.reply_to.as_mut(), &message.content.reply_to) {
                if known.author_jid.is_none() && reply.author_jid.is_some() {
                    known.author_jid = reply.author_jid.clone();
                }
                // Quotes stored before the quoted message's id was kept.
                if known.id.is_none() {
                    known.id = reply.id.clone();
                }
            }
            if known.content.link.as_ref().is_none_or(|link| link.large.is_none())
                && message.content.link.is_some()
            {
                known.content.link = message.content.link;
            }
            return None;
        }
        if count_unread && !message.from_me {
            chat.unread += 1;
        }
        // Replying reads the chat, but an older reply out of history does not
        // read what came after it.
        if message.from_me && message.ts_ms >= chat.last_ts_ms {
            chat.unread = 0;
        }
        chat.last_ts_ms = chat.last_ts_ms.max(message.ts_ms);
        let evicted = insert_message(chat, message.clone());
        self.evicted.extend(evicted.into_iter().map(|m| (chat_id.clone(), m)));
        Some(self.to_dto(&chat_id, &message))
    }

    pub fn ingest_conversation(&mut self, meta: ConversationMeta<'_>) {
        let id = meta.id;
        if let (Some(pn), Some(lid)) = (meta.pn_jid, meta.lid_jid) {
            self.learn_mapping(lid, pn);
        } else if let Some(pn) = meta.pn_jid {
            self.learn_mapping(id, pn);
        } else if let Some(lid) = meta.lid_jid {
            self.learn_mapping(id, lid);
        }
        let chat_id = self.canonical(id);
        if is_hidden_chat(&chat_id) {
            return;
        }
        let chat = self.chats.entry(chat_id.clone()).or_default();
        chat.is_group |= chat_id.ends_with("@g.us");
        if let Some(name) = meta.name.filter(|n| !n.trim().is_empty()) {
            chat.name = Some(name.to_string());
        }
        if let Some(unread) = meta.unread {
            chat.unread = unread;
        }
        if meta.marked_unread == Some(true) {
            chat.unread = chat.unread.max(1);
        }
        // App state is the authority on these and can arrive first, so history
        // only fills them in and never clears them.
        if let Some(pinned) = meta.pinned_at_s.filter(|p| *p > 0) {
            chat.pinned_at = Some(pinned as i64 * 1000);
        }
        if meta.archived == Some(true) {
            chat.archived = true;
        }
        if let Some(mute_end) = meta.mute_end.filter(|m| *m > 0) {
            chat.muted_until_ms = Some(normalize_mute(mute_end));
        }
        if let Some(ts) = meta.conversation_ts_s {
            chat.last_ts_ms = chat.last_ts_ms.max(ts as i64 * 1000);
        }
    }

    pub fn needs_chat_state_sync(&self) -> bool {
        !self.chat_state_synced
    }

    pub fn set_chat_state_synced(&mut self, synced: bool) {
        self.chat_state_synced = synced;
    }

    pub fn needs_lists_sync(&self) -> bool {
        !self.lists_synced
    }

    pub fn set_lists_synced(&mut self, synced: bool) {
        self.lists_synced = synced;
    }

    /// Replaces the favorites with the phone's list.
    pub fn set_favorites(&mut self, ids: Vec<String>) {
        self.favorites = ids;
    }

    /// The favorites with a chat added or removed, as the phone expects the
    /// whole list back.
    pub fn favorites_with(&self, chat: &str, favorite: bool) -> Vec<String> {
        let chat_id = self.canonical(chat);
        let mut kept: Vec<String> = self
            .favorites
            .iter()
            .filter(|id| self.canonical(id) != chat_id)
            .cloned()
            .collect();
        if favorite {
            kept.push(chat_id);
        }
        kept
    }

    /// Adds or removes a favorite. Returns the whole list.
    pub fn set_favorite(&mut self, chat: &str, favorite: bool) -> Vec<String> {
        self.favorites = self.favorites_with(chat, favorite);
        self.favorites.clone()
    }

    fn list_mut(&mut self, id: &str) -> &mut StoredList {
        let index = match self.lists.iter().position(|l| l.id == id) {
            Some(index) => index,
            None => {
                self.lists.push(StoredList { id: id.to_string(), ..Default::default() });
                self.lists.len() - 1
            }
        };
        &mut self.lists[index]
    }

    /// Creates, renames or deletes a list. Its chats can arrive first.
    pub fn edit_list(&mut self, id: &str, name: Option<&str>, custom: bool, order: Option<i32>, color: Option<i32>) {
        let list = self.list_mut(id);
        if let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) {
            list.name = name.to_string();
        }
        list.custom = custom;
        if let Some(order) = order {
            list.order = order;
        }
        if let Some(color) = color {
            list.color = color;
        }
    }

    pub fn delete_list(&mut self, id: &str) {
        self.lists.retain(|l| l.id != id);
    }

    pub fn set_list_chat(&mut self, id: &str, chat: &str, member: bool) {
        let chat_id = self.canonical(chat);
        let known: Vec<String> = self.lists.iter().find(|l| l.id == id).map_or_else(Vec::new, |l| {
            l.chats.iter().filter(|c| self.canonical(c) != chat_id).cloned().collect()
        });
        let list = self.list_mut(id);
        list.chats = known;
        if member {
            list.chats.push(chat_id);
        }
    }

    pub fn list(&self, id: &str) -> Option<StoredList> {
        self.lists.iter().find(|l| l.id == id).cloned()
    }

    /// The chats in a list, under the ids they are filed under here.
    pub fn list_chat_ids(&self, id: &str) -> Vec<String> {
        self.lists
            .iter()
            .find(|l| l.id == id)
            .map(|l| l.chats.iter().map(|c| self.canonical(c)).collect())
            .unwrap_or_default()
    }

    /// An id and a place at the end for a new list. WhatsApp numbers them.
    pub fn next_list(&self) -> (String, i32) {
        let id = self.lists.iter().filter_map(|l| l.id.parse::<u64>().ok()).max().unwrap_or(0) + 1;
        let order = self.lists.iter().map(|l| l.order).max().map_or(0, |o| o + 1);
        (id.to_string(), order)
    }

    /// The lists offered as filters, in the phone's order.
    pub fn lists(&self) -> Vec<ListDto> {
        let mut lists: Vec<&StoredList> =
            self.lists.iter().filter(|l| l.custom && !l.name.is_empty()).collect();
        lists.sort_by_key(|l| l.order);
        lists
            .into_iter()
            .map(|l| ListDto { id: l.id.clone(), name: l.name.clone() })
            .collect()
    }

    /// Sets a message's star. Returns the message when it is known.
    pub fn set_starred(&mut self, chat: &str, id: &str, starred: bool) -> Option<MessageDto> {
        let chat_id = self.canonical(chat);
        let found = self.chats.get_mut(&chat_id).and_then(|c| c.messages.iter_mut().find(|m| m.id == id));
        let Some(message) = found else {
            if starred {
                self.pending_stars.insert(id.to_string());
            } else {
                self.pending_stars.remove(id);
            }
            return None;
        };
        message.starred = starred;
        let message = message.clone();
        Some(self.to_dto(&chat_id, &message))
    }

    /// The sender a starred message is filed under, as it arrived, and
    /// whether it is ours.
    pub fn star_target(&self, chat: &str, id: &str) -> Option<(Option<String>, bool)> {
        let (_, message) = self.find_message(chat, id)?;
        let sender = message.wire.as_ref().and_then(|w| w.sender.clone()).or_else(|| message.author_jid.clone());
        Some((sender, message.from_me))
    }

    /// Every starred message, newest first.
    pub fn starred(&self) -> Vec<StarredDto> {
        let mut out: Vec<(i64, StarredDto)> = self
            .chats
            .iter()
            .filter(|(id, _)| !is_hidden_chat(id))
            .flat_map(|(id, chat)| {
                chat.messages.iter().filter(|m| m.starred && m.deleted.is_none()).map(move |m| {
                    (m.ts_ms, StarredDto { chat_name: self.chat_name(id, chat), message: self.to_dto(id, m) })
                })
            })
            .collect();
        out.sort_by_key(|(ts, _)| std::cmp::Reverse(*ts));
        out.into_iter().map(|(_, dto)| dto).collect()
    }

    /// Messages with every word of `query` in them, newest first, at most
    /// `limit`. Case and accents are ignored: "nao" finds "Não". `saved` is
    /// what the history database found, which memory may have newer copies of.
    pub fn search(&self, query: &str, only: Option<&str>, limit: usize, saved: Vec<(String, StoredMessage)>) -> Vec<StarredDto> {
        let words = search_words(query);
        if words.is_empty() {
            return Vec::new();
        }
        let only = only.map(|chat| self.canonical(chat));
        let mut out: Vec<(String, StoredMessage)> = Vec::new();
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let chats = self.chats.iter().filter(|(id, _)| !is_hidden_chat(id) && only.as_ref().is_none_or(|only| only == *id));
        for (id, chat) in chats {
            for m in chat.messages.iter().filter(|m| m.deleted.is_none()) {
                let Some(text) = search_text(m) else { continue };
                let text = fold(&text);
                if words.iter().all(|word| text.contains(word.as_str())) {
                    seen.insert((id.clone(), m.id.clone()));
                    out.push((id.clone(), m.clone()));
                }
            }
        }
        for (chat, m) in saved {
            let chat = self.canonical(&chat);
            let in_memory = self.chats.get(&chat).is_some_and(|c| c.messages.iter().any(|known| known.id == m.id));
            if is_hidden_chat(&chat) || in_memory || !seen.insert((chat.clone(), m.id.clone())) {
                continue;
            }
            out.push((chat, m));
        }
        out.sort_by_key(|(_, m)| std::cmp::Reverse(m.ts_ms));
        out.truncate(limit);
        out.into_iter()
            .filter_map(|(id, m)| {
                let chat = self.chats.get(&id)?;
                Some(StarredDto { chat_name: self.chat_name(&id, chat), message: self.to_dto(&id, &m) })
            })
            .collect()
    }

    /// Saved contacts, and ourselves.
    pub fn contacts(&self) -> Vec<ContactDto> {
        let own = self.own_id().map(str::to_string);
        let mut out: Vec<ContactDto> = self
            .contact_names
            .iter()
            .filter(|(id, _)| !is_hidden_chat(id) && !id.ends_with("@g.us") && !self.own_ids.contains(id))
            .map(|(id, name)| ContactDto {
                id: id.clone(),
                name: name.clone(),
                phone: format_phone(id),
                avatar: self
                    .chats
                    .get(id)
                    .and_then(|c| c.avatar_id.as_deref())
                    .map(|pic| avatar_file(id, pic)),
                is_me: false,
            })
            .collect();
        if let Some(id) = own {
            out.push(ContactDto {
                phone: format_phone(&id),
                name: "You".into(),
                avatar: self.own_avatar(),
                is_me: true,
                id,
            });
        }
        out
    }

    fn chat_notice(&self, chat_id: &str, chat: &StoredChat, body: String) -> Notice {
        Notice {
            chat_id: chat_id.to_string(),
            title: self.chat_name(chat_id, chat),
            body,
            muted: chat.muted_until_ms.is_some_and(|until| until > now_ms()),
            avatar: chat.avatar_id.as_deref().map(|pic| avatar_file(chat_id, pic)),
        }
    }

    /// The notification for a message someone sent us.
    pub fn message_notice(&self, chat: &str, id: &str) -> Option<Notice> {
        let chat_id = self.canonical(chat);
        let stored = self.chats.get(&chat_id)?;
        let message = stored.messages.iter().find(|m| m.id == id)?;
        if message.from_me {
            return None;
        }
        let preview = self.with_mentions(&message.content.preview()).0;
        let sender = message
            .author_jid
            .as_deref()
            .map(|j| self.display_name(j))
            .unwrap_or_else(|| "Someone".into());
        let replied_to = message.content.reply_to.as_ref().and_then(|r| r.author_jid.as_deref());
        let body = match replied_to {
            Some(to) if self.own_ids.iter().any(|own| own == to) => {
                if stored.is_group {
                    format!("{sender} replied to you: {preview}")
                } else {
                    format!("Replied to you: {preview}")
                }
            }
            Some(to) if stored.is_group => {
                format!("{sender} replied to {}: {preview}", self.display_name(to))
            }
            _ if stored.is_group => format!("{sender}: {preview}"),
            _ => preview,
        };
        Some(self.chat_notice(&chat_id, stored, body))
    }

    /// The notification for someone reacting to one of our messages.
    pub fn reaction_notice(&self, chat: &str, id: &str, author: &str, emoji: &str) -> Option<Notice> {
        let chat_id = self.canonical(chat);
        let author = self.canonical(author);
        if self.own_ids.contains(&author) {
            return None;
        }
        let stored = self.chats.get(&chat_id)?;
        let message = stored.messages.iter().find(|m| m.id == id)?;
        if !message.from_me {
            return None;
        }
        let quote = shorten(&self.with_mentions(&message.content.preview()).0, NOTICE_QUOTE_CHARS);
        let body = if stored.is_group {
            format!("{} reacted {emoji} to \u{201c}{quote}\u{201d}", self.display_name(&author))
        } else {
            format!("Reacted {emoji} to \u{201c}{quote}\u{201d}")
        };
        Some(self.chat_notice(&chat_id, stored, body))
    }

    /// The chat a pin, mute or archive applies to. These can arrive before the
    /// chat has any message.
    fn chat_for_state(&mut self, jid: &str) -> Option<&mut StoredChat> {
        let chat_id = self.canonical(jid);
        if is_hidden_chat(&chat_id) {
            return None;
        }
        let is_group = chat_id.ends_with("@g.us");
        let chat = self.chats.entry(chat_id).or_default();
        chat.is_group |= is_group;
        Some(chat)
    }

    pub fn set_pinned(&mut self, jid: &str, pinned_at_ms: Option<i64>) {
        if let Some(chat) = self.chat_for_state(jid) {
            chat.pinned_at = pinned_at_ms;
        }
    }

    pub fn set_muted(&mut self, jid: &str, muted_until_ms: Option<i64>) {
        if let Some(chat) = self.chat_for_state(jid) {
            chat.muted_until_ms = muted_until_ms;
        }
    }

    /// Names a group chat from its metadata, which can arrive before any message.
    pub fn set_group_name(&mut self, jid: &str, name: &str) {
        let name = name.trim();
        let Some(chat) = self.chat_for_state(jid) else {
            return;
        };
        chat.is_group = true;
        if !name.is_empty() {
            chat.name = Some(name.to_string());
        }
    }

    pub fn set_archived(&mut self, jid: &str, archived: bool) {
        if let Some(chat) = self.chat_for_state(jid) {
            chat.archived = archived;
        }
    }

    /// Read or marked unread on another device. A mutation older than the
    /// chat's newest message is stale: messages came in after it.
    pub fn set_read_elsewhere(&mut self, jid: &str, read: bool, at_ms: i64) {
        if let Some(chat) = self.chat_for_state(jid)
            && at_ms >= chat.last_ts_ms
        {
            chat.unread = if read { 0 } else { chat.unread.max(1) };
        }
    }

    /// Listed chats whose profile picture is due a check, in list order, with
    /// the picture id already on disk.
    pub fn avatars_due(&self, limit: usize) -> Vec<(String, Option<String>)> {
        let cutoff = now_ms() - AVATAR_TTL_MS;
        self.list_chats()
            .into_iter()
            .filter_map(|summary| {
                let chat = self.chats.get(&summary.id)?;
                (chat.avatar_checked_ms < cutoff).then(|| (summary.id, chat.avatar_id.clone()))
            })
            .take(limit)
            .collect()
    }

    pub fn set_avatar(&mut self, chat: &str, picture_id: Option<String>) {
        if let Some(stored) = self.chats.get_mut(chat) {
            stored.avatar_id = picture_id;
            stored.avatar_checked_ms = now_ms();
        }
    }

    pub fn avatar_checked(&mut self, chat: &str) {
        if let Some(stored) = self.chats.get_mut(chat) {
            stored.avatar_checked_ms = now_ms();
        }
    }

    /// Makes the chat's picture due for a check again, and our own when the
    /// id is ours.
    pub fn invalidate_avatar(&mut self, jid: &str) {
        let chat_id = self.canonical(jid);
        if self.own_ids.contains(&chat_id) {
            self.own_avatar_checked_ms = 0;
        }
        if let Some(stored) = self.chats.get_mut(&chat_id) {
            stored.avatar_checked_ms = 0;
        }
    }

    /// Makes a chat that has no picture due for a check again. Says whether
    /// there was such a chat.
    pub fn retry_missing_avatar(&mut self, jid: &str) -> bool {
        let chat_id = self.canonical(jid);
        match self.chats.get_mut(&chat_id) {
            Some(stored) if stored.avatar_id.is_none() => {
                stored.avatar_checked_ms = 0;
                true
            }
            _ => false,
        }
    }

    /// Our own id and the picture id on disk, when our picture is due a check.
    pub fn own_avatar_due(&self) -> Option<(String, Option<String>)> {
        (self.own_avatar_checked_ms < now_ms() - AVATAR_TTL_MS)
            .then(|| Some((self.own_id()?.to_string(), self.own_avatar_id.clone())))
            .flatten()
    }

    pub fn set_own_avatar(&mut self, picture_id: Option<String>) {
        self.own_avatar_id = picture_id;
        self.own_avatar_checked_ms = now_ms();
    }

    /// File name of our own profile picture inside the avatars directory.
    pub fn own_avatar(&self) -> Option<String> {
        self.own_avatar_id.as_deref().map(|pic| avatar_file(OWN_AVATAR, pic))
    }

    /// Raises the delivery state of messages we sent. Returns the ones that
    /// changed. `reader` is who the receipt came from, on a network where a
    /// group message only counts as read once everyone has read it; without
    /// one, a single reader is enough.
    pub fn apply_receipt(
        &mut self,
        chat: &str,
        ids: &[String],
        delivery: Delivery,
        reader: Option<&str>,
    ) -> Vec<MessageDto> {
        let chat_id = self.canonical(chat);
        let reader = reader.map(|r| self.canonical(r));
        if reader.as_ref().is_some_and(|r| self.own_ids.contains(r)) {
            return Vec::new();
        }
        let Some(stored) = self.chats.get_mut(&chat_id) else {
            return Vec::new();
        };
        let (is_group, members) = (stored.is_group, stored.members);
        // One to one, whoever got a message also got the ones before it.
        let up_to = (!is_group)
            .then(|| {
                let named = stored.messages.iter().filter(|m| m.from_me && ids.contains(&m.id));
                named.map(|m| m.ts_ms).max()
            })
            .flatten();
        let mut changed = Vec::new();
        for message in stored.messages.iter_mut().filter(|m| m.from_me) {
            let covered = ids.contains(&message.id) || up_to.is_some_and(|ts| message.ts_ms <= ts);
            if !covered || message.status == Some(Delivery::Read) {
                continue;
            }
            let mut delivery = delivery;
            if is_group
                && delivery == Delivery::Read
                && let Some(reader) = &reader
            {
                if !message.read_by.contains(reader) {
                    message.read_by.push(reader.clone());
                }
                if !everyone_read(message, members) {
                    delivery = Delivery::Delivered;
                }
            }
            if message.status.is_none_or(|s| s < delivery) {
                message.status = Some(delivery);
                if delivery == Delivery::Read {
                    message.read_by.clear();
                }
                changed.push(message.clone());
            }
        }
        changed
            .iter()
            .map(|m| self.to_dto(&chat_id, m))
            .collect()
    }

    /// Chats where a message of ours is not marked read, newest first, unless
    /// they were handed out before. Each comes with its latest message (id,
    /// whether it is ours, time): the phone can resend what precedes it.
    pub fn stale_ticks(&mut self, limit: usize, window: usize) -> Vec<(String, String, bool, i64)> {
        if std::mem::replace(&mut self.ticks_refreshed, true) {
            return Vec::new();
        }
        let mut stale: Vec<_> = self
            .chats
            .iter()
            .filter(|(id, _)| !is_hidden_chat(id))
            .filter_map(|(id, chat)| {
                let (last, before) = chat.messages.split_last()?;
                let recent = before.iter().rev().take(window);
                recent
                    .filter(|m| m.from_me)
                    .any(|m| m.status != Some(Delivery::Read))
                    .then(|| (id.clone(), last.id.clone(), last.from_me, last.ts_ms))
            })
            .collect();
        stale.sort_by_key(|(_, _, _, ts)| std::cmp::Reverse(*ts));
        stale.truncate(limit);
        stale
    }

    /// Chats holding a recent photo or file that cannot be fetched, newest first,
    /// unless they were handed out before. Same shape as `stale_ticks`.
    pub fn stale_media(&mut self, limit: usize, window: usize) -> Vec<(String, String, bool, i64)> {
        if std::mem::replace(&mut self.media_refreshed, true) {
            return Vec::new();
        }
        let mut stale: Vec<_> = self
            .chats
            .iter()
            .filter(|(id, _)| !is_hidden_chat(id))
            .filter_map(|(id, chat)| {
                let (last, before) = chat.messages.split_last()?;
                let recent = before.iter().rev().take(window);
                recent
                    .map(|m| &m.content)
                    .any(Content::lacks_wa_file)
                    .then(|| (id.clone(), last.id.clone(), last.from_me, last.ts_ms))
            })
            .collect();
        stale.sort_by_key(|(_, _, _, ts)| std::cmp::Reverse(*ts));
        stale.truncate(limit);
        stale
    }

    /// Whether a group's size should be looked up now. Saying yes counts as
    /// the lookup, so it is not started twice.
    pub fn claim_member_check(&mut self, chat: &str) -> bool {
        let chat_id = self.canonical(chat);
        match self.chats.get_mut(&chat_id) {
            Some(stored) if stored.is_group && stored.members_checked_ms < now_ms() - MEMBERS_TTL_MS => {
                stored.members_checked_ms = now_ms();
                true
            }
            _ => false,
        }
    }

    /// Records how many people are in a group, us included. Returns the
    /// messages that everyone turns out to have read.
    pub fn set_group_members(&mut self, chat: &str, members: u32) -> Vec<MessageDto> {
        let chat_id = self.canonical(chat);
        let Some(stored) = self.chats.get_mut(&chat_id) else {
            return Vec::new();
        };
        stored.members = Some(members);
        let mut changed = Vec::new();
        for message in stored.messages.iter_mut() {
            if !message.read_by.is_empty() && everyone_read(message, Some(members)) {
                message.status = Some(Delivery::Read);
                message.read_by.clear();
                changed.push(message.clone());
            }
        }
        changed
            .iter()
            .map(|m| self.to_dto(&chat_id, m))
            .collect()
    }

    /// Like [`Self::apply_receipt`], for receipts that don't say which chat
    /// the messages are in.
    pub fn apply_receipt_anywhere(&mut self, ids: &[String], delivery: Delivery) -> Vec<MessageDto> {
        let chats: Vec<String> = self
            .chats
            .iter()
            .filter(|(_, chat)| chat.messages.iter().any(|m| m.from_me && ids.contains(&m.id)))
            .map(|(id, _)| id.clone())
            .collect();
        chats
            .iter()
            .flat_map(|chat| self.apply_receipt(chat, ids, delivery, None))
            .collect()
    }

    pub fn own_id(&self) -> Option<&str> {
        self.own_ids.first().map(String::as_str)
    }

    fn find_message(&self, chat: &str, id: &str) -> Option<(&StoredChat, &StoredMessage)> {
        let chat = self.chats.get(&self.canonical(chat))?;
        Some((chat, chat.messages.iter().find(|m| m.id == id)?))
    }

    pub fn media(&self, chat: &str, id: &str, index: usize) -> Option<MediaRef> {
        self.find_message(chat, id)?.1.content.attachments.get(index).cloned()
    }

    pub fn wa_media(&self, chat: &str, id: &str) -> Option<CdnRef> {
        self.find_message(chat, id)?.1.content.wa_media.clone()
    }

    /// Points a message's photo at the path the sender's phone re-uploaded it to.
    pub fn set_wa_media_path(&mut self, chat: &str, id: &str, path: &str) {
        let chat_id = self.canonical(chat);
        if let Some(media) = self
            .chats
            .get_mut(&chat_id)
            .and_then(|c| c.messages.iter_mut().find(|m| m.id == id))
            .and_then(|m| m.content.wa_media.as_mut())
        {
            media.path = path.to_string();
        }
    }

    pub fn link_thumbnail(&self, chat: &str, id: &str) -> Option<CdnRef> {
        self.find_message(chat, id)?.1.content.link.as_ref()?.large.clone()
    }

    pub fn reaction_target(&self, chat: &str, id: &str) -> Option<ReactionTarget> {
        let (_, message) = self.find_message(chat, id)?;
        Some(ReactionTarget {
            author: message.author_jid.clone(),
            ts_ms: message.ts_ms,
            from_me: message.from_me,
            mine: message
                .reactions
                .iter()
                .find(|r| self.own_ids.contains(&r.author))
                .map(|r| r.emoji.clone()),
        })
    }

    /// What a reply to a message quotes. `None` once it is deleted.
    pub fn quote_source(&self, chat: &str, id: &str) -> Option<QuoteSource> {
        let (_, message) = self.find_message(chat, id)?;
        if message.deleted.is_some() {
            return None;
        }
        Some(QuoteSource {
            sender: (!message.from_me)
                .then(|| message.wire.as_ref().and_then(|w| w.sender.clone()).or_else(|| message.author_jid.clone()))
                .flatten(),
            from_me: message.from_me,
            ts_ms: message.ts_ms,
            text: message.content.preview(),
        })
    }

    /// Deletes a message for us, or replaces it with a placeholder when its
    /// sender took it back. Returns the message as the UI sees it now.
    pub fn delete_message(&mut self, chat: &str, id: &str, how: Deletion) -> Option<MessageDto> {
        let chat_id = self.canonical(chat);
        let message = self.chats.get_mut(&chat_id)?.messages.iter_mut().find(|m| m.id == id)?;
        if message.deleted == Some(Deletion::ForMe) {
            return None;
        }
        message.deleted = Some(how);
        message.starred = false;
        if how == Deletion::ForEveryone {
            message.content = Content { text: None, ..Content::text("", None) };
            message.reactions.clear();
        }
        let message = message.clone();
        Some(self.to_dto(&chat_id, &message))
    }

    /// Sets or clears one person's reaction. Returns the message when it is known.
    pub fn set_reaction(
        &mut self,
        chat: &str,
        id: &str,
        author: &str,
        emoji: Option<&str>,
    ) -> Option<MessageDto> {
        let chat_id = self.canonical(chat);
        let author = self.canonical(author);
        let message = self
            .chats
            .get_mut(&chat_id)?
            .messages
            .iter_mut()
            .find(|m| m.id == id)?;
        message.reactions.retain(|r| r.author != author);
        if let Some(emoji) = emoji {
            message.reactions.push(Reaction { author, emoji: emoji.to_string() });
        }
        let message = message.clone();
        Some(self.to_dto(&chat_id, &message))
    }

    /// Clears the unread count. Returns the messages it stood for, newest
    /// first, so their senders can be told.
    pub fn has_unread(&self, chat: &str) -> bool {
        self.chats.get(&self.canonical(chat)).is_some_and(|c| c.unread > 0)
    }

    pub fn mark_read(&mut self, chat: &str) -> Vec<ReadTarget> {
        let chat_id = self.canonical(chat);
        let Some(stored) = self.chats.get_mut(&chat_id) else {
            return Vec::new();
        };
        let unread = std::mem::take(&mut stored.unread) as usize;
        stored
            .messages
            .iter()
            .rev()
            .filter(|m| !m.from_me)
            .take(unread)
            .map(|m| {
                let (chat, sender) = match &m.wire {
                    Some(wire) => (wire.chat.clone(), wire.sender.clone()),
                    None => (chat_id.clone(), m.author_jid.clone()),
                };
                ReadTarget { id: m.id.clone(), chat, sender, ts_ms: m.ts_ms }
            })
            .collect()
    }

    /// The chat a message is in, for events that only name the message.
    pub fn chat_of(&self, id: &str) -> Option<String> {
        self.chats
            .iter()
            .find(|(_, chat)| chat.messages.iter().any(|m| m.id == id))
            .map(|(chat_id, _)| chat_id.clone())
    }

    /// Unread messages in the chats the list shows, muted ones left out.
    pub fn unread_total(&self) -> u32 {
        let now = now_ms();
        self.chats
            .iter()
            .filter(|(id, chat)| !chat.archived && !chat.messages.is_empty() && !is_hidden_chat(id))
            .filter(|(_, chat)| !chat.muted_until_ms.is_some_and(|until| until > now))
            .fold(0u32, |sum, (_, chat)| sum.saturating_add(chat.unread))
    }

    pub fn list_chats(&self) -> Vec<ChatSummary> {
        let now = now_ms();
        let favorites: Vec<String> = self.favorites.iter().map(|id| self.canonical(id)).collect();
        let lists: Vec<(&str, Vec<String>)> = self
            .lists
            .iter()
            .filter(|l| l.custom)
            .map(|l| (l.id.as_str(), l.chats.iter().map(|c| self.canonical(c)).collect()))
            .collect();
        let mut out: Vec<_> = self
            .chats
            .iter()
            // Chats the first sync named without sending any of their messages
            // are listed too: their history is fetched in the background.
            .filter(|(id, chat)| !chat.archived && !is_hidden_chat(id))
            .map(|(id, chat)| {
                let last = chat.messages.iter().rev().find(|m| m.deleted != Some(Deletion::ForMe));
                let preview = last.map(|m| self.with_mentions(&deleted_or_preview(m)).0).unwrap_or_default();
                let last_message = match last {
                    Some(m) if chat.is_group && !m.from_me => match &m.author_jid {
                        Some(author) => format!("{}: {preview}", self.display_name(author)),
                        None => preview,
                    },
                    _ => preview,
                };
                ChatSummary {
                    id: id.clone(),
                    name: self.chat_name(id, chat),
                    last_message,
                    last_at: last.map_or(chat.last_ts_ms, |m| m.ts_ms),
                    last_from_me: last.is_some_and(|m| m.from_me),
                    last_status: last.and_then(|m| m.status),
                    unread: chat.unread,
                    pinned: chat.pinned_at.is_some(),
                    muted: chat.muted_until_ms.is_some_and(|until| until > now),
                    is_group: chat.is_group,
                    avatar: chat.avatar_id.as_deref().map(|pic| avatar_file(id, pic)),
                    favorite: favorites.contains(id),
                    lists: lists
                        .iter()
                        .filter(|(_, chats)| chats.contains(id))
                        .map(|(list, _)| list.to_string())
                        .collect(),
                }
            })
            .collect();
        out.sort_by(|a, b| {
            let pin = |id: &str| self.chats.get(id).and_then(|c| c.pinned_at);
            match (pin(&a.id), pin(&b.id)) {
                (Some(x), Some(y)) => y.cmp(&x),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => b.last_at.cmp(&a.last_at),
            }
        });
        out
    }

    pub fn messages(&self, chat: &str) -> Vec<MessageDto> {
        let chat_id = self.canonical(chat);
        self.chats
            .get(&chat_id)
            .map(|c| c.messages.iter().filter(|m| m.deleted != Some(Deletion::ForMe)).map(|m| self.to_dto(&chat_id, m)).collect())
            .unwrap_or_default()
    }

    fn to_dto(&self, chat_id: &str, m: &StoredMessage) -> MessageDto {
        let is_group = self.chats.get(chat_id).is_some_and(|c| c.is_group);
        let (image_label, media_label) = match m.content.kind {
            MsgKind::Image => (m.content.media_label.clone(), None),
            MsgKind::Media => (None, m.content.media_label.clone()),
            _ => (None, None),
        };
        let photo = m.content.wa_media.as_ref().filter(|r| r.kind.is_none());
        let (text, mentions) = match &m.content.text {
            Some(text) => {
                let (text, names) = self.with_mentions(text);
                (Some(text), names)
            }
            None => (None, Vec::new()),
        };
        MessageDto {
            id: m.id.clone(),
            chat_id: chat_id.to_string(),
            kind: m.content.kind,
            sent_at: m.ts_ms,
            from_me: m.from_me,
            author: (is_group && !m.from_me)
                .then(|| m.author_jid.as_deref().map(|j| self.display_name(j)))
                .flatten(),
            text,
            mentions,
            image_label,
            media_label,
            voice_seconds: m.content.voice_seconds,
            reply_to: m.content.reply_to.as_ref().map(|r| {
                let from_me = r.author_jid.as_deref().is_some_and(|j| {
                    self.own_ids.iter().any(|own| own == j) || self.own_ids.contains(&self.canonical(j))
                });
                ReplyDto {
                    author: match &r.author_jid {
                        _ if from_me => "You".into(),
                        Some(j) => self.display_name(j),
                        None => "Message".into(),
                    },
                    from_me,
                    id: r.id.clone(),
                    text: self.with_mentions(&r.text).0,
                }
            }),
            status: m.status,
            has_media: !m.content.attachments.is_empty() || m.content.wa_media.is_some(),
            attachments: m
                .content
                .attachments
                .iter()
                .map(|a| AttachmentDto {
                    kind: a.kind().to_string(),
                    mime: a.mime.clone(),
                    file_name: a.file_name.clone(),
                    size: a.size,
                    width: Some(a.width).filter(|w| *w > 0),
                    height: Some(a.height).filter(|h| *h > 0),
                })
                .chain(m.content.wa_media.as_ref().and_then(CdnRef::attachment))
                .collect(),
            media_width: photo.map(|r| r.width).filter(|w| *w > 0),
            media_height: photo.map(|r| r.height).filter(|h| *h > 0),
            link_preview: m.content.link.as_ref().map(|l| LinkPreviewDto {
                url: l.url.clone(),
                title: l.title.clone(),
                description: l.description.clone(),
                thumbnail: l.thumbnail.clone(),
                has_large: l.large.is_some(),
            }),
            forwarded: m.content.forwarded,
            reactions: self.reaction_dtos(&m.reactions),
            starred: m.starred,
            deleted: m.deleted.is_some(),
        }
    }

    fn reaction_dtos(&self, reactions: &[Reaction]) -> Vec<ReactionDto> {
        let mut out: Vec<ReactionDto> = Vec::new();
        for reaction in reactions {
            let mine = self.own_ids.contains(&reaction.author);
            match out.iter_mut().find(|r| r.emoji == reaction.emoji) {
                Some(found) => {
                    found.count += 1;
                    found.mine |= mine;
                }
                None => out.push(ReactionDto { emoji: reaction.emoji.clone(), count: 1, mine }),
            }
        }
        out
    }
}

/// A message's line in the chat list.
fn deleted_or_preview(message: &StoredMessage) -> String {
    match (message.deleted, message.from_me) {
        (Some(Deletion::ForEveryone), true) => "You deleted this message".into(),
        (Some(Deletion::ForEveryone), false) => "This message was deleted".into(),
        _ => message.content.preview(),
    }
}

/// Files a message in order. Returns what no longer fits in memory, for
/// the history database.
fn insert_message(chat: &mut StoredChat, message: StoredMessage) -> Vec<StoredMessage> {
    if chat.messages.iter().any(|m| m.id == message.id) {
        return Vec::new();
    }
    let out_of_order = chat.messages.last().is_some_and(|last| last.ts_ms > message.ts_ms);
    chat.messages.push(message);
    if out_of_order {
        chat.messages.sort_by_key(|m| m.ts_ms);
    }
    if chat.messages.len() > MAX_MESSAGES_PER_CHAT {
        let excess = chat.messages.len() - MAX_MESSAGES_PER_CHAT;
        return chat.messages.drain(..excess).collect();
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incoming(chat: &str, id: &str, ts: i64, from_me: bool) -> Incoming {
        Incoming {
            chat: chat.into(),
            is_group: false,
            sender: Some(chat.into()),
            push_name: Some("Maya".into()),
            lid_pn_pairs: Vec::new(),
            message: StoredMessage {
                id: id.into(),
                from_me,
                ts_ms: ts,
                author_jid: None,
                content: Content::text("hi", None),
                status: from_me.then_some(Delivery::Sent),
                reactions: Vec::new(),
                wire: None,
                read_by: Vec::new(),
                starred: false,
                deleted: None,
            },
        }
    }

    #[test]
    fn mentions_show_the_name_when_known() {
        let mut store = ChatStore::default();
        store.learn_mapping("68689445540020@lid", "351900000000@s.whatsapp.net");
        store.set_contact_name("351900000000@s.whatsapp.net", "Rui Silva");
        store.set_push_name("351911111111@s.whatsapp.net", "Maya");
        store.set_own_ids(["351922222222@s.whatsapp.net".to_string()]);
        let (text, names) =
            store.with_mentions("@68689445540020 e @351911111111, @351922222222 @99999999 mail@351911111111");
        assert_eq!(text, "@Rui Silva e @Maya, @You @99999999 mail@351911111111");
        assert_eq!(names, ["@Rui Silva", "@Maya", "@You"]);

        let aci = "0a1b2c3d-0000-4000-8000-00000000abcd";
        store.set_contact_name(aci, "Inês");
        let (text, _) = store.with_mentions(&format!("olá @{aci}!"));
        assert_eq!(text, "olá @Inês!");
    }

    #[test]
    fn search_ignores_case_and_accents_and_wants_every_word() {
        let mut store = ChatStore::default();
        for (id, ts, text) in [("a", 1, "Não sei se amanhã dá"), ("b", 2, "Amanha às 9h"), ("c", 3, "outra coisa")] {
            let mut message = incoming("351900000000@s.whatsapp.net", id, ts, false);
            message.message.content = Content::text(text, None);
            store.ingest(message, false);
        }
        let ids = |query: &str| store.search(query, None, 10, Vec::new()).into_iter().map(|f| f.message.id).collect::<Vec<_>>();
        assert_eq!(ids("AMANHÃ"), ["b", "a"]);
        assert_eq!(ids("nao amanha"), ["a"]);
        assert!(ids("  ").is_empty());
        assert!(ids("ninguém").is_empty());
    }

    #[test]
    fn reading_names_the_unread_messages_as_they_arrived() {
        let mut store = ChatStore::default();
        store.learn_mapping("111@lid", "351900000000@s.whatsapp.net");
        store.ingest(incoming("351900000000@s.whatsapp.net", "a", 1, false), false);
        store.ingest(incoming("111@lid", "b", 2, false), true);
        store.ingest(incoming("351900000000@s.whatsapp.net", "c", 3, false), true);

        let read = store.mark_read("351900000000@s.whatsapp.net");
        assert_eq!(read.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["c", "b"]);
        assert_eq!(read[0].chat, "351900000000@s.whatsapp.net");
        assert_eq!(read[1].chat, "111@lid");
        assert_eq!(read[1].sender.as_deref(), Some("111@lid"));
        assert!(store.mark_read("351900000000@s.whatsapp.net").is_empty());
        assert_eq!(store.chat_of("b").as_deref(), Some("351900000000@s.whatsapp.net"));
    }

    #[test]
    fn lid_chat_merges_into_phone_chat() {
        let mut store = ChatStore::default();
        store.ingest(incoming("111@lid", "a", 1, false), true);
        store.ingest(incoming("351900000000@s.whatsapp.net", "b", 2, false), true);
        store.learn_mapping("111@lid", "351900000000@s.whatsapp.net");

        let chats = store.list_chats();
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].id, "351900000000@s.whatsapp.net");
        assert_eq!(chats[0].unread, 2);
        assert_eq!(store.messages("111@lid").len(), 2);
    }

    #[test]
    fn history_keeps_unread_behind_older_own_replies() {
        let mut store = ChatStore::default();
        store.ingest_conversation(ConversationMeta {
            id: "1@s.whatsapp.net",
            name: None,
            pn_jid: None,
            lid_jid: None,
            unread: Some(2),
            marked_unread: None,
            pinned_at_s: None,
            archived: None,
            mute_end: None,
            conversation_ts_s: None,
        });
        store.ingest(incoming("1@s.whatsapp.net", "c", 3, false), false);
        store.ingest(incoming("1@s.whatsapp.net", "b", 2, false), false);
        store.ingest(incoming("1@s.whatsapp.net", "a", 1, true), false);
        assert_eq!(store.list_chats()[0].unread, 2);

        store.ingest(incoming("1@s.whatsapp.net", "d", 4, true), true);
        assert_eq!(store.list_chats()[0].unread, 0);
    }

    #[test]
    fn duplicate_messages_are_ignored() {
        let mut store = ChatStore::default();
        assert!(store.ingest(incoming("1@s.whatsapp.net", "a", 1, false), true).is_some());
        assert!(store.ingest(incoming("1@s.whatsapp.net", "a", 1, false), true).is_none());
        assert_eq!(store.list_chats()[0].unread, 1);
    }

    #[test]
    fn receipts_only_raise_status() {
        let mut store = ChatStore::default();
        store.ingest(incoming("1@s.whatsapp.net", "a", 1, true), false);
        assert_eq!(store.apply_receipt("1@s.whatsapp.net", &["a".into()], Delivery::Read, None).len(), 1);
        assert!(store.apply_receipt("1@s.whatsapp.net", &["a".into()], Delivery::Delivered, None).is_empty());
    }

    #[test]
    fn history_raises_the_ticks_of_a_known_message() {
        let mut store = ChatStore::default();
        store.ingest(incoming("1@s.whatsapp.net", "a", 1, true), false);
        store.ingest(incoming("1@s.whatsapp.net", "b", 2, false), false);
        let stale = store.stale_ticks(5, 50);
        assert_eq!(stale, [("1@s.whatsapp.net".to_string(), "b".to_string(), false, 2)]);
        assert!(store.stale_ticks(5, 50).is_empty());

        let mut again = incoming("1@s.whatsapp.net", "a", 1, true);
        again.message.status = Some(Delivery::Read);
        assert!(store.ingest(again, false).is_none());
        assert_eq!(store.messages("1@s.whatsapp.net")[0].status, Some(Delivery::Read));
        assert!(store.ingest(incoming("1@s.whatsapp.net", "a", 1, true), false).is_none());
        assert_eq!(store.messages("1@s.whatsapp.net")[0].status, Some(Delivery::Read));
    }

    #[test]
    fn a_receipt_covers_earlier_messages_one_to_one() {
        let mut store = ChatStore::default();
        for (id, ts) in [("a", 1), ("b", 2), ("c", 3)] {
            store.ingest(incoming("1@s.whatsapp.net", id, ts, true), false);
        }
        let changed = store.apply_receipt("1@s.whatsapp.net", &["b".into()], Delivery::Read, Some("1@s.whatsapp.net"));
        assert_eq!(changed.len(), 2);
        let status = |id: &str| store.messages("1@s.whatsapp.net").into_iter().find(|m| m.id == id).unwrap().status;
        assert_eq!(status("a"), Some(Delivery::Read));
        assert_eq!(status("c"), Some(Delivery::Sent));
    }

    #[test]
    fn a_group_message_is_read_once_everyone_read_it() {
        let mut store = ChatStore::default();
        store.set_own_ids(["me@s.whatsapp.net".to_string()]);
        let mut message = incoming("crew@g.us", "a", 1, true);
        message.is_group = true;
        store.ingest(message, false);
        let read = |store: &mut ChatStore, who: &str| {
            store.apply_receipt("crew@g.us", &["a".into()], Delivery::Read, Some(who))
        };

        assert!(read(&mut store, "me@s.whatsapp.net").is_empty());
        assert_eq!(read(&mut store, "1@s.whatsapp.net")[0].status, Some(Delivery::Delivered));
        assert!(store.claim_member_check("crew@g.us"));
        assert!(!store.claim_member_check("crew@g.us"));
        assert!(store.set_group_members("crew@g.us", 3).is_empty());
        assert!(read(&mut store, "1@s.whatsapp.net").is_empty());
        assert_eq!(read(&mut store, "2@s.whatsapp.net")[0].status, Some(Delivery::Read));

        // The size can also arrive after the last reader.
        let mut message = incoming("crew@g.us", "b", 2, true);
        message.is_group = true;
        store.ingest(message, false);
        store.set_group_members("crew@g.us", 4);
        store.apply_receipt("crew@g.us", &["b".into()], Delivery::Read, Some("1@s.whatsapp.net"));
        store.apply_receipt("crew@g.us", &["b".into()], Delivery::Read, Some("2@s.whatsapp.net"));
        assert_eq!(store.set_group_members("crew@g.us", 3)[0].status, Some(Delivery::Read));
    }

    #[test]
    fn history_does_not_clear_app_state_pins() {
        let mut store = ChatStore::default();
        store.set_pinned("1@s.whatsapp.net", Some(5_000));
        store.set_archived("2@s.whatsapp.net", true);
        for id in ["1@s.whatsapp.net", "2@s.whatsapp.net", "3@s.whatsapp.net"] {
            store.ingest_conversation(ConversationMeta {
                id,
                name: None,
                pn_jid: None,
                lid_jid: None,
                unread: None,
                marked_unread: None,
                pinned_at_s: None,
                archived: None,
                mute_end: None,
                conversation_ts_s: None,
            });
            store.ingest(incoming(id, "a", 1, false), false);
        }
        store.ingest(incoming("3@s.whatsapp.net", "b", 9, false), false);

        let chats = store.list_chats();
        assert_eq!(chats.len(), 2);
        assert_eq!(chats[0].id, "1@s.whatsapp.net");
        assert!(chats[0].pinned);
        assert!(!chats[1].pinned);
    }

    #[test]
    fn old_cache_loads_and_asks_for_chat_state() {
        let old = br#"{"version":1,"chats":{"1@s.whatsapp.net":{"name":null,"is_group":false,"pinned_at":null,"archived":false,"muted_until_ms":null,"unread":0,"last_ts_ms":0,"messages":[]}},"contact_names":{},"push_names":{},"lid_to_pn":{},"own_ids":[]}"#;
        let store = ChatStore::from_json(old).expect("old cache still loads");
        assert!(store.needs_chat_state_sync());
        assert!(!ChatStore::default().needs_chat_state_sync());
    }

    #[test]
    fn reactions_replace_and_clear() {
        let mut store = ChatStore::default();
        store.set_own_ids(["me".to_string()]);
        store.ingest(incoming("maya", "a", 1, false), true);
        store.set_reaction("maya", "a", "maya", Some("👍"));
        store.set_reaction("maya", "a", "me", Some("👍"));
        let dto = store.set_reaction("maya", "a", "maya", Some("❤️")).unwrap();
        assert_eq!(dto.reactions.len(), 2);
        assert!(dto.reactions[0].mine && dto.reactions[0].count == 1);
        assert_eq!(store.reaction_target("maya", "a").unwrap().mine.as_deref(), Some("👍"));

        let dto = store.set_reaction("maya", "a", "me", None).unwrap();
        assert_eq!(dto.reactions.len(), 1);
        assert!(store.set_reaction("maya", "missing", "me", Some("👍")).is_none());
    }

    #[test]
    fn receipts_find_their_chat() {
        let mut store = ChatStore::default();
        store.set_group_name("g.ab", "Crew");
        let mut message = incoming("g.ab", "7", 7, true);
        message.is_group = true;
        store.ingest(message, false);
        store.ingest(incoming("someone", "8", 8, true), false);

        let changed = store.apply_receipt_anywhere(&["8".into()], Delivery::Read);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].chat_id, "someone");
        assert_eq!(store.list_chats().iter().find(|c| c.id == "g.ab").unwrap().name, "Crew");
    }

    #[test]
    fn own_avatar_is_checked_once_until_invalidated() {
        let mut store = ChatStore::default();
        assert!(store.own_avatar_due().is_none());
        store.set_own_ids(["1@s.whatsapp.net".to_string()]);
        assert_eq!(store.own_avatar_due(), Some(("1@s.whatsapp.net".into(), None)));

        store.set_own_avatar(Some("77".into()));
        assert!(store.own_avatar_due().is_none());
        assert_eq!(store.own_avatar().as_deref(), Some("me-77.jpg"));

        store.invalidate_avatar("1:3@s.whatsapp.net");
        assert_eq!(store.own_avatar_due(), Some(("1@s.whatsapp.net".into(), Some("77".into()))));

        // The picture survives a restart; having checked it does not.
        let reloaded = ChatStore::from_json(&store.to_json().unwrap()).unwrap();
        assert_eq!(reloaded.own_avatar().as_deref(), Some("me-77.jpg"));
        assert!(reloaded.own_avatar_due().is_some());
    }

    #[test]
    fn only_chats_without_a_picture_are_retried() {
        let mut store = ChatStore::default();
        store.ingest(incoming("maya", "a", 1, false), true);
        store.set_avatar("maya", None);
        assert!(store.avatars_due(5).is_empty());
        assert!(store.retry_missing_avatar("maya"));
        assert_eq!(store.avatars_due(5).len(), 1);

        store.set_avatar("maya", Some("p".into()));
        assert!(!store.retry_missing_avatar("maya"));
        assert!(!store.retry_missing_avatar("nobody"));
    }

    #[test]
    fn a_star_before_its_message_is_kept() {
        let mut store = ChatStore::default();
        assert!(store.set_starred("a@s.whatsapp.net", "m1", true).is_none());
        store.ingest(incoming("a@s.whatsapp.net", "m1", 1, false), false);
        assert_eq!(store.starred().len(), 1);
    }

    #[test]
    fn favorites_and_lists_follow_the_lid_mapping() {
        let mut store = ChatStore::default();
        store.ingest(incoming("351900000000@s.whatsapp.net", "a", 1, false), false);
        store.ingest(incoming("crew@g.us", "b", 2, false), false);
        store.set_favorites(vec!["111@lid".into()]);
        store.edit_list("1", Some("Family"), true, Some(0), None);
        store.set_list_chat("1", "111@lid", true);
        store.edit_list("2", Some("Unread"), false, Some(1), None);
        store.set_list_chat("2", "crew@g.us", true);
        store.learn_mapping("111@lid", "351900000000@s.whatsapp.net");

        let chats = store.list_chats();
        let maya = chats.iter().find(|c| c.id == "351900000000@s.whatsapp.net").unwrap();
        assert!(maya.favorite);
        assert_eq!(maya.lists, ["1"]);
        assert!(chats.iter().find(|c| c.id == "crew@g.us").unwrap().lists.is_empty());
        assert_eq!(store.lists().iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Family"]);
        assert_eq!(store.next_list(), ("3".to_string(), 2));

        assert!(store.set_favorite("351900000000@s.whatsapp.net", false).is_empty());
        assert_eq!(store.set_favorite("crew@g.us", true), ["crew@g.us"]);
        store.set_list_chat("1", "351900000000@s.whatsapp.net", false);
        assert!(store.list_chat_ids("1").is_empty());
    }

    #[test]
    fn stars_survive_history_and_list_newest_first() {
        let mut store = ChatStore::default();
        store.ingest(incoming("1@s.whatsapp.net", "a", 1, false), false);
        store.ingest(incoming("2@s.whatsapp.net", "b", 2, true), false);
        assert!(store.set_starred("1@s.whatsapp.net", "a", true).unwrap().starred);
        let mut again = incoming("2@s.whatsapp.net", "b", 2, true);
        again.message.starred = true;
        store.ingest(again, false);
        store.ingest(incoming("1@s.whatsapp.net", "a", 1, false), false);

        let starred = store.starred();
        assert_eq!(starred.iter().map(|s| s.message.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert!(store.set_starred("1@s.whatsapp.net", "missing", true).is_none());
    }

    #[test]
    fn notices_say_who_was_replied_to_and_reacted() {
        let mut store = ChatStore::default();
        store.set_own_ids(["me@s.whatsapp.net".to_string()]);
        store.set_contact_name("1@s.whatsapp.net", "Maya");
        store.set_contact_name("2@s.whatsapp.net", "Rui");
        store.set_group_name("crew@g.us", "Crew");
        let group_message = |id: &str, from: &str, reply_to: Option<&str>| {
            let mut m = incoming("crew@g.us", id, 1, false);
            m.is_group = true;
            m.sender = Some(from.into());
            m.message.content = Content::text(
                "sure",
                reply_to.map(|to| ReplyRef { author_jid: Some(to.into()), id: None, text: "?".into() }),
            );
            m
        };
        store.ingest(group_message("a", "1@s.whatsapp.net", Some("me@s.whatsapp.net")), true);
        store.ingest(group_message("b", "1@s.whatsapp.net", Some("2@s.whatsapp.net")), true);
        store.ingest(group_message("c", "2@s.whatsapp.net", None), true);

        let body = |id: &str| store.message_notice("crew@g.us", id).unwrap().body;
        assert_eq!(body("a"), "Maya replied to you: sure");
        assert_eq!(body("b"), "Maya replied to Rui: sure");
        assert_eq!(body("c"), "Rui: sure");
        assert_eq!(store.message_notice("crew@g.us", "a").unwrap().title, "Crew");

        let mut mine = incoming("1@s.whatsapp.net", "d", 2, true);
        mine.message.content = Content::text("see you at the station tomorrow morning, before the train leaves at nine", None);
        store.ingest(mine, false);
        let mut reply = incoming("1@s.whatsapp.net", "e", 3, false);
        reply.message.content = Content::text("ok", Some(ReplyRef { author_jid: Some("me@s.whatsapp.net".into()), id: None, text: "see".into() }));
        store.ingest(reply, true);
        assert_eq!(store.message_notice("1@s.whatsapp.net", "e").unwrap().body, "Replied to you: ok");
        assert!(store.message_notice("1@s.whatsapp.net", "d").is_none());

        let reacted = store.reaction_notice("1@s.whatsapp.net", "d", "1@s.whatsapp.net", "❤️").unwrap();
        assert_eq!(
            reacted.body,
            "Reacted ❤️ to \u{201c}see you at the station tomorrow morning, before the train le…\u{201d}"
        );
        assert!(store.reaction_notice("1@s.whatsapp.net", "d", "me@s.whatsapp.net", "❤️").is_none());
        assert!(store.reaction_notice("1@s.whatsapp.net", "e", "1@s.whatsapp.net", "❤️").is_none());
    }

    #[test]
    fn contacts_list_saved_people_and_us() {
        let mut store = ChatStore::default();
        store.set_own_ids(["351911111111@s.whatsapp.net".to_string()]);
        store.set_contact_name("351900000000@s.whatsapp.net", "Maya");
        store.set_push_name("351922222222@s.whatsapp.net", "Stranger");
        let contacts = store.contacts();
        assert_eq!(contacts.len(), 2);
        let maya = contacts.iter().find(|c| !c.is_me).unwrap();
        assert_eq!((maya.name.as_str(), maya.phone.as_deref()), ("Maya", Some("+351900000000")));
        assert!(contacts.iter().any(|c| c.is_me && c.id == "351911111111@s.whatsapp.net"));
    }

    #[test]
    fn device_suffix_is_stripped() {
        assert_eq!(strip_device("123:4@s.whatsapp.net"), "123@s.whatsapp.net");
        assert_eq!(strip_device("123@c.us"), "123@s.whatsapp.net");
    }

    #[test]
    fn single_signal_attachment_moves_into_the_list() {
        let mut json = serde_json::to_value(ChatStore::default()).unwrap();
        json["version"] = STORE_VERSION.into();
        json["chats"] = serde_json::json!({ "x": { "messages": [{
                "id": "1", "from_me": false, "ts_ms": 1, "author_jid": "x", "status": null,
                "content": {
                    "kind": "image", "text": null, "media_label": "Photo", "voice_seconds": null, "reply_to": null,
                    "media": { "mime": "image/jpeg", "cdn": 3, "cdn_key": "abc", "key": "00", "size": 10 },
                },
            }]}});
        let store = ChatStore::from_json(&serde_json::to_vec(&json).unwrap()).expect("old file loads");
        let dto = &store.messages("x")[0];
        assert!(dto.has_media);
        assert_eq!(dto.attachments.len(), 1);
        assert_eq!(dto.attachments[0].kind, "image");
        let saved = String::from_utf8(store.to_json().unwrap()).unwrap();
        assert!(saved.contains(r#""attachments":[{"#) && !saved.contains(r#""media":{"#));
    }

    #[test]
    fn deleted_messages_stay_deleted() {
        let chat = "351900000000@s.whatsapp.net";
        let mut store = ChatStore::default();
        store.ingest(incoming(chat, "a", 1, false), false);
        store.ingest(incoming(chat, "b", 2, true), false);

        let gone = store.delete_message(chat, "b", Deletion::ForEveryone).unwrap();
        assert!(gone.deleted && gone.text.is_none());
        assert_eq!(store.list_chats()[0].last_message, "You deleted this message");

        store.delete_message(chat, "a", Deletion::ForMe);
        assert_eq!(store.messages(chat).iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["b"]);
        // A history replay does not bring either back.
        store.ingest(incoming(chat, "a", 1, false), false);
        store.ingest(incoming(chat, "b", 2, true), false);
        let shown = store.messages(chat);
        assert_eq!(shown.len(), 1);
        assert!(shown[0].deleted && store.quote_source(chat, "b").is_none());
    }
}
