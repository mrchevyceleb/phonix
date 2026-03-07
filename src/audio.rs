use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// How many seconds of audio to keep in the pre-roll buffer.
/// When the hotkey fires, we include this pre-roll so the first
/// syllable is never clipped even if the user starts speaking
/// the instant they press the key.
const PRE_ROLL_SECS: f32 = 0.8;

pub struct AudioRecorder {
    stream: Option<Stream>,
    /// Rolling ring buffer — always capturing, capped at PRE_ROLL_SECS
    pre_roll: Arc<Mutex<VecDeque<f32>>>,
    /// Active recording buffer — filled from the moment RecordStart fires
    recording: Arc<Mutex<Vec<f32>>>,
    active: Arc<Mutex<bool>>,
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
        let channels = self.channels;
        let sample_rate = self.sample_rate;
        let pre_roll_cap = (sample_rate as f32 * PRE_ROLL_SECS) as usize;

        let stream = device.build_input_stream(
            &supported.into(),
            move |data: &[f32], _| {
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

    /// Begin an active recording. Seeds the buffer with pre-roll so the
    /// first word is never clipped.
    pub fn start(&self) {
        let mut rec = self.recording.lock().unwrap();
        rec.clear();
        // Seed with pre-roll audio captured before the key was pressed
        let pr = self.pre_roll.lock().unwrap();
        rec.extend(pr.iter().copied());
        drop(rec);

        *self.active.lock().unwrap() = true;
    }

    /// Stop the active recording and return the captured samples.
    pub fn stop(&self) -> Vec<f32> {
        *self.active.lock().unwrap() = false;
        self.recording.lock().unwrap().clone()
    }
}
