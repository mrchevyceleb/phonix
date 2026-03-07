use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WhisperProvider {
    Groq,
    OpenAI,
    /// Local whisper.cpp server (default: http://localhost:8080)
    Local,
}

impl Default for WhisperProvider {
    fn default() -> Self {
        Self::Groq
    }
}

impl WhisperProvider {
    pub fn url(&self) -> &str {
        match self {
            Self::Groq => "https://api.groq.com/openai/v1",
            Self::OpenAI => "https://api.openai.com/v1",
            Self::Local => "http://localhost:8080",
        }
    }

    pub fn model(&self) -> &str {
        match self {
            Self::Groq => "whisper-large-v3",
            Self::OpenAI => "whisper-1",
            Self::Local => "whisper-1", // whisper.cpp server uses this name
        }
    }

    pub fn needs_key(&self) -> bool {
        !matches!(self, Self::Local)
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Groq => "Groq (free, fast)",
            Self::OpenAI => "OpenAI",
            Self::Local => "Local (whisper.cpp server)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Virtual key name for push-to-talk.
    /// Options: "RightAlt", "RightControl", "LeftAlt", "LeftControl",
    ///          "CapsLock", "ScrollLock", "F13"–"F16"
    pub record_key: String,

    /// Automatically paste into the active window after transcription
    pub auto_paste: bool,

    // ── Whisper (speech → text) ───────────────────────────────────────────────
    pub whisper_provider: WhisperProvider,
    /// Override URL — leave blank to use the provider default
    pub whisper_url_override: String,
    pub whisper_api_key: String,
    /// Override model — leave blank to use the provider default
    pub whisper_model_override: String,

    // ── Cleanup LLM (text → polished text) ───────────────────────────────────
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

            whisper_provider: WhisperProvider::Groq,
            whisper_url_override: String::new(),
            whisper_api_key: String::new(),
            whisper_model_override: String::new(),

            cleanup_enabled: true,
            cleanup_url: "http://localhost:1234/v1".to_string(),
            cleanup_api_key: "lm-studio".to_string(),
            cleanup_model: "local-model".to_string(),
        }
    }
}

impl Config {
    /// Resolved Whisper API URL (override wins if set)
    pub fn whisper_url(&self) -> &str {
        if !self.whisper_url_override.is_empty() {
            &self.whisper_url_override
        } else {
            self.whisper_provider.url()
        }
    }

    /// Resolved Whisper model (override wins if set)
    pub fn whisper_model(&self) -> &str {
        if !self.whisper_model_override.is_empty() {
            &self.whisper_model_override
        } else {
            self.whisper_provider.model()
        }
    }

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
