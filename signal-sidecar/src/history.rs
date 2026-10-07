//! Message history transfer: right after linking, the phone uploads an
//! encrypted archive of its chats for the new device to import.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use futures::AsyncReadExt;
use libsignal_message_backup::frame::{CursorFactory, FramesReader};
use libsignal_message_backup::key::MessageBackupKey;
use libsignal_message_backup::parse::VarintDelimitedReader;
use libsignal_message_backup::proto::backup as proto;
use presage::libsignal_service::configuration::Endpoint;
use presage::libsignal_service::libsignal_account_keys::BackupKey;
use presage::libsignal_service::prelude::{ProfileKey, Uuid};
use presage::libsignal_service::proto::AttachmentPointer;
use presage::libsignal_service::proto::attachment_pointer::AttachmentIdentifier;
use presage::libsignal_service::protocol::Aci;
use presage::libsignal_service::push_service::{HttpAuthOverride, PushService};
use presage::store::ContentsStore;
use presage_store_sqlite::SqliteStore;
use protobuf::Message as _;
use serde::Deserialize;

use crate::{Event, GROUP_PREFIX, Media, Quote, attachment_kind, summarize};

/// How long to wait for the phone to build and upload the archive.
const UPLOAD_WINDOW: Duration = Duration::from_secs(15 * 60);
/// Seconds the server holds one request open; below the HTTP client's timeout.
const POLL_SECS: u32 = 50;

pub struct HistoryAccess {
    /// One-time key the phone sent while linking.
    pub key: [u8; 32],
    pub aci: Uuid,
    /// Authenticated as the new device.
    pub service: PushService,
}

#[derive(Deserialize)]
struct TransferArchive {
    cdn: Option<u32>,
    key: Option<String>,
    error: Option<String>,
}

fn progress(state: &'static str, percent: Option<u32>, error: Option<String>) {
    crate::emit(Event::History { state, percent, error });
}

/// Waits for the phone's upload. `Ok(None)` means there is nothing to import.
async fn locate(service: &PushService) -> Result<Option<(u32, String)>, String> {
    let deadline = Instant::now() + UPLOAD_WINDOW;
    while Instant::now() < deadline {
        let path = format!("/v1/devices/transfer_archive?timeout={POLL_SECS}");
        let response = service
            .request(reqwest::Method::GET, Endpoint::service(path), HttpAuthOverride::NoOverride)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        match response.status().as_u16() {
            // Nothing uploaded yet.
            204 => continue,
            200 => {
                let archive: TransferArchive = response.json().await.map_err(|e| e.to_string())?;
                return match (archive.cdn, archive.key, archive.error) {
                    (Some(cdn), Some(key), _) => Ok(Some((cdn, key))),
                    (_, _, Some(error)) if error == "CONTINUE_WITHOUT_UPLOAD" => Ok(None),
                    (_, _, Some(error)) => Err(format!("the phone reported {error}")),
                    _ => Err("Signal sent an empty archive location".into()),
                };
            }
            status => return Err(format!("Signal answered {status}")),
        }
    }
    Err("the phone did not upload the archive in time".into())
}

async fn download(service: &mut PushService, cdn: u32, key: String) -> Result<Vec<u8>, String> {
    let pointer = AttachmentPointer {
        cdn_number: Some(cdn),
        attachment_identifier: Some(AttachmentIdentifier::CdnKey(key)),
        ..Default::default()
    };
    let mut archive = Vec::new();
    let mut download = service.get_attachment(&pointer).await.map_err(|e| e.to_string())?;
    download.stream.read_to_end(&mut archive).await.map_err(|e| e.to_string())?;
    Ok(archive)
}

enum Recipient {
    Contact(String),
    Group(String),
    Me,
    Other,
}

fn uuid(bytes: &Option<Vec<u8>>) -> Option<Uuid> {
    Uuid::from_slice(bytes.as_deref()?).ok()
}

