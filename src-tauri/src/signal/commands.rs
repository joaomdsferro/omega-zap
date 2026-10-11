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
