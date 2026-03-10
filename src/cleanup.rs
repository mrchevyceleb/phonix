use anyhow::Result;
use serde_json::json;

use crate::config::{CleanupProvider, Config};

const SYSTEM_PROMPT: &str = "\
Clean up this voice dictation. Remove filler words (um, uh, like, you know), \
fix repetitions, keep only the final version when the speaker corrects themselves, \
add proper punctuation and capitalization. Preserve the speaker's voice. \
Output ONLY the cleaned text, nothing else.";

/// Result of a cleanup call, including any warning about provider fallback.
pub struct CleanupResult {
    pub text: String,
    /// If set, the local LLM failed and we fell back to another provider.
    pub warning: Option<String>,
}

/// Send raw Whisper output to the LLM for Wispr-style cleanup.
/// If the local provider fails and a Groq API key is available, falls back to Groq.
/// On total failure, returns the raw text unmodified — never blocks the pipeline.
pub async fn cleanup(raw: &str, config: &Config, client: &reqwest::Client) -> CleanupResult {
    if !config.cleanup_enabled || raw.is_empty() {
        return CleanupResult { text: raw.to_string(), warning: None };
    }

    match call_lm(raw, config, client).await {
        Ok(clean) => CleanupResult { text: clean, warning: None },
        Err(e) => {
            eprintln!("[phonix/cleanup] LLM failed: {e}");

            // If local provider failed, try falling back to Groq
            if config.cleanup_provider == CleanupProvider::Local
                && !config.whisper_api_key.is_empty()
            {
                eprintln!("[phonix/cleanup] Falling back to Groq for cleanup");
                match call_lm_with(
                    raw,
                    CleanupProvider::Groq.url(),
                    CleanupProvider::Groq.model(),
                    &config.whisper_api_key,
                    30,
                    client,
                ).await {
                    Ok(clean) => CleanupResult {
                        text: clean,
                        warning: Some("No local model loaded, cleaned up with Groq".into()),
                    },
                    Err(e2) => {
                        eprintln!("[phonix/cleanup] Groq fallback also failed: {e2}");
                        CleanupResult {
                            text: raw.to_string(),
                            warning: Some("Cleanup failed (local + Groq), using raw text".into()),
                        }
                    }
                }
            } else {
                CleanupResult { text: raw.to_string(), warning: None }
            }
        }
    }
}

async fn call_lm(raw: &str, config: &Config, client: &reqwest::Client) -> Result<String> {
    // Use a short timeout for local providers (should respond in <5s if model is loaded)
    let timeout = if config.cleanup_provider == CleanupProvider::Local { 5 } else { 30 };
    call_lm_with(raw, config.cleanup_url(), config.cleanup_model(), config.cleanup_key(), timeout, client).await
}

async fn call_lm_with(raw: &str, base_url: &str, model: &str, api_key: &str, timeout_secs: u64, client: &reqwest::Client) -> Result<String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user",   "content": raw }
        ],
        "temperature": 0.1,
        "max_tokens": 512
    });

    let mut req = client.post(&url).json(&body).timeout(std::time::Duration::from_secs(timeout_secs));

    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
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
