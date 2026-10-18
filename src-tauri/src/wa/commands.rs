use std::sync::Arc;

use tauri::State;
use tauri::ipc::{CommandArg, CommandItem, InvokeError};
use tauri::Manager as _;

use super::store::{ContactDto, ListDto, MessageDto, StarredDto};
use super::{ACCOUNT_HEADER, ChatSummary, WaAccounts, WaManager};
use crate::account::AccountStatus as WaStatus;

/// The account a command is for: the business one when the UI names it in
/// the [`ACCOUNT_HEADER`], the one with the stickers when it says `stickers`,
/// else the personal one.
pub struct Mgr<'r>(&'r Arc<WaManager>);

impl Mgr<'_> {
    fn inner(&self) -> &Arc<WaManager> {
        self.0
    }
}

impl std::ops::Deref for Mgr<'_> {
    type Target = Arc<WaManager>;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'r, 'de: 'r, R: tauri::Runtime> CommandArg<'de, R> for Mgr<'r> {
    fn from_command(command: CommandItem<'de, R>) -> Result<Self, InvokeError> {
        let message = command.message;
        let accounts = message
            .webview_ref()
            .try_state::<WaAccounts>()
            .ok_or_else(|| InvokeError::from("WhatsApp is not ready yet."))?
            .inner();
        Ok(Mgr(match message.headers().get(ACCOUNT_HEADER).and_then(|v| v.to_str().ok()) {
            Some("business") => &accounts.business,
            // The sticker library, as the other networks use it.
            Some("stickers") => accounts.stickers(),
            _ => &accounts.personal,
        }))
    }
}

#[tauri::command]
pub fn wa_status(mgr: Mgr<'_>) -> WaStatus {
    mgr.status()
}

#[tauri::command]
pub async fn wa_start_linking(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.inner().clone().start_linking().await
}

#[tauri::command]
pub async fn wa_cancel_linking(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.inner().clone().cancel_linking().await;
    Ok(())
}

#[tauri::command]
pub async fn wa_reconnect(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.inner().clone().reconnect().await
}

#[tauri::command]
pub async fn wa_logout(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.inner().clone().logout().await;
    Ok(())
}

#[tauri::command]
pub fn wa_list_chats(mgr: Mgr<'_>) -> Vec<ChatSummary> {
    mgr.list_chats()
}

#[tauri::command]
pub fn wa_get_messages(mgr: Mgr<'_>, chat_id: String) -> Vec<MessageDto> {
    mgr.messages(&chat_id)
}

#[tauri::command]
pub fn wa_group_info(mgr: Mgr<'_>, id: String) -> crate::store::GroupInfoDto {
    mgr.group_info(&id)
}

#[tauri::command]
pub async fn wa_group_details(mgr: Mgr<'_>, id: String) -> Result<crate::store::GroupInfoDto, String> {
    mgr.group_details(&id).await
}

#[tauri::command]
pub fn wa_pinned_messages(mgr: Mgr<'_>, chat_id: String) -> crate::store::PinsDto {
    mgr.pinned_messages(&chat_id)
}

#[tauri::command]
pub async fn wa_pin_message(mgr: Mgr<'_>, chat_id: String, message_id: String, secs: Option<u32>) -> Result<(), String> {
    mgr.pin_message(&chat_id, &message_id, secs).await
}

#[tauri::command]
pub fn wa_mark_read(mgr: Mgr<'_>, chat_id: String) {
    mgr.mark_read(&chat_id);
}

#[tauri::command]
pub async fn wa_person_details(mgr: Mgr<'_>, id: String) -> Result<crate::store::PersonDetailsDto, String> {
    mgr.person_details(&id).await
}

#[tauri::command]
pub async fn wa_set_muted(mgr: Mgr<'_>, chat_id: String, muted: bool, until_ms: Option<i64>) -> Result<(), String> {
    mgr.set_muted(&chat_id, muted, until_ms).await
}

#[tauri::command]
pub async fn wa_set_typing(mgr: Mgr<'_>, chat_id: String, state: String) -> Result<(), String> {
    mgr.set_typing(&chat_id, &state).await
}

#[tauri::command]
pub async fn wa_watch_presence(mgr: Mgr<'_>, chat_id: String) -> Result<(), String> {
    mgr.watch_presence(&chat_id).await
}

#[tauri::command]
pub async fn wa_privacy(mgr: Mgr<'_>) -> Result<super::PrivacyDto, String> {
    mgr.privacy().await
}

