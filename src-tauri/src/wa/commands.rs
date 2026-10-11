use std::sync::Arc;

use tauri::State;

use super::store::{ContactDto, ListDto, MessageDto, StarredDto};
use super::{ChatSummary, WaManager};
use crate::account::AccountStatus as WaStatus;

type Mgr<'a> = State<'a, Arc<WaManager>>;

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
) -> Result<MessageDto, String> {
    mgr.send_text(&chat_id, &text, reply_to.as_deref(), link, forwarded.unwrap_or(0)).await
}

#[tauri::command]
pub async fn wa_send_file(mgr: Mgr<'_>, request: tauri::ipc::Request<'_>) -> Result<MessageDto, String> {
    let (file, bytes) = crate::account::outgoing_file(&request)?;
    mgr.send_file(file, bytes).await
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
