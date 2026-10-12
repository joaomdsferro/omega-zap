//! Records voice messages from the microphone, as WhatsApp sends them: Opus
//! in an Ogg file, mono, with the loudness over its length for the waveform.
//!
//! Recorded here rather than in the page: WebKitGTK ships with the
//! microphone turned off, and what its recorder writes depends on the
//! GStreamer plugins installed. One recording at a time.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Opus runs at 48 kHz; what the microphone gives is brought to it.
const RATE: u32 = 48_000;
/// 20 ms, the usual Opus frame.
const FRAME: usize = 960;
/// Enough for speech; about 180 KB a minute.
const BITRATE: i32 = 24_000;
/// The loudness is told to the page this often, in frames (100 ms).
const LEVEL_EVERY: usize = 5;
/// WhatsApp's waveform has this many values, 0–100.
const WAVEFORM_LEN: usize = 64;
/// Ogg pages are closed at about a second of sound.
const PAGE_PACKETS: usize = 50;

const RECORDING: u8 = 0;
const PAUSED: u8 = 1;
const STOP: u8 = 2;
const CANCEL: u8 = 3;

/// A finished recording.
pub struct Recording {
    pub ogg: Vec<u8>,
    pub seconds: u32,
    pub waveform: Vec<u8>,
}

/// What the page is told while recording.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Level {
    /// How long has been recorded, pauses left out.
    ms: u64,
    /// The loudness of the last 100 ms, 0–1.
    level: f32,
}

struct Session {
    state: Arc<AtomicU8>,
    worker: JoinHandle<Result<Recording, String>>,
}

/// The recording going on, if any. Kept by the app.
#[derive(Default, Clone)]
pub struct Recorder(Arc<Mutex<Option<Session>>>);

impl Recorder {
    /// Starts recording from the default microphone; one already going is thrown away.
    pub fn start(&self, app: AppHandle) -> Result<(), String> {
        // Held throughout, so two starts at once can't leave one running unseen.
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = slot.take() {
            let _ = session.end(CANCEL);
        }
        let state = Arc::new(AtomicU8::new(RECORDING));
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = {
            let state = state.clone();
            std::thread::Builder::new()
                .name("voice".into())
                .spawn(move || record(app, state, ready_tx))
                .map_err(|e| format!("Could not start recording: {e}"))?
        };
        match ready_rx.recv() {
            Ok(Ok(())) => {
                *slot = Some(Session { state, worker });
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(worker.join().ok().and_then(Result::err).unwrap_or_else(|| "Could not start recording.".into())),
        }
    }

    pub fn set_paused(&self, paused: bool) {
        if let Some(session) = self.0.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let (from, to) = if paused { (RECORDING, PAUSED) } else { (PAUSED, RECORDING) };
            let _ = session.state.compare_exchange(from, to, Ordering::Relaxed, Ordering::Relaxed);
        }
    }

    pub fn cancel(&self) {
        if let Some(session) = self.0.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = session.end(CANCEL);
        }
    }

    /// Stops recording and hands over what was recorded. Blocks while the last of it is encoded.
    pub fn finish(&self) -> Result<Recording, String> {
        let session = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .ok_or("Nothing is being recorded.")?;
        session.end(STOP)
    }
}

impl Session {
    /// Tells the recording thread to stop or cancel, and waits for it.
    fn end(self, how: u8) -> Result<Recording, String> {
        self.state.store(how, Ordering::Relaxed);
        self.worker.join().map_err(|_| "The recording stopped unexpectedly.".to_string())?
    }
}