#[tauri::command]
pub async fn wa_set_privacy(mgr: Mgr<'_>, category: String, value: String) -> Result<(), String> {
    mgr.set_privacy(&category, &value).await
}

#[tauri::command]
pub async fn wa_set_archived(mgr: Mgr<'_>, chat_id: String, archived: bool) -> Result<(), String> {
    mgr.set_archived(&chat_id, archived).await
}

#[tauri::command]
pub async fn wa_leave_group(mgr: Mgr<'_>, id: String) -> Result<(), String> {
    mgr.leave_group(&id).await
}

#[tauri::command]
pub async fn wa_edit_group(
    mgr: Mgr<'_>,
    id: String,
    name: Option<String>,
    description: Option<String>,
) -> Result<crate::store::GroupInfoDto, String> {
    mgr.edit_group(&id, name, description).await
}

#[tauri::command]
pub async fn wa_change_members(
    mgr: Mgr<'_>,
    id: String,
    change: String,
    members: Vec<String>,
) -> Result<crate::store::GroupInfoDto, String> {
    mgr.change_members(&id, &change, members).await
}

#[tauri::command]
pub async fn wa_set_blocked(mgr: Mgr<'_>, id: String, blocked: bool) -> Result<(), String> {
    mgr.set_blocked(&id, blocked).await
}

#[tauri::command]
pub async fn wa_delete_chat(mgr: Mgr<'_>, chat_id: String) -> Result<(), String> {
    mgr.delete_chat(&chat_id).await
}

#[tauri::command]
pub async fn wa_clear_chat(mgr: Mgr<'_>, chat_id: String, keep_starred: bool) -> Result<(), String> {
    mgr.clear_chat(&chat_id, keep_starred).await
}

#[tauri::command]
pub async fn wa_set_pinned(mgr: Mgr<'_>, chat_id: String, pinned: bool) -> Result<(), String> {
    mgr.set_pinned(&chat_id, pinned).await
}

#[tauri::command]
pub async fn wa_set_unread(mgr: Mgr<'_>, chat_id: String, unread: bool) -> Result<(), String> {
    mgr.set_unread(&chat_id, unread).await
}

#[tauri::command]
pub async fn wa_send_text(
    mgr: Mgr<'_>,
    chat_id: String,
    text: String,
    reply_to: Option<String>,
    link: Option<crate::account::OutgoingLink>,
    forwarded: Option<u32>,
    mentions: Option<Vec<String>>,
    mention_all: Option<bool>,
) -> Result<MessageDto, String> {
    let mentions = mentions.unwrap_or_default();
    let everyone = mention_all.unwrap_or(false);
    mgr.send_text(&chat_id, &text, reply_to.as_deref(), link, forwarded.unwrap_or(0), &mentions, everyone).await
}

#[tauri::command]
pub async fn wa_send_file(mgr: Mgr<'_>, request: tauri::ipc::Request<'_>) -> Result<MessageDto, String> {
    let (file, bytes) = crate::account::outgoing_file(&request)?;
    mgr.send_file(file, bytes).await
}

#[tauri::command]
pub async fn wa_vote_poll(mgr: Mgr<'_>, chat_id: String, message_id: String, options: Vec<usize>) -> Result<MessageDto, String> {
    mgr.vote_poll(&chat_id, &message_id, options).await
}

#[tauri::command]
pub async fn wa_edit_message(mgr: Mgr<'_>, chat_id: String, message_id: String, text: String) -> Result<MessageDto, String> {
    mgr.edit_message(&chat_id, &message_id, &text).await
}

#[tauri::command]
pub async fn wa_delete_message(
    mgr: Mgr<'_>,
    chat_id: String,
    message_id: String,
    for_everyone: bool,
) -> Result<Option<MessageDto>, String> {
    mgr.delete_message(&chat_id, &message_id, for_everyone).await
}

#[tauri::command]
pub async fn wa_fetch_media(mgr: Mgr<'_>, chat_id: String, message_id: String) -> Result<String, String> {
    mgr.fetch_media(&chat_id, &message_id).await
}

#[tauri::command]
pub async fn wa_open_media(mgr: Mgr<'_>, chat_id: String, message_id: String) -> Result<(), String> {
    mgr.open_media(&chat_id, &message_id).await
}

#[tauri::command]
pub async fn wa_link_thumbnail(mgr: Mgr<'_>, chat_id: String, message_id: String) -> Result<String, String> {
    mgr.link_thumbnail(&chat_id, &message_id).await
}

