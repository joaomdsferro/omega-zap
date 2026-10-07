//! What every linked account reports to the UI, whatever the network.

use std::fs;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use serde::Serialize;

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
            error: None,
            notice: None,
            me: None,
        }
    }
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
