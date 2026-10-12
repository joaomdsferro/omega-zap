//! What every linked account reports to the UI, whatever the network.

use std::fs;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AccountState {
    Idle,
    Starting,
    Qr,
    QrExpired,
    Connecting,
    Connected,
    LoggedOut,
    Error,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub phone: Option<String>,
    pub name: Option<String>,
    /// File name of our own profile picture inside the avatars directory.
    pub avatar: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub seq: u64,
    pub state: AccountState,
    pub linked: bool,
    pub qr_svg: Option<String>,
    pub qr_timeout_secs: Option<u64>,
    pub sync_progress: Option<u32>,
    /// Older history being fetched from the phone, chat by chat.
    pub backfill: Option<Backfill>,
    pub error: Option<String>,
    /// Something that went wrong without breaking the connection.
    pub notice: Option<String>,
    pub me: Option<Me>,
}

impl AccountStatus {
    pub fn idle(linked: bool) -> Self {
        Self {
            seq: 0,
            state: AccountState::Idle,
            linked,
            qr_svg: None,
            qr_timeout_secs: None,
            sync_progress: None,
            backfill: None,
            error: None,
            notice: None,
            me: None,
        }
    }
}

/// Where fetching the whole history from the phone has got to.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Backfill {
    pub state: BackfillState,
    /// The chat being fetched.
    pub chat: Option<String>,
    /// Chats with all their history here, of `total`.
    pub done: u32,
    pub total: u32,
    /// Messages fetched since the app started.
    pub messages: u32,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BackfillState {
    Fetching,
    /// The phone stopped answering, probably off or offline; tried again later.
    WaitingForPhone,
    Done,
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The app's identifier (tauri.conf.json), which also names the data folder
/// and the storage key.
pub const IDENTIFIER: &str = "app.wisething.omegazap";
/// The identifier of builds up to 0.0.3.
pub const OLD_IDENTIFIER: &str = "rocks.omegazap.desktop";

/// Moves the data of builds up to 0.0.3, kept under the old identifier, to
/// the folder of `dir`, once. A rename: nothing is copied.
pub fn move_old_data(dir: &Path) {
    let renamed = |old: &Path, new: &Path| {
        // An empty folder made in the meantime does not stop the move.
        if !old.is_dir() || (new.exists() && fs::remove_dir(new).is_err()) {
            return;
        }
        match fs::rename(old, new) {
            Ok(()) => log::info!("moved {} to {}", old.display(), new.display()),
            Err(e) => log::warn!("could not move {} to {}: {e}", old.display(), new.display()),
        }
    };
    if let Some(parent) = dir.parent() {
        renamed(&parent.join(OLD_IDENTIFIER), dir);
    }
    // The webview's own data (the UI's settings) is kept apart on macOS.
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        let webkit = Path::new(&home).join("Library").join("WebKit");
        renamed(&webkit.join(OLD_IDENTIFIER), &webkit.join(IDENTIFIER));
    }
}

/// What the phone lists this link as under Linked devices: by default the
/// computer's name, so several installs can be told apart; the link screen
/// can change it. Sent only when linking: an existing link keeps its name.
#[tauri::command]
pub fn device_name() -> String {
    lock(chosen_name()).clone().unwrap_or_else(suggested_device_name)
}

/// Sets the name the next link is made with; empty means the suggested one.
/// Says whether it changed.
pub fn set_device_name(name: &str) -> bool {
    const MAX: usize = 50;
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = (!name.is_empty()).then(|| name.chars().take(MAX).collect::<String>());
    let before = device_name();
    *lock(chosen_name()) = name;
    device_name() != before
}

fn chosen_name() -> &'static Mutex<Option<String>> {
    static CHOSEN: Mutex<Option<String>> = Mutex::new(None);
    &CHOSEN
}

/// "Omega Zap · <computer name>".
fn suggested_device_name() -> String {
    const MAX: usize = 30;
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| match computer_name() {
        Some(name) if name.chars().count() > MAX => format!("Omega Zap · {}…", name.chars().take(MAX - 1).collect::<String>()),
        Some(name) => format!("Omega Zap · {name}"),
        None => "Omega Zap".into(),
    })
    .clone()
}

