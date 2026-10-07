//! Desktop notifications. The UI decides whether a message is worth one;
//! this shows it and brings the chat up when it is clicked.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use notify_rust::Notification;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

#[cfg(all(unix, not(target_os = "macos")))]
use crate::account::lock;
use crate::{signal, wa};

/// The notification each chat last showed, so the next one replaces it
/// instead of piling up.
#[derive(Default)]
pub struct Shown(Mutex<HashMap<String, u32>>);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
struct Open {
    account: String,
    chat_id: String,
}

/// Where a chat's saved profile picture is, when the name is one of ours.
fn avatar_path(app: &AppHandle, account: &str, name: &str) -> Option<PathBuf> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    let dir = app.path().app_data_dir().ok()?;
    let path = match account {
        "whatsapp" => dir.join(wa::AVATAR_DIR).join(name),
        "signal" => dir.join("signal").join(signal::AVATAR_DIR).join(name),
        _ => return None,
    };
    (plain && path.exists()).then_some(path)
}

#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn bring_up(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
pub fn show_notification(
    app: AppHandle,
    shown: State<'_, Shown>,
    account: String,
    chat_id: String,
    title: String,
    body: String,
    avatar: Option<String>,
) -> Result<(), String> {
    let mut notification = Notification::new();
    notification.appname("Omega Zap").summary(&title).body(&body);
    if let Some(path) = avatar.and_then(|name| avatar_path(&app, &account, &name)) {
        notification.icon(&path.to_string_lossy());
    }
    let key = format!("{account}:{chat_id}");

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(id) = lock(&shown.0).get(&key) {
            notification.id(*id);
        }
        // Clicking the notification itself runs its "default" action.
        notification.action("default", "Open chat");
        let handle = notification.show().map_err(|e| format!("Could not show a notification: {e}"))?;
        lock(&shown.0).insert(key, handle.id());
        let open = Open { account, chat_id };
        std::thread::spawn(move || {
            handle.wait_for_action(|action| {
                if action == "default" {
                    bring_up(&app);
                    let _ = app.emit("notification://open", open);
                }
            });
        });
    }

    // Elsewhere clicks are not reported back.
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let _ = (shown, key, chat_id);
        notification.show().map_err(|e| format!("Could not show a notification: {e}"))?;
    }

    Ok(())
}