/// Starts recording a voice message. Its length and loudness come as `voice://level` events.
#[tauri::command]
pub async fn voice_start(app: AppHandle, recorder: tauri::State<'_, Recorder>) -> Result<(), String> {
    // Opening the microphone can take a moment; not on the main thread.
    let recorder = recorder.inner().clone();
    tauri::async_runtime::spawn_blocking(move || recorder.start(app))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn voice_pause(recorder: tauri::State<'_, Recorder>, paused: bool) {
    recorder.set_paused(paused);
}

#[tauri::command]
pub async fn voice_cancel(recorder: tauri::State<'_, Recorder>) -> Result<(), String> {
    let recorder = recorder.inner().clone();
    tauri::async_runtime::spawn_blocking(move || recorder.cancel())
        .await
        .map_err(|e| e.to_string())
}

/// Stops recording and returns the voice message, ready to send.
pub async fn finish(recorder: &Recorder) -> Result<Recording, String> {
    let recorder = recorder.clone();
    tauri::async_runtime::spawn_blocking(move || recorder.finish())
        .await
        .map_err(|e| e.to_string())?
}

/// The recording thread: the microphone's stream can't leave the thread that opened it.
fn record(app: AppHandle, state: Arc<AtomicU8>, ready: mpsc::Sender<Result<(), String>>) -> Result<Recording, String> {
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let failed = Arc::new(Mutex::new(None::<String>));
    let opened = open(tx, failed.clone());
    let (stream, rate) = match opened {
        Ok(opened) => opened,
        Err(e) => {
            let _ = ready.send(Err(e.clone()));
            return Err(e);
        }
    };
    let mut encoder = match Encoder::new(rate) {
        Ok(encoder) => encoder,
        Err(e) => {
            let _ = ready.send(Err(e.clone()));
            return Err(e);
        }
    };
    let _ = ready.send(Ok(()));

    let mut told = 0;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(chunk) if state.load(Ordering::Relaxed) == RECORDING => encoder.push(&chunk),
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if let Some(e) = failed.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = app.emit("voice://error", &e);
            return Err(e);
        }
        match state.load(Ordering::Relaxed) {
            STOP => break,
            CANCEL => return Err("Cancelled.".into()),
            _ => {}
        }
        let frames = encoder.levels.len();
        if frames >= told + LEVEL_EVERY {
            told = frames;
            let recent = &encoder.levels[frames - LEVEL_EVERY..];
            let level = recent.iter().copied().fold(0.0, f32::max);
            let _ = app.emit("voice://level", Level { ms: encoder.samples * 1000 / RATE as u64, level });
        }
    }
    drop(stream);
    // What the microphone had already handed over still counts.
    while let Ok(chunk) = rx.try_recv() {
        encoder.push(&chunk);
    }
    encoder.finish()
}

/// Opens the default microphone. Its sound arrives on `tx` as mono samples at the returned rate.
fn open(tx: mpsc::Sender<Vec<f32>>, failed: Arc<Mutex<Option<String>>>) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("No microphone was found.")?;
    let config = device
        .default_input_config()
        .map_err(|e| format!("The microphone can't be used: {e}"))?;
    let rate = config.sample_rate();
    let channels = usize::from(config.channels()).max(1);
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, channels, tx, failed),
        SampleFormat::I16 => build::<i16>(&device, config, channels, tx, failed),
        SampleFormat::I32 => build::<i32>(&device, config, channels, tx, failed),
        SampleFormat::U16 => build::<u16>(&device, config, channels, tx, failed),
        SampleFormat::U8 => build::<u8>(&device, config, channels, tx, failed),
        SampleFormat::I8 => build::<i8>(&device, config, channels, tx, failed),
        other => return Err(format!("The microphone's sound format ({other}) isn't supported.")),
    }
    .map_err(|e| format!("The microphone can't be used: {e}"))?;
    stream.play().map_err(|e| format!("The microphone can't be used: {e}"))?;
    Ok((stream, rate))
}

fn build<T: SizedSample>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    tx: mpsc::Sender<Vec<f32>>,
    failed: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, cpal::Error>
where
    f32: FromSample<T>,
{
    device.build_input_stream(
        config,
        move |data: &[T], _: &_| {
            let mono = data
                .chunks(channels)
                .map(|frame| frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / frame.len() as f32)
                .collect();
            let _ = tx.send(mono);
        },
        move |e| {
            log::warn!("voice: microphone: {e}");
            if matches!(e.kind(), cpal::ErrorKind::DeviceNotAvailable) {
                *failed.lock().unwrap_or_else(|e| e.into_inner()) = Some("The microphone was disconnected.".into());
            }
        },
        None,
    )
}

/// Brings mono sound from the microphone's rate to 48 kHz, straight lines between samples.
struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// The next output's place in the input, where -1 is `prev`.
    pos: f64,
    prev: f32,
}

impl Resampler {
    fn new(from: u32) -> Self {
        Self { step: f64::from(from) / f64::from(RATE), pos: 0.0, prev: 0.0 }
    }

    fn run(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        if self.step == 1.0 {
            out.extend_from_slice(input);
            return;
        }
        let at = |i: isize| if i < 0 { self.prev } else { input[i as usize] };
        let last = (input.len() - 1) as f64;
        while self.pos < last {
            let i = self.pos.floor();
            let (a, b) = (at(i as isize), at(i as isize + 1));
            out.push(a + (b - a) * (self.pos - i) as f32);
            self.pos += self.step;
        }
        self.pos -= input.len() as f64;
        self.prev = input[input.len() - 1];
    }
}

/// Turns 48 kHz mono sound into an Ogg Opus file, and notes its loudness per frame.
struct Encoder {
    opus: opus::Encoder,
    resampler: Resampler,
    pending: Vec<f32>,
    ogg: Ogg,
    pre_skip: u64,
    /// 48 kHz samples taken in.
    samples: u64,
    /// Each frame's loudness, 0–1.
    levels: Vec<f32>,
}

