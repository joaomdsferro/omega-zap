use whatsapp_rust::prelude::*;
use whatsapp_rust::waproto::whatsapp::web_message_info::Status as WebStatus;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

use super::store::{Content, Delivery, Incoming, LinkPreview, MsgKind, ReplyRef, StoredMessage, CdnRef};

fn non_empty(text: &Option<String>) -> Option<String> {
    text.as_ref().filter(|t| !t.trim().is_empty()).cloned()
}

fn reply_ref(ctx: &wa::ContextInfo) -> Option<ReplyRef> {
    let quoted = ctx.quoted_message.as_option()?;
    let text = content(quoted).map(|c| c.preview()).unwrap_or_else(|| "Message".into());
    Some(ReplyRef {
        author_jid: ctx.participant.clone(),
        text,
    })
}

fn encoded(field: &Option<Vec<u8>>) -> Option<String> {
    field.as_ref().filter(|b| !b.is_empty()).map(|b| BASE64.encode(b))
}

fn large_thumbnail(ext: &wa::message::ExtendedTextMessage) -> Option<CdnRef> {
    Some(CdnRef {
        path: non_empty(&ext.thumbnail_direct_path)?,
        key: encoded(&ext.media_key)?,
        sha256: encoded(&ext.thumbnail_sha256)?,
        enc_sha256: encoded(&ext.thumbnail_enc_sha256)?,
        size: 0,
        mime: None,
        width: ext.thumbnail_width.unwrap_or_default(),
        height: ext.thumbnail_height.unwrap_or_default(),
        kind: None,
        file_name: None,
    })
}

/// The fields every WhatsApp media message carries, whatever its type.
struct Fields<'a> {
    direct_path: &'a Option<String>,
    media_key: &'a Option<Vec<u8>>,
    file_sha256: &'a Option<Vec<u8>>,
    file_enc_sha256: &'a Option<Vec<u8>>,
    file_length: Option<u64>,
    mimetype: &'a Option<String>,
}

/// Where a file is on the CDN. `kind` is `None` for a photo.
fn file_ref(f: Fields<'_>, kind: Option<&str>, width: Option<u32>, height: Option<u32>) -> Option<CdnRef> {
    Some(CdnRef {
        path: non_empty(f.direct_path)?,
        key: encoded(f.media_key)?,
        sha256: encoded(f.file_sha256)?,
        enc_sha256: encoded(f.file_enc_sha256)?,
        size: f.file_length.unwrap_or_default(),
        mime: non_empty(f.mimetype),
        width: width.unwrap_or_default(),
        height: height.unwrap_or_default(),
        kind: kind.map(str::to_string),
        file_name: None,
    })
}

macro_rules! fields {
    ($m:expr) => {
        Fields {
            direct_path: &$m.direct_path,
            media_key: &$m.media_key,
            file_sha256: &$m.file_sha256,
            file_enc_sha256: &$m.file_enc_sha256,
            file_length: $m.file_length,
            mimetype: &$m.mimetype,
        }
    };
}

fn image_ref(image: &wa::message::ImageMessage) -> Option<CdnRef> {
    file_ref(fields!(image), None, image.width, image.height)
}

fn video_ref(video: &wa::message::VideoMessage, kind: &str) -> Option<CdnRef> {
    file_ref(fields!(video), Some(kind), video.width, video.height)
}

/// The link card of a text message, when the sender's phone made one.
fn link_preview(ext: &wa::message::ExtendedTextMessage) -> Option<LinkPreview> {
    let url = non_empty(&ext.matched_text)?;
    let title = non_empty(&ext.title);
    let description = non_empty(&ext.description);
    let thumbnail = ext
        .jpeg_thumbnail
        .as_ref()
        .filter(|bytes| !bytes.is_empty())
        .map(|bytes| format!("data:image/jpeg;base64,{}", BASE64.encode(bytes)));
    let large = large_thumbnail(ext);
    if title.is_none() && description.is_none() && thumbnail.is_none() && large.is_none() {
        return None;
    }
    Some(LinkPreview { url, title, description, thumbnail, large })
}

