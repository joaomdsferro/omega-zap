mod account;
mod badge_font;
mod dropped;
#[cfg(debug_assertions)]
mod devfiles;
#[cfg(all(unix, not(target_os = "macos")))]
mod emoji;
mod link_preview;
mod klipy;
mod links;
mod notify;
#[cfg(target_os = "linux")]
mod players;
mod signal;
mod sqlcipher;
mod sticker_image;
mod store;
mod tray;
mod vault;
mod wa;

use std::sync::Arc;

use tauri::Manager;

/// Something the media protocol can serve: its length, and a way to read a
/// part of it.
trait Body {
    fn len(&self) -> std::io::Result<usize>;
    fn read(&self, start: usize, end: usize) -> std::io::Result<Vec<u8>>;
}

/// A file kept encrypted on disk. A part of it decrypts only the chunks it
/// is in, so seeking in a long video stays cheap.
impl Body for std::path::PathBuf {
    fn len(&self) -> std::io::Result<usize> {
        Ok(vault::get().len(self)? as usize)
    }
    fn read(&self, start: usize, end: usize) -> std::io::Result<Vec<u8>> {
        if start > end {
            return Ok(Vec::new());
        }
        vault::get().read_range(self, start as u64, end as u64)
    }
}

#[cfg(test)]
impl Body for Vec<u8> {
    fn len(&self) -> std::io::Result<usize> {
        Ok(self.len())
    }
    fn read(&self, start: usize, end: usize) -> std::io::Result<Vec<u8>> {
        Ok(self.get(start..=end).map(<[u8]>::to_vec).unwrap_or_default())
    }
}

/// A downloaded file, or the part a `Range` header asks for: video and audio
/// players fetch in parts to seek.
fn media_response(
    body: &impl Body,
    mime: &str,
    range: Option<&tauri::http::HeaderValue>,
) -> tauri::http::Result<tauri::http::Response<Vec<u8>>> {
    let builder = tauri::http::Response::builder()
        .header("Content-Type", mime)
        .header("Accept-Ranges", "bytes")
        // Videos and audio are fetched from the page into blobs (see
        // `useMediaUrl`), and the page has another origin.
        .header("Access-Control-Allow-Origin", "*")
        // The files are encrypted on disk; the webview must not keep plain
        // copies of them in its own cache.
        .header("Cache-Control", "no-store");
    let Ok(len) = body.len() else {
        return tauri::http::Response::builder().status(404).body(Vec::new());
    };
    let wanted = range
        .and_then(|r| r.to_str().ok())
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.split_once('-'))
        .and_then(|(start, end)| {
            let (start, end) = match (start.trim(), end.trim()) {
                // The last `end` bytes.
                ("", end) => (len.checked_sub(end.parse().ok()?)?, len.checked_sub(1)?),
                (start, "") => (start.parse().ok()?, len.checked_sub(1)?),
                (start, end) => (start.parse().ok()?, end.parse::<usize>().ok()?.min(len.checked_sub(1)?)),
            };
            (start <= end).then_some((start, end))
        });
    let part = |start: usize, end: usize| body.read(start, end);
    match (range, wanted) {
        (None, _) => match part(0, len.saturating_sub(1)) {
            Ok(bytes) => builder.body(bytes),
            Err(_) => tauri::http::Response::builder().status(500).body(Vec::new()),
        },
        (Some(_), Some((start, end))) => match part(start, end) {
            Ok(bytes) => builder
                .status(206)
                .header("Content-Range", format!("bytes {start}-{end}/{len}"))
                .body(bytes),
            Err(_) => tauri::http::Response::builder().status(500).body(Vec::new()),
        },
        (Some(_), None) => builder.status(416).header("Content-Range", format!("bytes */{len}")).body(Vec::new()),
    }
}

#[derive(Clone, Copy)]
enum Scheme {
    Avatar,
    Media,
}