impl Encoder {
    fn new(from: u32) -> Result<Self, String> {
        let fail = |e: opus::Error| format!("Could not start the voice encoder: {e}");
        let mut opus = opus::Encoder::new(RATE, opus::Channels::Mono, opus::Application::Voip).map_err(fail)?;
        opus.set_bitrate(opus::Bitrate::Bits(BITRATE)).map_err(fail)?;
        let pre_skip = opus.get_lookahead().map_err(fail)?.max(0) as u64;
        let mut ogg = Ogg::new(rand_serial());
        ogg.page(&[&opus_head(pre_skip as u16)], 0, BOS);
        ogg.page(&[&opus_tags()], 0, 0);
        Ok(Self {
            opus,
            resampler: Resampler::new(from),
            pending: Vec::with_capacity(FRAME * 4),
            ogg,
            pre_skip,
            samples: 0,
            levels: Vec::new(),
        })
    }

    fn push(&mut self, input: &[f32]) {
        let before = self.pending.len();
        self.resampler.run(input, &mut self.pending);
        self.samples += (self.pending.len() - before) as u64;
        let whole = self.pending.len() / FRAME * FRAME;
        for start in (0..whole).step_by(FRAME) {
            let frame: Vec<f32> = self.pending[start..start + FRAME].to_vec();
            self.encode(&frame, false);
        }
        self.pending.drain(..whole);
    }

    fn encode(&mut self, frame: &[f32], last: bool) {
        let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
        self.levels.push(loudness(rms));
        match self.opus.encode_vec_float(frame, 4000) {
            Ok(packet) => {
                // Frames padded at the end don't count: the file ends where the sound did.
                let end = self.pre_skip + (self.levels.len() * FRAME) as u64;
                self.ogg.packet(packet, end.min(self.pre_skip + self.samples), last);
            }
            Err(e) => log::warn!("voice: a frame was not encoded: {e}"),
        }
    }

    fn finish(mut self) -> Result<Recording, String> {
        // Opus holds back its lookahead: silence pushes the end of the sound out.
        let mut tail = std::mem::take(&mut self.pending);
        tail.resize(FRAME, 0.0);
        self.encode(&tail, false);
        self.encode(&[0.0; FRAME], true);
        // The two frames added above aren't part of what was said.
        self.levels.truncate(self.levels.len().saturating_sub(2));
        if self.samples < u64::from(RATE) / 2 {
            return Err("The voice message is too short.".into());
        }
        let seconds = ((self.samples + u64::from(RATE) / 2) / u64::from(RATE)) as u32;
        Ok(Recording { ogg: self.ogg.out, seconds: seconds.max(1), waveform: waveform(&self.levels) })
    }
}

/// Loudness on a scale people hear: -50 dB and below is 0, -10 dB and above is 1.
fn loudness(rms: f32) -> f32 {
    let db = 20.0 * rms.max(1e-6).log10();
    ((db + 50.0) / 40.0).clamp(0.0, 1.0)
}

/// The frames' loudness in `WAVEFORM_LEN` parts, 0–100, each part's average.
fn waveform(levels: &[f32]) -> Vec<u8> {
    if levels.is_empty() {
        return Vec::new();
    }
    (0..WAVEFORM_LEN)
        .map(|part| {
            let from = part * levels.len() / WAVEFORM_LEN;
            let to = ((part + 1) * levels.len() / WAVEFORM_LEN).max(from + 1).min(levels.len());
            let slice = &levels[from.min(levels.len() - 1)..to];
            (slice.iter().sum::<f32>() / slice.len() as f32 * 100.0).round() as u8
        })
        .collect()
}

fn rand_serial() -> u32 {
    let mut bytes = [0; 4];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes).ok();
    u32::from_le_bytes(bytes)
}

/// RFC 7845's identification header: one channel, 48 kHz.
fn opus_head(pre_skip: u16) -> Vec<u8> {
    let mut head = b"OpusHead".to_vec();
    head.push(1);
    head.push(1);
    head.extend_from_slice(&pre_skip.to_le_bytes());
    head.extend_from_slice(&RATE.to_le_bytes());
    head.extend_from_slice(&0i16.to_le_bytes());
    head.push(0);
    head
}

fn opus_tags() -> Vec<u8> {
    let vendor = b"Omega Zap";
    let mut tags = b"OpusTags".to_vec();
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0u32.to_le_bytes());
    tags
}

const BOS: u8 = 0x02;
const EOS: u8 = 0x04;

/// Writes Ogg pages (RFC 3533), packets gathered a second to a page.
struct Ogg {
    out: Vec<u8>,
    serial: u32,
    sequence: u32,
    packets: Vec<Vec<u8>>,
    /// Where the last packet gathered ends.
    granule_before: u64,
}

