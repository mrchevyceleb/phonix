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
- Output ONLY the cleaned text. No explanation, no quotes, no preamble.";

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

    let client = reqwest::Client::new();
    let mut req = client.post(&url).json(&body);

    if !config.cleanup_api_key.is_empty() {
        req = req.bearer_auth(&config.cleanup_api_key);
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("LM Studio {} — {}", resp.status(), resp.text().await?);
    }

    let json: serde_json::Value = resp.json().await?;
    let text = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or(raw)
        .trim()
        .to_string();

    Ok(text)
}
