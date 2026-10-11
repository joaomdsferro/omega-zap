//! Debug builds only: the `avatar://` and `media://` files over loopback
//! HTTP, for the dev server to hand to the page. WebKitGTK 2.54 refuses
//! custom-scheme images on the http dev page (tauri-apps/tauri#16201), and
//! the files are encrypted, so the dev server cannot read them itself.
//!
//! The port and a random token are written to `dev-files.json` in the
//! private runtime directory, where the dev server reads them.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

use ring::rand::{SecureRandom, SystemRandom};

use crate::{Scheme, serve, vault};

pub fn start(dir: PathBuf) {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(e) => return log::warn!("dev files: could not listen: {e}"),
    };
    let mut token = [0u8; 16];
    if SystemRandom::new().fill(&mut token).is_err() {
        return;
    }
    let token: String = token.iter().map(|b| format!("{b:02x}")).collect();
    let port = listener.local_addr().map(|a| a.port()).unwrap_or_default();
    let written = vault::runtime_dir().and_then(|runtime| {
        let file = runtime.join("dev-files.json");
        std::fs::write(&file, format!("{{\"port\":{port},\"token\":\"{token}\"}}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    });
    if let Err(e) = written {
        return log::warn!("dev files: could not publish the port: {e}");
    }
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (dir, token) = (dir.clone(), token.clone());
            std::thread::spawn(move || {
                if let Err(e) = answer(stream, &dir, &token) {
                    log::debug!("dev files: {e}");
                }
            });
        }
    });
}

fn answer(stream: TcpStream, dir: &std::path::Path, token: &str) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let path = line.split_whitespace().nth(1).unwrap_or_default().to_string();
    let (mut range, mut authorized) = (None, false);
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "range" => range = tauri::http::HeaderValue::from_str(value.trim()).ok(),
                "x-dev-token" => authorized = value.trim() == token,
                _ => {}
            }
        }
    }
    let response = match (authorized, path.split_once('/').map(|(_, rest)| rest.split_once('/'))) {
        (true, Some(Some(("avatar", name)))) => serve(Some(dir), Scheme::Avatar, name, range.as_ref()),
        (true, Some(Some(("media", name)))) => serve(Some(dir), Scheme::Media, name, range.as_ref()),
        _ => tauri::http::Response::builder().status(404).body(Vec::new()).expect("static response parts are valid"),
    };
    let mut out = stream;
    write!(out, "HTTP/1.1 {}\r\n", response.status())?;
    for (name, value) in response.headers() {
        out.write_all(name.as_str().as_bytes())?;
        out.write_all(b": ")?;
        out.write_all(value.as_bytes())?;
        out.write_all(b"\r\n")?;
    }
    write!(out, "Content-Length: {}\r\nConnection: close\r\n\r\n", response.body().len())?;
    out.write_all(response.body())?;
    out.flush()
}
