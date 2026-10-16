//! Telegram, spoken directly over MTProto (grammers). This computer signs in
//! by QR code, as Telegram Desktop does, then keeps the chat list here in the
//! same store as the other networks.

pub mod commands;
mod session;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use grammers_client::client::UpdatesConfiguration;
use grammers_client::message::{InputMessage, Message};
use grammers_client::peer::{Peer, User};
use grammers_client::session::Session as _;
use grammers_client::session::types::{PeerAuth, PeerId, PeerInfo, PeerKind, PeerRef, UpdateState, UpdatesState};
use grammers_client::session::updates::UpdatesLike;
use grammers_client::update::Update;
use grammers_client::{Client, InvocationError, SenderPool, SignInError, client::PasswordToken, tl};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::account::{AccountState, AccountStatus, Me, lock, render_qr, secure_dir};
use crate::store::{
    ChatStore, ChatSummary, ContactDto, Content, ConversationMeta, Deletion, Delivery, History, Incoming, ListDto, MessageDto, MsgKind,
    OWN_AVATAR, ReplyRef, StarredDto, StoredMessage, avatar_file, now_ms,
};
use session::TgSession;

const SESSION_FILE: &str = "session.json";
const LINKED_MARKER: &str = "linked";
const CACHE_FILE: &str = "chats.json";
pub const AVATAR_DIR: &str = "avatars";
const SAVE_INTERVAL: Duration = Duration::from_secs(3);
/// QR codes shown, about half a minute each, before the screen asks to reload.
const QR_ROUNDS: u32 = 10;
/// Messages fetched from Telegram when a chat is opened or scrolled back.
const PAGE: usize = 50;
/// The most chats Telegram pins in the main list without Premium.
const MAX_PINNED: usize = 5;
/// Pause between profile picture downloads, to stay clear of flood waits.
const AVATAR_PACE: Duration = Duration::from_millis(120);
/// Pause after the update stream fails, before asking again.
const RETRY_PAUSE: Duration = Duration::from_secs(5);
/// Telegram's folder for archived chats.
const ARCHIVE_FOLDER: i32 = 1;
const NOT_YET: &str = "Not available on Telegram yet.";

/// The app's API id and hash, from my.telegram.org (see `keys`).
fn api() -> Result<(i32, String), String> {
    let id = crate::keys::built_in!("TELEGRAM_API_ID").and_then(|id| id.parse().ok());
    let hash = crate::keys::built_in!("TELEGRAM_API_HASH");
    match (id, hash) {
        (Some(id), Some(hash)) => Ok((id, hash)),
        _ => Err("This build of Omega Zap has no Telegram API key (TELEGRAM_API_ID and TELEGRAM_API_HASH).".into()),
    }
}

/// A chat's id here: Telegram's "Bot API" id, negative for groups and channels.
fn chat_id(peer: PeerId) -> String {
    peer.bot_api_dialog_id_unchecked().to_string()
}

fn is_channel_chat(chat: &str) -> bool {
    chat.starts_with("-100")
}

fn rpc_code(e: &InvocationError) -> Option<i32> {
    match e {
        InvocationError::Rpc(rpc) => Some(rpc.code),
        _ => None,
    }
}

/// How signing in by QR code ended.
enum SignIn {
    Done,
    /// No phone took the codes in time.
    Expired,
    /// Waiting for the two-step password.
    Password,
}

pub struct TgManager {
    app: AppHandle,
    dir: PathBuf,
    status: Mutex<AccountStatus>,
    store: Mutex<ChatStore>,
    /// Every message, and the store, on disk.
    history: History,
    session: Mutex<Option<Arc<TgSession>>>,
    client: Mutex<Option<Client>>,
    /// The connection's tasks, ended together.
    tasks: Mutex<Vec<JoinHandle<()>>>,
    /// Bumped whenever a connection is started or torn down, so a task of
    /// one already abandoned stops.
    generation: AtomicU64,
    /// Asked for after the QR code, kept until the right password comes.
    password: tokio::sync::Mutex<Option<PasswordToken>>,
    password_done: tokio::sync::Notify,
    /// The peers the chat list came with: their pictures are fetched from them.
    peers: Mutex<HashMap<String, Peer>>,
    /// Chats whose latest messages were fetched since connecting.
    fetched: Mutex<HashSet<String>>,
    chats_dirty: AtomicBool,
    save_dirty: AtomicBool,
    /// Wakes the refresh / save loop, which otherwise sleeps.
    wake: tokio::sync::Notify,
    avatars_running: AtomicBool,
}

impl TgManager {
    pub fn new(app: AppHandle, dir: PathBuf) -> std::io::Result<Arc<Self>> {
        secure_dir(&dir)?;
        let linked = dir.join(LINKED_MARKER).exists();
        let history = History::new(&dir, "telegram history", CACHE_FILE);
        let store = linked.then(|| history.load()).flatten().unwrap_or_default();
        Ok(Arc::new(Self {
            app,
            dir,
            status: Mutex::new(AccountStatus::idle(linked)),
            store: Mutex::new(store),
            history,
            session: Mutex::new(None),
            client: Mutex::new(None),
            tasks: Mutex::new(Vec::new()),
            generation: AtomicU64::new(0),
            password: tokio::sync::Mutex::new(None),
            password_done: tokio::sync::Notify::new(),
            peers: Mutex::new(HashMap::new()),
            fetched: Mutex::new(HashSet::new()),
            chats_dirty: AtomicBool::new(false),
            save_dirty: AtomicBool::new(false),
            wake: tokio::sync::Notify::new(),
            avatars_running: AtomicBool::new(false),
        }))
    }

