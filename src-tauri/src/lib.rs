mod account;
mod links;
mod notify;
mod signal;
mod store;
mod wa;

use std::sync::Arc;

use tauri::Manager;

/// A downloaded file, or the part a `Range` header asks for: video and audio
/// players fetch in parts to seek.
fn media_response(
    bytes: Vec<u8>,
    mime: &str,
    range: Option<&tauri::http::HeaderValue>,
) -> tauri::http::Result<tauri::http::Response<Vec<u8>>> {
    let builder = tauri::http::Response::builder()
        .header("Content-Type", mime)
        .header("Accept-Ranges", "bytes")
        // Videos and audio are fetched from the page into blobs (see
        // `useMediaUrl`), and the page has another origin.
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "max-age=31536000, immutable");
    let len = bytes.len();
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
    match (range, wanted) {
        (None, _) => builder.body(bytes),
        (Some(_), Some((start, end))) => builder
            .status(206)
            .header("Content-Range", format!("bytes {start}-{end}/{len}"))
            .body(bytes[start..=end].to_vec()),
        (Some(_), None) => builder.status(416).header("Content-Range", format!("bytes */{len}")).body(Vec::new()),
    }
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
        .plugin(tauri_plugin_opener::init())
        .manage(notify::Shown::default())
        // Serves saved profile pictures: avatar://localhost/<file name>. Each
        // network keeps its own, under names that cannot clash.
        .register_uri_scheme_protocol("avatar", |ctx, request| {
            let name = request.uri().path().trim_start_matches('/');
            let plain = !name.is_empty()
                && !name.starts_with('.')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
            let body = plain
                .then(|| ctx.app_handle().path().app_data_dir().ok())
                .flatten()
                .and_then(|dir| {
                    std::fs::read(dir.join(wa::AVATAR_DIR).join(name))
                        .or_else(|_| std::fs::read(dir.join("signal").join(signal::AVATAR_DIR).join(name)))
                        .ok()
                });
            let response = match body {
                Some(bytes) => tauri::http::Response::builder()
                    .header("Content-Type", "image/jpeg")
                    .header("Cache-Control", "max-age=31536000, immutable")
                    .body(bytes),
                None => tauri::http::Response::builder().status(404).body(Vec::new()),
            };
            response.expect("static response parts are valid")
        })
        // Serves downloaded attachments and WhatsApp link pictures:
        // media://localhost/<file name>.
        .register_uri_scheme_protocol("media", |ctx, request| {
            let name = request.uri().path().trim_start_matches('/');
            let plain = !name.is_empty()
                && !name.starts_with('.')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
            let body = plain
                .then(|| ctx.app_handle().path().app_data_dir().ok())
                .flatten()
                .and_then(|dir| {
                    std::fs::read(dir.join("signal").join(signal::MEDIA_DIR).join(name))
                        .or_else(|_| std::fs::read(dir.join(wa::MEDIA_DIR).join(name)))
                        .or_else(|_| std::fs::read(dir.join(wa::LINK_THUMB_DIR).join(name)))
                        .ok()
                });
            let mime = match name.rsplit('.').next() {
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
            };
            let response = match body {
                Some(bytes) => media_response(bytes, mime, request.headers().get("range")),
                None => tauri::http::Response::builder().status(404).body(Vec::new()),
            };
            response.expect("static response parts are valid")
        })
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let signal = signal::SignalManager::new(app.handle().clone(), dir.join("signal"))?;
            let manager = wa::WaManager::new(app.handle().clone(), dir)?;
            manager.start();
            app.manage(manager);
            signal.start();
            app.manage(signal);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            wa::commands::wa_starred,
            wa::commands::wa_contacts,
            wa::commands::wa_lookup_number,
            notify::show_notification,
            links::resolve_short_link,
            links::instagram_embed_plays,
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
            signal::commands::signal_fetch_media,
            signal::commands::signal_open_media,
            signal::commands::signal_react,
            signal::commands::signal_lists,
            signal::commands::signal_set_favorite,
            signal::commands::signal_save_list,
            signal::commands::signal_delete_list,
            signal::commands::signal_set_starred,
            signal::commands::signal_starred,
            signal::commands::signal_contacts,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(manager) = app.try_state::<Arc<wa::WaManager>>() {
                    manager.save_now();
                }
                if let Some(manager) = app.try_state::<Arc<signal::SignalManager>>() {
                    manager.save_now();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::media_response;
    use tauri::http::HeaderValue;

    fn part(range: &str) -> (u16, Vec<u8>, Option<String>) {
        let header = HeaderValue::from_str(range).unwrap();
        let response = media_response(b"0123456789".to_vec(), "video/mp4", Some(&header)).unwrap();
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
        let whole = media_response(b"abc".to_vec(), "image/png", None).unwrap();
        assert_eq!((whole.status().as_u16(), whole.body().as_slice()), (200, &b"abc"[..]));
    }
}
