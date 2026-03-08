/// Generate a short tone as a WAV byte buffer in memory.
/// `freq` in Hz, `duration_ms` length, `sample_rate` typically 44100.
/// Applies a quick fade-in/fade-out envelope so it sounds smooth.
fn generate_tone(freq: f32, duration_ms: u32, sample_rate: u32) -> Vec<u8> {
    let num_samples = (sample_rate * duration_ms / 1000) as usize;
    let fade_samples = (sample_rate as usize * 8) / 1000; // 8ms fade

    // Generate PCM samples
    let mut pcm = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let mut sample = (t * freq * 2.0 * std::f32::consts::PI).sin();

        // Fade envelope
        let env = if i < fade_samples {
            i as f32 / fade_samples as f32
        } else if i > num_samples - fade_samples {
            (num_samples - i) as f32 / fade_samples as f32
        } else {
            1.0
        };
        sample *= env * 0.35; // 35% volume

        pcm.push((sample * 32767.0) as i16);
    }

    // Build WAV in memory
    let data_size = (num_samples * 2) as u32;
    let file_size = 36 + data_size;
    let mut wav = Vec::with_capacity(file_size as usize + 8);

    // RIFF header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&file_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");

    // fmt chunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data chunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    for s in &pcm {
        wav.extend_from_slice(&s.to_le_bytes());
    }

    wav
}

/// Two-tone ascending chirp for record start.
fn start_tone() -> Vec<u8> {
    let rate = 44100u32;
    let tone1 = generate_tone(660.0, 60, rate);  // E5
    let tone2 = generate_tone(880.0, 60, rate);  // A5

    // Concatenate: splice the PCM data from tone2 onto tone1's WAV
    let samples1: usize = (rate * 60 / 1000) as usize;
    let samples2: usize = (rate * 60 / 1000) as usize;
    let total_samples = samples1 + samples2;
    let data_size = (total_samples * 2) as u32;
    let file_size = 36 + data_size;

    let mut wav = Vec::with_capacity(file_size as usize + 8);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&file_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());

    // PCM data from both tones (skip their WAV headers, take raw PCM)
    wav.extend_from_slice(&tone1[44..]);
    wav.extend_from_slice(&tone2[44..]);

    wav
}

/// Single lower tone for record stop.
fn stop_tone() -> Vec<u8> {
    generate_tone(520.0, 80, 44100) // C5, slightly longer
}

/// Play record-start sound (non-blocking).
pub fn play_start() {
    #[cfg(windows)]
    {
        let wav = start_tone();
        play_wav_async(wav);
    }
}

/// Play record-stop sound (non-blocking).
pub fn play_stop() {
    #[cfg(windows)]
    {
        let wav = stop_tone();
        play_wav_async(wav);
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
        std::thread::sleep(std::time::Duration::from_millis(300));
    });
}
