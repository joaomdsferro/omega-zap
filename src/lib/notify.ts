import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AccountId } from "../accounts/accounts";
import type { Notice } from "../backend";
import { isTauri } from "./platform";

/** Shows a desktop notification. A newer one from the same chat replaces it. */
export function showNotification(account: AccountId, notice: Notice, preview: boolean) {
  if (!isTauri) return;
  invoke("show_notification", {
    account,
    chatId: notice.chatId,
    title: notice.title,
    body: preview ? notice.body : "New message",
    avatar: notice.avatar ?? null,
  }).catch(console.error);
}

/** Calls back with the chat of a notification that was clicked, including
 *  one clicked while the window was closed, which this page opened for. */
export function onNotificationOpen(open: (account: AccountId, chatId: string) => void): () => void {
  if (!isTauri) return () => {};
  const pending = listen<{ account: AccountId; chatId: string }>("notification://open", (e) =>
    open(e.payload.account, e.payload.chatId),
  );
  void pending
    .then(() => invoke<{ account: AccountId; chatId: string } | null>("take_pending_open"))
    .then((chat) => chat && open(chat.account, chat.chatId))
    .catch(console.error);
  return () => void pending.then((unlisten) => unlisten());
}

/** Takes a chat's notification off the screen and out of the history. */
export function dismissNotification(account: AccountId, chatId: string) {
  if (!isTauri) return;
  invoke("dismiss_notification", { account, chatId }).catch(console.error);
}

/** Tells the backend the colour of the unread count on the tray icon. */
export function syncTrayBadge(badge: string) {
  if (!isTauri) return;
  invoke("set_tray_badge", { badge }).catch(console.error);
}

/** Tells the backend the notification settings, which it follows on its own
 *  while the window is closed. */
export function syncNotificationPrefs(notifications: boolean, preview: boolean) {
  if (!isTauri) return;
  invoke("set_notification_prefs", { notifications, preview }).catch(console.error);
}
