use std::sync::Arc;

use tauri::State;

use super::TgManager;
use crate::account::AccountStatus;
use crate::store::{ChatSummary, ContactDto, ListDto, MessageDto, StarredDto};

type Mgr<'a> = State<'a, Arc<TgManager>>;

#[tauri::command]
pub fn tg_status(mgr: Mgr<'_>) -> AccountStatus {
    mgr.status()
}

#[tauri::command]
pub async fn tg_start_linking(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.inner().clone().start_linking().await
}

#[tauri::command]
pub async fn tg_cancel_linking(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.inner().clone().cancel_linking().await;
    Ok(())
}

#[tauri::command]
pub async fn tg_submit_password(mgr: Mgr<'_>, password: String) -> Result<(), String> {
    mgr.submit_password(&password).await
}

#[tauri::command]
pub async fn tg_reconnect(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.inner().clone().reconnect().await
}

#[tauri::command]
pub async fn tg_logout(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.inner().clone().logout().await;
    Ok(())
}

#[tauri::command]
pub fn tg_list_chats(mgr: Mgr<'_>) -> Vec<ChatSummary> {
    mgr.list_chats()
}

#[tauri::command]
pub fn tg_get_messages(mgr: Mgr<'_>, chat_id: String) -> Vec<MessageDto> {
    mgr.inner().messages(&chat_id)
}

/// Messages of a chat from before `before_ms`, oldest first, past what memory keeps.
#[tauri::command]
pub async fn tg_older_messages(mgr: Mgr<'_>, chat_id: String, before_ms: i64) -> Result<Vec<MessageDto>, String> {
    Ok(mgr.older_messages(&chat_id, before_ms, crate::store::OLDER_PAGE).await)
}

#[tauri::command]
pub fn tg_mark_read(mgr: Mgr<'_>, chat_id: String) {
    mgr.inner().mark_read(&chat_id);
}

#[tauri::command]
pub async fn tg_send_text(mgr: Mgr<'_>, chat_id: String, text: String, reply_to: Option<String>) -> Result<MessageDto, String> {
    mgr.send_text(&chat_id, &text, reply_to.as_deref()).await
}

#[tauri::command]
pub fn tg_send_file(mgr: Mgr<'_>) -> Result<MessageDto, String> {
    mgr.not_yet()
}

#[tauri::command]
pub fn tg_edit_message(mgr: Mgr<'_>) -> Result<MessageDto, String> {
    mgr.not_yet()
}

#[tauri::command]
pub fn tg_delete_message(mgr: Mgr<'_>) -> Result<Option<MessageDto>, String> {
    mgr.not_yet()
}

#[tauri::command]
pub fn tg_react(mgr: Mgr<'_>) -> Result<MessageDto, String> {
    mgr.not_yet()
}

#[tauri::command]
pub fn tg_fetch_media(mgr: Mgr<'_>) -> Result<String, String> {
    mgr.not_yet()
}

#[tauri::command]
pub fn tg_open_media(mgr: Mgr<'_>) -> Result<(), String> {
    mgr.not_yet()
}

#[tauri::command]
pub fn tg_lists(mgr: Mgr<'_>) -> Vec<ListDto> {
    mgr.lists()
}

#[tauri::command]
pub fn tg_set_favorite(mgr: Mgr<'_>, chat_id: String, favorite: bool) {
    mgr.set_favorite(&chat_id, favorite);
}

#[tauri::command]
pub fn tg_save_list(mgr: Mgr<'_>, id: Option<String>, name: String, add: Vec<String>, remove: Vec<String>) -> Result<String, String> {
    mgr.save_list(id, &name, &add, &remove)
}

#[tauri::command]
pub fn tg_delete_list(mgr: Mgr<'_>, id: String) {
    mgr.delete_list(&id);
}

#[tauri::command]
pub fn tg_set_starred(mgr: Mgr<'_>, chat_id: String, message_id: String, starred: bool) -> Result<MessageDto, String> {
    mgr.set_starred(&chat_id, &message_id, starred)
}

#[tauri::command]
pub fn tg_starred(mgr: Mgr<'_>) -> Vec<StarredDto> {
    mgr.starred()
}

#[tauri::command]
pub fn tg_search(mgr: Mgr<'_>, query: String, chat_id: Option<String>) -> Vec<StarredDto> {
    mgr.search(&query, chat_id.as_deref())
}

#[tauri::command]
pub fn tg_contacts(mgr: Mgr<'_>) -> Vec<ContactDto> {
    mgr.contacts()
}

#[tauri::command]
pub fn tg_person(mgr: Mgr<'_>, id: String) -> crate::store::PersonDto {
    mgr.person(&id)
}

#[tauri::command]
pub fn tg_person_details(mgr: Mgr<'_>, id: String) -> crate::store::PersonDetailsDto {
    mgr.person_details(&id)
}

#[tauri::command]
pub fn tg_group_info(mgr: Mgr<'_>, id: String) -> crate::store::GroupInfoDto {
    mgr.group_info(&id)
}

#[tauri::command]
pub fn tg_set_unread(mgr: Mgr<'_>, chat_id: String, unread: bool) {
    mgr.set_unread(&chat_id, unread);
}

#[tauri::command]
pub fn tg_set_muted(mgr: Mgr<'_>, chat_id: String, muted: bool, until_ms: Option<i64>) {
    mgr.set_muted(&chat_id, muted, until_ms);
}

#[tauri::command]
pub fn tg_set_archived(mgr: Mgr<'_>, chat_id: String, archived: bool) {
    mgr.set_archived(&chat_id, archived);
}

#[tauri::command]
pub fn tg_set_pinned(mgr: Mgr<'_>, chat_id: String, pinned: bool) -> Result<(), String> {
    mgr.set_pinned(&chat_id, pinned)
}

#[tauri::command]
pub fn tg_delete_chat(mgr: Mgr<'_>, chat_id: String) {
    mgr.delete_chat(&chat_id);
}

#[tauri::command]
pub fn tg_clear_chat(mgr: Mgr<'_>, chat_id: String, keep_starred: bool) {
    mgr.clear_chat(&chat_id, keep_starred);
}