#[tauri::command]
pub async fn wa_react(
    mgr: Mgr<'_>,
    chat_id: String,
    message_id: String,
    emoji: String,
) -> Result<MessageDto, String> {
    mgr.react(&chat_id, &message_id, &emoji).await
}

#[tauri::command]
pub fn wa_lists(mgr: Mgr<'_>) -> Vec<ListDto> {
    mgr.lists()
}

#[tauri::command]
pub async fn wa_set_favorite(mgr: Mgr<'_>, chat_id: String, favorite: bool) -> Result<(), String> {
    mgr.set_favorite(&chat_id, favorite).await
}

#[tauri::command]
pub async fn wa_save_list(
    mgr: Mgr<'_>,
    id: Option<String>,
    name: String,
    add: Vec<String>,
    remove: Vec<String>,
) -> Result<String, String> {
    mgr.save_list(id, &name, &add, &remove).await
}

#[tauri::command]
pub async fn wa_delete_list(mgr: Mgr<'_>, id: String) -> Result<(), String> {
    mgr.delete_list(&id).await
}

#[tauri::command]
pub async fn wa_set_starred(
    mgr: Mgr<'_>,
    chat_id: String,
    message_id: String,
    starred: bool,
) -> Result<MessageDto, String> {
    mgr.set_starred(&chat_id, &message_id, starred).await
}

#[tauri::command]
pub fn wa_starred(mgr: Mgr<'_>) -> Vec<StarredDto> {
    mgr.starred()
}

/// Messages with the words of `query` in them, newest first.
/// Messages of a chat from before `before_ms`, oldest first, past what memory keeps.
#[tauri::command]
pub fn wa_older_messages(mgr: Mgr<'_>, chat_id: String, before_ms: i64) -> Vec<MessageDto> {
    mgr.older_messages(&chat_id, before_ms, crate::store::OLDER_PAGE)
}

/// Fetches a chat's history from the phone before the others', as when it is opened empty.
#[tauri::command]
pub fn wa_fetch_history_first(mgr: Mgr<'_>, chat_id: String) {
    mgr.fetch_history_first(&chat_id);
}

/// Whether all of a chat's history is here; if not, it comes from the phone before the others'.
#[tauri::command]
pub fn wa_complete_history(mgr: Mgr<'_>, chat_id: String) -> bool {
    mgr.complete_history(&chat_id)
}

#[tauri::command]
pub fn wa_search(mgr: Mgr<'_>, query: String, chat_id: Option<String>) -> Vec<StarredDto> {
    mgr.search(&query, chat_id.as_deref())
}

#[tauri::command]
pub fn wa_contacts(mgr: Mgr<'_>) -> Vec<ContactDto> {
    mgr.contacts()
}

#[tauri::command]
pub fn wa_person(mgr: Mgr<'_>, id: String) -> crate::store::PersonDto {
    mgr.person(&id)
}

#[tauri::command]
pub async fn wa_lookup_number(mgr: Mgr<'_>, phone: String) -> Result<Option<String>, String> {
    mgr.lookup_number(&phone).await
}

#[tauri::command]
pub fn wa_sticker_library(mgr: Mgr<'_>) -> crate::stickers::LibraryDto {
    mgr.sticker_library()
}

#[tauri::command]
pub async fn wa_library_media(mgr: Mgr<'_>, id: String) -> Result<String, String> {
    mgr.library_media(&id).await
}

#[tauri::command]
pub async fn wa_send_sticker(mgr: Mgr<'_>, chat_id: String, id: String, reply_to: Option<String>) -> Result<MessageDto, String> {
    mgr.send_library_sticker(&chat_id, &id, reply_to.as_deref()).await
}

/// A picture sent as the body, with `x-chat` (and `x-reply`) or `x-pack` headers.
fn picture(request: &tauri::ipc::Request<'_>) -> Result<Vec<u8>, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("The picture is missing.".into());
    };
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return Err("A sticker can be made from a picture of up to 16 MB.".into());
    }
    Ok(bytes.clone())
}

fn header(request: &tauri::ipc::Request<'_>, name: &str) -> Option<String> {
    request.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string).filter(|v| !v.is_empty())
}

#[tauri::command]
pub async fn wa_send_new_sticker(mgr: Mgr<'_>, request: tauri::ipc::Request<'_>) -> Result<MessageDto, String> {
    let chat_id = header(&request, "x-chat").ok_or("The chat is missing.")?;
    let reply_to = header(&request, "x-reply");
    mgr.send_new_sticker(&chat_id, picture(&request)?, reply_to.as_deref()).await
}

