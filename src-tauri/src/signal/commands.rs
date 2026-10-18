use std::sync::Arc;

use tauri::State;

use super::SignalManager;
use crate::account::AccountStatus;
use crate::store::{ChatSummary, ContactDto, ListDto, MessageDto, StarredDto};

type Mgr<'a> = State<'a, Arc<SignalManager>>;

#[tauri::command]
pub fn signal_status(mgr: Mgr<'_>) -> AccountStatus {
    mgr.status()
}

#[tauri::command]
pub async fn signal_start_linking(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.inner().clone().start_linking().await
}

#[tauri::command]
pub async fn signal_cancel_linking(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.inner().clone().cancel_linking().await;
    Ok(())
}

#[tauri::command]
pub async fn signal_reconnect(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.inner().clone().reconnect().await
}

#[tauri::command]
pub async fn signal_logout(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.inner().clone().logout().await;
    Ok(())
}

#[tauri::command]
pub fn signal_list_chats(mgr: Mgr<'_>) -> Vec<ChatSummary> {
    mgr.list_chats()
}

#[tauri::command]
pub fn signal_get_messages(mgr: Mgr<'_>, chat_id: String) -> Vec<MessageDto> {
    mgr.messages(&chat_id)
}

#[tauri::command]
pub fn signal_mark_read(mgr: Mgr<'_>, chat_id: String) {
    mgr.mark_read(&chat_id);
}

/// Signal tells linked devices nothing more about a person; the groups in
/// common come from what this computer knows.
#[tauri::command]
pub fn signal_person_details(mgr: Mgr<'_>, id: String) -> crate::store::PersonDetailsDto {
    mgr.person_details(&id)
}

#[tauri::command]
pub fn signal_set_muted(mgr: Mgr<'_>, chat_id: String, muted: bool, until_ms: Option<i64>) {
    mgr.set_muted(&chat_id, muted, until_ms);
}

#[tauri::command]
pub fn signal_delete_chat(mgr: Mgr<'_>, chat_id: String) {
    mgr.delete_chat(&chat_id);
}

#[tauri::command]
pub fn signal_clear_chat(mgr: Mgr<'_>, chat_id: String, keep_starred: bool) {
    mgr.clear_chat(&chat_id, keep_starred);
}

#[tauri::command]
pub fn signal_set_archived(mgr: Mgr<'_>, chat_id: String, archived: bool) {
    mgr.set_archived(&chat_id, archived);
}

#[tauri::command]
pub fn signal_set_pinned(mgr: Mgr<'_>, chat_id: String, pinned: bool) -> Result<(), String> {
    mgr.set_pinned(&chat_id, pinned)
}

#[tauri::command]
pub fn signal_set_unread(mgr: Mgr<'_>, chat_id: String, unread: bool) {
    mgr.set_unread(&chat_id, unread);
}

#[tauri::command]
pub async fn signal_send_text(
    mgr: Mgr<'_>,
    chat_id: String,
    text: String,
    reply_to: Option<String>,
    link: Option<crate::account::OutgoingLink>,
    styled: Option<super::Styled>,
) -> Result<MessageDto, String> {
    mgr.send_text(&chat_id, &text, reply_to.as_deref(), link, styled).await
}

#[tauri::command]
pub async fn signal_edit_message(
    mgr: Mgr<'_>,
    chat_id: String,
    message_id: String,
    text: String,
    styled: Option<super::Styled>,
) -> Result<MessageDto, String> {
    mgr.edit_message(&chat_id, &message_id, &text, styled).await
}

#[tauri::command]
pub async fn signal_delete_message(
    mgr: Mgr<'_>,
    chat_id: String,
    message_id: String,
    for_everyone: bool,
) -> Result<Option<MessageDto>, String> {
    mgr.delete_message(&chat_id, &message_id, for_everyone).await
}

#[tauri::command]
pub async fn signal_fetch_media(
    mgr: Mgr<'_>,
    chat_id: String,
    message_id: String,
    index: Option<usize>,
) -> Result<String, String> {
    mgr.fetch_media(&chat_id, &message_id, index.unwrap_or(0)).await
}

#[tauri::command]
pub async fn signal_send_file(mgr: Mgr<'_>, request: tauri::ipc::Request<'_>) -> Result<MessageDto, String> {
    let (file, bytes) = crate::account::outgoing_file(&request)?;
    mgr.send_file(file, bytes).await
}

