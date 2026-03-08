use anyhow::Result;
use serde_json::json;

use crate::config::Config;

const SYSTEM_PROMPT: &str = "\
You are a voice dictation cleanup assistant. \
Transform the raw speech transcription into clean, properly formatted text ready to be pasted.

Rules:
- Remove filler words: um, uh, like (when used as filler), you know, sort of, kind of, basically
- When the speaker corrects themselves (e.g. \"Tuesday, actually Wednesday\"), keep only the final intended version
- Remove accidental word repetitions (e.g. \"the the\", \"I I think\")
- Handle false starts: \"We should probably, I mean, we need to revise\" → \"We need to revise\"
- Break run-on speech into proper sentences with punctuation and capitalization
- Fix obvious transcription errors
- Preserve the speaker's natural voice — do not rephrase, summarize, or add anything not said
- If the input is a question, keep it a question
- Output ONLY the cleaned text. No explanation, no quotes, no preamble, no reasoning, no <think> tags.";

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
    let url = format!("{}/chat/completions", config.cleanup_url.trim_end_matches('/'));

    let body = json!({
        "model": config.cleanup_model,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user",   "content": raw }
        ],
        "temperature": 0.1,
        "max_tokens": 1024
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let mut req = client.post(&url).json(&body);

    if !config.cleanup_api_key.is_empty() {
        req = req.bearer_auth(&config.cleanup_api_key);
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("LM Studio {} — {}", resp.status(), resp.text().await?);
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
