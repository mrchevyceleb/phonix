use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// How many seconds of audio to keep in the pre-roll buffer.
/// When the hotkey fires, we include this pre-roll so the first
/// syllable is never clipped even if the user starts speaking
/// the instant they press the key.
const PRE_ROLL_SECS: f32 = 0.8;

/// If the audio callback hasn't fired in this many seconds, consider the
/// stream dead and reopen the mic.
const STREAM_STALE_SECS: u64 = 2;

pub struct AudioRecorder {
    stream: Option<Stream>,
    /// Rolling ring buffer — always capturing, capped at PRE_ROLL_SECS
    pre_roll: Arc<Mutex<VecDeque<f32>>>,
    /// Active recording buffer — filled from the moment RecordStart fires
    recording: Arc<Mutex<Vec<f32>>>,
    active: Arc<Mutex<bool>>,
    /// Epoch millis of the last audio callback — used to detect dead streams
    last_callback: Arc<AtomicU64>,
    pub sample_rate: u32,
    channels: usize,
}

unsafe impl Send for AudioRecorder {}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            stream: None,
            pre_roll: Arc::new(Mutex::new(VecDeque::new())),
            recording: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(Mutex::new(false)),
            last_callback: Arc::new(AtomicU64::new(0)),
            sample_rate: 44100,
            channels: 1,
        }
    }

    /// Open the mic and start the always-on pre-roll buffer.
    /// Call this once at app startup, not on every keypress.
    pub fn open(&mut self) -> Result<u32> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("No microphone found")?;

        let supported = device.default_input_config()?;
        self.sample_rate = supported.sample_rate().0;
        self.channels = supported.channels() as usize;

        let pre_roll = Arc::clone(&self.pre_roll);
        let recording = Arc::clone(&self.recording);
        let active = Arc::clone(&self.active);
        let last_cb = Arc::clone(&self.last_callback);
        let channels = self.channels;
        let sample_rate = self.sample_rate;
        let pre_roll_cap = (sample_rate as f32 * PRE_ROLL_SECS) as usize;

        let stream = device.build_input_stream(
            &supported.into(),
            move |data: &[f32], _| {
                // Stamp the callback time so the health check can detect dead streams
                last_cb.store(epoch_millis(), Ordering::Relaxed);

                // Downmix to mono
                let mono: Vec<f32> = if channels > 1 {
                    data.chunks(channels)
                        .map(|c| c.iter().sum::<f32>() / channels as f32)
                        .collect()
                } else {
                    data.to_vec()
                };

                let is_active = *active.lock().unwrap();

                if is_active {
                    // Append to active recording buffer
                    recording.lock().unwrap().extend_from_slice(&mono);
                } else {
                    // Maintain rolling pre-roll ring buffer
                    let mut pr = pre_roll.lock().unwrap();
                    pr.extend(mono.iter().copied());
                    // Trim to cap
                    while pr.len() > pre_roll_cap {
                        pr.pop_front();
                    }
                }
            },
            |e| eprintln!("[phonix/audio] stream error: {e}"),
            None,
        )?;

        stream.play()?;
        self.stream = Some(stream);
        Ok(self.sample_rate)
    }

    /// Returns true if the audio callback has fired within STREAM_STALE_SECS.
    /// A stale stream means CPAL stopped delivering data (device unplugged,
    /// driver glitch, etc.).
    pub fn is_stream_alive(&self) -> bool {
        let last = self.last_callback.load(Ordering::Relaxed);
        if last == 0 {
            // Callback has never fired. If a stream exists it's just warming up
            // (give it the benefit of the doubt). If no stream, it needs opening.
            return self.stream.is_some();
        }
        let now = epoch_millis();
        now.saturating_sub(last) < STREAM_STALE_SECS * 1000
    }

    /// Reopen the mic if the stream has gone stale. Returns the new sample rate
    /// on success, or None if the stream was still healthy.
    pub fn ensure_stream(&mut self) -> Option<u32> {
        if self.is_stream_alive() {
            return None;
        }
        eprintln!("[phonix/audio] stream stale — reopening mic");
        // Drop old stream before opening a new one
        self.stream = None;
        match self.open() {
            Ok(sr) => Some(sr),
            Err(e) => {
                eprintln!("[phonix/audio] reopen failed: {e}");
                None
            }
        }
    }

    /// Begin an active recording. Seeds the buffer with pre-roll so the
    /// first word is never clipped. Returns the number of pre-roll samples
    /// so the caller can distinguish pre-roll from real speech.
    pub fn start(&self) -> usize {
        // Snapshot pre-roll first, then clear recording and seed it.
        // This lock ordering avoids holding both locks simultaneously,
        // which would block the audio callback.
        let pre_roll_data: Vec<f32> = {
            let pr = self.pre_roll.lock().unwrap();
            pr.iter().copied().collect()
        };
        let pre_roll_len = pre_roll_data.len();

        let mut rec = self.recording.lock().unwrap();
        rec.clear();
        rec.extend_from_slice(&pre_roll_data);
        drop(rec);

        *self.active.lock().unwrap() = true;
        pre_roll_len
    }

    /// Stop the active recording and return the captured samples.
    pub fn stop(&self) -> Vec<f32> {
        *self.active.lock().unwrap() = false;
        self.recording.lock().unwrap().clone()
    }
}

/// Current time as milliseconds since the Unix epoch.
fn epoch_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