impl Ogg {
    fn new(serial: u32) -> Self {
        Self { out: Vec::new(), serial, sequence: 0, packets: Vec::new(), granule_before: 0 }
    }

    /// Adds a packet that ends at `granule`; `last` closes the stream.
    fn packet(&mut self, packet: Vec<u8>, granule: u64, last: bool) {
        let lacing = |p: &Vec<u8>| p.len() / 255 + 1;
        let used: usize = self.packets.iter().map(lacing).sum();
        if !self.packets.is_empty() && used + lacing(&packet) > 255 {
            self.flush(self.granule_before, 0);
        }
        self.packets.push(packet);
        self.granule_before = granule;
        if last || self.packets.len() >= PAGE_PACKETS {
            self.flush(granule, if last { EOS } else { 0 });
        }
    }

    fn flush(&mut self, granule: u64, flags: u8) {
        let packets = std::mem::take(&mut self.packets);
        let refs: Vec<&[u8]> = packets.iter().map(Vec::as_slice).collect();
        self.page(&refs, granule, flags);
    }

    /// Writes one page holding whole `packets`.
    fn page(&mut self, packets: &[&[u8]], granule: u64, flags: u8) {
        let start = self.out.len();
        self.out.extend_from_slice(b"OggS");
        self.out.push(0);
        self.out.push(flags);
        self.out.extend_from_slice(&granule.to_le_bytes());
        self.out.extend_from_slice(&self.serial.to_le_bytes());
        self.out.extend_from_slice(&self.sequence.to_le_bytes());
        self.out.extend_from_slice(&[0; 4]);
        let mut lacing = Vec::new();
        for packet in packets {
            lacing.extend(std::iter::repeat_n(255, packet.len() / 255));
            lacing.push((packet.len() % 255) as u8);
        }
        self.out.push(lacing.len() as u8);
        self.out.extend_from_slice(&lacing);
        for packet in packets {
            self.out.extend_from_slice(packet);
        }
        let crc = crc32(&self.out[start..]);
        self.out[start + 22..start + 26].copy_from_slice(&crc.to_le_bytes());
        self.sequence += 1;
    }
}

/// Ogg's CRC: polynomial 0x04c11db7, most significant bit first, no reflection.
fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0; 256];
        for (i, entry) in table.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 { (r << 1) ^ 0x04c1_1db7 } else { r << 1 };
            }
            *entry = r;
        }
        table
    });
    data.iter()
        .fold(0, |crc, &b| (crc << 8) ^ table[(((crc >> 24) as u8) ^ b) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_keeps_the_length() {
        let mut r = Resampler::new(44_100);
        let mut out = Vec::new();
        for _ in 0..100 {
            r.run(&[0.5; 441], &mut out);
        }
        // One second in, one second out, give or take a sample.
        assert!((out.len() as i64 - 48_000).abs() <= 1, "{}", out.len());
        assert!(out.iter().skip(1).all(|s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn encodes_a_tone_into_ogg_opus() {
        let mut encoder = Encoder::new(44_100).unwrap();
        let tone: Vec<f32> = (0..44_100 * 2).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 44_100.0).sin() * 0.3).collect();
        for chunk in tone.chunks(512) {
            encoder.push(chunk);
        }
        let recording = encoder.finish().unwrap();
        assert_eq!(recording.seconds, 2);
        assert_eq!(recording.waveform.len(), WAVEFORM_LEN);
        assert!(recording.waveform.iter().all(|&v| v > 50));
        let ogg = &recording.ogg;
        assert_eq!(&ogg[..4], b"OggS");
        assert_eq!(&ogg[28..36], b"OpusHead");
        // Every page's checksum is right, and the last one ends the stream.
        let mut at = 0;
        let mut last_flags = 0;
        while at < ogg.len() {
            assert_eq!(&ogg[at..at + 4], b"OggS");
            let segments = ogg[at + 26] as usize;
            let body: usize = ogg[at + 27..at + 27 + segments].iter().map(|&l| l as usize).sum();
            let len = 27 + segments + body;
            let mut page = ogg[at..at + len].to_vec();
            let crc = u32::from_le_bytes(page[22..26].try_into().unwrap());
            page[22..26].fill(0);
            assert_eq!(crc32(&page), crc);
            last_flags = ogg[at + 5];
            at += len;
        }
        assert_eq!(last_flags & EOS, EOS);
    }

    #[test]
    fn too_short_is_refused() {
        let mut encoder = Encoder::new(48_000).unwrap();
        encoder.push(&[0.0; 4_800]);
        assert!(encoder.finish().is_err());
    }
}
