//! Running in the background. Closing the window destroys it, webview and
//! all, so what stays behind is only this process and the Signal bridge: the
//! connections, the stores and a tray icon that counts unread messages.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WebviewWindowBuilder};

use crate::account::lock;
use crate::{signal, wa};

const MAIN: &str = "main";
const PREFS_FILE: &str = "background.json";
/// Side of the tray picture, big enough for a sharp badge on HiDPI panels.
const ICON_SIDE: u32 = 64;

/// What the UI decided about notifications, kept here so they still follow
/// its settings while there is no UI.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    pub notifications: bool,
    pub preview: bool,
    #[serde(default)]
    pub badge: Badge,
    /// How the window was when it was last closed, to open it the same way.
    #[serde(default)]
    pub window: Option<WindowState>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { notifications: true, preview: true, badge: Badge::default(), window: None }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    /// Logical size when not maximized.
    pub width: f64,
    pub height: f64,
    pub maximized: bool,
}

/// The colour of the unread count on the tray icon.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Badge {
    Graphite,
    Green,
    #[default]
    Red,
    Blue,
}

impl Badge {
    fn gradient(self) -> Gradient {
        match self {
            Badge::Graphite => ([0x4a, 0x50, 0x5a], [0x23, 0x27, 0x2e]),
            Badge::Green => ([0x25, 0xd3, 0x66], [0x00, 0xa8, 0x84]),
            Badge::Red => ([0xff, 0x4d, 0x5e], [0xe0, 0x24, 0x3b]),
            Badge::Blue => ([0x4c, 0x9a, 0xff], [0x1d, 0x6f, 0xf2]),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OpenChat {
    pub account: String,
    pub chat_id: String,
}

pub struct Background {
    file: PathBuf,
    prefs: Mutex<Prefs>,
    /// A chat a notification asked to open while the window was being made,
    /// for the new page to pick up.
    pending: Mutex<Option<OpenChat>>,
    tray: Mutex<Option<TrayIcon>>,
    /// The count the tray icon shows; `u32::MAX` before the first one.
    shown: AtomicU32,
}

impl Background {
    pub fn new(dir: PathBuf) -> Self {
        let file = dir.join(PREFS_FILE);
        let prefs = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            file,
            prefs: Mutex::new(prefs),
            pending: Mutex::new(None),
            tray: Mutex::new(None),
            shown: AtomicU32::new(u32::MAX),
        }
    }

    pub fn prefs(&self) -> Prefs {
        *lock(&self.prefs)
    }
}

/// Whether the window is there. Without it, nobody listens to events.
pub fn has_window(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN).is_some()
}

/// Shows the window, making it again if it was closed.
pub fn bring_up(app: &AppHandle) {
    bring_up_with(app, None);
}

/// Shows the window, made again as it was if it was closed. `token` is the
/// one a notification click hands over: on Wayland the compositor lets a
/// window to the front only with it, and otherwise only marks it as wanting
/// attention.
pub(crate) fn bring_up_with(app: &AppHandle, token: Option<String>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let window = match handle.get_webview_window(MAIN) {
            Some(window) => window,
            None => match build_window(&handle) {
                Some(window) => window,
                None => return,
            },
        };
        let _ = window.unminimize();
        let _ = window.show();
        #[cfg(target_os = "linux")]
        if let (Some(token), Ok(gtk)) = (token.as_deref(), window.gtk_window()) {
            use gtk::prelude::GtkWindowExt;
            gtk.set_startup_id(token);
            gtk.present();
            return;
        }
        let _ = token;
        let _ = window.set_focus();
    });
}

fn build_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    let config = app.config().app.windows.iter().find(|w| w.label == MAIN)?;
    let mut builder = WebviewWindowBuilder::from_config(app, config).ok()?;
    builder = match app.state::<Background>().prefs().window {
        Some(state) => builder.inner_size(state.width, state.height).maximized(state.maximized),
        // The first time, all the room there is.
        None => builder.maximized(true),
    };
    // A dev build says so in its icon, to be told apart from an installed one.
    #[cfg(debug_assertions)]
    if let Ok(icon) = Image::from_bytes(include_bytes!("../icons/dev/128x128.png")) {
        builder = builder.icon(icon).ok()?;
    }
    let window = builder.build().inspect_err(|e| log::warn!("could not open the window: {e}")).ok()?;
    #[cfg(target_os = "linux")]
    let _ = window.with_webview(|platform| {
        crate::players::fix_tiktok(&platform.inner());
        crate::players::add_player_script(&platform.inner());
    });
    Some(window)
}

