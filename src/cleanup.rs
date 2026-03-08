use anyhow::Result;
use serde_json::json;

use crate::config::Config;

const SYSTEM_PROMPT: &str = "\
Clean up this voice dictation. Remove filler words (um, uh, like, you know), \
fix repetitions, keep only the final version when the speaker corrects themselves, \
add proper punctuation and capitalization. Preserve the speaker's voice. \
Output ONLY the cleaned text, nothing else.";

/// Send raw Whisper output to the LLM for Wispr-style cleanup.
/// On failure, returns the raw text unmodified — never blocks the pipeline.
pub async fn cleanup(raw: &str, config: &Config) -> String {
    if !config.cleanup_enabled || raw.is_empty() {
        return raw.to_string();
    }

    match call_lm(raw, config).await {
        Ok(clean) => clean,
        Err(e) => {
            eprintln!("[phonix/cleanup] LLM failed, using raw: {e}");
            raw.to_string()
        }
    }
}

async fn call_lm(raw: &str, config: &Config) -> Result<String> {
    let url = format!("{}/chat/completions", config.cleanup_url().trim_end_matches('/'));

    let body = json!({
        "model": config.cleanup_model(),
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user",   "content": raw }
        ],
        "temperature": 0.1,
        "max_tokens": 512
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let mut req = client.post(&url).json(&body);

    let key = config.cleanup_key();
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("Cleanup API {} — {}", resp.status(), resp.text().await?);
    }

    let json: serde_json::Value = resp.json().await?;
    let mut text = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or(raw)
        .trim()
        .to_string();

    // Strip <think>...</think> blocks from reasoning models (DeepSeek, etc.)
    if let Some(end) = text.find("</think>") {
        text = text[end + 8..].trim().to_string();
    } else if text.contains("<think>") {
        // Truncated thinking (hit token limit) — discard, use raw
        text = raw.to_string();
    }

    // Sanity check: cleanup should never make text significantly longer.
    // If the output is >3x the input length, the model is reasoning/explaining
    // instead of cleaning. Fall back to raw.
    if text.len() > raw.len() * 3 + 50 {
        eprintln!("[phonix/cleanup] response too long ({}B vs {}B input), using raw", text.len(), raw.len());
        text = raw.to_string();
    }

    // If the model returned nothing useful, fall back to raw
    if text.is_empty() {
        text = raw.to_string();
    }

    Ok(text)
}