    /// Starts the UI refresh / save loop and reconnects an existing session.
    pub fn start(self: &Arc<Self>) {
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut last_save = Instant::now();
            // Idle until something changes, so a quiet background costs no
            // wake-ups; then tick until the change is shown and saved.
            loop {
                mgr.wake.notified().await;
                loop {
                    tokio::time::sleep(Duration::from_millis(350)).await;
                    if mgr.chats_dirty.swap(false, Ordering::AcqRel) {
                        let _ = mgr.app.emit("tg://chats", ());
                        crate::tray::refresh(&mgr.app);
                    }
                    if !mgr.save_dirty.load(Ordering::Acquire) {
                        break;
                    }
                    if last_save.elapsed() >= SAVE_INTERVAL && mgr.save_dirty.swap(false, Ordering::AcqRel) {
                        mgr.save_now();
                        last_save = Instant::now();
                    }
                }
            }
        });

        if self.is_linked() {
            let mgr = self.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = mgr.connect() {
                    mgr.fail(e);
                }
            });
        }
    }

    pub fn unread_total(&self) -> u32 {
        lock(&self.store).unread_total()
    }

    fn is_linked(&self) -> bool {
        self.dir.join(LINKED_MARKER).exists()
    }

    fn current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    pub fn status(&self) -> AccountStatus {
        lock(&self.status).clone()
    }

    fn set_status(&self, f: impl FnOnce(&mut AccountStatus)) {
        let snapshot = {
            let mut status = lock(&self.status);
            f(&mut status);
            status.seq += 1;
            status.clone()
        };
        let _ = self.app.emit("tg://status", snapshot);
    }

    fn fail(&self, error: String) {
        log::warn!("telegram: {error}");
        self.set_status(|s| {
            s.state = AccountState::Error;
            s.error = Some(error);
            s.qr_svg = None;
            s.password_hint = None;
        });
    }

    fn mark_dirty(&self) {
        self.chats_dirty.store(true, Ordering::Release);
        self.save_dirty.store(true, Ordering::Release);
        self.wake.notify_one();
    }

    pub fn save_now(&self) {
        if let Some(session) = lock(&self.session).as_ref() {
            session.save();
        }
        if !self.is_linked() {
            return;
        }
        self.history.save(&self.store);
    }

    fn client(&self) -> Result<Client, String> {
        if lock(&self.status).state != AccountState::Connected {
            return Err("Telegram is not connected.".into());
        }
        lock(&self.client).clone().ok_or_else(|| "Telegram is not connected.".into())
    }

    /// The chat as Telegram addresses it, from the peers this session met.
    async fn peer_ref(&self, chat: &str) -> Result<PeerRef, String> {
        let unknown = || "Telegram doesn't know this chat yet.".to_string();
        let id = chat.parse::<i64>().ok().and_then(PeerId::from_bot_api_dialog_id).ok_or_else(unknown)?;
        let session = lock(&self.session).clone().ok_or("Telegram is not connected.")?;
        session.peer_ref(id).await.ok().flatten().ok_or_else(unknown)
    }

    // ---- Connection ----

    /// Ends the connection, if there is one.
    fn disconnect(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(client) = lock(&self.client).take() {
            client.disconnect();
        }
        for task in lock(&self.tasks).drain(..) {
            task.abort();
        }
        if let Some(session) = lock(&self.session).take() {
            session.save();
        }
        lock(&self.fetched).clear();
    }

    /// Connects, signing in by QR code first when this computer isn't yet.
    fn connect(self: &Arc<Self>) -> Result<(), String> {
        let (api_id, api_hash) = api()?;
        self.disconnect();
        let generation = self.generation.load(Ordering::SeqCst);
        let linked = self.is_linked();
        self.set_status(|s| {
            s.state = if linked { AccountState::Connecting } else { AccountState::Starting };
            s.linked = linked;
            s.error = None;
            s.qr_svg = None;
            s.password_hint = None;
        });

        let session = Arc::new(TgSession::load(&self.dir.join(SESSION_FILE)));
        let SenderPool { runner, handle, updates } = SenderPool::new(session.clone(), api_id);
        let client = Client::new(handle);
        *lock(&self.session) = Some(session.clone());
        *lock(&self.client) = Some(client.clone());
        let runner = tauri::async_runtime::spawn(runner.run());
        let mgr = self.clone();
        let main = tauri::async_runtime::spawn(async move {
            let outcome = mgr.run(generation, client, session, updates, api_id, api_hash).await;
            if let Err(e) = outcome
                && mgr.current(generation)
            {
                mgr.fail(e);
            }
        });
        lock(&self.tasks).extend([runner, main]);
        Ok(())
    }

    async fn run(
        self: &Arc<Self>,
        generation: u64,
        client: Client,
        session: Arc<TgSession>,
        mut updates: UnboundedReceiver<UpdatesLike>,
        api_id: i32,
        api_hash: String,
    ) -> Result<(), String> {
        if session.own_user().is_some() {
            match client.is_authorized().await {
                Ok(true) => {}
                Ok(false) => return self.signed_out(generation).await,
                Err(e) => return Err(format!("Could not reach Telegram: {e}")),
            }
        } else {
            match self.sign_in(generation, &client, &session, &mut updates, api_id, &api_hash).await? {
                SignIn::Done => {}
                SignIn::Expired => {
                    if self.current(generation) {
                        self.disconnect();
                        self.set_status(|s| {
                            s.state = AccountState::QrExpired;
                            s.qr_svg = None;
                        });
                    }
                    return Ok(());
                }
                SignIn::Password => loop {
                    self.password_done.notified().await;
                    if !self.current(generation) {
                        return Ok(());
                    }
                    if session.own_user().is_some() {
                        break;
                    }
                },
            }
        }
        if !self.current(generation) {
            return Ok(());
        }
        session.save();
        if let Err(e) = fs::write(self.dir.join(LINKED_MARKER), b"") {
            log::warn!("telegram: could not write link marker: {e}");
        }
        let me = client.get_me().await.map_err(|e| format!("Could not reach Telegram: {e}"))?;
        self.found_me(&me);
        self.set_status(|s| {
            s.state = AccountState::Connected;
            s.linked = true;
            s.error = None;
            s.qr_svg = None;
            s.password_hint = None;
        });

        // Listening first, so nothing that arrives while the list loads is lost.
        let mut stream = client
            .stream_updates(updates, UpdatesConfiguration { catch_up: true, ..Default::default() })
            .await
            .map_err(|e| format!("Could not follow Telegram's updates: {e}"))?;
        let mgr = self.clone();
        let sync_client = client.clone();
        let sync = tauri::async_runtime::spawn(async move {
            if let Err(e) = mgr.sync_dialogs(generation, &sync_client).await {
                log::warn!("telegram: chat list not loaded: {e}");
            }
            mgr.sync_avatars(generation, sync_client).await;
        });
        lock(&self.tasks).push(sync);

        let mut last_state_save = Instant::now();
        while self.current(generation) {
            match stream.next().await {
                Ok(update) => self.handle_update(update),
                Err(e) if rpc_code(&e) == Some(401) => return self.signed_out(generation).await,
                Err(e) => {
                    log::warn!("telegram: updates: {e}");
                    tokio::time::sleep(RETRY_PAUSE).await;
                }
            }
            if last_state_save.elapsed() >= SAVE_INTERVAL {
                if let Err(e) = stream.sync_update_state().await {
                    log::warn!("telegram: update state not kept: {e}");
                }
                last_state_save = Instant::now();
                self.save_dirty.store(true, Ordering::Release);
                self.wake.notify_one();
            }
        }
        Ok(())
    }

    /// Shows QR codes until a phone signs this computer in with one.
    async fn sign_in(
        &self,
        generation: u64,
        client: &Client,
        session: &TgSession,
        updates: &mut UnboundedReceiver<UpdatesLike>,
        api_id: i32,
        api_hash: &str,
    ) -> Result<SignIn, String> {
        use tl::enums::auth::LoginToken as Token;
        let export = tl::functions::auth::ExportLoginToken { api_id, api_hash: api_hash.to_string(), except_ids: Vec::new() };
        let mut rounds = 0;
        loop {
            if !self.current(generation) {
                return Ok(SignIn::Expired);
            }
            let answer = match client.invoke(&export).await {
                Ok(Token::MigrateTo(to)) => {
                    // The account lives in another datacenter: finish there.
                    session.set_home_dc_id(to.dc_id).await.ok();
                    client.invoke_in_dc(to.dc_id, &tl::functions::auth::ImportLoginToken { token: to.token }).await
                }
                answer => answer,
            };
            let token = match answer {
                Ok(Token::Token(token)) => token,
                Ok(Token::Success(success)) => {
                    self.signed_in(client, session, success.authorization).await?;
                    return Ok(SignIn::Done);
                }
                Ok(Token::MigrateTo(_)) => continue,
                Err(e) if e.is("SESSION_PASSWORD_NEEDED") => {
                    let password: tl::enums::account::Password = client
                        .invoke(&tl::functions::account::GetPassword {})
                        .await
                        .map_err(|e| format!("Could not reach Telegram: {e}"))?;
                    let token = PasswordToken::new(password.into());
                    let hint = token.hint().map(str::to_string);
                    *self.password.lock().await = Some(token);
                    self.set_status(|s| {
                        s.state = AccountState::Password;
                        s.qr_svg = None;
                        s.password_hint = hint;
                        s.error = None;
                    });
                    return Ok(SignIn::Password);
                }
                Err(e) => return Err(format!("Telegram did not give a QR code: {e}")),
            };

            rounds += 1;
            if rounds > QR_ROUNDS {
                return Ok(SignIn::Expired);
            }
            let url = format!("tg://login?token={}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&token.token));
            let ttl = (token.expires as i64 - now_ms() / 1000).clamp(5, 120) as u64;
            let svg = render_qr(&url);
            self.set_status(|s| {
                s.state = AccountState::Qr;
                s.qr_svg = svg;
                s.qr_timeout_secs = Some(ttl);
                s.error = None;
            });

            // Until a phone takes it (Telegram says so with an update), or it
            // expires; either way, asking again gives what comes next.
            let expiry = tokio::time::sleep(Duration::from_secs(ttl));
            tokio::pin!(expiry);
            loop {
                tokio::select! {
                    _ = &mut expiry => break,
                    update = updates.recv() => match update {
                        None => return Err("Telegram disconnected.".into()),
                        Some(UpdatesLike::Updates(tl::enums::Updates::UpdateShort(short)))
                            if matches!(short.update, tl::enums::Update::LoginToken) => break,
                        Some(_) => {}
                    },
                }
            }
        }
    }

    /// Keeps who signed in, as grammers does after a code or password.
    async fn signed_in(&self, client: &Client, session: &TgSession, authorization: tl::enums::auth::Authorization) -> Result<(), String> {
        let tl::enums::auth::Authorization::Authorization(authorization) = authorization else {
            return Err("This phone number has no Telegram account yet. Sign up in the Telegram app first.".into());
        };
        let tl::enums::User::User(user) = authorization.user else {
            return Err("Telegram signed in an account it can't describe.".into());
        };
        session
            .cache_peer(&PeerInfo::User {
                id: user.id,
                auth: user.access_hash.map(PeerAuth::from_hash),
                bot: Some(user.bot),
                is_self: Some(true),
            })
            .await
            .ok();
        if let Ok(tl::enums::updates::State::State(state)) = client.invoke(&tl::functions::updates::GetState {}).await {
            let all = UpdatesState { pts: state.pts, qts: state.qts, date: state.date, seq: state.seq, channels: Vec::new() };
            session.set_update_state(UpdateState::All(all)).await.ok();
        }
        session.save();
        Ok(())
    }

    /// The two-step password, after the QR code.
    pub async fn submit_password(&self, password: &str) -> Result<(), String> {
        let mut slot = self.password.lock().await;
        let token = slot.take().ok_or("Telegram isn't asking for a password.")?;
        let client = lock(&self.client).clone().ok_or("Telegram is not connected.")?;
        match client.check_password(token, password.as_bytes()).await {
            Ok(_) => {
                drop(slot);
                self.set_status(|s| {
                    s.state = AccountState::Connecting;
                    s.password_hint = None;
                    s.error = None;
                });
                self.password_done.notify_one();
                Ok(())
            }
            Err(SignInError::InvalidPassword(token)) => {
                *slot = Some(token);
                Err("That password is not right.".into())
            }
            Err(e) => {
                drop(slot);
                Err(format!("Telegram did not accept the password: {e}"))
            }
        }
    }

    /// Telegram ended this computer's session (from another device).
    async fn signed_out(&self, generation: u64) -> Result<(), String> {
        if !self.current(generation) {
            return Ok(());
        }
        self.disconnect();
        self.wipe_files();
        self.set_status(|s| {
            *s = AccountStatus {
                seq: s.seq,
                state: AccountState::LoggedOut,
                error: Some("This computer was logged out of Telegram.".into()),
                ..AccountStatus::idle(false)
            }
        });
        Ok(())
    }

    fn wipe_files(&self) {
        self.history.close();
        let files = [SESSION_FILE, LINKED_MARKER].into_iter().map(|name| self.dir.join(name)).chain(self.history.files());
        for path in files {
            if path.exists()
                && let Err(e) = fs::remove_file(&path)
            {
                log::warn!("telegram: could not remove {}: {e}", path.display());
            }
        }
        let avatars = self.dir.join(AVATAR_DIR);
        if avatars.exists()
            && let Err(e) = fs::remove_dir_all(&avatars)
        {
            log::warn!("telegram: could not remove {}: {e}", avatars.display());
        }
        *lock(&self.store) = ChatStore::default();
        lock(&self.peers).clear();
        self.save_dirty.store(false, Ordering::Release);
        self.chats_dirty.store(true, Ordering::Release);
        self.wake.notify_one();
    }

    pub async fn start_linking(self: &Arc<Self>) -> Result<(), String> {
        if self.is_linked() {
            return Err("This computer is already signed in to Telegram.".into());
        }
        // Leftovers from an abandoned attempt would otherwise be reused.
        self.disconnect();
        self.wipe_files();
        self.connect().inspect_err(|e| self.fail(e.clone()))
    }

    pub async fn cancel_linking(self: &Arc<Self>) {
        if self.is_linked() {
            return;
        }
        self.disconnect();
        *self.password.lock().await = None;
        self.wipe_files();
        self.set_status(|s| *s = AccountStatus { seq: s.seq, ..AccountStatus::idle(false) });
    }

    pub async fn reconnect(self: &Arc<Self>) -> Result<(), String> {
        if self.is_linked() {
            self.connect().inspect_err(|e| self.fail(e.clone()))
        } else {
            self.start_linking().await
        }
    }

    /// Ends this computer's session on Telegram too, then forgets it here.
    pub async fn logout(self: &Arc<Self>) {
        let client = lock(&self.client).clone();
        if let Some(client) = client
            && let Err(e) = client.sign_out().await
        {
            log::warn!("telegram: not signed out on Telegram: {e}");
        }
        self.disconnect();
        self.wipe_files();
        self.set_status(|s| *s = AccountStatus { seq: s.seq, ..AccountStatus::idle(false) });
    }

    fn found_me(&self, me: &User) {
        let avatar = {
            let mut store = lock(&self.store);
            store.set_own_ids([chat_id(me.id())]);
            store.own_avatar()
        };
        let name = me.full_name();
        self.set_status(|s| {
            s.me = Some(Me {
                phone: me.phone().map(|p| format!("+{p}")),
                name: (!name.trim().is_empty()).then_some(name),
                avatar,
            });
        });
        lock(&self.peers).insert(crate::store::OWN_AVATAR.to_string(), Peer::User(me.clone()));
        self.mark_dirty();
    }

    // ---- Chats and messages from Telegram ----

    /// Names a chat, and its people, from what Telegram says about its peer.
    fn learn_peer(&self, store: &mut ChatStore, peer: &Peer) {
        let id = chat_id(peer.id());
        match peer {
            Peer::User(user) => {
                let name = if user.is_self() { "Saved Messages".to_string() } else { user.full_name() };
                if user.contact() || user.is_self() {
                    store.set_contact_name(&id, &name);
                } else {
                    store.set_push_name(&id, &name);
                }
            }
            Peer::Group(_) | Peer::Channel(_) => store.set_group_name(&id, peer.name().unwrap_or_default()),
        }
    }

    /// Loads the chat list: names, unread counts, pins, archive and mutes,
    /// with each chat's last message.
    async fn sync_dialogs(&self, generation: u64, client: &Client) -> Result<(), InvocationError> {
        let mut dialogs = client.iter_dialogs();
        let now_s = now_ms() / 1000;
        let mut pinned = 0;
        while let Some(dialog) = dialogs.next().await? {
            if !self.current(generation) {
                return Ok(());
            }
            let tl::enums::Dialog::Dialog(raw) = &dialog.raw else { continue };
            let id = chat_id(dialog.peer.id());
            let incoming = dialog.last_message.as_ref().and_then(|m| self.incoming(m));
            {
                let mut store = lock(&self.store);
                self.learn_peer(&mut store, &dialog.peer);
                if let Some(incoming) = incoming {
                    store.ingest(incoming, false);
                }
                let tl::enums::PeerNotifySettings::Settings(notify) = &raw.notify_settings;
                // Pinned chats come first, in their order on Telegram.
                let pin = raw.pinned.then(|| {
                    pinned += 1;
                    (now_s - pinned) as u32
                });
                store.ingest_conversation(ConversationMeta {
                    id: &id,
                    name: None,
                    pn_jid: None,
                    lid_jid: None,
                    unread: Some(raw.unread_count.max(0) as u32),
                    marked_unread: Some(raw.unread_mark),
                    pinned_at_s: pin,
                    archived: Some(raw.folder_id == Some(ARCHIVE_FOLDER)),
                    mute_end: notify.mute_until.filter(|until| *until as i64 > now_s).map(|until| until as u64),
                    conversation_ts_s: None,
                });
                let read = store.own_read_up_to(&id, raw.read_outbox_max_id);
                store.apply_receipt(&id, &read, Delivery::Read, None);
            }
            lock(&self.peers).insert(id, dialog.peer.clone());
            self.mark_dirty();
        }
        Ok(())
    }

    /// A Telegram message as the store keeps it. Service messages (joins,
    /// title changes…) are left out for now.
    fn incoming(&self, message: &Message) -> Option<Incoming> {
        let tl::enums::Message::Message(_) = &message.raw else { return None };
        let chat = chat_id(message.peer_id());
        let is_group = message.peer_id().kind() != PeerKind::User;
        let sender = message.sender_id().map(chat_id).filter(|s| *s != chat || is_group);
        let push_name = message.sender().and_then(|p| p.name()).map(str::to_string);
        let text = Some(message.text().to_string()).filter(|t| !t.trim().is_empty());
        let mut content = match message.media() {
            Some(media) => Content::media(MsgKind::Media, media_label(&media), text),
            None => Content::text(text?, None),
        };
        if let Some(reply) = message.reply_to_message_id() {
            let id = reply.to_string();
            let quoted = lock(&self.store).quote_source(&chat, &id);
            content.reply_to = Some(ReplyRef {
                author_jid: quoted.as_ref().and_then(|q| q.sender.clone()),
                id: Some(id),
                text: quoted.map(|q| q.text).unwrap_or_default(),
            });
        }
        content.forwarded = u32::from(message.forward_header().is_some());
        let from_me = message.outgoing();
        Some(Incoming {
            chat,
            is_group,
            sender,
            push_name,
            lid_pn_pairs: Vec::new(),
            message: StoredMessage {
                id: message.id().to_string(),
                from_me,
                ts_ms: message.date().timestamp_millis(),
                author_jid: None,
                content,
                status: from_me.then_some(Delivery::Sent),
                reactions: Vec::new(),
                wire: None,
                read_by: Vec::new(),
                starred: false,
                deleted: None,
                edited_ms: message.edit_date().filter(|_| !message.edit_hide()).map(|at| at.timestamp_millis()),
            },
        })
    }

    fn handle_update(self: &Arc<Self>, update: Update) {
        match update {
            Update::NewMessage(message) => {
                let Some(incoming) = self.incoming(&message) else { return };
                if let Some(peer) = message.peer() {
                    self.learn_peer(&mut lock(&self.store), peer);
                }
                let from_me = incoming.message.from_me;
                let mut store = lock(&self.store);
                if let Some(dto) = store.ingest(incoming, !from_me) {
                    // As in Telegram, a new message brings an archived chat
                    // back, unless it is muted.
                    store.unarchive_unless_muted(&dto.chat_id);
                    let notice = store.message_notice(&dto.chat_id, &dto.id);
                    drop(store);
                    let _ = self.app.emit("tg://message", dto);
                    if let Some(notice) = notice {
                        crate::notify::deliver(&self.app, "telegram", "tg://notice", notice);
                    }
                }
                self.mark_dirty();
            }
            Update::MessageEdited(message) => {
                let Some(incoming) = self.incoming(&message) else { return };
                let at = incoming.message.edited_ms.unwrap_or_else(now_ms);
                let editor = (!incoming.message.from_me).then(|| incoming.sender.clone().unwrap_or_else(|| incoming.chat.clone()));
                let edited = lock(&self.store).edit_message(&incoming.chat, &incoming.message.id, editor.as_deref(), incoming.message.content, at);
                if let Some(dto) = edited {
                    let _ = self.app.emit("tg://message", dto);
                    self.mark_dirty();
                }
            }
            Update::MessageDeleted(deletion) => {
                let channel = deletion.channel_id().map(|id| chat_id(PeerId::channel_unchecked(id)));
                let mut store = lock(&self.store);
                let mut changed = Vec::new();
                for id in deletion.messages().iter().map(i32::to_string) {
                    // Outside channels, ids are the account's own: one chat has it.
                    let chats = match &channel {
                        Some(chat) => vec![chat.clone()],
                        None => store.chats_with(&id).into_iter().filter(|c| !is_channel_chat(c)).collect(),
                    };
                    for chat in chats {
                        changed.extend(store.delete_message(&chat, &id, Deletion::ForEveryone));
                    }
                }
                drop(store);
                for dto in changed {
                    let _ = self.app.emit("tg://message", dto);
                }
                self.mark_dirty();
            }
            Update::Raw(raw) => self.handle_raw(raw.raw),
            _ => {}
        }
    }

    fn handle_raw(&self, update: tl::enums::Update) {
        use tl::enums::Update as U;
        let (chat, read_out, read_in) = match update {
            U::ReadHistoryOutbox(u) => (chat_id(PeerId::from(u.peer)), Some(u.max_id), None),
            U::ReadChannelOutbox(u) => (chat_id(PeerId::channel_unchecked(u.channel_id)), Some(u.max_id), None),
            U::ReadHistoryInbox(u) => (chat_id(PeerId::from(u.peer)), None, Some(u.still_unread_count)),
            U::ReadChannelInbox(u) => (chat_id(PeerId::channel_unchecked(u.channel_id)), None, Some(u.still_unread_count)),
            _ => return,
        };
        let mut store = lock(&self.store);
        if let Some(max_id) = read_out {
            let read = store.own_read_up_to(&chat, max_id);
            for dto in store.apply_receipt(&chat, &read, Delivery::Read, None) {
                let _ = self.app.emit("tg://message", dto);
            }
        }
        // Read on another device.
        if let Some(left) = read_in {
            if left <= 0 {
                store.mark_read(&chat);
            } else {
                store.ingest_conversation(ConversationMeta {
                    id: &chat,
                    name: None,
                    pn_jid: None,
                    lid_jid: None,
                    unread: Some(left as u32),
                    marked_unread: None,
                    pinned_at_s: None,
                    archived: None,
                    mute_end: None,
                    conversation_ts_s: None,
                });
            }
        }
        drop(store);
        self.mark_dirty();
    }

    /// Fetches a chat's latest messages from Telegram, once per connection,
    /// and has an open chat show them.
    fn fetch_recent(self: &Arc<Self>, chat: &str) {
        if !lock(&self.fetched).insert(chat.to_string()) {
            return;
        }
        let mgr = self.clone();
        let chat = chat.to_string();
        tauri::async_runtime::spawn(async move {
            match mgr.fetch_page(&chat, None).await {
                Ok(_) => {
                    let _ = mgr.app.emit("tg://history", chat);
                }
                Err(e) => {
                    lock(&mgr.fetched).remove(&chat);
                    log::warn!("telegram: messages not fetched: {e}");
                }
            }
        });
    }

    /// Up to a page of a chat's messages from Telegram, from before message
    /// `before` or the newest, stored here. Returns them oldest first.
    async fn fetch_page(&self, chat: &str, before: Option<i32>) -> Result<Vec<StoredMessage>, String> {
        let client = self.client()?;
        let peer = self.peer_ref(chat).await?;
        let mut messages = client.iter_messages(peer).limit(PAGE);
        if let Some(before) = before {
            messages = messages.offset_id(before);
        }
        let mut fetched = Vec::new();
        while let Some(message) = messages.next().await.map_err(|e| e.to_string())? {
            if let Some(incoming) = self.incoming(&message) {
                fetched.push(incoming);
            }
        }
        fetched.reverse();
        let mut stored = Vec::with_capacity(fetched.len());
        let mut store = lock(&self.store);
        for incoming in fetched {
            stored.push(incoming.message.clone());
            store.ingest(incoming, false);
        }
        drop(store);
        self.mark_dirty();
        Ok(stored)
    }

    /// Downloads the profile pictures that changed or were never fetched.
    async fn sync_avatars(&self, generation: u64, client: Client) {
        if self.avatars_running.swap(true, Ordering::AcqRel) {
            return;
        }
        let dir = self.dir.join(AVATAR_DIR);
        if let Err(e) = secure_dir(&dir) {
            log::warn!("telegram: no avatar directory: {e}");
            self.avatars_running.store(false, Ordering::Release);
            return;
        }
        let mut due = lock(&self.store).avatars_due(usize::MAX);
        if let Some((_, current)) = lock(&self.store).own_avatar_due() {
            due.push((OWN_AVATAR.to_string(), current));
        }
        for (chat, current) in due {
            if !self.current(generation) {
                break;
            }
            let Some(peer) = lock(&self.peers).get(&chat).cloned() else { continue };
            let picture = match peer.photo(false).await {
                Ok(picture) => picture,
                Err(e) => {
                    log::warn!("telegram: picture of {chat} not found: {e}");
                    continue;
                }
            };
            let picture_id = photo_id(&peer).map(|id| id.to_string());
            let saved = match (picture, &picture_id) {
                (Some(_), Some(id)) if current.as_deref() == Some(id) && dir.join(avatar_file(&chat, id)).exists() => Ok(()),
                (Some(picture), Some(id)) => {
                    let mut bytes = Vec::new();
                    let mut download = client.iter_download(&picture);
                    let mut outcome = Ok(());
                    loop {
                        match download.next().await {
                            Ok(Some(chunk)) => bytes.extend(chunk),
                            Ok(None) => break,
                            Err(e) => {
                                outcome = Err(e.to_string());
                                break;
                            }
                        }
                    }
                    outcome.and_then(|()| crate::vault::get().write(&dir.join(avatar_file(&chat, id)), &bytes).map_err(|e| e.to_string()))
                }
                _ => Ok(()),
            };
            if let Err(e) = saved {
                log::warn!("telegram: picture of {chat} not saved: {e}");
                continue;
            }
            if let Some(old) = current.filter(|old| Some(old) != picture_id.as_ref()) {
                let _ = fs::remove_file(dir.join(avatar_file(&chat, &old)));
            }
            {
                let mut store = lock(&self.store);
                if chat == OWN_AVATAR {
                    store.set_own_avatar(picture_id);
                } else {
                    store.set_avatar(&chat, picture_id);
                }
            }
            if chat == OWN_AVATAR {
                let avatar = lock(&self.store).own_avatar();
                self.set_status(|s| {
                    if let Some(me) = s.me.as_mut() {
                        me.avatar = avatar;
                    }
                });
            }
            self.mark_dirty();
            tokio::time::sleep(AVATAR_PACE).await;
        }
        self.avatars_running.store(false, Ordering::Release);
    }

    // ---- What the UI asks for ----

    pub fn list_chats(&self) -> Vec<ChatSummary> {
        lock(&self.store).list_chats()
    }

    /// A chat's messages here; its latest are fetched from Telegram the first
    /// time it is opened, and shown when they arrive.
    pub fn messages(self: &Arc<Self>, chat_id: &str) -> Vec<MessageDto> {
        if lock(&self.status).state == AccountState::Connected {
            self.fetch_recent(chat_id);
        }
        lock(&self.store).messages(chat_id)
    }

    /// Messages of a chat from before `before_ms`, from the history database,
    /// or from Telegram past what it has.
    pub async fn older_messages(&self, chat_id: &str, before_ms: i64, limit: usize) -> Vec<MessageDto> {
        let older = self.history.older(chat_id, before_ms, limit);
        if !older.is_empty() {
            return lock(&self.store).dtos(chat_id, &older);
        }
        let oldest = lock(&self.store).oldest_message(chat_id).and_then(|m| m.id.parse::<i32>().ok());
        let Some(oldest) = oldest.or_else(|| self.history.oldest(chat_id).and_then(|m| m.id.parse().ok())) else {
            return Vec::new();
        };
        match self.fetch_page(chat_id, Some(oldest)).await {
            Ok(fetched) => {
                let fetched: Vec<_> = fetched.into_iter().filter(|m| m.ts_ms < before_ms).collect();
                lock(&self.store).dtos(chat_id, &fetched)
            }
            Err(e) => {
                log::warn!("telegram: older messages not fetched: {e}");
                Vec::new()
            }
        }
    }

    pub fn mark_read(self: &Arc<Self>, chat_id: &str) {
        {
            let mut store = lock(&self.store);
            if !store.has_unread(chat_id) {
                return;
            }
            store.mark_read(chat_id);
        }
        self.mark_dirty();
        let mgr = self.clone();
        let chat = chat_id.to_string();
        tauri::async_runtime::spawn(async move {
            let Ok(client) = mgr.client() else { return };
            let read = match mgr.peer_ref(&chat).await {
                Ok(peer) => client.mark_as_read(peer).await.map_err(|e| e.to_string()),
                Err(e) => Err(e),
            };
            if let Err(e) = read {
                log::warn!("telegram: chat not marked read: {e}");
            }
        });
    }

    pub async fn send_text(&self, chat_id: &str, text: &str, reply_to: Option<&str>) -> Result<MessageDto, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("The message is empty.".into());
        }
        let client = self.client()?;
        let peer = self.peer_ref(chat_id).await?;
        let reply = reply_to.and_then(|id| id.parse::<i32>().ok());
        let sent = client
            .send_message(peer, InputMessage::new().text(text).reply_to(reply))
            .await
            .map_err(|e| format!("The message was not sent: {e}"))?;
        let mut incoming = self.incoming(&sent).ok_or("Telegram sent back something unexpected.")?;
        // Sent from here, the reply is known even when its message isn't.
        incoming.chat = chat_id.to_string();
        let id = incoming.message.id.clone();
        let mut store = lock(&self.store);
        store.ingest(incoming, false);
        let dto = store.message_dto(chat_id, &id).ok_or("The message was sent, but is not shown.")?;
        drop(store);
        self.mark_dirty();
        Ok(dto)
    }

    pub fn lists(&self) -> Vec<ListDto> {
        lock(&self.store).lists()
    }

    pub fn starred(&self) -> Vec<StarredDto> {
        lock(&self.store).starred()
    }

    pub fn search(&self, query: &str, chat: Option<&str>) -> Vec<StarredDto> {
        let limit = if chat.is_some() { crate::store::CHAT_SEARCH_LIMIT } else { crate::store::SEARCH_LIMIT };
        let terms = lock(&self.store).search_terms(query);
        let saved = self.history.search(&terms, chat, limit);
        lock(&self.store).search(query, chat, limit, saved)
    }

    pub fn contacts(&self) -> Vec<ContactDto> {
        lock(&self.store).contacts()
    }

    pub fn person(&self, id: &str) -> crate::store::PersonDto {
        lock(&self.store).person(id)
    }

    pub fn person_details(&self, id: &str) -> crate::store::PersonDetailsDto {
        crate::store::PersonDetailsDto { common_groups: lock(&self.store).common_groups(id), ..Default::default() }
    }

    pub fn group_info(&self, id: &str) -> crate::store::GroupInfoDto {
        lock(&self.store).group_info(id)
    }

    // Favorites, lists, stars, pins, mutes and the archive stay on this
    // computer for now, as they do for Signal.

    pub fn set_favorite(&self, chat_id: &str, favorite: bool) {
        lock(&self.store).set_favorite(chat_id, favorite);
        self.mark_dirty();
    }

    /// Creates a list, or renames an existing one, and adds and removes
    /// chats. Returns its id.
    pub fn save_list(&self, id: Option<String>, name: &str, add: &[String], remove: &[String]) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("The list needs a name.".into());
        }
        let mut store = lock(&self.store);
        let (id, order) = match id {
            Some(id) => (id.clone(), store.list(&id).ok_or("That list is gone.")?.order),
            None => store.next_list(),
        };
        store.edit_list(&id, Some(name), true, Some(order), None);
        for chat in remove {
            store.set_list_chat(&id, chat, false);
        }
        for chat in add {
            store.set_list_chat(&id, chat, true);
        }
        drop(store);
        self.mark_dirty();
        Ok(id)
    }

    pub fn delete_list(&self, id: &str) {
        lock(&self.store).delete_list(id);
        self.mark_dirty();
    }

    pub fn set_starred(&self, chat_id: &str, message_id: &str, starred: bool) -> Result<MessageDto, String> {
        let dto = lock(&self.store).set_starred(chat_id, message_id, starred).ok_or("That message is gone.")?;
        self.mark_dirty();
        Ok(dto)
    }

    pub fn set_muted(&self, chat_id: &str, muted: bool, until_ms: Option<i64>) {
        lock(&self.store).set_muted(chat_id, muted.then(|| until_ms.unwrap_or(i64::MAX)));
        self.mark_dirty();
    }

    pub fn set_archived(&self, chat_id: &str, archived: bool) {
        let mut store = lock(&self.store);
        store.set_archived(chat_id, archived);
        if archived {
            store.set_pinned(chat_id, None);
        }
        drop(store);
        self.mark_dirty();
    }

    pub fn set_pinned(&self, chat_id: &str, pinned: bool) -> Result<(), String> {
        let mut store = lock(&self.store);
        if pinned && store.pinned_count() >= MAX_PINNED {
            return Err(format!("You can only pin up to {MAX_PINNED} chats."));
        }
        store.set_pinned(chat_id, pinned.then(now_ms));
        drop(store);
        self.mark_dirty();
        Ok(())
    }

    pub fn set_unread(&self, chat_id: &str, unread: bool) {
        lock(&self.store).set_marked_unread(chat_id, unread);
        self.mark_dirty();
    }

    /// Deletes a chat and its messages on this computer.
    pub fn delete_chat(&self, chat_id: &str) {
        let mut store = lock(&self.store);
        let before = store.clear_point(chat_id);
        store.delete_chat(chat_id, before);
        drop(store);
        self.chat_emptied(chat_id);
    }

    /// Empties a chat of its messages on this computer, keeping it in the
    /// list; starred messages stay when `keep_starred`.
    pub fn clear_chat(&self, chat_id: &str, keep_starred: bool) {
        let mut store = lock(&self.store);
        let before = store.clear_point(chat_id);
        store.clear_chat(chat_id, before, keep_starred);
        drop(store);
        self.chat_emptied(chat_id);
    }

    fn chat_emptied(&self, chat_id: &str) {
        let chat = lock(&self.store).chat_key(chat_id);
        let _ = self.app.emit("tg://cleared", chat);
        self.mark_dirty();
    }

    pub fn not_yet<T>(&self) -> Result<T, String> {
        Err(NOT_YET.into())
    }
}

