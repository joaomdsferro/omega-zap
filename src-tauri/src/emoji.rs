//! Apple's emoji, as the UI draws them, in text shown outside the window:
//! a notification's body is markup, and the desktop draws the images in it.

use std::path::PathBuf;

use tauri::AppHandle;
use unicode_segmentation::UnicodeSegmentation;

/// The images as the UI ships them, 64 pixels square.
#[cfg(debug_assertions)]
const DEV_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../node_modules/emoji-datasource-apple/img/apple/64");

/// `text` as notification markup, with each emoji that has an image swapped
/// for one `px` pixels square. The desktop ignores the size an `<img>` asks
/// for, so each is resized once and kept in the runtime directory.
pub fn markup(app: &AppHandle, text: &str, px: u32) -> String {
    let mut out = String::with_capacity(text.len());
    for grapheme in text.graphemes(true) {
        match maybe_emoji(grapheme).then(|| image(app, grapheme, px)).flatten() {
            Some(path) => {
                out.push_str("<img src=\"file://");
                out.push_str(&escape(&path.to_string_lossy()));
                out.push_str("\" alt=\"");
                out.push_str(&escape(grapheme));
                out.push_str("\"/>");
            }
            None => out.push_str(&escape(grapheme)),
        }
    }
    out
}

/// Text as markup shows it.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// A cheap test that leaves letters, digits and punctuation alone.
fn maybe_emoji(grapheme: &str) -> bool {
    grapheme.chars().any(|c| matches!(c as u32, 0xA9 | 0xAE | 0x203C.. ) && !c.is_alphanumeric() || c == '\u{20E3}')
}

/// Where the emoji's image is at `px` pixels, made the first time it is asked for.
fn image(app: &AppHandle, emoji: &str, px: u32) -> Option<PathBuf> {
    let bare: Vec<u32> = emoji.chars().map(|c| c as u32).filter(|&c| c != 0xFE0F).collect();
    let hex = |cps: &[u32]| cps.iter().map(|c| format!("{c:04x}")).collect::<Vec<_>>().join("-");
    // Images are named by code point, without variation selectors, except
    // the few that keep one after their first code point.
    let mut candidates = vec![hex(&bare)];
    if !bare.is_empty() {
        let mut kept = vec![bare[0], 0xFE0F];
        kept.extend_from_slice(&bare[1..]);
        candidates.push(hex(&kept));
    }
    let dir = crate::vault::runtime_dir().ok()?.join(format!("emoji-{px}"));
    for stem in candidates {
        let path = dir.join(format!("{stem}.png"));
        if path.exists() {
            return Some(path);
        }
        let Some(source) = source(app, &stem) else { continue };
        let small = image::load_from_memory_with_format(&source, image::ImageFormat::Png).ok()?.resize_exact(
            px,
            px,
            image::imageops::FilterType::Lanczos3,
        );
        std::fs::create_dir_all(&dir).ok()?;
        small.save_with_format(&path, image::ImageFormat::Png).ok()?;
        return Some(path);
    }
    None
}

/// The 64-pixel image, from the app's assets or, in development, from the package.
fn source(app: &AppHandle, stem: &str) -> Option<Vec<u8>> {
    if let Some(asset) = app.asset_resolver().get(format!("emoji/{stem}.png")) {
        return Some(asset.bytes);
    }
    #[cfg(debug_assertions)]
    if let Ok(bytes) = std::fs::read(format!("{DEV_DIR}/{stem}.png")) {
        return Some(bytes);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup() {
        assert_eq!(escape("a < b & \"c\" >"), "a &lt; b &amp; &quot;c&quot; &gt;");
    }

    #[test]
    fn spots_emoji_and_leaves_text() {
        assert!(maybe_emoji("😅"));
        assert!(maybe_emoji("❤️"));
        assert!(maybe_emoji("#️⃣"));
        assert!(maybe_emoji("©"));
        assert!(!maybe_emoji("a"));
        assert!(!maybe_emoji("é"));
        assert!(!maybe_emoji("1"));
        assert!(!maybe_emoji("—"));
    }
}