/// The name the user gave the computer ("MacBook Pro do João"), or its host name.
fn computer_name() -> Option<String> {
    #[cfg(target_os = "macos")]
    let name = std::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok());
    #[cfg(all(unix, not(target_os = "macos")))]
    let name = fs::read_to_string("/etc/machine-info")
        .ok()
        .and_then(|info| {
            info.lines()
                .find_map(|line| line.strip_prefix("PRETTY_HOSTNAME="))
                .map(|value| value.trim().trim_matches('"').to_owned())
        })
        .filter(|name| !name.is_empty())
        .or_else(|| fs::read_to_string("/proc/sys/kernel/hostname").ok());
    #[cfg(not(unix))]
    let name = std::env::var("COMPUTERNAME").ok();
    name.map(|name| name.trim().to_owned()).filter(|name| !name.is_empty())
}

pub fn secure_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn render_qr(code: &str) -> Option<String> {
    use qrcode::QrCode;
    use qrcode::render::svg;
    let qr = QrCode::new(code.as_bytes()).ok()?;
    Some(
        qr.render::<svg::Color<'_>>()
            .min_dimensions(264, 264)
            .quiet_zone(true)
            .dark_color(svg::Color("#111b21"))
            .light_color(svg::Color("#ffffff"))
            .build(),
    )
}

/// The largest file sent from here, in bytes. It travels to the backend in
/// one piece and is uploaded from memory.
pub const MAX_SEND_BYTES: usize = 100 * 1024 * 1024;

/// A file to send, as the UI describes it. Its bytes are the request body.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingFile {
    pub chat_id: String,
    pub name: String,
    pub mime: String,
    #[serde(default)]
    pub caption: Option<String>,
    /// The id of the message this one answers.
    #[serde(default)]
    pub reply_to: Option<String>,
    /// Pixels, for photos and videos.
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// For videos and audio.
    #[serde(default)]
    pub seconds: Option<u32>,
    /// A small JPEG preview, base64.
    #[serde(default)]
    pub thumbnail: Option<String>,
    /// Sent on from another chat: the forwarding score it goes out with.
    #[serde(default)]
    pub forwarded: u32,
}

/// The card sent with a text that has a link in it, as the UI made it.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingLink {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// A small JPEG, base64, carried in the message itself.
    #[serde(default)]
    pub thumbnail: Option<String>,
    /// The full picture as a JPEG, base64, uploaded beside the message.
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

impl OutgoingLink {
    pub fn decoded(field: &Option<String>) -> Option<Vec<u8>> {
        use base64::Engine as _;
        field.as_deref().and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok()).filter(|b| !b.is_empty())
    }
}

impl OutgoingFile {
    /// The type without parameters, lowercased.
    pub fn base_mime(&self) -> String {
        self.mime.split(';').next().unwrap_or_default().trim().to_ascii_lowercase()
    }

    pub fn caption(&self) -> Option<String> {
        self.caption.as_deref().map(str::trim).filter(|c| !c.is_empty()).map(str::to_string)
    }
}

/// Reads a send-file request: the file's description from the `x-file`
/// header (base64 JSON, since headers are ASCII) and its bytes from the body.
pub fn outgoing_file(request: &tauri::ipc::Request<'_>) -> Result<(OutgoingFile, Vec<u8>), String> {
    use base64::Engine as _;
    let header = request
        .headers()
        .get("x-file")
        .and_then(|v| v.to_str().ok())
        .ok_or("The file's description is missing.")?;
    let json = base64::engine::general_purpose::STANDARD
        .decode(header)
        .map_err(|e| format!("The file's description is not valid: {e}"))?;
    let file: OutgoingFile =
        serde_json::from_slice(&json).map_err(|e| format!("The file's description is not valid: {e}"))?;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("The file's contents are missing.".into());
    };
    if bytes.is_empty() {
        return Err("The file is empty.".into());
    }
    if bytes.len() > MAX_SEND_BYTES {
        return Err(format!("The file is larger than {} MB.", MAX_SEND_BYTES / 1024 / 1024));
    }
    Ok((file, bytes.clone()))
}
