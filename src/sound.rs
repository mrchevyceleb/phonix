use crate::config::SoundPreset;

const RATE: u32 = 44100;

/// Generate PCM samples for a sine tone with fade envelope.
fn tone_samples(freq: f32, duration_ms: u32, volume: f32) -> Vec<i16> {
    let num = (RATE * duration_ms / 1000) as usize;
    let fade = (RATE as usize * 10) / 1000; // 10ms fade
    let mut out = Vec::with_capacity(num);
    for i in 0..num {
        let t = i as f32 / RATE as f32;
        let mut s = (t * freq * 2.0 * std::f32::consts::PI).sin();
        let env = if i < fade {
            i as f32 / fade as f32
        } else if i > num - fade {
            (num - i) as f32 / fade as f32
        } else {
            1.0
        };
        s *= env * volume;
        out.push((s * 32767.0) as i16);
    }
    out
}

/// Generate a frequency sweep from start_freq to end_freq.
fn sweep_samples(start_freq: f32, end_freq: f32, duration_ms: u32, volume: f32) -> Vec<i16> {
    let num = (RATE * duration_ms / 1000) as usize;
    let fade = (RATE as usize * 5) / 1000; // 5ms fade
    let mut out = Vec::with_capacity(num);
    let mut phase: f32 = 0.0;
    for i in 0..num {
        let t = i as f32 / num as f32;
        let freq = start_freq + (end_freq - start_freq) * t;
        phase += freq / RATE as f32;
        let mut s = (phase * 2.0 * std::f32::consts::PI).sin();
        let env = if i < fade {
            i as f32 / fade as f32
        } else if i > num - fade {
            (num - i) as f32 / fade as f32
        } else {
            1.0
        };
        s *= env * volume;
        out.push((s * 32767.0) as i16);
    }
    out
}

/// Generate silent PCM samples.
fn silence_samples(duration_ms: u32) -> Vec<i16> {
    vec![0i16; (RATE * duration_ms / 1000) as usize]
}

/// Wrap raw PCM samples into a WAV byte buffer.
fn wrap_wav(pcm: &[i16]) -> Vec<u8> {
    let data_size = (pcm.len() * 2) as u32;
    let file_size = 36 + data_size;
    let mut wav = Vec::with_capacity(file_size as usize + 8);

    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&file_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());   // PCM
    wav.extend_from_slice(&1u16.to_le_bytes());   // mono
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());   // block align
    wav.extend_from_slice(&16u16.to_le_bytes());  // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    for s in pcm {
        wav.extend_from_slice(&s.to_le_bytes());
    }
    wav
}

/// Build start-recording PCM for a given preset.
fn start_pcm(preset: &SoundPreset) -> Vec<i16> {
    let mut pcm = silence_samples(80); // pad for audio device init
    match preset {
        SoundPreset::Off => return Vec::new(),
        SoundPreset::Ping => {
            // Rising two-note (original)
            pcm.extend_from_slice(&tone_samples(660.0, 60, 0.35));  // E5
            pcm.extend_from_slice(&tone_samples(880.0, 60, 0.35));  // A5
        }
        SoundPreset::Click => {
            // Ultra-short percussive tap
            pcm.extend_from_slice(&tone_samples(1200.0, 15, 0.40));
        }
        SoundPreset::Chime => {
            // Pleasant bell: two harmonics layered
            let c5 = tone_samples(523.0, 100, 0.20);
            let e5 = tone_samples(659.0, 100, 0.15);
            let mixed: Vec<i16> = c5.iter().zip(e5.iter())
                .map(|(&a, &b)| a.saturating_add(b))
                .collect();
            pcm.extend_from_slice(&mixed);
        }
        SoundPreset::Chirp => {
            // Quick ascending sweep
            pcm.extend_from_slice(&sweep_samples(400.0, 1000.0, 50, 0.35));
        }
        SoundPreset::Blip => {
            // Retro game blip
            pcm.extend_from_slice(&tone_samples(1047.0, 30, 0.30)); // C6
            pcm.extend_from_slice(&silence_samples(15));
            pcm.extend_from_slice(&tone_samples(1319.0, 30, 0.30)); // E6
        }
    }
    pcm
}

/// Build stop-recording PCM for a given preset.
fn stop_pcm(preset: &SoundPreset) -> Vec<i16> {
    let mut pcm = silence_samples(15);
    match preset {
        SoundPreset::Off => return Vec::new(),
        SoundPreset::Ping => {
            // Single descending note (original)
            pcm.extend_from_slice(&tone_samples(520.0, 80, 0.35));  // C5
        }
        SoundPreset::Click => {
            pcm.extend_from_slice(&tone_samples(800.0, 15, 0.35));
        }
        SoundPreset::Chime => {
            pcm.extend_from_slice(&tone_samples(392.0, 120, 0.25)); // G4
        }
        SoundPreset::Chirp => {
            // Quick descending sweep
            pcm.extend_from_slice(&sweep_samples(1000.0, 400.0, 50, 0.35));
        }
        SoundPreset::Blip => {
            pcm.extend_from_slice(&tone_samples(1319.0, 30, 0.30)); // E6
            pcm.extend_from_slice(&silence_samples(15));
            pcm.extend_from_slice(&tone_samples(1047.0, 30, 0.30)); // C6
        }
    }
    pcm
}

/// Play record-start sound (non-blocking).
pub fn play_start_with_preset(preset: &SoundPreset) {
    if *preset == SoundPreset::Off {
        return;
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        let pcm = start_pcm(preset);
        if !pcm.is_empty() {
            play_wav_async(wrap_wav(&pcm));
        }
    }
}

/// Play record-stop sound (non-blocking).
pub fn play_stop_with_preset(preset: &SoundPreset) {
    if *preset == SoundPreset::Off {
        return;
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        let pcm = stop_pcm(preset);
        if !pcm.is_empty() {
            play_wav_async(wrap_wav(&pcm));
        }
    }
}

/// Play a preview of the start sound (for Settings UI).
pub fn play_preview(preset: &SoundPreset) {
    if *preset == SoundPreset::Off {
        return;
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        let pcm = start_pcm(preset);
        if !pcm.is_empty() {
            play_wav_async(wrap_wav(&pcm));
        }
    }
}

#[cfg(windows)]
fn play_wav_async(wav: Vec<u8>) {
    std::thread::spawn(move || {
        use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY};
        unsafe {
            let _ = PlaySoundW(
                windows::core::PCWSTR(wav.as_ptr() as *const u16),
                None,
                SND_MEMORY | SND_ASYNC,
            );
        }
        // Keep the buffer alive while playback completes
        std::thread::sleep(std::time::Duration::from_millis(400));
    });
}

#[cfg(target_os = "macos")]
fn play_wav_async(wav: Vec<u8>) {
    std::thread::spawn(move || {
        use std::io::Cursor;
        use rodio::{Decoder, OutputStream, Sink};

        let Ok((_stream, stream_handle)) = OutputStream::try_default() else {
            return;
        };
        let Ok(sink) = Sink::try_new(&stream_handle) else {
            return;
        };
        let cursor = Cursor::new(wav);
        let Ok(source) = Decoder::new(cursor) else {
            return;
        };
        sink.append(source);
        // Keep thread alive while sound plays
        std::thread::sleep(std::time::Duration::from_millis(400));
    });
}
