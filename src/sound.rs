const RATE: u32 = 44100;

/// Generate PCM samples for a sine tone with fade envelope.
fn tone_samples(freq: f32, duration_ms: u32) -> Vec<i16> {
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
        s *= env * 0.35;
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

/// Play record-start sound (non-blocking).
pub fn play_start() {
    #[cfg(windows)]
    {
        // Small silence pad so audio device initializes before the tone hits
        let mut pcm = silence_samples(20);
        pcm.extend_from_slice(&tone_samples(660.0, 60));  // E5
        pcm.extend_from_slice(&tone_samples(880.0, 60));  // A5
        play_wav_async(wrap_wav(&pcm));
    }
}

/// Play record-stop sound (non-blocking).
pub fn play_stop() {
    #[cfg(windows)]
    {
        let mut pcm = silence_samples(15);
        pcm.extend_from_slice(&tone_samples(520.0, 80));  // C5
        play_wav_async(wrap_wav(&pcm));
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