fn full_name(given: &str, family: &str) -> Option<String> {
    let name = format!("{} {}", given.trim(), family.trim());
    Some(name.trim().to_string()).filter(|n| !n.is_empty())
}

/// Profile keys the archive holds: they decrypt names and pictures.
type Keys = Vec<(Uuid, [u8; 32])>;

fn recipient(
    me: Uuid,
    recipient: &proto::Recipient,
    emit: &mut impl FnMut(Event),
    keys: &mut Keys,
) -> Recipient {
    use proto::recipient::Destination;
    match &recipient.destination {
        Some(Destination::Self_(_)) => Recipient::Me,
        Some(Destination::Contact(contact)) => {
            let Some(id) = uuid(&contact.aci).or_else(|| uuid(&contact.pni)) else {
                return Recipient::Other;
            };
            if id == me {
                return Recipient::Me;
            }
            let key = contact.profileKey.as_deref().and_then(|k| <[u8; 32]>::try_from(k).ok());
            if let (Some(aci), Some(key)) = (uuid(&contact.aci), key) {
                keys.push((aci, key));
            }
            let saved = full_name(&contact.systemGivenName, &contact.systemFamilyName)
                .or_else(|| full_name(&contact.nickname.given, &contact.nickname.family));
            let profile = full_name(
                contact.profileGivenName.as_deref().unwrap_or(""),
                contact.profileFamilyName.as_deref().unwrap_or(""),
            );
            if let Some(name) = saved.clone().or(profile) {
                emit(Event::Contact { id: id.to_string(), name, saved: saved.is_some() });
            }
            Recipient::Contact(id.to_string())
        }
        Some(Destination::Group(group)) => {
            let id = format!("{GROUP_PREFIX}{}", hex::encode(&group.masterKey));
            if let Some(proto::group::group_attribute_blob::Content::Title(title)) =
                &group.snapshot.title.content
            {
                emit(Event::Group { id: id.clone(), name: title.clone() });
            }
            Recipient::Group(id)
        }
        _ => Recipient::Other,
    }
}

/// A message's attachments with their kinds; a sticker counts as one.
fn pointers(item: &proto::chat_item::Item) -> Vec<(&proto::FilePointer, &'static str)> {
    use proto::chat_item::Item;
    use proto::message_attachment::Flag;
    match item {
        Item::StandardMessage(message) => message
            .attachments
            .iter()
            .map(|a| {
                let flag = a.flag.enum_value();
                let mime = a.pointer.contentType.as_deref().unwrap_or("");
                (&*a.pointer, attachment_kind(mime, flag == Ok(Flag::VOICE_MESSAGE), flag == Ok(Flag::GIF)))
            })
            .collect(),
        Item::StickerMessage(message) => vec![(&*message.sticker.data, "sticker")],
        _ => Vec::new(),
    }
}

/// An attachment the phone re-uploaded for the transfer. Older ones are not
/// on the server any more.
fn media_of(pointer: &proto::FilePointer, kind: &'static str) -> Option<Media> {
    let locator = pointer.locatorInfo.as_ref()?;
    if locator.key.is_empty() {
        return None;
    }
    Some(Media {
        mime: pointer.contentType.clone().unwrap_or_default(),
        cdn: locator.transitCdnNumber?,
        cdn_key: locator.transitCdnKey.clone()?,
        key: hex::encode(&locator.key),
        size: locator.size,
        kind: kind.into(),
        file_name: pointer.fileName.clone().filter(|n| !n.trim().is_empty()),
        width: pointer.width.unwrap_or(0),
        height: pointer.height.unwrap_or(0),
    })
}