/// Remembers how the window is, for the next time it is made.
fn save_window_state(background: &Background, window: &tauri::WebviewWindow) {
    let maximized = window.is_maximized().unwrap_or(false);
    let size = window
        .inner_size()
        .ok()
        .zip(window.scale_factor().ok())
        .map(|(size, scale)| size.to_logical::<f64>(scale));
    background.update(|prefs| {
        let previous = prefs.window;
        prefs.window = match (maximized, size, previous) {
            // A maximized window keeps the size it goes back to.
            (true, _, Some(state)) => Some(WindowState { maximized: true, ..state }),
            (_, Some(size), _) => Some(WindowState { width: size.width, height: size.height, maximized }),
            (_, None, previous) => previous,
        };
    });
}

/// Closing the window frees what it holds. WebKitGTK keeps the page's
/// process running after its view is gone, so it is ended here first.
pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    // We show as online on WhatsApp only while the window is in front.
    let active = match event {
        tauri::WindowEvent::Focused(focused) => Some(*focused),
        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => Some(false),
        _ => None,
    };
    if let Some(active) = active
        && let Some(accounts) = window.app_handle().try_state::<wa::WaAccounts>()
    {
        for manager in accounts.all() {
            manager.set_active(active);
        }
    }
    let tauri::WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    let Some(webview) = window.app_handle().get_webview_window(window.label()) else {
        return;
    };
    api.prevent_close();
    save_window_state(&window.state::<Background>(), &webview);
    let _ = webview.hide();
    #[cfg(target_os = "linux")]
    let _ = webview.with_webview(|platform| {
        use webkit2gtk::WebViewExt;
        platform.inner().terminate_web_process();
    });
    // Queued after the above on the main thread, so it runs once the
    // process is told to end.
    let _ = webview.destroy();
}

/// Brings the window up on a chat. A page made just now asks for it with
/// `take_pending_open` once it is listening.
pub fn open_chat(app: &AppHandle, chat: OpenChat, token: Option<String>) {
    use tauri::Emitter;
    if has_window(app) {
        bring_up_with(app, token);
        let _ = app.emit("notification://open", chat);
    } else {
        *lock(&app.state::<Background>().pending) = Some(chat);
        bring_up_with(app, token);
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Omega Zap", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::with_id(MAIN)
        .tooltip("Omega Zap")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => bring_up(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                bring_up(tray.app_handle());
            }
        });
    if let Some(icon) = badged_icon(app, 0, Badge::default()) {
        builder = builder.icon(icon);
    }
    let tray = builder.build(app)?;
    let background = app.state::<Background>();
    *lock(&background.tray) = Some(tray);
    background.shown.store(0, Ordering::Release);
    refresh(app);
    Ok(())
}

/// Puts the number of unread messages on the tray icon, when it changed.
pub fn refresh(app: &AppHandle) {
    let Some(background) = app.try_state::<Background>() else {
        return;
    };
    let mut total = 0u32;
    if let Some(accounts) = app.try_state::<wa::WaAccounts>() {
        for manager in accounts.all() {
            total = total.saturating_add(manager.unread_total());
        }
    }
    if let Some(manager) = app.try_state::<std::sync::Arc<signal::SignalManager>>() {
        total = total.saturating_add(manager.unread_total());
    }
    if let Some(manager) = app.try_state::<std::sync::Arc<crate::telegram::TgManager>>() {
        total = total.saturating_add(manager.unread_total());
    }
    if background.shown.swap(total, Ordering::AcqRel) == total {
        return;
    }
    let tray = lock(&background.tray);
    let Some(tray) = tray.as_ref() else {
        return;
    };
    let _ = tray.set_icon(badged_icon(app, total, background.prefs().badge));
    let tooltip = match total {
        0 => "Omega Zap".to_string(),
        1 => "Omega Zap: 1 unread message".to_string(),
        n => format!("Omega Zap: {n} unread messages"),
    };
    let _ = tray.set_tooltip(Some(tooltip));
}

/// The tray's own picture: the bare mark, which a panel shows at about 22 px.
const TRAY_PNG: &[u8] = include_bytes!("../icons/tray.png");

