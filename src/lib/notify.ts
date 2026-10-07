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

/** Calls back with the chat of a notification that was clicked. */
export function onNotificationOpen(open: (account: AccountId, chatId: string) => void): () => void {
  if (!isTauri) return () => {};
  const pending = listen<{ account: AccountId; chatId: string }>("notification://open", (e) =>
    open(e.payload.account, e.payload.chatId),
  );
  return () => void pending.then((unlisten) => unlisten());
}
