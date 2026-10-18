//! Pictures made into stickers: 512×512 WebP on a transparent square, as
//! the phone makes them. A GIF or an animated WebP becomes an animated sticker.
//! A WebP that is already a sticker is sent as it is.

use std::io::Cursor;

use image::codecs::gif::GifDecoder;
use image::codecs::webp::WebPDecoder;
use image::imageops::{self, FilterType};
use image::{AnimationDecoder, ImageDecoder, ImageFormat, RgbaImage};

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

/// A picture as a sticker: a WebP that already is one (at most 512 pixels a
/// side and within WhatsApp's size) as it is, anything else made into one.
pub fn prepare(bytes: Vec<u8>) -> Result<Sticker, String> {
    match fits(&bytes) {
        Some(animated) => Ok(Sticker { bytes, animated }),
        None => make(&bytes),
    }
}

/// Whether a picture can be sent as a sticker untouched, and then whether it moves.
fn fits(bytes: &[u8]) -> Option<bool> {
    if image::guess_format(bytes).ok()? != ImageFormat::WebP {
        return None;
    }
    let decoder = WebPDecoder::new(Cursor::new(bytes)).ok()?;
    let (w, h) = decoder.dimensions();
    let animated = decoder.has_animation();
    let max = if animated { ANIMATED_MAX } else { STATIC_MAX };
    (w <= SIDE && h <= SIDE && bytes.len() <= max).then_some(animated)
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
            _ => out.push((fit_with(frame.buffer(), FilterType::Triangle), ms)),
        }
    }
    Ok(out)
}

/// The picture scaled to fit the square, centred on transparency.
fn fit(picture: &RgbaImage) -> RgbaImage {
    fit_with(picture, FilterType::Lanczos3)
}