/// The tray picture, with the count in a badge when there is one.
fn badged_icon(app: &AppHandle, count: u32, badge: Badge) -> Option<Image<'static>> {
    let base = match Image::from_bytes(TRAY_PNG) {
        Ok(image) => image,
        Err(_) => app.default_window_icon()?.clone(),
    };
    let mut rgba = scale(base.rgba(), base.width(), base.height(), ICON_SIDE);
    if count > 0 {
        draw_badge(&mut rgba, ICON_SIDE, count, badge.gradient());
    }
    Some(Image::new_owned(rgba, ICON_SIDE, ICON_SIDE))
}

/// Box-filters (or repeats pixels of) a square RGBA picture to `side`.
fn scale(src: &[u8], width: u32, height: u32, side: u32) -> Vec<u8> {
    let mut out = vec![0u8; (side * side * 4) as usize];
    for y in 0..side {
        for x in 0..side {
            let (x0, x1) = (x * width / side, ((x + 1) * width / side).max(x * width / side + 1));
            let (y0, y1) = (y * height / side, ((y + 1) * height / side).max(y * height / side + 1));
            let mut sum = [0u32; 4];
            for sy in y0..y1.min(height) {
                for sx in x0..x1.min(width) {
                    let i = ((sy * width + sx) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += src[i + c] as u32;
                    }
                }
            }
            let n = ((x1.min(width) - x0) * (y1.min(height) - y0)).max(1);
            let o = ((y * side + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (sum[c] / n) as u8;
            }
        }
    }
    out
}

/// The badge's colour, top and bottom of its gradient.
type Gradient = ([u8; 3], [u8; 3]);

/// The count in a pill in the top right corner, cut out of the icon by a
/// thin transparent ring, all anti-aliased.
fn draw_badge(rgba: &mut [u8], side: u32, count: u32, (top_rgb, bottom_rgb): Gradient) {
    use crate::badge_font::{CAP_HEIGHT, GLYPHS};
    let text: Vec<usize> = if count > 99 {
        vec![9, 9, 10]
    } else {
        count.to_string().bytes().map(|b| (b - b'0') as usize).collect()
    };
    let s = side as f32 / 64.0;
    // Small enough to leave the mark recognisable under it.
    let digit_h = if text.len() > 2 { 14.0 } else { 19.0 } * s;
    let scale = digit_h / CAP_HEIGHT;
    // Digits sit closer than their advances say: they read as one number.
    let tracking = -0.02 * digit_h / scale;
    let advances: Vec<f32> = text.iter().map(|&g| GLYPHS[g].0 + tracking).collect();
    let text_w = advances.iter().sum::<f32>() * scale;
    // One height whatever the count, so the corner it takes stays the same.
    let h = (31.0 * s).round();
    let w = (text_w + h * 0.62).max(h).min(side as f32);
    let r = h / 2.0;
    let gap = 3.5 * s;
    // Top right, where the mark has the most room to spare.
    let (right, top) = (side as f32, 0.0);
    let (left, bottom) = (right - w, top + h);
    // The pill is a segment between these two centres, widened by `r`.
    let (ax, bx, cy) = (left + r, right - r, top + r);
    let pill = |x: f32, y: f32, grow: f32| {
        let px = x.clamp(ax, bx);
        let d = ((x - px).powi(2) + (y - cy).powi(2)).sqrt() - (r + grow);
        (0.5 - d).clamp(0.0, 1.0)
    };
    let origin_x = left + (w - text_w) / 2.0;
    // Optically centred: digits have no descenders.
    let baseline = cy + digit_h / 2.0;
    let mut pens = Vec::with_capacity(text.len());
    let mut x = origin_x;
    for (&g, adv) in text.iter().zip(&advances) {
        pens.push((g, x));
        x += adv * scale;
    }
    // Nonzero winding at a point, over the outlines placed along the text.
    let inside = |x: f32, y: f32| {
        let mut winding = 0i32;
        for &(g, pen) in &pens {
            let fx = (x - pen) / scale;
            let fy = (baseline - y) / scale;
            for contour in GLYPHS[g].1 {
                for i in 0..contour.len() {
                    let (x0, y0) = (contour[i].0 as f32, contour[i].1 as f32);
                    let (x1, y1) = {
                        let p = contour[(i + 1) % contour.len()];
                        (p.0 as f32, p.1 as f32)
                    };
                    if (y0 <= fy) != (y1 <= fy) {
                        let cross = x0 + (fy - y0) / (y1 - y0) * (x1 - x0);
                        if cross > fx {
                            winding += if y1 > y0 { 1 } else { -1 };
                        }
                    }
                }
            }
        }
        winding != 0
    };
    const SAMPLES: u32 = 4;
    let y_from = (top - gap).floor().max(0.0) as u32;
    let x_from = (left - gap).floor().max(0.0) as u32;
    let y_to = ((bottom + gap).ceil() as u32).min(side);
    for py in y_from..y_to {
        for px in x_from..side {
            let (cx, cyy) = (px as f32 + 0.5, py as f32 + 0.5);
            let i = ((py * side + px) * 4) as usize;
            // Clear the ring, then lay the pill and the digits over it.
            let cut = pill(cx, cyy, gap);
            if cut == 0.0 {
                continue;
            }
            let mut color = [rgba[i] as f32 / 255.0, rgba[i + 1] as f32 / 255.0, rgba[i + 2] as f32 / 255.0];
            let mut alpha = rgba[i + 3] as f32 / 255.0 * (1.0 - cut);
            let cover = pill(cx, cyy, 0.0);
            if cover > 0.0 {
                let t = ((cyy - top) / h).clamp(0.0, 1.0);
                let mut fill = [0.0; 3];
                for c in 0..3 {
                    let (a, b) = (top_rgb[c] as f32 / 255.0, bottom_rgb[c] as f32 / 255.0);
                    fill[c] = a + (b - a) * t;
                }
                let mut hits = 0;
                for sy in 0..SAMPLES {
                    for sx in 0..SAMPLES {
                        let ox = px as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                        let oy = py as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                        hits += inside(ox, oy) as u32;
                    }
                }
                let ink = hits as f32 / (SAMPLES * SAMPLES) as f32;
                for c in 0..3 {
                    fill[c] += (1.0 - fill[c]) * ink;
                }
                // "Over", in premultiplied terms.
                let out_a = cover + alpha * (1.0 - cover);
                for c in 0..3 {
                    color[c] = (fill[c] * cover + color[c] * alpha * (1.0 - cover)) / out_a.max(1e-6);
                }
                alpha = out_a;
            }
            for c in 0..3 {
                rgba[i + c] = (color[c] * 255.0).round() as u8;
            }
            rgba[i + 3] = (alpha * 255.0).round() as u8;
        }
    }
}

impl Background {
    /// Changes the settings, saving them if they changed. Returns whether they did.
    fn update(&self, change: impl FnOnce(&mut Prefs)) -> bool {
        let prefs = {
            let mut current = lock(&self.prefs);
            let before = serde_json::to_vec(&*current).ok();
            change(&mut current);
            if serde_json::to_vec(&*current).ok() == before {
                return false;
            }
            *current
        };
        if let Ok(bytes) = serde_json::to_vec(&prefs)
            && let Err(e) = std::fs::write(&self.file, bytes)
        {
            log::warn!("could not save the background settings: {e}");
        }
        true
    }
}

#[tauri::command]
pub fn set_notification_prefs(background: State<'_, Background>, notifications: bool, preview: bool) {
    background.update(|prefs| {
        prefs.notifications = notifications;
        prefs.preview = preview;
    });
}

#[tauri::command]
pub fn set_tray_badge(app: AppHandle, background: State<'_, Background>, badge: Badge) {
    if background.update(|prefs| prefs.badge = badge) {
        // Drawn again in the new colour.
        background.shown.store(u32::MAX, Ordering::Release);
        refresh(&app);
    }
}

#[tauri::command]
pub fn take_pending_open(background: State<'_, Background>) -> Option<OpenChat> {
    lock(&background.pending).take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badges_fit_the_icon() {
        for count in [1, 9, 10, 99, 100, 12345] {
            let mut rgba = vec![0u8; (ICON_SIDE * ICON_SIDE * 4) as usize];
            draw_badge(&mut rgba, ICON_SIDE, count, Badge::default().gradient());
            assert!(rgba.chunks(4).any(|p| p == [0xff; 4]), "{count} drew no text");
            assert!(rgba.chunks(4).any(|p| p[3] == 0), "{count} cut no ring");
        }
    }

    #[test]
    fn scaling_keeps_a_flat_colour() {
        let src = [10u8, 20, 30, 255].repeat(32 * 32);
        let out = scale(&src, 32, 32, ICON_SIDE);
        assert!(out.chunks(4).all(|p| p == [10, 20, 30, 255]));
        let src = [10u8, 20, 30, 255].repeat(128 * 128);
        assert!(scale(&src, 128, 128, ICON_SIDE).chunks(4).all(|p| p == [10, 20, 30, 255]));
    }
}