#[tauri::command]
pub async fn signal_open_media(
    mgr: Mgr<'_>,
    chat_id: String,
    message_id: String,
    index: Option<usize>,
) -> Result<(), String> {
    mgr.open_media(&chat_id, &message_id, index.unwrap_or(0)).await
}

#[tauri::command]
pub async fn signal_react(
    mgr: Mgr<'_>,
    chat_id: String,
    message_id: String,
    emoji: String,
) -> Result<MessageDto, String> {
    mgr.react(&chat_id, &message_id, &emoji).await
}

#[tauri::command]
pub async fn signal_set_typing(mgr: Mgr<'_>, chat_id: String, state: String) -> Result<(), String> {
    mgr.set_typing(&chat_id, &state).await
}

#[tauri::command]
pub async fn signal_set_disappearing(mgr: Mgr<'_>, chat_id: String, secs: u32) -> Result<MessageDto, String> {
    mgr.set_disappearing(&chat_id, secs).await
}

#[tauri::command]
pub async fn signal_set_blocked(mgr: Mgr<'_>, id: String, blocked: bool) -> Result<(), String> {
    mgr.set_blocked(&id, blocked).await
}

#[tauri::command]
pub async fn signal_respond_request(mgr: Mgr<'_>, chat_id: String, action: String) -> Result<(), String> {
    mgr.respond_request(&chat_id, &action).await
}

#[tauri::command]
pub fn signal_lists(mgr: Mgr<'_>) -> Vec<ListDto> {
    mgr.lists()
}

#[tauri::command]
pub fn signal_set_favorite(mgr: Mgr<'_>, chat_id: String, favorite: bool) {
    mgr.set_favorite(&chat_id, favorite);
}

#[tauri::command]
pub fn signal_save_list(
    mgr: Mgr<'_>,
    id: Option<String>,
    name: String,
    add: Vec<String>,
    remove: Vec<String>,
) -> Result<String, String> {
    mgr.save_list(id, &name, &add, &remove)
}

#[tauri::command]
pub fn signal_delete_list(mgr: Mgr<'_>, id: String) {
    mgr.delete_list(&id);
}

#[tauri::command]
pub fn signal_set_starred(
    mgr: Mgr<'_>,
    chat_id: String,
    message_id: String,
    starred: bool,
) -> Result<MessageDto, String> {
    mgr.set_starred(&chat_id, &message_id, starred)
}

#[tauri::command]
pub fn signal_starred(mgr: Mgr<'_>) -> Vec<StarredDto> {
    mgr.starred()
}

/// Messages with the words of `query` in them, newest first.
/// Messages of a chat from before `before_ms`, oldest first, past what memory keeps.
#[tauri::command]
pub fn signal_older_messages(mgr: Mgr<'_>, chat_id: String, before_ms: i64) -> Vec<MessageDto> {
    mgr.older_messages(&chat_id, before_ms, crate::store::OLDER_PAGE)
}

#[tauri::command]
pub fn signal_search(mgr: Mgr<'_>, query: String, chat_id: Option<String>) -> Vec<StarredDto> {
    mgr.search(&query, chat_id.as_deref())
}

#[tauri::command]
pub fn signal_contacts(mgr: Mgr<'_>) -> Vec<ContactDto> {
    mgr.contacts()
}

#[tauri::command]
pub fn signal_person(mgr: Mgr<'_>, id: String) -> crate::store::PersonDto {
    mgr.person(&id)
}

#[tauri::command]
pub fn signal_group_info(mgr: Mgr<'_>, id: String) -> crate::store::GroupInfoDto {
    mgr.group_info(&id)
}

#[tauri::command]
pub fn signal_sticker_packs(mgr: Mgr<'_>) -> Result<Vec<super::stickers::SignalPackDto>, String> {
    mgr.sticker_packs()
}

#[tauri::command]
pub async fn signal_upload_stickers(mgr: Mgr<'_>, source: String) -> Result<(), String> {
    mgr.upload_stickers(&source).await
}

#[tauri::command]
pub async fn signal_send_sticker(mgr: Mgr<'_>, chat_id: String, id: String, reply_to: Option<String>) -> Result<MessageDto, String> {
    mgr.send_sticker(&chat_id, &id, reply_to.as_deref()).await
}