#[tauri::command]
pub async fn wa_add_new_sticker(mgr: Mgr<'_>, request: tauri::ipc::Request<'_>) -> Result<(), String> {
    let pack = header(&request, "x-pack").ok_or("The pack is missing.")?;
    mgr.add_new_sticker(&pack, picture(&request)?).await
}

#[tauri::command]
pub async fn wa_set_sticker_favorite(
    mgr: Mgr<'_>,
    id: Option<String>,
    chat_id: Option<String>,
    message_id: Option<String>,
    favorite: bool,
) -> Result<(), String> {
    let message = chat_id.as_deref().zip(message_id.as_deref());
    mgr.set_sticker_favorite(id.as_deref(), message, favorite).await
}

#[tauri::command]
pub fn wa_message_sticker(mgr: Mgr<'_>, chat_id: String, message_id: String) -> Option<String> {
    mgr.message_sticker(&chat_id, &message_id)
}

#[tauri::command]
pub async fn wa_sticker_store(mgr: Mgr<'_>) -> Result<Vec<super::stickers::StorePackDto>, String> {
    mgr.sticker_store().await
}

#[tauri::command]
pub async fn wa_install_pack(mgr: Mgr<'_>, id: String) -> Result<(), String> {
    mgr.install_store_pack(&id).await
}

#[tauri::command]
pub fn wa_remove_pack(mgr: Mgr<'_>, id: String) {
    mgr.remove_pack(&id)
}

#[tauri::command]
pub fn wa_create_pack(mgr: Mgr<'_>, name: String) -> Result<String, String> {
    mgr.create_pack(&name)
}

#[tauri::command]
pub fn wa_rename_pack(mgr: Mgr<'_>, id: String, name: String) -> Result<(), String> {
    mgr.rename_pack(&id, &name)
}

#[tauri::command]
pub fn wa_add_to_pack(
    mgr: Mgr<'_>,
    pack_id: String,
    id: Option<String>,
    chat_id: Option<String>,
    message_id: Option<String>,
) -> Result<(), String> {
    let message = chat_id.as_deref().zip(message_id.as_deref());
    mgr.add_to_pack(&pack_id, id.as_deref(), message)
}

#[tauri::command]
pub fn wa_remove_from_pack(mgr: Mgr<'_>, pack_id: String, id: String) -> Result<(), String> {
    mgr.remove_from_pack(&pack_id, &id)
}

/// Sends the voice message being recorded, or the one kept after a failed send.
#[tauri::command]
pub async fn wa_send_voice(
    mgr: Mgr<'_>,
    recorder: State<'_, crate::voice::Recorder>,
    chat_id: String,
    reply_to: Option<String>,
) -> Result<MessageDto, String> {
    let recording = crate::voice::finish(&recorder).await?;
    let ogg = recording.ogg.clone();
    let file = crate::account::OutgoingFile {
        chat_id,
        name: "voice.ogg".into(),
        mime: "audio/ogg; codecs=opus".into(),
        caption: None,
        reply_to,
        width: None,
        height: None,
        seconds: Some(recording.seconds),
        thumbnail: None,
        forwarded: 0,
        voice: true,
        waveform: recording.waveform.clone(),
        mentions: Vec::new(),
        mention_all: false,
    };
    let sent = mgr.send_file(file, ogg).await;
    // Kept, so it can be sent again rather than recorded again.
    if sent.is_err() {
        recorder.keep(recording);
    }
    sent
}

#[tauri::command]
pub async fn wa_send_gif(
    mgr: Mgr<'_>,
    chat_id: String,
    pick: crate::klipy::GifPick,
    reply_to: Option<String>,
) -> Result<MessageDto, String> {
    mgr.send_klipy_gif(&chat_id, pick, reply_to.as_deref()).await
}

#[tauri::command]
pub async fn wa_send_recent_gif(mgr: Mgr<'_>, chat_id: String, id: String, reply_to: Option<String>) -> Result<MessageDto, String> {
    mgr.send_recent_gif(&chat_id, &id, reply_to.as_deref()).await
}

#[tauri::command]
pub async fn wa_send_klipy_sticker(
    mgr: Mgr<'_>,
    chat_id: String,
    pick: crate::klipy::StickerPick,
    reply_to: Option<String>,
) -> Result<MessageDto, String> {
    mgr.send_klipy_sticker(&chat_id, pick, reply_to.as_deref()).await
}

