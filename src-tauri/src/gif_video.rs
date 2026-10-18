//! GIFs made in the editor: WhatsApp sends a GIF as a short H.264 video in an
//! mp4 that the phone plays on a loop, so frames are encoded with OpenH264
//! and put in an mp4 here, each sample lasting as long as its frame showed.

use image::RgbaImage;
use openh264::encoder::{BitRate, Complexity, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, RateControlMode};
use openh264::formats::{RgbSliceU8, YUVBuffer};
use openh264::OpenH264API;

/// The longest side of a made GIF, as the GIFs WhatsApp offers are.
pub const SIDE_MAX: u32 = 480;
/// Sample times are in milliseconds.
const TIMESCALE: u32 = 1000;

pub struct Video {
    pub mp4: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// One encoded frame: its NAL units each after its length (as an mp4 keeps
/// them), how long it shows, and whether it starts afresh.
struct Sample {
    data: Vec<u8>,
    ms: u32,
    sync: bool,
}

/// The size a frame is encoded at: within [`SIDE_MAX`], and even, as 4:2:0 needs.
pub fn size_for(width: u32, height: u32) -> (u32, u32) {
    let scale = (f64::from(SIDE_MAX) / f64::from(width.max(height))).min(1.0);
    let even = |v: u32| ((f64::from(v) * scale) as u32 & !1).max(2);
    (even(width), even(height))
}

/// Frames (all the same size, even sides) as an mp4, each shown for its
/// milliseconds. Transparency becomes white.
pub fn encode(frames: &[(RgbaImage, u32)]) -> Result<Video, String> {
    let (width, height) = frames.first().ok_or("There is nothing to make a GIF of.")?.0.dimensions();
    if width % 2 != 0 || height % 2 != 0 {
        return Err("A GIF's sides have to be even.".into());
    }
    let shortest = frames.iter().map(|(_, ms)| *ms).filter(|ms| *ms > 0).min().unwrap_or(100);
    let config = EncoderConfig::new()
        .rate_control_mode(RateControlMode::Quality)
        .bitrate(BitRate::from_bps(bitrate(width, height)))
        .max_frame_rate(FrameRate::from_hz((1000.0 / shortest as f32).clamp(1.0, 30.0)))
        .skip_frames(false)
        .complexity(Complexity::High)
        .intra_frame_period(IntraFramePeriod::from_num_frames(90));
    let mut encoder = Encoder::with_api_config(OpenH264API::from_source(), config).map_err(|e| format!("The GIF could not be made: {e}"))?;
    let (mut sps, mut pps) = (None, None);
    let mut samples: Vec<Sample> = Vec::with_capacity(frames.len());
    let mut rgb = vec![0u8; (width * height * 3) as usize];
    for (frame, ms) in frames {
        if frame.dimensions() != (width, height) {
            return Err("The GIF's frames are not all the same size.".into());
        }
        for (out, p) in rgb.chunks_exact_mut(3).zip(frame.pixels()) {
            let a = u16::from(p[3]);
            for c in 0..3 {
                out[c] = ((u16::from(p[c]) * a + 255 * (255 - a) + 127) / 255) as u8;
            }
        }
        let yuv = YUVBuffer::from_rgb_source(RgbSliceU8::new(&rgb, (width as usize, height as usize)));
        let stream = encoder.encode(&yuv).map_err(|e| format!("The GIF could not be made: {e}"))?;
        let mut sample = Sample { data: Vec::new(), ms: (*ms).max(1), sync: false };
        for layer in (0..stream.num_layers()).filter_map(|i| stream.layer(i)) {
            for nal in (0..layer.nal_count()).filter_map(|i| layer.nal_unit(i)) {
                let nal = without_start_code(nal);
                match nal.first().map(|b| b & 0x1f) {
                    Some(7) => sps = sps.or_else(|| Some(nal.to_vec())),
                    Some(8) => pps = pps.or_else(|| Some(nal.to_vec())),
                    Some(kind) => {
                        sample.sync |= kind == 5;
                        sample.data.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                        sample.data.extend_from_slice(nal);
                    }
                    None => {}
                }
            }
        }
        // A frame the encoder left out shows the one before it for longer.
        match samples.last_mut() {
            Some(last) if sample.data.is_empty() => last.ms += sample.ms,
            _ if sample.data.is_empty() => {}
            _ => samples.push(sample),
        }
    }
    let (Some(sps), Some(pps)) = (sps, pps) else {
        return Err("The GIF could not be made: the encoder gave no picture.".into());
    };
    if samples.first().is_none_or(|s| !s.sync) {
        return Err("The GIF could not be made: it does not start with a whole picture.".into());
    }
    Ok(Video { mp4: mux(width, height, &sps, &pps, &samples), width, height })
}

/// Enough for a GIF to look clean, kept small: about 3 bits a pixel a second.
fn bitrate(width: u32, height: u32) -> u32 {
    (width * height * 3).clamp(300_000, 1_200_000)
}

fn without_start_code(nal: &[u8]) -> &[u8] {
    if nal.starts_with(&[0, 0, 0, 1]) {
        &nal[4..]
    } else if nal.starts_with(&[0, 0, 1]) {
        &nal[3..]
    } else {
        nal
    }
}

/// A box: its size, its type, then what is in it.
fn mp4_box(kind: &[u8; 4], parts: &[&[u8]]) -> Vec<u8> {
    let len = 8 + parts.iter().map(|p| p.len()).sum::<usize>();
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&(len as u32).to_be_bytes());
    out.extend_from_slice(kind);
    for part in parts {
        out.extend_from_slice(part);
    }
    out
}