/// Kind, text and media label for a message, as the live stream reports them.
fn describe(item: &proto::chat_item::Item) -> Option<(&'static str, Option<String>, Option<String>)> {
    use proto::chat_item::Item;
    match item {
        Item::StandardMessage(message) => {
            let text = Some(message.text.body.clone()).filter(|t| !t.trim().is_empty());
            let kinds: Vec<_> = pointers(item)
                .into_iter()
                .map(|(pointer, kind)| (kind, pointer.fileName.clone()))
                .collect();
            summarize(text, &kinds)
        }
        Item::StickerMessage(_) => Some(("media", None, Some("Sticker".into()))),
        Item::ContactMessage(_) => Some(("media", None, Some("Contact".into()))),
        Item::ViewOnceMessage(_) => Some(("media", None, Some("View-once media".into()))),
        Item::Poll(_) => Some(("media", None, Some("Poll".into()))),
        _ => None,
    }
}

/// The archive is encrypted like a regular backup, with the one-time key
/// standing in for the account's backup key.
fn archive_key(key: [u8; 32], aci: Uuid) -> MessageBackupKey {
    let backup_key: BackupKey = BackupKey(key);
    let backup_id = backup_key.derive_backup_id(&Aci::from(aci));
    MessageBackupKey::derive(&backup_key, &backup_id, None)
}

/// Decrypts the archive and reports its chats as events, collecting the
/// profile keys on the way. Returns how many messages it held.
async fn read(
    key: [u8; 32],
    aci: Uuid,
    archive: &[u8],
    emit: &mut impl FnMut(Event),
    keys: &mut Keys,
) -> Result<usize, String> {
    let key = archive_key(key, aci);
    let reader = FramesReader::new(&key, CursorFactory::new(archive))
        .await
        .map_err(|e| format!("could not decrypt the archive: {e}"))?;
    let mut frames = VarintDelimitedReader::new(reader);

    // The first record describes the archive; the rest are its contents.
    let mut records = Vec::new();
    while let Some(record) = frames.read_next().await.map_err(|e| e.to_string())? {
        records.push(record);
    }
    let total = records.len().saturating_sub(1).max(1);

    let me = aci.to_string();
    let mut recipients: HashMap<u64, Recipient> = HashMap::new();
    let mut chats: HashMap<u64, u64> = HashMap::new();
    let mut messages = 0;
    let mut last_percent = 0;
    for (index, record) in records.iter().skip(1).enumerate() {
        let percent = (index * 100 / total) as u32;
        if percent > last_percent {
            last_percent = percent;
            emit(Event::History { state: "importing", percent: Some(percent), error: None });
        }
        // A record this build cannot read is skipped, as the format asks.
        let Ok(frame) = proto::Frame::parse_from_bytes(record) else { continue };
        match frame.item {
            Some(proto::frame::Item::Recipient(r)) => {
                recipients.insert(r.id, recipient(aci, &r, emit, keys));
            }
            Some(proto::frame::Item::Chat(chat)) => {
                chats.insert(chat.id, chat.recipientId);
            }
            Some(proto::frame::Item::ChatItem(item)) => {
                use proto::chat_item::DirectionalDetails;
                let (from_me, delivery) = match &item.directionalDetails {
                    Some(DirectionalDetails::Outgoing(sent)) => (true, Some(delivery(&sent.sendStatus))),
                    Some(DirectionalDetails::Incoming(_)) => (false, None),
                    _ => continue,
                };
                let Some((kind, text, media_label)) = item.item.as_ref().and_then(describe) else {
                    continue;
                };
                let (chat, is_group) = match chats.get(&item.chatId).and_then(|id| recipients.get(id)) {
                    Some(Recipient::Contact(id)) => (id.clone(), false),
                    Some(Recipient::Group(id)) => (id.clone(), true),
                    Some(Recipient::Me) => (me.clone(), false),
                    _ => continue,
                };
                let sender = match recipients.get(&item.authorId) {
                    Some(Recipient::Contact(id)) => id.clone(),
                    _ => me.clone(),
                };
                let quote = match &item.item {
                    Some(proto::chat_item::Item::StandardMessage(m)) => m.quote.as_ref().map(|q| Quote {
                        author: match recipients.get(&q.authorId) {
                            Some(Recipient::Contact(id)) => Some(id.clone()),
                            Some(Recipient::Me) => Some(me.clone()),
                            _ => None,
                        },
                        text: Some(q.text.body.clone())
                            .filter(|t| !t.trim().is_empty())
                            .unwrap_or_else(|| "Message".into()),
                    }),
                    _ => None,
                };
                let reactions = match &item.item {
                    Some(proto::chat_item::Item::StandardMessage(m)) => m.reactions.as_slice(),
                    Some(proto::chat_item::Item::StickerMessage(m)) => m.reactions.as_slice(),
                    _ => &[],
                };
                let reactions: Vec<Event> = reactions
                    .iter()
                    .map(|reaction| Event::Reaction {
                        chat: chat.clone(),
                        sender: match recipients.get(&reaction.authorId) {
                            Some(Recipient::Contact(id)) => id.clone(),
                            _ => me.clone(),
                        },
                        target_author: sender.clone(),
                        target_ts: item.dateSent,
                        emoji: reaction.emoji.clone(),
                        remove: false,
                        history: true,
                    })
                    .collect();
                messages += 1;
                emit(Event::Message {
                    chat,
                    is_group,
                    sender,
                    from_me,
                    ts: item.dateSent,
                    kind,
                    text,
                    media_label,
                    quote,
                    attachments: item
                        .item
                        .as_ref()
                        .map(|i| pointers(i).into_iter().filter_map(|(p, kind)| media_of(p, kind)).collect())
                        .unwrap_or_default(),
                    history: true,
                    delivery,
                });
                reactions.into_iter().for_each(&mut *emit);
            }
            _ => {}
        }
    }
    Ok(messages)
}