/// Where a file the page asks for by name is kept, if the name is one of ours.
fn find_file(dir: &std::path::Path, scheme: Scheme, name: &str) -> Option<std::path::PathBuf> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if !plain {
        return None;
    }
    let candidates = match scheme {
        Scheme::Avatar => vec![dir.join(wa::AVATAR_DIR).join(name), dir.join("signal").join(signal::AVATAR_DIR).join(name)],
        Scheme::Media => vec![
            dir.join("signal").join(signal::MEDIA_DIR).join(name),
            dir.join(wa::MEDIA_DIR).join(name),
            dir.join(wa::LINK_THUMB_DIR).join(name),
            dir.join(link_preview::PREVIEW_DIR).join(name),
        ],
    };
    candidates.into_iter().find(|path| path.is_file())
}

fn mime_of(scheme: Scheme, name: &str) -> &'static str {
    if let Scheme::Avatar = scheme {
        return "image/jpeg";
    }
    match name.rsplit('.').next() {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("heic") => "image/heic",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("aac") => "audio/aac",
        Some("m4a") => "audio/mp4",
        Some("mp3") => "audio/mpeg",
        Some("ogg" | "opus") => "audio/ogg",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}

/// A saved picture or downloaded file, decrypted, or the part of it a
/// `Range` header asks for.
fn serve(
    dir: Option<&std::path::Path>,
    scheme: Scheme,
    path: &str,
    range: Option<&tauri::http::HeaderValue>,
) -> tauri::http::Response<Vec<u8>> {
    let name = path.trim_start_matches('/');
    let response = match dir.and_then(|dir| find_file(dir, scheme, name)) {
        Some(file) => media_response(&file, mime_of(scheme, name), range),
        None => tauri::http::Response::builder().status(404).body(Vec::new()),
    };
    let response = response.expect("static response parts are valid");
    if response.status().as_u16() >= 400 {
        log::warn!("{name}: {}", response.status());
    }
    response
}

/// Prints warnings and errors to stderr. `OMEGA_ZAP_LOG` raises it, for
/// everything (`debug`) or per target (`warn,AppState=debug`).
struct StderrLogger {
    default: log::LevelFilter,
    targets: Vec<(String, log::LevelFilter)>,
}

impl StderrLogger {
    fn from_env() -> Self {
        let mut logger = Self { default: log::LevelFilter::Warn, targets: Vec::new() };
        for part in std::env::var("OMEGA_ZAP_LOG").unwrap_or_default().split(',') {
            match part.trim().split_once('=') {
                Some((target, level)) => {
                    if let Ok(level) = level.parse() {
                        logger.targets.push((target.to_string(), level));
                    }
                }
                None => {
                    if let Ok(level) = part.trim().parse() {
                        logger.default = level;
                    }
                }
            }
        }
        logger
    }