/// The id of the peer's current profile picture.
fn photo_id(peer: &Peer) -> Option<i64> {
    match peer {
        Peer::User(user) => user.photo().map(|photo| photo.photo_id),
        Peer::Group(group) => group.photo().map(|photo| photo.photo_id),
        Peer::Channel(channel) => channel.photo().map(|photo| photo.photo_id),
    }
}

/// What a message's attachment is called in the chat until it can be shown.
fn media_label(media: &grammers_client::media::Media) -> String {
    use grammers_client::media::Media as M;
    match media {
        M::Photo(_) => "Photo".into(),
        M::Sticker(_) => "Sticker".into(),
        M::Contact(_) => "Contact".into(),
        M::Poll(_) => "Poll".into(),
        M::Geo(_) | M::Venue(_) => "Location".into(),
        M::GeoLive(_) => "Live location".into(),
        M::Dice(_) => "Dice".into(),
        M::WebPage(_) => "Link".into(),
        M::Document(document) => {
            let mime = document.mime_type().unwrap_or_default();
            if mime.starts_with("video/") {
                if document.is_animated() { "GIF" } else { "Video" }.into()
            } else if mime == "audio/ogg" {
                "Voice message".into()
            } else if mime.starts_with("audio/") {
                "Audio".into()
            } else {
                document.name().filter(|n| !n.is_empty()).map_or_else(|| "File".into(), str::to_string)
            }
        }
        _ => "Attachment".into(),
    }
}