/// A full box: a box with a version and flags first.
fn full_box(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let head = (u32::from(version) << 24 | flags).to_be_bytes();
    mp4_box(kind, &[&head, body])
}

fn be32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// The 3×3 matrix that leaves the picture as it is.
const UNITY: [u32; 9] = [0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x4000_0000];

/// The samples in an mp4 with the index first, so it plays as it arrives.
fn mux(width: u32, height: u32, sps: &[u8], pps: &[u8], samples: &[Sample]) -> Vec<u8> {
    let ftyp = mp4_box(b"ftyp", &[b"isom", &0x200u32.to_be_bytes(), b"isomiso2avc1mp41"]);
    let mdat_len: usize = samples.iter().map(|s| s.data.len()).sum();
    // The index's size does not depend on where the samples start.
    let probe = moov(width, height, sps, pps, samples, 0);
    let data_at = (ftyp.len() + probe.len() + 8) as u32;
    let moov = moov(width, height, sps, pps, samples, data_at);
    let mut out = Vec::with_capacity(ftyp.len() + moov.len() + 8 + mdat_len);
    out.extend_from_slice(&ftyp);
    out.extend_from_slice(&moov);
    out.extend_from_slice(&((8 + mdat_len) as u32).to_be_bytes());
    out.extend_from_slice(b"mdat");
    for sample in samples {
        out.extend_from_slice(&sample.data);
    }
    out
}

