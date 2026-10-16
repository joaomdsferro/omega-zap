//! Desktop notifications. The UI decides whether a message is worth one;
//! this shows it and brings the chat up when it is clicked.
//!
//! Messages that arrive in a rush (the ones that waited while the app was
//! off or the connection was down) are held back until the rush is over,
//! and then each chat shows only its latest one, with a single sound.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::account::lock;
use crate::store::Notice;
use crate::tray::{self, Background};
use crate::{signal, wa};

/// More notifications than this within `BURST_WINDOW` is a rush.
const BURST_COUNT: usize = 3;
const BURST_WINDOW: Duration = Duration::from_secs(3);
/// A rush is over once nothing new came for this long…
const QUIET: Duration = Duration::from_millis(1500);
/// …or, at the latest, this long after it started.
const MAX_HOLD: Duration = Duration::from_secs(8);
/// Right after start everything waiting arrives at once.
const STARTUP: Duration = Duration::from_secs(20);
/// After a rush, chats beyond this many are folded into one notification.
const MAX_AFTER_RUSH: usize = 4;

/// The notification each chat last showed, so the next one replaces it
/// instead of piling up, and the ones held back during a rush.
pub struct Shown {
    ids: Mutex<HashMap<String, u32>>,
    gate: Mutex<Gate>,
    started: Instant,
}

impl Default for Shown {
    fn default() -> Self {
        Self { ids: Mutex::default(), gate: Mutex::default(), started: Instant::now() }
    }
}

#[derive(Default)]
struct Gate {
    recent: VecDeque<Instant>,
    /// Latest notification of each chat, oldest chat first.
    held: Vec<Note>,
    /// When the rush began and when its last notification came.
    rush: Option<(Instant, Instant)>,
}

struct Note {
    account: String,
    chat_id: String,
    title: String,
    body: String,
    avatar: Option<String>,
}

impl Note {
    fn key(&self) -> String {
        format!("{}:{}", self.account, self.chat_id)
    }
}

/// Key of the notification that stands for the chats a rush left out.
const SUMMARY: &str = "summary";

/// Where a chat's saved profile picture is, when the name is one of ours.
fn avatar_path(app: &AppHandle, account: &str, name: &str) -> Option<PathBuf> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    let dir = app.path().app_data_dir().ok()?;
    let path = match account {
        "whatsapp" => dir.join(wa::AVATAR_DIR).join(name),
        "business" => wa::Kind::Business.dir(&dir).join(wa::AVATAR_DIR).join(name),
        "signal" => dir.join("signal").join(signal::AVATAR_DIR).join(name),
        _ => return None,
    };
    // The notification daemon needs a file it can read: a plain copy, in memory.
    (plain && path.exists()).then(|| crate::vault::get().export(&path, &format!("notify-{name}")).ok()).flatten()
}

/// Hands a notice to the UI, which knows whether the chat is in front of the
/// user. With no window open, it is shown here as the UI's settings say.
pub fn deliver(app: &AppHandle, account: &str, event: &str, notice: Notice) {
    if tray::has_window(app) {
        let _ = app.emit(event, notice);
        return;
    }
    let prefs = app.state::<Background>().prefs();
    if !prefs.notifications || notice.muted {
        return;
    }
    let body = if prefs.preview { notice.body } else { "New message".into() };
    let note = Note { account: account.into(), chat_id: notice.chat_id, title: notice.title, body, avatar: notice.avatar };
    if let Err(e) = show(app, &app.state::<Shown>(), note) {
        log::warn!("{e}");
    }
}