#[tauri::command]
pub async fn wa_add_klipy_sticker(mgr: Mgr<'_>, pack_id: String, pick: crate::klipy::StickerPick) -> Result<(), String> {
    mgr.add_klipy_sticker(&pack_id, pick).await
}

#[tauri::command]
pub async fn wa_add_shared_pack(mgr: Mgr<'_>, chat_id: String, message_id: String) -> Result<String, String> {
    mgr.add_shared_pack(&chat_id, &message_id).await
}

#[tauri::command]
pub fn wa_move_in_pack(mgr: Mgr<'_>, pack_id: String, id: String, to: usize) -> Result<(), String> {
    mgr.move_in_pack(&pack_id, &id, to)
}

/// What the editor sends with `wa_make_sticker`, base64 JSON in `x-make`.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MakeRequest {
    pack: Option<String>,
    new_pack: Option<String>,
    #[serde(default)]
    emojis: Vec<String>,
    edit: crate::sticker_image::Edit,
    /// The body's parts, by length: the source file, or a video's frames
    /// (PNG), then the caption layer when there is one.
    parts: Vec<usize>,
    /// How long each frame shows, when the parts are a video's frames.
    #[serde(default)]
    frame_ms: Option<Vec<u32>>,
    #[serde(default)]
    overlay: bool,
}

/// The editor's request and what it sent: the frames, each with how long it
/// shows, and the caption layer.
type Made = (MakeRequest, Vec<(image::RgbaImage, u32)>, Option<image::RgbaImage>);

/// Reads what the editor sent, the frames decoded off the async runtime.
async fn made_from(request: &tauri::ipc::Request<'_>) -> Result<Made, String> {
    use base64::Engine as _;
    let make: MakeRequest = header(request, "x-make")
        .and_then(|h| base64::engine::general_purpose::STANDARD.decode(h).ok())
        .and_then(|json| serde_json::from_slice(&json).ok())
        .ok_or("The sticker's settings are missing.")?;
    let tauri::ipc::InvokeBody::Raw(body) = request.body() else {
        return Err("The picture is missing.".into());
    };
    if body.len() > 64 * 1024 * 1024 || make.parts.iter().sum::<usize>() != body.len() {
        return Err("The picture did not arrive whole.".into());
    }
    let mut parts = Vec::with_capacity(make.parts.len());
    let mut at = 0;
    for len in &make.parts {
        parts.push(body[at..at + len].to_vec());
        at += len;
    }
    let overlay = if make.overlay { parts.pop() } else { None };
    let frame_ms = make.frame_ms.clone();
    let (frames, overlay) = tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
        let frames = match frame_ms {
            Some(ms) => parts
                .iter()
                .zip(ms)
                .map(|(png, ms)| Ok((image::load_from_memory(png).map_err(|e| format!("A frame could not be read: {e}"))?.to_rgba8(), ms)))
                .collect::<Result<Vec<_>, String>>()?,
            None => crate::sticker_image::source_frames(parts.first().ok_or("The picture is missing.")?)?,
        };
        let overlay = match overlay {
            Some(png) => Some(image::load_from_memory(&png).map_err(|e| format!("The caption could not be read: {e}"))?.to_rgba8()),
            None => None,
        };
        Ok((frames, overlay))
    })
    .await
    .map_err(|e| format!("The picture could not be read: {e}"))??;
    Ok((make, frames, overlay))
}

/// Makes a sticker in the editor's way and keeps it in a pack; returns the pack's id.
#[tauri::command]
pub async fn wa_make_sticker(mgr: Mgr<'_>, request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let (make, frames, overlay) = made_from(&request).await?;
    let edit = make.edit.clone();
    let made = tauri::async_runtime::spawn_blocking(move || crate::sticker_image::render(frames, &edit, overlay.as_ref()))
        .await
        .map_err(|e| format!("The sticker could not be made: {e}"))??;
    mgr.make_sticker(made, make.pack.as_deref(), make.new_pack.as_deref(), make.emojis).await
}

/// Makes a GIF in the editor's way (an mp4, as WhatsApp sends GIFs) and keeps it with the GIFs.
#[tauri::command]
pub async fn wa_make_gif(mgr: Mgr<'_>, request: tauri::ipc::Request<'_>) -> Result<(), String> {
    let (make, frames, overlay) = made_from(&request).await?;
    let video = tauri::async_runtime::spawn_blocking(move || crate::sticker_image::render_gif(frames, &make.edit, overlay.as_ref()))
        .await
        .map_err(|e| format!("The GIF could not be made: {e}"))??;
    mgr.make_gif(video).await
}