/// How far an outgoing message got with whoever got it furthest, which is
/// what Signal shows.
fn delivery(statuses: &[proto::SendStatus]) -> &'static str {
    use proto::send_status::DeliveryStatus;
    let rank = |status: &proto::SendStatus| match status.deliveryStatus {
        Some(DeliveryStatus::Read(_) | DeliveryStatus::Viewed(_)) => 2,
        Some(DeliveryStatus::Delivered(_)) => 1,
        _ => 0,
    };
    match statuses.iter().map(rank).max() {
        Some(2) => "read",
        Some(1) => "delivered",
        _ => "sent",
    }
}

/// Imports the phone's message history. Linking works without it, so a
/// failure is reported and nothing more.
pub async fn import(mut access: HistoryAccess, dir: &Path, mut store: SqliteStore) {
    progress("waiting", Some(0), None);
    let outcome = async {
        let Some((cdn, key)) = locate(&access.service).await? else {
            return Ok(0);
        };
        let archive = download(&mut access.service, cdn, key).await?;
        let mut keys = Keys::new();
        let read = read(access.key, access.aci, &archive, &mut crate::emit, &mut keys).await;
        for (uuid, key) in keys {
            let _ = store.upsert_profile_key(&uuid, ProfileKey::create(key)).await;
        }
        if read.is_err() {
            // The phone only uploads once per link. Keeping the archive and
            // its key lets a failed import be retried without linking again.
            let _ = std::fs::write(dir.join("history-failed.bin"), &archive);
            let _ = std::fs::write(
                dir.join("history-failed.key"),
                format!("{} {}", hex::encode(access.key), access.aci),
            );
        }
        read
    }
    .await;
    match outcome {
        Ok(messages) => {
            eprintln!("signal: imported {messages} messages from the phone");
            progress("done", None, None);
        }
        Err(e) => {
            eprintln!("signal: history import failed: {e}");
            progress("failed", None, Some(format!("Could not import your message history: {e}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use libsignal_message_backup::export::{
        aes_cbc_encrypt, gzip_compress, hmac_checksum, pad_gzipped_bucketed,
    };

    use super::*;

    const ME: Uuid = Uuid::from_bytes([1; 16]);
    const MAYA: Uuid = Uuid::from_bytes([2; 16]);

    fn record(out: &mut Vec<u8>, message: &impl protobuf::Message) {
        message.write_length_delimited_to_vec(out).expect("writes to a buffer");
    }

    fn frame(item: proto::frame::Item) -> proto::Frame {
        proto::Frame { item: Some(item), ..Default::default() }
    }

    fn text(chat: u64, author: u64, sent: u64, outgoing: bool, body: &str) -> proto::Frame {
        use proto::chat_item::{DirectionalDetails, Item};
        frame(proto::frame::Item::ChatItem(proto::ChatItem {
            chatId: chat,
            authorId: author,
            dateSent: sent,
            directionalDetails: Some(if outgoing {
                DirectionalDetails::Outgoing(Default::default())
            } else {
                DirectionalDetails::Incoming(Default::default())
            }),
            item: Some(Item::StandardMessage(proto::StandardMessage {
                text: Some(proto::Text { body: body.into(), ..Default::default() }).into(),
                ..Default::default()
            })),
            ..Default::default()
        }))
    }

    /// An archive laid out the way the phone uploads one.
    fn archive(key: [u8; 32]) -> Vec<u8> {
        use proto::recipient::Destination;
        let mut plain = Vec::new();
        record(&mut plain, &proto::BackupInfo { version: 1, ..Default::default() });
        record(&mut plain, &frame(proto::frame::Item::Recipient(proto::Recipient {
            id: 1,
            destination: Some(Destination::Self_(Default::default())),
            ..Default::default()
        })));
        record(&mut plain, &frame(proto::frame::Item::Recipient(proto::Recipient {
            id: 2,
            destination: Some(Destination::Contact(proto::Contact {
                aci: Some(MAYA.as_bytes().to_vec()),
                profileKey: Some(vec![3; 32]),
                systemGivenName: "Maya".into(),
                systemFamilyName: "Chen".into(),
                ..Default::default()
            })),
            ..Default::default()
        })));
        record(&mut plain, &frame(proto::frame::Item::Chat(proto::Chat {
            id: 7,
            recipientId: 2,
            ..Default::default()
        })));
        record(&mut plain, &text(7, 2, 1000, false, "hi"));
        record(&mut plain, &text(7, 1, 2000, true, "hello"));

        let key = archive_key(key, ME);
        let iv = [9; 16];
        let mut body = gzip_compress(futures::io::Cursor::new(plain));
        pad_gzipped_bucketed(&mut body);
        aes_cbc_encrypt(&key.aes_key, &iv, &mut body);
        let hmac = hmac_checksum(&key.hmac_key, &iv, &body);
        [&iv[..], &body, &hmac].concat()
    }

    #[test]
    fn archive_becomes_chat_events() {
        let archive = archive([5; 32]);
        let mut events = Vec::new();
        let mut keys = Keys::new();
        let count = futures::executor::block_on(read([5; 32], ME, &archive, &mut |e| events.push(e), &mut keys))
            .expect("the archive decrypts");
        assert_eq!(count, 2);

        let lines: Vec<String> = events
            .iter()
            .filter(|e| !matches!(e, Event::History { .. }))
            .map(|e| serde_json::to_string(e).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains(r#""event":"contact""#) && lines[0].contains("Maya Chen"));
        assert!(lines[1].contains(&format!(r#""chat":"{MAYA}""#)) && lines[1].contains(r#""fromMe":false"#));
        assert!(lines[2].contains(r#""fromMe":true"#) && lines[2].contains(r#""history":true"#));
        assert_eq!(keys, [(MAYA, [3; 32])]);
    }

    #[test]
    fn wrong_key_is_rejected() {
        let archive = archive([5; 32]);
        let read = futures::executor::block_on(read([6; 32], ME, &archive, &mut |_| {}, &mut Keys::new()));
        assert!(read.is_err());
    }
}