/// Takes a chat's notification away once the chat has been opened.
#[tauri::command]
pub fn dismiss_notification(shown: State<'_, Shown>, account: String, chat_id: String) {
    let key = format!("{account}:{chat_id}");
    lock(&shown.gate).held.retain(|note| note.key() != key);
    let id = lock(&shown.ids).remove(&key);
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(id) = id {
        // Off the main thread: the session bus can be slow to answer.
        std::thread::spawn(move || freedesktop::close(id));
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let _ = id;
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
    show(&app, &shown, Note { account, chat_id, title, body, avatar })
}

/// Shows a notification now, or holds it back while messages are rushing in.
fn show(app: &AppHandle, shown: &Shown, note: Note) -> Result<(), String> {
    let now = Instant::now();
    let mut gate = lock(&shown.gate);
    gate.recent.retain(|t| now.duration_since(*t) < BURST_WINDOW);
    gate.recent.push_back(now);
    let rushing = gate.rush.is_some()
        || gate.recent.len() > BURST_COUNT
        || now.duration_since(shown.started) < STARTUP;
    if !rushing {
        drop(gate);
        return present(app, shown, note, true);
    }
    let key = note.key();
    gate.held.retain(|held| held.key() != key);
    gate.held.push(note);
    match &mut gate.rush {
        Some((_, last)) => *last = now,
        None => {
            gate.rush = Some((now, now));
            let app = app.clone();
            std::thread::spawn(move || release_after_rush(&app));
        }
    }
    Ok(())
}

/// Waits for the rush to end, then shows the latest notification of each
/// chat, folding the oldest chats into one when there are many.
fn release_after_rush(app: &AppHandle) {
    let shown = app.state::<Shown>();
    let held = loop {
        std::thread::sleep(QUIET / 3);
        let mut gate = lock(&shown.gate);
        let Some((start, last)) = gate.rush else { return };
        let now = Instant::now();
        if now.duration_since(last) >= QUIET || now.duration_since(start) >= MAX_HOLD {
            gate.rush = None;
            break std::mem::take(&mut gate.held);
        }
    };
    // The oldest chats, beyond what fits, share one notification.
    let mut held = held;
    let folded = if held.len() > MAX_AFTER_RUSH { held.len() - (MAX_AFTER_RUSH - 1) } else { 0 };
    let latest = held.split_off(folded);
    let mut sound = true;
    if folded > 0 {
        let summary = Note {
            account: String::new(),
            chat_id: SUMMARY.into(),
            title: "Omega Zap".into(),
            body: format!("New messages in {folded} more chats"),
            avatar: None,
        };
        if let Err(e) = present(app, &shown, summary, std::mem::take(&mut sound)) {
            log::warn!("{e}");
        }
    }
    for note in latest {
        if let Err(e) = present(app, &shown, note, std::mem::take(&mut sound)) {
            log::warn!("{e}");
        }
    }
}

fn present(app: &AppHandle, shown: &Shown, note: Note, sound: bool) -> Result<(), String> {
    let key = note.key();
    let Note { account, chat_id, title, body, avatar } = note;
    let icon = avatar.and_then(|name| avatar_path(app, &account, &name));

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Replacing the old notification in place updates it silently once
        // it has left the screen, so it is closed and a new one pops up.
        if let Some(id) = lock(&shown.ids).remove(&key) {
            freedesktop::close(id);
        }
        let open = (!account.is_empty()).then(|| tray::OpenChat { account, chat_id });
        let id = freedesktop::notify(app, &title, &body, icon.as_deref(), open, sound)?;
        lock(&shown.ids).insert(key, id);
    }

    // Elsewhere clicks are not reported back.
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let _ = (shown, key, account, chat_id, sound);
        let mut notification = notify_rust::Notification::new();
        notification.appname("Omega Zap").summary(&title).body(&body);
        if let Some(path) = icon {
            notification.icon(&path.to_string_lossy());
        }
        notification.show().map_err(|e| format!("Could not show a notification: {e}"))?;
    }

    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
pub use freedesktop::register_desktop_entry;