/// As [`fit`], with a choice of filter: animations take a quicker one, as
/// they have many frames and each shows briefly.
fn fit_with(picture: &RgbaImage, filter: FilterType) -> RgbaImage {
    let (w, h) = picture.dimensions();
    let scale = SIDE as f64 / w.max(h).max(1) as f64;
    let (nw, nh) = (((w as f64 * scale).round() as u32).clamp(1, SIDE), ((h as f64 * scale).round() as u32).clamp(1, SIDE));
    let resized = if (nw, nh) == (w, h) { picture.clone() } else { imageops::resize(picture, nw, nh, filter) };
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

/// Every other frame dropped, each kept one showing for the pair.
fn halve(frames: &[(RgbaImage, u32)]) -> Vec<(RgbaImage, u32)> {
    frames
        .chunks(2)
        .map(|pair| (pair[0].0.clone(), pair.iter().map(|(_, ms)| ms).sum()))
        .collect()
}

/// Lossy animated WebP. When it is too big, half the frames go and the
/// quality drops, rather than trying many qualities over every frame.
fn encode_animated(frames: &[(RgbaImage, u32)]) -> Result<Vec<u8>, String> {
    use webp_animation::{Encoder, EncoderOptions, EncodingConfig};
    let fail = |e: webp_animation::Error| format!("The animated sticker could not be made: {e:?}");
    let mut frames = frames.to_vec();
    let mut best = Vec::new();
    for quality in [50.0, 35.0, 25.0] {
        let options = EncoderOptions {
            // Method 2 of 0–6: much quicker than the default 4, a little bigger.
            encoding_config: Some(EncodingConfig { method: 2, ..EncodingConfig::new_lossy(quality) }),
            ..Default::default()
        };
        let mut encoder = Encoder::new_with_options((SIDE, SIDE), options).map_err(fail)?;
        let mut at = 0i32;
        for (frame, ms) in &frames {
            encoder.add_frame(frame.as_raw(), at).map_err(fail)?;
            at += *ms as i32;
        }
        best = encoder.finalize(at).map_err(fail)?.to_vec();
        if best.len() <= ANIMATED_MAX || frames.len() < 4 {
            break;
        }
        frames = halve(&frames);
    }
    Ok(best)
}

/// What Signal takes for one sticker in a pack.
const SIGNAL_MAX: usize = 300 * 1024;
/// The sides an animated sticker shrinks through until it fits.
const SIGNAL_SIDES: [u32; 4] = [SIDE, 384, 320, 256];

/// A sticker as Signal takes it.
pub struct SignalSticker {
    pub bytes: Vec<u8>,
    /// `image/webp`, or `image/png` for an animated one (APNG).
    pub mime: &'static str,
}

/// A sticker as Signal's packs take it: a still WebP as it is when it fits,
/// an animated one made into an APNG within Signal's 300 KB, at the speed it
/// had. (Signal's creator also asks for 3 seconds at most, which only it
/// checks; playing longer ones quicker looked wrong.) One that can't be made
/// to fit keeps its first frame.
pub fn for_signal(bytes: &[u8]) -> Result<SignalSticker, String> {
    let format = image::guess_format(bytes).map_err(|_| "That file is not a picture.".to_string())?;
    let frames = match format {
        ImageFormat::Gif => frames(GifDecoder::new(Cursor::new(bytes)).map_err(unreadable)?)?,
        ImageFormat::WebP => {
            let decoder = WebPDecoder::new(Cursor::new(bytes)).map_err(unreadable)?;
            let (w, h) = decoder.dimensions();
            if !decoder.has_animation() {
                if w <= SIDE && h <= SIDE && bytes.len() <= SIGNAL_MAX {
                    return Ok(SignalSticker { bytes: bytes.to_vec(), mime: "image/webp" });
                }
                Vec::new()
            } else {
                frames(decoder)?
            }
        }
        _ => Vec::new(),
    };
    if frames.len() > 1 {
        if let Some(bytes) = SIGNAL_SIDES.iter().find_map(|&side| apng_within(&frames, side, SIGNAL_MAX)) {
            return Ok(SignalSticker { bytes, mime: "image/png" });
        }
        let still = encode_static(&frames[0].0)?;
        return Ok(SignalSticker { bytes: still, mime: "image/webp" });
    }
    let made = make(bytes)?;
    Ok(SignalSticker { bytes: made.bytes, mime: "image/webp" })
}

/// The animation as a palette APNG at `side` pixels, when it fits in `max`
/// bytes. Fewer frames are tried before giving up on the size.
fn apng_within(frames: &[(RgbaImage, u32)], side: u32, max: usize) -> Option<Vec<u8>> {
    let mut frames: Vec<(RgbaImage, u32)> = frames
        .iter()
        .map(|(f, ms)| (if side == SIDE { f.clone() } else { imageops::resize(f, side, side, FilterType::Triangle) }, *ms))
        .collect();
    loop {
        let bytes = encode_apng(&frames).ok()?;
        if bytes.len() <= max {
            return Some(bytes);
        }
        // At the smallest side, half the frames before giving up.
        if side != SIGNAL_SIDES[SIGNAL_SIDES.len() - 1] || frames.len() < 8 {
            return None;
        }
        frames = halve(&frames);
    }
}

/// Alpha below this counts as transparent, so the palette isn't spent on
/// the colours of invisible pixels.
const CLEAR: u8 = 8;
/// The palette entry kept for transparency.
const CLEAR_INDEX: u8 = 255;

/// The frames as one APNG on a shared 256-colour palette (one entry kept for
/// transparency). Each frame after the first carries only the box that
/// changed, drawn over the one before.
fn encode_apng(frames: &[(RgbaImage, u32)]) -> Result<Vec<u8>, String> {
    let fail = |e: png::EncodingError| format!("The animated sticker could not be made: {e}");
    let (w, h) = frames[0].0.dimensions();
    // Trained on every frame, a sample of each to keep it quick.
    let mut sample = Vec::new();
    let step = (frames.len() / 12).max(1);
    for (frame, _) in frames.iter().step_by(step) {
        sample.extend(frame.pixels().filter(|p| p[3] >= CLEAR).flat_map(|p| p.0));
    }
    if sample.is_empty() {
        sample.extend([0, 0, 0, 255]);
    }
    let quant = color_quant::NeuQuant::new(10, 255, &sample);
    let indexed: Vec<Vec<u8>> = frames
        .iter()
        .map(|(frame, _)| frame.pixels().map(|p| if p[3] < CLEAR { CLEAR_INDEX } else { quant.index_of(&p.0) as u8 }).collect())
        .collect();
    let map = quant.color_map_rgba();
    let mut palette = Vec::with_capacity(256 * 3);
    let mut alpha = Vec::with_capacity(256);
    for rgba in map.chunks(4).chain(std::iter::once(&[0u8, 0, 0, 0][..])) {
        palette.extend_from_slice(&rgba[..3]);
        alpha.push(rgba[3]);
    }

    // Identical frames are one, showing for both.
    let mut kept: Vec<(usize, u32)> = Vec::new();
    for (i, (_, ms)) in frames.iter().enumerate() {
        match kept.last_mut() {
            Some((last, total)) if indexed[*last] == indexed[i] => *total += ms,
            _ => kept.push((i, *ms)),
        }
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(palette);
        encoder.set_trns(alpha);
        encoder.set_compression(png::Compression::High);
        encoder.set_animated(kept.len() as u32, 0).map_err(fail)?;
        let mut writer = encoder.write_header().map_err(fail)?;
        let mut previous: Option<&[u8]> = None;
        for &(i, ms) in &kept {
            let current = indexed[i].as_slice();
            let (x, y, bw, bh) = match previous {
                Some(before) => changed_box(before, current, w, h),
                None => (0, 0, w, h),
            };
            writer.set_frame_dimension(bw, bh).map_err(fail)?;
            writer.set_frame_position(x, y).map_err(fail)?;
            writer.set_dispose_op(png::DisposeOp::None).map_err(fail)?;
            writer.set_blend_op(png::BlendOp::Source).map_err(fail)?;
            writer.set_frame_delay(ms.min(u16::MAX.into()) as u16, 1000).map_err(fail)?;
            let mut data = Vec::with_capacity((bw * bh) as usize);
            for row in y..y + bh {
                let start = (row * w + x) as usize;
                data.extend_from_slice(&current[start..start + bw as usize]);
            }
            writer.write_image_data(&data).map_err(fail)?;
            previous = Some(current);
        }
        writer.finish().map_err(fail)?;
    }
    Ok(out)
}

/// The smallest box (x, y, width, height) holding every pixel that changed;
/// one pixel when none did.
fn changed_box(before: &[u8], after: &[u8], w: u32, h: u32) -> (u32, u32, u32, u32) {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        let row = (y * w) as usize..((y + 1) * w) as usize;
        let (a, b) = (&before[row.clone()], &after[row]);
        if a == b {
            continue;
        }
        let first = a.iter().zip(b).position(|(p, q)| p != q).unwrap_or(0) as u32;
        let last = (w as usize - 1 - a.iter().rev().zip(b.iter().rev()).position(|(p, q)| p != q).unwrap_or(0)) as u32;
        x0 = x0.min(first);
        x1 = x1.max(last);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    if x0 > x1 {
        return (0, 0, 1, 1);
    }
    (x0, y0, x1 - x0 + 1, y1 - y0 + 1)
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
    fn a_small_webp_sticker_goes_untouched() {
        let made = make(&png(300, 300)).unwrap();
        let again = prepare(made.bytes.clone()).unwrap();
        assert_eq!(again.bytes, made.bytes);
        // A PNG is always made into one.
        let png = png(100, 100);
        assert_ne!(prepare(png.clone()).unwrap().bytes, png);
    }

    fn animated_webp(frames: u32, ms: i32) -> Vec<u8> {
        let mut encoder = webp_animation::Encoder::new((SIDE, SIDE)).unwrap();
        for i in 0..frames {
            let picture = RgbaImage::from_fn(SIDE, SIDE, |x, y| {
                let inside = (x as i64 - 256).pow(2) + (y as i64 - 256).pow(2) < 200i64.pow(2);
                if inside { image::Rgba([(x / 2) as u8, (i * 20) as u8, (y / 2) as u8, 255]) } else { image::Rgba([0, 0, 0, 0]) }
            });
            encoder.add_frame(picture.as_raw(), i as i32 * ms).unwrap();
        }
        encoder.finalize(frames as i32 * ms).unwrap().to_vec()
    }

    #[test]
    fn signal_takes_a_still_webp_as_it_is() {
        let still = make(&png(300, 300)).unwrap().bytes;
        let made = for_signal(&still).unwrap();
        assert_eq!((made.mime, made.bytes == still), ("image/webp", true));
    }

    #[test]
    fn signal_gets_an_animated_sticker_as_an_apng_within_its_size_at_its_speed() {
        let made = for_signal(&animated_webp(50, 100)).unwrap();
        assert_eq!(made.mime, "image/png");
        assert!(made.bytes.len() <= SIGNAL_MAX, "{} bytes", made.bytes.len());
        let decoder = image::codecs::png::PngDecoder::new(Cursor::new(&made.bytes)).unwrap();
        let apng = decoder.apng().unwrap();
        let frames: Vec<_> = apng.into_frames().collect::<Result<_, _>>().unwrap();
        assert!(frames.len() > 1);
        let total: u32 = frames
            .iter()
            .map(|f| {
                let (n, d) = f.delay().numer_denom_ms();
                n / d
            })
            .sum();
        // Five seconds, as made: frames may merge, never quicken.
        assert_eq!(total, 5000);
        // Every frame draws the whole picture once composed.
        let last = frames.last().unwrap().buffer();
        assert_eq!(last.dimensions().0, last.dimensions().1);
        assert_eq!(last.get_pixel(0, 0)[3], 0);
        assert_eq!(last.get_pixel(last.width() / 2, last.height() / 2)[3], 255);
    }

    #[test]
    fn the_changed_box_holds_only_what_changed() {
        let before = vec![0u8; 16];
        let mut after = before.clone();
        after[5] = 1; // (1, 1)
        after[10] = 1; // (2, 2)
        assert_eq!(changed_box(&before, &after, 4, 4), (1, 1, 2, 2));
        assert_eq!(changed_box(&before, &before, 4, 4), (0, 0, 1, 1));
    }

    #[test]
    fn something_else_is_refused() {
        assert!(make(b"not a picture").is_err());
    }
}

#[cfg(test)]
mod timing {
    /// Times making real KLIPY stickers; needs the key in `.env` and the network.
    #[test]
    #[ignore]
    fn klipy_stickers_timing() {
        let dir = std::env::temp_dir();
        let page = crate::klipy::stickers(&dir, "cat dance", 1).unwrap();
        for item in page.items.iter().take(4) {
            let t = std::time::Instant::now();
            let bytes = crate::klipy::download(&item.send.url).unwrap();
            let downloaded = t.elapsed();
            let t = std::time::Instant::now();
            let made = super::prepare(bytes.clone()).unwrap();
            println!(
                "{}: {} KB in, downloaded {:?}, sent {} KB animated={} as-is={} in {:?}",
                item.slug,
                bytes.len() / 1024,
                downloaded,
                made.bytes.len() / 1024,
                made.animated,
                made.bytes == bytes,
                t.elapsed()
            );
        }
    }
}
