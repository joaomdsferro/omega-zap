import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "../lib/platform";
import type { AccountId } from "./accounts";

/** Prefix of the Tauri commands and events that serve each account. */
export const COMMAND_PREFIX: Record<AccountId, string> = { whatsapp: "wa", signal: "signal" };

export type AccountState =
  | "unsupported"
  | "idle"
  | "starting"
  | "qr"
  | "qr_expired"
  | "connecting"
  | "connected"
  | "logged_out"
  | "error";

export interface Backfill {
  state: "fetching" | "waitingForPhone" | "done";
  /** The chat being fetched. */
  chat: string | null;
  /** Chats with all their history here, of `total`. */
  done: number;
  total: number;
  /** Messages fetched since the app started. */
  messages: number;
}

export interface AccountStatus {
  seq: number;
  state: AccountState;
  /** A paired session exists on this computer. */
  linked: boolean;
  qrSvg: string | null;
  qrTimeoutSecs: number | null;
  syncProgress: number | null;
  /** Older history being fetched from the phone, chat by chat. */
  backfill: Backfill | null;
  error: string | null;
  /** Something that went wrong without breaking the connection. */
  notice: string | null;
  /** `avatar` is the file name of our own profile picture, once downloaded. */
  me: { phone: string | null; name: string | null; avatar: string | null } | null;
}

const UNSUPPORTED: AccountStatus = {
  seq: 0,
  state: "unsupported",
  linked: false,
  qrSvg: null,
  qrTimeoutSecs: null,
  syncProgress: null,
  backfill: null,
  error: null,
  notice: null,
  me: null,
};

export interface Account {
  id: AccountId;
  status: AccountStatus;
  startLinking(): Promise<void>;
  cancelLinking(): Promise<void>;
  reconnect(): Promise<void>;
  logout(): Promise<void>;
}

export function useAccount(id: AccountId): Account {
  const prefix = COMMAND_PREFIX[id];
  const [status, setStatus] = useState<AccountStatus>(
    isTauri ? { ...UNSUPPORTED, state: "idle" } : UNSUPPORTED,
  );

  useEffect(() => {
    if (!isTauri) return;
    // Keep the newest status: the initial fetch can resolve after an event.
    const accept = (next: AccountStatus) => setStatus((cur) => (next.seq >= cur.seq ? next : cur));
    const unlisten = listen<AccountStatus>(`${prefix}://status`, (e) => accept(e.payload));
    invoke<AccountStatus>(`${prefix}_status`).then(accept).catch(console.error);
    return () => {
      void unlisten.then((f) => f());
    };
  }, [prefix]);

  const call = useCallback(
    async (command: string) => {
      if (!isTauri) return;
      await invoke(`${prefix}_${command}`);
    },
    [prefix],
  );

  return useMemo(
    () => ({
      id,
      status,
      startLinking: () => call("start_linking"),
      cancelLinking: () => call("cancel_linking"),
      reconnect: () => call("reconnect"),
      logout: () => call("logout"),
    }),
    [id, status, call],
  );
}