/// What the chat view can show for a message. `None` for protocol traffic
/// (reactions, edits, revokes, key distribution) and types we don't render.
pub fn content(message: &wa::Message) -> Option<Content> {
    let mut base = message.get_base_message();
    // Each picture of an album arrives wrapped as a child of the album.
    if let Some(child) = base.associated_child_message.as_option().and_then(|w| w.message.as_option()) {
        base = child.get_base_message();
    }

    if let Some(text) = non_empty(&base.conversation) {
        return Some(Content::text(text, None));
    }
    if let Some(ext) = base.extended_text_message.as_option() {
        let text = non_empty(&ext.text)?;
        let reply = ext.context_info.as_option().and_then(reply_ref);
        let mut c = Content::text(text, reply);
        c.link = link_preview(ext);
        return Some(c);
    }
    if let Some(image) = base.image_message.as_option() {
        let mut c = Content::media(MsgKind::Image, "Photo", non_empty(&image.caption));
        c.reply_to = image.context_info.as_option().and_then(reply_ref);
        c.wa_media = image_ref(image);
        return Some(c);
    }
    if let Some(video) = base.video_message.as_option() {
        let gif = video.gif_playback == Some(true);
        let mut c = Content::media(MsgKind::Media, if gif { "GIF" } else { "Video" }, non_empty(&video.caption));
        c.reply_to = video.context_info.as_option().and_then(reply_ref);
        c.wa_media = video_ref(video, if gif { "gif" } else { "video" });
        return Some(c);
    }
    if let Some(video) = base.ptv_message.as_option() {
        let mut c = Content::media(MsgKind::Media, "Video message", None);
        c.reply_to = video.context_info.as_option().and_then(reply_ref);
        c.wa_media = video_ref(video, "video");
        return Some(c);
    }
    if let Some(audio) = base.audio_message.as_option() {
        let voice = audio.ptt == Some(true);
        let mut c = if voice {
            Content::media(MsgKind::Voice, "Voice message", None)
        } else {
            Content::media(MsgKind::Media, "Audio", None)
        };
        c.voice_seconds = audio.seconds;
        c.reply_to = audio.context_info.as_option().and_then(reply_ref);
        c.wa_media = file_ref(fields!(audio), Some(if voice { "voice" } else { "audio" }), None, None);
        return Some(c);
    }
    if let Some(doc) = base.document_message.as_option() {
        let name = non_empty(&doc.file_name)
            .or_else(|| non_empty(&doc.title))
            .unwrap_or_else(|| "Document".into());
        let mut c = Content::media(MsgKind::Media, name.clone(), non_empty(&doc.caption));
        c.reply_to = doc.context_info.as_option().and_then(reply_ref);
        c.wa_media = file_ref(fields!(doc), Some("file"), None, None).map(|r| CdnRef { file_name: Some(name), ..r });
        return Some(c);
    }
    if let Some(sticker) = base.sticker_message.as_option() {
        let mut c = Content::media(MsgKind::Media, "Sticker", None);
        c.reply_to = sticker.context_info.as_option().and_then(reply_ref);
        c.wa_media = file_ref(fields!(sticker), Some("sticker"), sticker.width, sticker.height);
        return Some(c);
    }
    if let Some(location) = base.location_message.as_option() {
        return Some(Content::media(MsgKind::Media, "Location", non_empty(&location.name)));
    }
    if base.live_location_message.as_option().is_some() {
        return Some(Content::media(MsgKind::Media, "Live location", None));
    }
    if let Some(contact) = base.contact_message.as_option() {
        return Some(Content::media(MsgKind::Media, "Contact", non_empty(&contact.display_name)));
    }
    if base.contacts_array_message.as_option().is_some() {
        return Some(Content::media(MsgKind::Media, "Contacts", None));
    }
    let poll = [
        &base.poll_creation_message,
        &base.poll_creation_message_v2,
        &base.poll_creation_message_v3,
        &base.poll_creation_message_v5,
        &base.poll_creation_message_v6,
    ]
    .into_iter()
    .find_map(|p| p.as_option());
    if let Some(poll) = poll {
        return Some(Content::media(MsgKind::Media, "Poll", non_empty(&poll.name)));
    }
    None
}

