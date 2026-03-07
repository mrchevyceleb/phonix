use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Virtual key name for push-to-talk. Default: "RightAlt"
    /// Options: "RightAlt", "RightControl", "F13", "CapsLock", "ScrollLock"
    pub record_key: String,

    /// Automatically paste into the active window after transcription
    pub auto_paste: bool,

    /// Whisper-compatible endpoint (OpenAI, Groq, local whisper.cpp server, etc.)
    pub whisper_url: String,
    pub whisper_api_key: String,
    pub whisper_model: String,

    /// LM Studio (or any OpenAI-compatible) endpoint for cleanup
    pub cleanup_enabled: bool,
    pub cleanup_url: String,
    pub cleanup_api_key: String,
    pub cleanup_model: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            record_key: "RightAlt".to_string(),
            auto_paste: true,

            // Default to Groq (free, near-instant). Swap for local whisper.cpp server.
            whisper_url: "https://api.groq.com/openai/v1".to_string(),
            whisper_api_key: String::new(),
            whisper_model: "whisper-large-v3".to_string(),

            cleanup_enabled: true,
            cleanup_url: "http://localhost:1234/v1".to_string(),
            cleanup_api_key: "lm-studio".to_string(),
            // Set this to whatever model name LM Studio shows at the top of its UI
            cleanup_model: "local-model".to_string(),
        }
    }
}

impl Config {
    fn path() -> Option<PathBuf> {
        ProjectDirs::from("io", "phonix", "Phonix")
            .map(|d| d.config_dir().join("config.toml"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        if !path.exists() {
            let cfg = Self::default();
            let _ = cfg.save();
            return cfg;
        }
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        toml::from_str(&content).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("No config dir"))?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}
