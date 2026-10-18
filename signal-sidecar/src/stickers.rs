//! Sticker packs made here, put on Signal's CDN the way Signal Desktop's
//! sticker creator does it, and sticker messages.
//!
//! A pack is a manifest and its stickers, each encrypted with keys derived
//! from a random pack key that only the people a sticker is sent to (and our
//! own devices) ever learn. The server hands out one signed upload form per
//! file and names the pack.

use std::path::PathBuf;

use presage::libsignal_service::configuration::Endpoint;
use presage::libsignal_service::content::DataMessage;
use presage::libsignal_service::prelude::{ProtobufMessage, Uuid};
use presage::libsignal_service::proto::data_message::{Quote as QuoteProto, Sticker};
use presage::libsignal_service::proto::sync_message::{StickerPackOperation, sticker_pack_operation};
use presage::libsignal_service::proto::{Pack, SyncMessage, pack};
use presage::libsignal_service::push_service::{AttachmentV2UploadAttributes, HttpAuthOverride, PushService};
use presage::libsignal_service::sender::AttachmentSpec;
use presage::libsignal_service::sticker_cipher::{derive_key, encrypt_in_place};
use rand::RngCore;
use serde::Deserialize;

use crate::{Media, SendQuote, Signal, UPLOAD_TIMEOUT, media_of, send, sync_to_self};

/// The most stickers Signal puts in a pack.
pub const PACK_MAX: usize = 200;
/// What the server takes for one sticker, before encryption adds its share.
const STICKER_MAX: usize = 300 * 1024;

/// One sticker of a pack to upload, waiting as a plain file at `path`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSticker {
    pub path: PathBuf,
    pub emoji: String,
    /// `image/webp` or `image/png` (APNG for an animated one).
    pub mime: String,
}

/// Where each file of a new pack goes, as the server signed it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadForm {
    pack_id: String,
    manifest: AttachmentV2UploadAttributes,
    stickers: Vec<AttachmentV2UploadAttributes>,
}

fn encrypted(key: [u8; 64], mut plain: Vec<u8>) -> Vec<u8> {
    let mut iv = [0u8; 16];
    rand::rng().fill_bytes(&mut iv);
    encrypt_in_place(iv, key, &mut plain);
    plain
}

/// Puts a pack on Signal's CDN and returns its id and key, both hex. The
/// first sticker is its cover, as in Signal's sticker creator.
pub async fn upload_pack(service: &PushService, title: &str, author: &str, stickers: Vec<PackSticker>) -> Result<(String, String), String> {
    if stickers.is_empty() {
        return Err("A pack needs at least one sticker.".into());
    }
    if stickers.len() > PACK_MAX {
        return Err(format!("A Signal pack holds {PACK_MAX} stickers at most."));
    }
    let mut pack_key = [0u8; 32];
    rand::rng().fill_bytes(&mut pack_key);
    let key = derive_key(&pack_key).map_err(|e| e.to_string())?;

    let mut files = Vec::with_capacity(stickers.len());
    let mut entries = Vec::with_capacity(stickers.len());
    for (id, sticker) in stickers.iter().enumerate() {
        let bytes = tokio::fs::read(&sticker.path).await.map_err(|e| format!("could not read a sticker: {e}"))?;
        if bytes.len() > STICKER_MAX {
            return Err(format!("Sticker {} is larger than Signal takes (300 KB).", id + 1));
        }
        files.push(encrypted(key, bytes));
        entries.push(pack::Sticker {
            id: Some(id as u32),
            emoji: Some(sticker.emoji.clone()),
            content_type: Some(sticker.mime.clone()),
        });
    }
    let manifest = Pack {
        title: Some(title.to_string()),
        author: Some(author.to_string()),
        cover: entries.first().cloned(),
        stickers: entries,
    };
    let manifest = encrypted(key, manifest.encode_to_vec());

    let mut service = service.clone();
    let form: UploadForm = {
        let path = format!("/v1/sticker/pack/form/{}", files.len());
        let response = service
            .request(reqwest::Method::GET, Endpoint::service(path), HttpAuthOverride::NoOverride)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        if status.as_u16() == 413 || status.as_u16() == 429 {
            return Err("Signal limits how many packs can be made at a time. Try again later.".into());
        }
        if !status.is_success() {
            return Err(format!("Signal refused to make the pack ({status})."));
        }
        let body = response.bytes().await.map_err(|e| e.to_string())?;
        serde_json::from_slice(&body).map_err(|e| format!("Signal's answer could not be read: {e}"))?
    };
    if form.stickers.len() != files.len() {
        return Err("Signal's answer did not match the pack.".into());
    }
    let uploading = async {
        service
            .upload_to_cdn0("/", form.manifest, "manifest.proto".into(), manifest.as_slice())
            .await
            .map_err(|e| format!("The pack was not uploaded: {e}"))?;
        for (n, (attributes, file)) in form.stickers.into_iter().zip(files).enumerate() {
            service
                .upload_to_cdn0("/", attributes, n.to_string(), file.as_slice())
                .await
                .map_err(|e| format!("Sticker {} was not uploaded: {e}", n + 1))?;
        }
        Ok::<_, String>(())
    };
    tokio::time::timeout(UPLOAD_TIMEOUT, uploading)
        .await
        .map_err(|_| "Signal did not take the pack in time.".to_string())??;
    Ok((form.pack_id, hex::encode(pack_key)))
}

