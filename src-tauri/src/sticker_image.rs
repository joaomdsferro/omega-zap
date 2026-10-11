//! Pictures made into stickers: 512×512 WebP on a transparent square, as
//! the phone makes them. A GIF or an animated WebP becomes an animated sticker.

use std::io::Cursor;

use image::codecs::gif::GifDecoder;
use image::codecs::webp::WebPDecoder;
use image::imageops::{self, FilterType};
use image::{AnimationDecoder, ImageFormat, RgbaImage};

/// Stickers are square, this many pixels a side.
pub const SIDE: u32 = 512;
/// What WhatsApp accepts for a sticker in a pack; chats take bigger ones, but
/// the phone's own stickers stay under these.
const STATIC_MAX: usize = 100 * 1024;
const ANIMATED_MAX: usize = 500 * 1024;
/// The longest an animated sticker plays.
const MAX_ANIMATION_MS: u32 = 10_000;
/// Frames closer together than this are merged: more only adds bytes.
const MIN_FRAME_MS: u32 = 1000 / 15;

pub struct Sticker {
    pub bytes: Vec<u8>,
    pub animated: bool,
}

/// Makes a sticker from a picture's bytes (PNG, JPEG, GIF or WebP).
pub fn make(bytes: &[u8]) -> Result<Sticker, String> {
    let format = image::guess_format(bytes).map_err(|_| "That file is not a picture.".to_string())?;
    let frames = match format {
        ImageFormat::Gif => frames(GifDecoder::new(Cursor::new(bytes)).map_err(unreadable)?)?,
        ImageFormat::WebP => {
            let decoder = WebPDecoder::new(Cursor::new(bytes)).map_err(unreadable)?;
            if decoder.has_animation() { frames(decoder)? } else { Vec::new() }
        }
        _ => Vec::new(),
    };
    if frames.len() > 1 {
        return Ok(Sticker { bytes: encode_animated(&frames)?, animated: true });
    }
    let picture = match frames.into_iter().next() {
        Some((frame, _)) => frame,
        None => image::load_from_memory_with_format(bytes, format).map_err(unreadable)?.to_rgba8(),
    };
    Ok(Sticker { bytes: encode_static(&fit(&picture))?, animated: false })
}

fn unreadable(e: image::ImageError) -> String {
    format!("The picture could not be read: {e}")
}

/// An animation's frames fitted to the square, with how long each shows,
/// cut at the longest a sticker plays.
fn frames<'a>(decoder: impl AnimationDecoder<'a>) -> Result<Vec<(RgbaImage, u32)>, String> {
    let mut out: Vec<(RgbaImage, u32)> = Vec::new();
    let mut total = 0u32;
    for frame in decoder.into_frames() {
        let frame = frame.map_err(unreadable)?;
        let (num, den) = frame.delay().numer_denom_ms();
        // Browsers show a GIF's 0 and 10 ms frames for 100 ms.
        let ms = match num.checked_div(den).unwrap_or(0) {
            0..=10 => 100,
            ms => ms,
        };
        if total >= MAX_ANIMATION_MS {
            break;
        }
        total += ms;
        match out.last_mut() {
            // Too quick to see on its own: the previous frame takes its time.
            Some((_, last)) if *last < MIN_FRAME_MS => *last += ms,
            _ => out.push((fit(frame.buffer()), ms)),
        }
    }
    Ok(out)
}

/// The picture scaled to fit the square, centred on transparency.
fn fit(picture: &RgbaImage) -> RgbaImage {
    let (w, h) = picture.dimensions();
    let scale = SIDE as f64 / w.max(h).max(1) as f64;
    let (nw, nh) = (((w as f64 * scale).round() as u32).clamp(1, SIDE), ((h as f64 * scale).round() as u32).clamp(1, SIDE));
    let resized = if (nw, nh) == (w, h) { picture.clone() } else { imageops::resize(picture, nw, nh, FilterType::Lanczos3) };
    let mut square = RgbaImage::new(SIDE, SIDE);
    imageops::overlay(&mut square, &resized, ((SIDE - nw) / 2).into(), ((SIDE - nh) / 2).into());
    square
}

/// Lossy WebP, lowering the quality until it fits WhatsApp's size.
fn encode_static(picture: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut best = Vec::new();
    for quality in [85.0, 70.0, 55.0, 40.0, 25.0] {
        best = webp_rgba(picture, quality)?;
        if best.len() <= STATIC_MAX {
            break;
        }
    }
    Ok(best)
}

fn webp_rgba(picture: &RgbaImage, quality: f32) -> Result<Vec<u8>, String> {
    use libwebp_sys as webp;
    let (w, h) = picture.dimensions();
    let mut out: *mut u8 = std::ptr::null_mut();
    // SAFETY: the buffer is w×h RGBA rows of 4·w bytes, and libwebp's output
    // is copied out and then released with its own allocator.
    unsafe {
        let len = webp::WebPEncodeRGBA(picture.as_ptr(), w as i32, h as i32, (w * 4) as i32, quality, &mut out);
        if len == 0 || out.is_null() {
            return Err("The sticker could not be made.".into());
        }
        let bytes = std::slice::from_raw_parts(out, len).to_vec();
        webp::WebPFree(out.cast());
        Ok(bytes)
    }
}

fn encode_animated(frames: &[(RgbaImage, u32)]) -> Result<Vec<u8>, String> {
    use webp_animation::{Encoder, EncoderOptions, EncodingConfig};
    let fail = |e: webp_animation::Error| format!("The animated sticker could not be made: {e:?}");
    let mut best = Vec::new();
    for quality in [60.0, 45.0, 30.0, 20.0] {
        let options = EncoderOptions {
            encoding_config: Some(EncodingConfig::new_lossy(quality)),
            ..Default::default()
        };
        let mut encoder = Encoder::new_with_options((SIDE, SIDE), options).map_err(fail)?;
        let mut at = 0i32;
        for (frame, ms) in frames {
            encoder.add_frame(frame.as_raw(), at).map_err(fail)?;
            at += *ms as i32;
        }
        best = encoder.finalize(at).map_err(fail)?.to_vec();
        if best.len() <= ANIMATED_MAX {
            break;
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let picture = RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]));
        picture.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn a_wide_picture_becomes_a_square_static_sticker() {
        let sticker = make(&png(800, 200)).unwrap();
        assert!(!sticker.animated);
        assert!(sticker.bytes.len() <= STATIC_MAX);
        let back = image::load_from_memory(&sticker.bytes).unwrap();
        assert_eq!((back.width(), back.height()), (SIDE, SIDE));
        assert!(!whatsapp_rust::webp::is_animated(&sticker.bytes));
    }

    #[test]
    fn a_gif_becomes_an_animated_sticker() {
        let mut out = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut out);
            for i in 0..4u8 {
                let picture = RgbaImage::from_pixel(64, 64, image::Rgba([i * 60, 0, 0, 255]));
                let delay = image::Delay::from_numer_denom_ms(100, 1);
                encoder.encode_frame(image::Frame::from_parts(picture, 0, 0, delay)).unwrap();
            }
        }
        let sticker = make(&out).unwrap();
        assert!(sticker.animated);
        assert!(whatsapp_rust::webp::is_animated(&sticker.bytes));
    }

    #[test]
    fn something_else_is_refused() {
        assert!(make(b"not a picture").is_err());
    }
}
