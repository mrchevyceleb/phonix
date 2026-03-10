use anyhow::{Context, Result};
use hound::{SampleFormat, WavSpec, WavWriter};
use reqwest::multipart;
use std::io::Cursor;

use crate::config::Config;

/// Send audio samples to a Whisper-compatible API and return the transcript.
/// Compatible with: OpenAI, Groq, local whisper.cpp --server, LocalAI, etc.
pub async fn transcribe(samples: Vec<f32>, sample_rate: u32, config: &Config, client: &reqwest::Client) -> Result<String> {
    if samples.is_empty() {
        return Ok(String::new());
    }

    let wav = encode_wav(samples, sample_rate)?;
    let url = format!("{}/audio/transcriptions", config.whisper_url().trim_end_matches('/'));

    let file_part = multipart::Part::bytes(wav)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;

    let form = multipart::Form::new()
        .part("file", file_part)
        .text("model", config.whisper_model().to_string())
        .text("language", "en")
        .text("response_format", "text");

    let mut req = client.post(&url).multipart(form);

    if !config.whisper_api_key.is_empty() {
        req = req.bearer_auth(&config.whisper_api_key);
    }

    let resp = req.send().await.context("Whisper API request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Whisper API {}: {}", status, body);
    }

    Ok(resp.text().await?.trim().to_string())
}

/// Encode mono f32 samples as a WAV file in memory.
fn encode_wav(samples: Vec<f32>, sample_rate: u32) -> Result<Vec<u8>> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut buf = Vec::new();
    {
        let cursor = Cursor::new(&mut buf);
        let mut writer = WavWriter::new(cursor, spec)?;
        for s in samples {
            writer.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16)?;
        }
        writer.finalize()?;
    }
    Ok(buf)
}