/// Tells our other devices, the phone among them, to install a pack (or
/// remove it), as Signal's apps do when one is added on any of them.
pub async fn sync_pack(manager: &mut Signal, me: Uuid, pack_id: &str, pack_key: &str, install: bool, ts: u64) -> Result<(), String> {
    let kind = if install { sticker_pack_operation::Type::Install } else { sticker_pack_operation::Type::Remove };
    let sync = SyncMessage {
        sticker_pack_operation: vec![StickerPackOperation {
            pack_id: Some(hex::decode(pack_id).map_err(|_| "Unknown pack.")?),
            pack_key: Some(hex::decode(pack_key).map_err(|_| "Unknown pack.")?),
            r#type: Some(kind as i32),
        }],
        ..SyncMessage::with_padding(&mut rand::rng())
    };
    sync_to_self(manager, me, sync, ts).await
}

/// A sticker to send: which pack it is from, and its file.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingSticker {
    pub chat: String,
    pub pack_id: String,
    pub pack_key: String,
    pub sticker_id: u32,
    pub emoji: Option<String>,
    pub path: PathBuf,
    pub mime: String,
    pub quote: Option<SendQuote>,
}

/// Sends a sticker from a pack on Signal: the file goes as an attachment, so
/// it shows for people who don't have the pack, which they can then add.
pub async fn send_sticker(manager: &mut Signal, sticker: OutgoingSticker, ts: u64) -> Result<Media, String> {
    let bytes = tokio::fs::read(&sticker.path).await.map_err(|e| format!("could not read the sticker: {e}"))?;
    let spec = AttachmentSpec {
        content_type: sticker.mime.clone(),
        length: bytes.len(),
        width: Some(crate::STICKER_SIDE),
        height: Some(crate::STICKER_SIDE),
        ..Default::default()
    };
    let uploading = manager.upload_attachments(vec![(spec, bytes)]);
    let pointer = tokio::time::timeout(UPLOAD_TIMEOUT, uploading)
        .await
        .map_err(|_| "Signal did not take the sticker in time.".to_string())?
        .map_err(|e| e.to_string())?
        .pop()
        .ok_or("Nothing was uploaded.")?
        .map_err(|e| e.to_string())?;
    let media = media_of(&pointer, "sticker").ok_or("The upload came back without a reference.")?;
    let quote = sticker.quote.map(|q| QuoteProto {
        id: Some(q.ts),
        author_aci: Some(q.author),
        text: Some(q.text),
        ..Default::default()
    });
    let message = DataMessage {
        sticker: Some(Sticker {
            pack_id: Some(hex::decode(&sticker.pack_id).map_err(|_| "Unknown pack.")?),
            pack_key: Some(hex::decode(&sticker.pack_key).map_err(|_| "Unknown pack.")?),
            sticker_id: Some(sticker.sticker_id),
            data: Some(pointer),
            emoji: sticker.emoji,
        }),
        quote,
        ..Default::default()
    };
    send(manager, &sticker.chat, message, ts).await?;
    Ok(media)
}

#[cfg(test)]
mod tests {
    use super::*;
    use presage::libsignal_service::sticker_cipher::decrypt_in_place;

    #[test]
    fn stickers_decrypt_with_the_pack_key() {
        let pack_key = [9u8; 32];
        let key = derive_key(&pack_key).unwrap();
        let mut file = encrypted(key, b"RIFF....WEBP".to_vec());
        assert_ne!(file, b"RIFF....WEBP");
        decrypt_in_place(derive_key(&pack_key).unwrap(), &mut file).unwrap();
        assert_eq!(file, b"RIFF....WEBP");
    }

    #[test]
    fn the_upload_form_is_read() {
        let attrs = r#"{"id":0,"key":"stickers/ab/full/0","credential":"c","acl":"private","algorithm":"AWS4-HMAC-SHA256","date":"20261018T000000Z","policy":"p","signature":"s"}"#;
        let json = format!(r#"{{"packId":"ab","manifest":{},"stickers":[{attrs}]}}"#, attrs.replace("\"id\":0", "\"id\":-1"));
        let form: UploadForm = serde_json::from_str(&json).unwrap();
        assert_eq!(form.pack_id, "ab");
        assert_eq!(form.stickers.len(), 1);
    }
}
