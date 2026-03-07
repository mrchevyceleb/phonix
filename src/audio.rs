use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;
use std::sync::{Arc, Mutex};

pub struct AudioRecorder {
    stream: Option<Stream>,
    samples: Arc<Mutex<Vec<f32>>>,
    active: Arc<Mutex<bool>>,
    pub sample_rate: u32,
    pub channels: usize,
}

// SAFETY: cpal::Stream is not Send on all platforms, but we only access it
// from the single pipeline thread that owns AudioRecorder.
unsafe impl Send for AudioRecorder {}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            stream: None,
            samples: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(Mutex::new(false)),
            sample_rate: 44100,
            channels: 1,
        }
    }

    /// Start recording. Returns the actual sample rate.
    pub fn start(&mut self) -> Result<u32> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("No microphone found")?;

        let supported = device.default_input_config()?;
        self.sample_rate = supported.sample_rate().0;
        self.channels = supported.channels() as usize;

        let samples = Arc::clone(&self.samples);
        let active = Arc::clone(&self.active);
        let channels = self.channels;

        {
            let mut s = samples.lock().unwrap();
            s.clear();
        }
        *active.lock().unwrap() = true;

        let stream = device.build_input_stream(
            &supported.into(),
            move |data: &[f32], _| {
                if *active.lock().unwrap() {
                    let mut buf = samples.lock().unwrap();
                    if channels > 1 {
                        // Downmix to mono
                        for chunk in data.chunks(channels) {
                            buf.push(chunk.iter().sum::<f32>() / channels as f32);
                        }
                    } else {
                        buf.extend_from_slice(data);
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

    /// Stop recording and return captured mono samples.
    pub fn stop(&mut self) -> Vec<f32> {
        *self.active.lock().unwrap() = false;
        // Drop the stream to stop the audio device
        self.stream = None;
        self.samples.lock().unwrap().clone()
    }
}
