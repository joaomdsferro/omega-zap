//! Downloaded videos and audio for the page's players, over loopback HTTP.
//!
//! WebKitGTK's players take only http(s) and `blob:` URLs, never the app's
//! `media://` scheme, and WebKitGTK 2.54 feeds a `blob:` URL to GStreamer as
//! corrupted data: the picture freezes for seconds, the sound breaks up and
//! going back fails. Over http the same files play smoothly and seek at once.
//!
//! The server listens on 127.0.0.1 only, under a random token, and reads each
//! part as it is sent, so a long video starts at once and only the chunks a
//! player asks for are decrypted.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ring::rand::{SecureRandom, SystemRandom};

use crate::{Body, Scheme, byte_range, find_file, mime_of};

/// `http://127.0.0.1:<port>/<token>/`, once the server is up.
static BASE: OnceLock<String> = OnceLock::new();

/// How much is read and sent at a time.
const CHUNK: usize = 256 * 1024;

pub fn start(dir: PathBuf) {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(e) => return log::warn!("media server: could not listen: {e}"),
    };
    let mut token = [0u8; 16];
    if SystemRandom::new().fill(&mut token).is_err() {
        return;
    }
    let token: String = token.iter().map(|b| format!("{b:02x}")).collect();
    let Ok(port) = listener.local_addr().map(|a| a.port()) else {
        return;
    };
    let _ = BASE.set(format!("http://127.0.0.1:{port}/{token}/"));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (dir, token) = (dir.clone(), token.clone());
            std::thread::spawn(move || {
                if let Err(e) = answer(stream, &dir, &token) {
                    // Players drop a connection as soon as they seek.
                    log::debug!("media server: {e}");
                }
            });
        }
    });
}

/// Where the page's players fetch a downloaded file: this plus its name.
/// `None` where the files are not served this way.
#[tauri::command]
pub fn media_server() -> Option<String> {
    BASE.get().cloned()
}

fn answer(stream: TcpStream, dir: &Path, token: &str) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut words = line.split_whitespace();
    let (method, path) = (words.next().unwrap_or_default(), words.next().unwrap_or_default());
    let mut range = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("range")
        {
            range = tauri::http::HeaderValue::from_str(value.trim()).ok();
        }
    }
    let mut out = stream;
    let name = path
        .strip_prefix('/')
        .and_then(|rest| rest.split_once('/'))
        .and_then(|(given, name)| (given == token).then_some(name));
    let file = match (method, name) {
        ("GET" | "HEAD", Some(name)) => find_file(dir, Scheme::Media, name).map(|file| (file, mime_of(Scheme::Media, name))),
        _ => None,
    };
    let Some((file, mime)) = file else {
        return out.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    };
    let Ok(len) = Body::len(&file) else {
        return out.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    };
    let (status, start, end) = match byte_range(range.as_ref(), len) {
        Ok(None) => ("200 OK", 0, len),
        Ok(Some((start, end))) => ("206 Partial Content", start, end + 1),
        Err(()) => {
            return write!(
                out,
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        }
    };
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n",
        end - start
    );
    if status.starts_with("206") {
        head.push_str(&format!("Content-Range: bytes {start}-{}/{len}\r\n", end - 1));
    }
    head.push_str("\r\n");
    out.write_all(head.as_bytes())?;
    if method == "HEAD" {
        return out.flush();
    }
    let mut at = start;
    while at < end {
        let next = (at + CHUNK).min(end);
        out.write_all(&file.read(at, next - 1)?)?;
        at = next;
    }
    out.flush()
}