fn moov(width: u32, height: u32, sps: &[u8], pps: &[u8], samples: &[Sample], data_at: u32) -> Vec<u8> {
    let duration: u32 = samples.iter().map(|s| s.ms).sum();
    let mvhd = full_box(
        b"mvhd",
        0,
        0,
        &[
            be32s(&[0, 0, TIMESCALE, duration, 0x10000]),
            vec![1, 0, 0, 0],
            vec![0; 8],
            be32s(&UNITY),
            vec![0; 24],
            be32s(&[2]),
        ]
        .concat(),
    );
    let tkhd = full_box(
        b"tkhd",
        0,
        3,
        &[be32s(&[0, 0, 1, 0, duration, 0, 0, 0, 0]), be32s(&UNITY), be32s(&[width << 16, height << 16])].concat(),
    );
    let mdhd = full_box(b"mdhd", 0, 0, &[be32s(&[0, 0, TIMESCALE, duration]), vec![0x55, 0xc4, 0, 0]].concat());
    let hdlr = full_box(b"hdlr", 0, 0, &[be32s(&[0]), b"vide".to_vec(), vec![0; 12], b"VideoHandler\0".to_vec()].concat());
    let vmhd = full_box(b"vmhd", 0, 1, &[0; 8]);
    let dinf = mp4_box(b"dinf", &[&full_box(b"dref", 0, 0, &[be32s(&[1]), full_box(b"url ", 0, 1, &[])].concat())]);

    let avcc = mp4_box(
        b"avcC",
        &[
            &[1, sps[1], sps[2], sps[3], 0xff, 0xe1],
            &(sps.len() as u16).to_be_bytes(),
            sps,
            &[1],
            &(pps.len() as u16).to_be_bytes(),
            pps,
        ],
    );
    let mut avc1 = vec![0u8; 6];
    avc1.extend_from_slice(&1u16.to_be_bytes()); // data reference index
    avc1.extend_from_slice(&[0; 16]);
    avc1.extend_from_slice(&(width as u16).to_be_bytes());
    avc1.extend_from_slice(&(height as u16).to_be_bytes());
    avc1.extend_from_slice(&be32s(&[0x0048_0000, 0x0048_0000, 0]));
    avc1.extend_from_slice(&1u16.to_be_bytes()); // frames per sample
    avc1.extend_from_slice(&[0; 32]); // compressor name
    avc1.extend_from_slice(&0x18u16.to_be_bytes());
    avc1.extend_from_slice(&0xffffu16.to_be_bytes());
    let stsd = full_box(b"stsd", 0, 0, &[be32s(&[1]), mp4_box(b"avc1", &[&avc1, &avcc])].concat());

    // Runs of samples that last as long.
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for s in samples {
        match runs.last_mut() {
            Some((count, ms)) if *ms == s.ms => *count += 1,
            _ => runs.push((1, s.ms)),
        }
    }
    let stts = full_box(b"stts", 0, 0, &[be32s(&[runs.len() as u32]), runs.iter().flat_map(|(c, ms)| be32s(&[*c, *ms])).collect()].concat());
    let syncs: Vec<u32> = samples.iter().enumerate().filter(|(_, s)| s.sync).map(|(i, _)| i as u32 + 1).collect();
    let stss = full_box(b"stss", 0, 0, &[be32s(&[syncs.len() as u32]), be32s(&syncs)].concat());
    // One chunk holds them all.
    let stsc = full_box(b"stsc", 0, 0, &be32s(&[1, 1, samples.len() as u32, 1]));
    let sizes: Vec<u32> = samples.iter().map(|s| s.data.len() as u32).collect();
    let stsz = full_box(b"stsz", 0, 0, &[be32s(&[0, sizes.len() as u32]), be32s(&sizes)].concat());
    let stco = full_box(b"stco", 0, 0, &be32s(&[1, data_at]));

    let stbl = mp4_box(b"stbl", &[&stsd, &stts, &stss, &stsc, &stsz, &stco]);
    let minf = mp4_box(b"minf", &[&vmhd, &dinf, &stbl]);
    let mdia = mp4_box(b"mdia", &[&mdhd, &hdlr, &minf]);
    let trak = mp4_box(b"trak", &[&tkhd, &mdia]);
    mp4_box(b"moov", &[&mvhd, &trak])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(n: usize, ms: u32) -> Vec<(RgbaImage, u32)> {
        (0..n)
            .map(|i| (RgbaImage::from_fn(64, 48, |x, y| image::Rgba([(x * 4) as u8, (y * 5) as u8, (i * 20) as u8, 255])), ms))
            .collect()
    }

    #[test]
    fn makes_an_mp4_as_long_as_its_frames() {
        let video = encode(&frames(20, 100)).unwrap();
        assert_eq!((video.width, video.height), (64, 48));
        assert_eq!(&video.mp4[4..8], b"ftyp");
        assert_eq!(crate::klipy::mp4_seconds(&video.mp4), Some(2));
    }

    #[test]
    fn sizes_are_even_and_within_bounds() {
        assert_eq!(size_for(1024, 576), (480, 270));
        assert_eq!(size_for(301, 201), (300, 200));
        assert_eq!(size_for(1, 1), (2, 2));
    }

    #[test]
    fn odd_sides_are_refused() {
        let odd = vec![(RgbaImage::new(63, 48), 100)];
        assert!(encode(&odd).is_err());
    }
}