    fn max(&self) -> log::LevelFilter {
        self.targets.iter().map(|(_, level)| *level).fold(self.default, Ord::max)
    }
}

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let level = self
            .targets
            .iter()
            .find(|(target, _)| metadata.target().starts_with(target.as_str()))
            .map_or(self.default, |(_, level)| *level);
        metadata.level() <= level
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{} {}] {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let logger = StderrLogger::from_env();
    let max = logger.max();
    if log::set_logger(Box::leak(Box::new(logger))).is_ok() {
        log::set_max_level(max);
    }

    tauri::Builder::default()
        // A second launch shows the window of the one already running in
        // the background, instead of fighting it over the sessions.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| tray::bring_up(app)))
        .plugin(tauri_plugin_opener::init())
        .manage(notify::Shown::default())
        .manage(dropped::Dropped::default())
        .on_window_event(|window, event| {
            dropped::on_window_event(window, event);
            tray::on_window_event(window, event);
        })
        // Serves saved profile pictures: avatar://localhost/<file name>. Each
        // network keeps its own, under names that cannot clash.
        .register_uri_scheme_protocol("avatar", |ctx, request| {
            let dir = ctx.app_handle().path().app_data_dir().ok();
            serve(dir.as_deref(), Scheme::Avatar, request.uri().path(), request.headers().get("range"))
        })
        // Serves downloaded attachments and WhatsApp link pictures:
        // media://localhost/<file name>.
        .register_uri_scheme_protocol("media", |ctx, request| {
            let dir = ctx.app_handle().path().app_data_dir().ok();
            serve(dir.as_deref(), Scheme::Media, request.uri().path(), request.headers().get("range"))
        })
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            if let Err(e) = vault::open(&dir) {
                log::error!("{e}");
                let _ = notify_rust::Notification::new()
                    .appname("Omega Zap")
                    .summary("Omega Zap could not open your messages")
                    .body(&e)
                    .show();
                std::process::exit(1);
            }
            // The chat stores first, before anything reads or saves them.
            for store in [dir.join("chats.json"), dir.join("signal").join("chats.json")] {
                if store.exists()
                    && let Err(e) = vault::get().seal_file(&store)
                {
                    log::warn!("could not encrypt {}: {e}", store.display());
                }
            }
            vault::migrate(
                dir.clone(),
                vec![
                    dir.join(wa::MEDIA_DIR),
                    dir.join(wa::AVATAR_DIR),
                    dir.join(wa::LINK_THUMB_DIR),
                    dir.join("signal").join(signal::MEDIA_DIR),
                    dir.join("signal").join(signal::AVATAR_DIR),
                ],
                vec![
                    dir.join("WebKitCache"),
                    dir.join("CacheStorage"),
                    dir.join("signal").join("outbox"),
                    dir.join("signal").join(signal::AVATAR_DIR).join(".incoming"),
                ],
            );
            #[cfg(debug_assertions)]
            devfiles::start(dir.clone());
            app.manage(tray::Background::new(dir.clone()));
            let signal = signal::SignalManager::new(app.handle().clone(), dir.join("signal"))?;
            let manager = wa::WaManager::new(app.handle().clone(), dir)?;
            manager.start();
            app.manage(manager);
            signal.start();
            app.manage(signal);
            #[cfg(all(unix, not(target_os = "macos")))]
            if let Err(e) = notify::register_desktop_entry() {
                log::warn!("could not register with the desktop: {e}");
            }
            tray::setup(app.handle())?;
            #[cfg(target_os = "linux")]
            players::allow_autoplay(match &app.config().build.dev_url {
                Some(url) if cfg!(debug_assertions) => url.to_string(),
                _ => "tauri://localhost".into(),
            });
            // Made here rather than from the config, so it never opens on
            // a webview cache the migration above is still clearing.
            tray::bring_up(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dropped::read_dropped_file,
            wa::commands::wa_status,
            wa::commands::wa_start_linking,
            wa::commands::wa_cancel_linking,
            wa::commands::wa_reconnect,
            wa::commands::wa_logout,
            wa::commands::wa_list_chats,
            wa::commands::wa_get_messages,
            wa::commands::wa_mark_read,
            wa::commands::wa_send_text,
            wa::commands::wa_delete_message,
            wa::commands::wa_react,
            wa::commands::wa_fetch_media,
            wa::commands::wa_open_media,
            wa::commands::wa_send_file,
            wa::commands::wa_link_thumbnail,
            wa::commands::wa_lists,
            wa::commands::wa_set_favorite,
            wa::commands::wa_save_list,
            wa::commands::wa_delete_list,
            wa::commands::wa_set_starred,
            wa::commands::wa_pinned_messages,
            wa::commands::wa_pin_message,
            wa::commands::wa_group_info,
            wa::commands::wa_group_details,
            wa::commands::wa_edit_message,
            wa::commands::wa_vote_poll,
            wa::commands::wa_starred,
            wa::commands::wa_search,
            wa::commands::wa_older_messages,
            wa::commands::wa_fetch_history_first,
            wa::commands::wa_contacts,
            wa::commands::wa_person,
            wa::commands::wa_set_unread,
            wa::commands::wa_person_details,
            wa::commands::wa_set_muted,
            wa::commands::wa_set_archived,
            wa::commands::wa_set_typing,
            wa::commands::wa_watch_presence,
            wa::commands::wa_privacy,
            wa::commands::wa_set_privacy,
            wa::commands::wa_set_pinned,
            wa::commands::wa_delete_chat,
            wa::commands::wa_set_blocked,
            wa::commands::wa_leave_group,
            wa::commands::wa_edit_group,
            wa::commands::wa_change_members,
            wa::commands::wa_clear_chat,
            wa::commands::wa_lookup_number,
            wa::commands::wa_sticker_library,
            wa::commands::wa_library_media,
            wa::commands::wa_send_sticker,
            wa::commands::wa_send_new_sticker,
            wa::commands::wa_add_new_sticker,
            wa::commands::wa_set_sticker_favorite,
            wa::commands::wa_message_sticker,
            wa::commands::wa_sticker_store,
            wa::commands::wa_install_pack,
            wa::commands::wa_remove_pack,
            wa::commands::wa_create_pack,
            wa::commands::wa_rename_pack,
            wa::commands::wa_add_to_pack,
            wa::commands::wa_remove_from_pack,
            wa::commands::wa_send_gif,
            wa::commands::wa_send_recent_gif,
            wa::commands::wa_send_klipy_sticker,
            wa::commands::wa_add_klipy_sticker,
            klipy::gif_search_available,
            klipy::gif_search,
            klipy::sticker_search,
            notify::show_notification,
            notify::dismiss_notification,
            tray::set_notification_prefs,
            tray::set_tray_badge,
            tray::take_pending_open,
            links::resolve_short_link,
            link_preview::link_preview,
            links::instagram_embed_plays,
            links::x_video,
            signal::commands::signal_status,
            signal::commands::signal_start_linking,
            signal::commands::signal_cancel_linking,
            signal::commands::signal_reconnect,
            signal::commands::signal_logout,
            signal::commands::signal_list_chats,
            signal::commands::signal_get_messages,
            signal::commands::signal_mark_read,
            signal::commands::signal_send_text,
            signal::commands::signal_send_file,
            signal::commands::signal_delete_message,
            signal::commands::signal_edit_message,
            signal::commands::signal_fetch_media,
            signal::commands::signal_open_media,
            signal::commands::signal_react,
            signal::commands::signal_lists,
            signal::commands::signal_set_favorite,
            signal::commands::signal_save_list,
            signal::commands::signal_delete_list,
            signal::commands::signal_set_starred,
            signal::commands::signal_starred,
            signal::commands::signal_search,
            signal::commands::signal_older_messages,
            signal::commands::signal_contacts,
            signal::commands::signal_person,
            signal::commands::signal_set_unread,
            signal::commands::signal_person_details,
            signal::commands::signal_set_muted,
            signal::commands::signal_set_archived,
            signal::commands::signal_set_pinned,
            signal::commands::signal_delete_chat,
            signal::commands::signal_clear_chat,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Closing the window keeps the app running in the tray; only
            // Quit, which asks with an exit code, ends it.
            if let tauri::RunEvent::ExitRequested { code: None, api, .. } = &event {
                api.prevent_exit();
            }
            if let tauri::RunEvent::Exit = event {
                if let Some(manager) = app.try_state::<Arc<wa::WaManager>>() {
                    manager.save_now();
                }
                if let Some(manager) = app.try_state::<Arc<signal::SignalManager>>() {
                    manager.save_now();
                }
                vault::clear_runtime_dir();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::media_response;
    use tauri::http::HeaderValue;

    fn part(range: &str) -> (u16, Vec<u8>, Option<String>) {
        let header = HeaderValue::from_str(range).unwrap();
        let response = media_response(&b"0123456789".to_vec(), "video/mp4", Some(&header)).unwrap();
        let content_range = response.headers().get("Content-Range").map(|v| v.to_str().unwrap().to_string());
        (response.status().as_u16(), response.body().clone(), content_range)
    }

    #[test]
    fn players_get_the_part_they_ask_for() {
        assert_eq!(part("bytes=0-"), (206, b"0123456789".to_vec(), Some("bytes 0-9/10".into())));
        assert_eq!(part("bytes=2-4"), (206, b"234".to_vec(), Some("bytes 2-4/10".into())));
        assert_eq!(part("bytes=-3"), (206, b"789".to_vec(), Some("bytes 7-9/10".into())));
        assert_eq!(part("bytes=8-99"), (206, b"89".to_vec(), Some("bytes 8-9/10".into())));
        assert_eq!(part("bytes=20-"), (416, Vec::new(), Some("bytes */10".into())));
        let whole = media_response(&b"abc".to_vec(), "image/png", None).unwrap();
        assert_eq!((whole.status().as_u16(), whole.body().as_slice()), (200, &b"abc"[..]));
    }
}