/// The id of the message a live revoke takes back.
pub fn revoke(message: &wa::Message) -> Option<String> {
    let protocol = message.get_base_message().protocol_message.as_option()?;
    if protocol.r#type != Some(wa::message::protocol_message::Type::REVOKE) {
        return None;
    }
    protocol.key.as_option()?.id.clone()
}

/// A reaction carried by a live message: the id of the message it is on, and
/// the emoji, or `None` when the reaction was taken back.
pub fn reaction(message: &wa::Message) -> Option<(String, Option<String>)> {
    let reaction = message.get_base_message().reaction_message.as_option()?;
    let target = reaction.key.as_option()?.id.clone()?;
    Some((target, non_empty(&reaction.text)))
}

/// Reactions recorded on a history-sync message: who reacted and with what.
/// `None` as the author means us.
pub fn history_reactions(chat_id: &str, item: &wa::HistorySyncMsg) -> Vec<(Option<String>, String)> {
    let Some(web) = item.message.as_option() else {
        return Vec::new();
    };
    web.reactions
        .iter()
        .filter_map(|reaction| {
            let emoji = non_empty(&reaction.text)?;
            let key = reaction.key.as_option()?;
            let author = if key.from_me.unwrap_or(false) {
                None
            } else {
                Some(key.participant.clone().unwrap_or_else(|| chat_id.to_string()))
            };
            Some((author, emoji))
        })
        .collect()
}

/// A live (or offline-drained) message from the event stream.
pub fn live(message: &wa::Message, info: &MessageInfo) -> Option<Incoming> {
    let content = content(message)?;
    let source = &info.source;

    let mut lid_pn_pairs = Vec::new();
    if let Some(alt) = &source.sender_alt {
        lid_pn_pairs.push((source.sender.to_non_ad_string(), alt.to_non_ad_string()));
    }
    if !source.is_group
        && let Some(alt) = &source.recipient_alt
    {
        lid_pn_pairs.push((source.chat.to_non_ad_string(), alt.to_non_ad_string()));
    }

    Some(Incoming {
        chat: source.chat.to_non_ad_string(),
        is_group: source.is_group,
        sender: Some(source.sender.to_non_ad_string()),
        push_name: Some(info.push_name.to_string()).filter(|n| !n.is_empty()),
        lid_pn_pairs,
        message: StoredMessage {
            id: info.id.to_string(),
            from_me: source.is_from_me,
            ts_ms: info.timestamp.timestamp_millis(),
            author_jid: None,
            content,
            status: source.is_from_me.then_some(Delivery::Sent),
            reactions: Vec::new(),
            wire: None,
            read_by: Vec::new(),
            starred: false,
            deleted: None,
        },
    })
}

/// A message from a history-sync conversation.
pub fn history(chat_id: &str, item: &wa::HistorySyncMsg) -> Option<Incoming> {
    let web = item.message.as_option()?;
    let key = web.key.as_option()?;
    let content = content(web.message.as_option()?)?;
    let id = key.id.clone()?;
    let from_me = key.from_me.unwrap_or(false);
    let is_group = chat_id.ends_with("@g.us");
    let sender = if is_group {
        key.participant.clone().or_else(|| web.participant.clone())
    } else if from_me {
        None
    } else {
        Some(chat_id.to_string())
    };
    let status = from_me.then_some(match web.status {
        Some(WebStatus::READ | WebStatus::PLAYED) => Delivery::Read,
        Some(WebStatus::DELIVERY_ACK) => Delivery::Delivered,
        _ => Delivery::Sent,
    });

    Some(Incoming {
        chat: chat_id.to_string(),
        is_group,
        sender,
        push_name: non_empty(&web.push_name),
        lid_pn_pairs: Vec::new(),
        message: StoredMessage {
            id,
            from_me,
            ts_ms: web.message_timestamp.unwrap_or_default() as i64 * 1000,
            author_jid: None,
            content,
            status,
            reactions: Vec::new(),
            wire: None,
            read_by: Vec::new(),
            starred: web.starred.unwrap_or(false),
            deleted: None,
        },
    })
}
