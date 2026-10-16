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
    /// Signed in with the QR code; the account's two-step password is next.
    Password,
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
    /// The hint the account set for its two-step password (Telegram).
    pub password_hint: Option<String>,
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
            password_hint: None,
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
    /// A voice message recorded here, rather than an audio file.
    #[serde(default)]
    pub voice: bool,
    /// The people the caption mentions, as "@<number>" there (WhatsApp).
    #[serde(default)]
    pub mentions: Vec<String>,
    /// The caption's "@all" mentions everyone in the group (WhatsApp).
    #[serde(default)]
    pub mention_all: bool,
    /// A voice message's loudness over its length, 0–100.
    #[serde(default)]
    pub waveform: Vec<u8>,
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