/// The desktop's notification server, spoken to directly. Plasma sends a
/// notification's click, and the token that lets the window come to the
/// front on Wayland, only to the connection that showed it, so every
/// notification goes out on one connection, and one thread listens on it.
#[cfg(all(unix, not(target_os = "macos")))]
mod freedesktop {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};

    use tauri::AppHandle;
    use zbus::blocking::{Connection, MessageIterator};
    use zbus::zvariant::Value;

    use crate::account::lock;
    use crate::tray::{self, OpenChat};

    const DEST: &str = "org.freedesktop.Notifications";
    /// The desktop entry notifications name. Plasma keeps a notification in
    /// its history only when it knows the app it came from.
    const DESKTOP_ID: &str = "omega-zap";
    /// The entry the Linux packages install, named after the product.
    const PACKAGED_ID: &str = "Omega Zap";
    /// The entry in use: the packaged one when installed, else ours.
    static ENTRY: OnceLock<&'static str> = OnceLock::new();
    const PATH: &str = "/org/freedesktop/Notifications";
    /// Emoji in a notification, about as tall as its text.
    const EMOJI_PX: u32 = 16;

    struct Bus {
        conn: Connection,
        /// The chat each notification still on screen or in the history opens.
        targets: Mutex<HashMap<u32, Option<OpenChat>>>,
        /// The body is read as markup: `<` and `&` have to be escaped.
        markup: bool,
        /// Images in the body are drawn, so emoji can be Apple's.
        images: bool,
    }

    static BUS: OnceLock<Option<Bus>> = OnceLock::new();

    fn bus(app: &AppHandle) -> Option<&'static Bus> {
        BUS.get_or_init(|| {
            let conn = Connection::session().inspect_err(|e| log::warn!("no session bus for notifications: {e}")).ok()?;
            let app = app.clone();
            let listening = conn.clone();
            std::thread::Builder::new()
                .name("notifications".into())
                .spawn(move || listen(&app, &listening))
                .ok()?;
            let caps: Vec<String> = conn
                .call_method(Some(DEST), PATH, Some(DEST), "GetCapabilities", &())
                .ok()
                .and_then(|reply| reply.body().deserialize().ok())
                .unwrap_or_default();
            let markup = caps.iter().any(|c| c == "body-markup");
            let images = markup && caps.iter().any(|c| c == "body-images");
            Some(Bus { conn, targets: Mutex::new(HashMap::new()), markup, images })
        })
        .as_ref()
    }

    /// `open` is the chat a click opens; without one it just brings the window up.
    pub fn notify(
        app: &AppHandle,
        title: &str,
        body: &str,
        icon: Option<&Path>,
        open: Option<OpenChat>,
        sound: bool,
    ) -> Result<u32, String> {
        let bus = bus(app).ok_or("Notifications are not available.")?;
        let icon = icon.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let body = if bus.images {
            crate::emoji::markup(app, body, EMOJI_PX)
        } else if bus.markup {
            crate::emoji::escape(body)
        } else {
            body.to_string()
        };
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        hints.insert("category", Value::from("im.received"));
        hints.insert("urgency", Value::U8(1));
        hints.insert("desktop-entry", Value::from(*ENTRY.get().unwrap_or(&DESKTOP_ID)));
        // Clicking the notification itself runs its "default" action.
        let actions = vec!["default", "Open chat"];
        let reply = bus
            .conn
            .call_method(Some(DEST), PATH, Some(DEST), "Notify", &("Omega Zap", 0u32, icon, title, body, actions, hints, -1i32))
            .map_err(|e| format!("Could not show a notification: {e}"))?;
        let id: u32 = reply.body().deserialize().map_err(|e| format!("Could not show a notification: {e}"))?;
        lock(&bus.targets).insert(id, open);
        if sound && !inhibited(&bus.conn) {
            play_sound();
        }
        Ok(id)
    }

    /// Makes sure the desktop knows the app by `DESKTOP_ID`, which is also
    /// the window's Wayland app id (the executable's name), so the taskbar
    /// and notifications show its name and icon. An installed package brings
    /// its own entry; otherwise one is written for the user, hidden from the
    /// menu, pointing at this executable. Debug builds call themselves "Dev".
    pub fn register_desktop_entry() -> std::io::Result<()> {
        let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        let installed = |id: &str| {
            let file = format!("{id}.desktop");
            data_dirs.split(':').any(|dir| Path::new(dir).join("applications").join(&file).exists())
        };
        // Debug builds keep their own "Dev" entry beside an installed copy.
        if !cfg!(debug_assertions) {
            for id in [DESKTOP_ID, PACKAGED_ID] {
                if installed(id) {
                    let _ = ENTRY.set(id);
                    return Ok(());
                }
            }
        }
        let _ = ENTRY.set(DESKTOP_ID);
        let file = format!("{DESKTOP_ID}.desktop");
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/share")))
            .ok_or_else(|| std::io::Error::other("no home folder"))?;
        #[cfg(debug_assertions)]
        let (name, icon_bytes): (&str, &[u8]) = ("Omega Zap (Dev)", include_bytes!("../icons/dev/128x128.png"));
        #[cfg(not(debug_assertions))]
        let (name, icon_bytes): (&str, &[u8]) = ("Omega Zap", include_bytes!("../icons/128x128.png"));
        // Named after its contents: the desktop caches icons by name, so a
        // changed icon needs a new one to show.
        let hash = icon_bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100_0000_01b3));
        let icon_name = format!("{DESKTOP_ID}-{:08x}", hash as u32);
        let icon_dir = data_home.join("icons/hicolor/128x128/apps");
        std::fs::create_dir_all(&icon_dir)?;
        let icon = icon_dir.join(format!("{icon_name}.png"));
        if !icon.exists() {
            std::fs::write(&icon, icon_bytes)?;
        }
        for entry in std::fs::read_dir(&icon_dir)?.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if file.starts_with(DESKTOP_ID) && file.ends_with(".png") && file != format!("{icon_name}.png") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        // An AppImage runs from a mount that changes every start; the
        // desktop has to start the image itself.
        let exe = match std::env::var_os("APPIMAGE") {
            Some(image) => std::path::PathBuf::from(image),
            None => std::env::current_exe()?,
        };
        let entry = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name={name}\n\
             Comment=WhatsApp and Signal in one place\n\
             Exec=\"{}\"\n\
             Icon={icon_name}\n\
             Categories=Network;InstantMessaging;\n\
             NoDisplay=true\n\
             X-GNOME-UsesNotifications=true\n",
            exe.display()
        );
        let apps = data_home.join("applications");
        std::fs::create_dir_all(&apps)?;
        let path = apps.join(&file);
        if std::fs::read_to_string(&path).ok().as_deref() == Some(entry.as_str()) {
            return Ok(());
        }
        std::fs::write(&path, entry)?;
        // KDE reads entries from a cache; have it see the new one now.
        std::thread::spawn(|| {
            let _ = std::process::Command::new("kbuildsycoca6")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        });
        Ok(())
    }

    pub fn close(id: u32) {
        let Some(Some(bus)) = BUS.get() else {
            return;
        };
        lock(&bus.targets).remove(&id);
        if let Err(e) = bus.conn.call_method(Some(DEST), PATH, Some(DEST), "CloseNotification", &(id,)) {
            log::debug!("could not close notification {id}: {e}");
        }
    }

    /// Whether the desktop is holding notifications back ("Do not disturb").
    fn inhibited(conn: &Connection) -> bool {
        conn.call_method(Some(DEST), PATH, Some("org.freedesktop.DBus.Properties"), "Get", &(DEST, "Inhibited"))
            .ok()
            .and_then(|reply| reply.body().deserialize::<zbus::zvariant::OwnedValue>().ok())
            .and_then(|value| bool::try_from(value).ok())
            .unwrap_or(false)
    }

    /// The sound theme's sound for a new instant message. Plasma plays none
    /// for apps it has no notification settings for.
    fn play_sound() {
        std::thread::spawn(|| {
            let themed = std::process::Command::new("canberra-gtk-play")
                .args(["--id", "message-new-instant", "--description", "Omega Zap message"])
                .status();
            if themed.is_ok_and(|status| status.success()) {
                return;
            }
            const FALLBACK: &str = "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga";
            for player in ["pw-play", "paplay"] {
                if std::process::Command::new(player).arg(FALLBACK).status().is_ok_and(|s| s.success()) {
                    return;
                }
            }
        });
    }

    fn listen(app: &AppHandle, conn: &Connection) {
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(DEST)
            .map(|rule| rule.build());
        let messages = match rule.map(|rule| MessageIterator::for_match_rule(rule, conn, None)) {
            Ok(Ok(messages)) => messages,
            Ok(Err(e)) | Err(e) => return log::warn!("cannot listen for notification clicks: {e}"),
        };
        // Sent just before a click, for the window to be let to the front.
        let mut tokens: HashMap<u32, String> = HashMap::new();
        for message in messages.flatten() {
            let header = message.header();
            let body = message.body();
            match header.member().map(|m| m.as_str()) {
                Some("ActivationToken") => {
                    if let Ok((id, token)) = body.deserialize::<(u32, String)>() {
                        tokens.insert(id, token);
                    }
                }
                Some("ActionInvoked") => {
                    let Ok((id, action)) = body.deserialize::<(u32, String)>() else { continue };
                    let open = BUS.get().and_then(Option::as_ref).and_then(|bus| lock(&bus.targets).remove(&id));
                    match (open, action.as_str()) {
                        (Some(Some(open)), "default") => tray::open_chat(app, open, tokens.remove(&id)),
                        (Some(None), "default") => tray::bring_up_with(app, tokens.remove(&id)),
                        _ => {}
                    }
                }
                Some("NotificationClosed") => {
                    // 1 is expired: off the screen but still in the history,
                    // where it can be clicked.
                    if let Ok((id, reason)) = body.deserialize::<(u32, u32)>()
                        && reason != 1
                    {
                        tokens.remove(&id);
                        if let Some(bus) = BUS.get().and_then(Option::as_ref) {
                            lock(&bus.targets).remove(&id);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
