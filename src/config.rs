use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::hotkey::SUPPORTED_KEYS;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SoundPreset {
    Off,
    Ping,
    Click,
    Chime,
    Chirp,
    Blip,
}

impl Default for SoundPreset {
    fn default() -> Self {
        Self::Ping
    }
}

impl SoundPreset {
    pub fn label(&self) -> &str {
        match self {
            Self::Off => "Off",
            Self::Ping => "Ping",
            Self::Click => "Click",
            Self::Chime => "Chime",
            Self::Chirp => "Chirp",
            Self::Blip => "Blip",
        }
    }

    pub fn all() -> &'static [SoundPreset] {
        &[
            SoundPreset::Off,
            SoundPreset::Ping,
            SoundPreset::Click,
            SoundPreset::Chime,
            SoundPreset::Chirp,
            SoundPreset::Blip,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LocalModelSize {
    Tiny,
    Base,
    Small,
    Medium,
}

impl Default for LocalModelSize {
    fn default() -> Self {
        Self::Small
    }
}

impl LocalModelSize {
    pub fn label(&self) -> &str {
        match self {
            Self::Tiny => "Tiny (fastest)",
            Self::Base => "Base",
            Self::Small => "Small (recommended)",
            Self::Medium => "Medium (slow on CPU)",
        }
    }

    pub fn arg(&self) -> &str {
        match self {
            Self::Tiny => "tiny",
            Self::Base => "base",
            Self::Small => "small",
            Self::Medium => "medium",
        }
    }

    pub fn all() -> &'static [LocalModelSize] {
        &[
            LocalModelSize::Tiny,
            LocalModelSize::Base,
            LocalModelSize::Small,
            LocalModelSize::Medium,
        ]
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CleanupProvider {
    Groq,
    OpenAI,
    /// Local LM Studio or any OpenAI-compatible server
    Local,
}

impl Default for CleanupProvider {
    fn default() -> Self {
        Self::Local
    }
}

impl CleanupProvider {
    pub fn url(&self) -> &str {
        match self {
            Self::Groq => "https://api.groq.com/openai/v1",
            Self::OpenAI => "https://api.openai.com/v1",
            Self::Local => "http://localhost:1234/v1",
        }
    }

    pub fn model(&self) -> &str {
        match self {
            Self::Groq => "llama-3.1-8b-instant",
            Self::OpenAI => "gpt-4o-mini",
            Self::Local => "local-model",
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Groq => "Groq (free, fast)",
            Self::OpenAI => "OpenAI (gpt-4o-mini)",
            Self::Local => "Local (LM Studio)",
        }
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
    /// Options: "RightAlt", "LeftAlt", "RightCtrl", "LeftCtrl",
    ///          "RightShift", "LeftShift", "CapsLock", "ScrollLock", "F13"–"F16"
    pub record_key: String,

    /// Automatically paste into the active window after transcription
    pub auto_paste: bool,

    /// Sound preset for record start/stop
    #[serde(default)]
    pub sound_preset: SoundPreset,

    /// Legacy: migrated to sound_preset on load.
    /// Default must be true so that absence of this field (old configs that never
    /// wrote it) correctly indicates "sound was on" and doesn't false-migrate to Off.
    #[serde(default = "default_true", skip_serializing)]
    sound_enabled: bool,

    /// Hide to system tray instead of quitting when the window is closed
    #[serde(default = "default_true")]
    pub close_to_tray: bool,

    // ── Whisper (speech → text) ───────────────────────────────────────────────
    /// Model size for local whisper server
    #[serde(default)]
    pub local_model_size: LocalModelSize,

    pub whisper_provider: WhisperProvider,
    /// Override URL — leave blank to use the provider default
    pub whisper_url_override: String,
    pub whisper_api_key: String,
    /// Override model — leave blank to use the provider default
    pub whisper_model_override: String,

    // ── Cleanup LLM (text → polished text) ───────────────────────────────────
    pub cleanup_enabled: bool,
    #[serde(default)]
    pub cleanup_provider: CleanupProvider,
    /// Override URL — leave blank to use provider default
    #[serde(default)]
    pub cleanup_url_override: String,
    /// Separate API key for cleanup (only needed if provider differs from whisper)
    #[serde(default)]
    pub cleanup_api_key: String,
    /// Override model — leave blank to use provider default
    #[serde(default)]
    pub cleanup_model_override: String,

    // Legacy fields kept for backwards-compatible deserialization
    #[serde(default, rename = "cleanup_url", skip_serializing)]
    _cleanup_url_legacy: String,
    #[serde(default, rename = "cleanup_model", skip_serializing)]
    _cleanup_model_legacy: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            record_key: if cfg!(target_os = "macos") { "F13" } else { "RightAlt" }.to_string(),
            auto_paste: true,
            sound_preset: SoundPreset::Ping,
            sound_enabled: true, // legacy default
            close_to_tray: true,

            local_model_size: LocalModelSize::Small,
            whisper_provider: WhisperProvider::Groq,
            whisper_url_override: String::new(),
            whisper_api_key: String::new(),
            whisper_model_override: String::new(),

            cleanup_enabled: true,
            cleanup_provider: CleanupProvider::Local,
            cleanup_url_override: String::new(),
            cleanup_api_key: String::new(),
            cleanup_model_override: String::new(),
            _cleanup_url_legacy: String::new(),
            _cleanup_model_legacy: String::new(),
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

    /// Resolved cleanup API URL
    pub fn cleanup_url(&self) -> &str {
        if !self.cleanup_url_override.is_empty() {
            &self.cleanup_url_override
        } else {
            self.cleanup_provider.url()
        }
    }

    /// Resolved cleanup model
    pub fn cleanup_model(&self) -> &str {
        if !self.cleanup_model_override.is_empty() {
            &self.cleanup_model_override
        } else {
            self.cleanup_provider.model()
        }
    }

    /// Whether whisper and cleanup use the same cloud provider.
    pub fn cleanup_shares_whisper_key(&self) -> bool {
        matches!(
            (&self.cleanup_provider, &self.whisper_provider),
            (CleanupProvider::Groq, WhisperProvider::Groq)
                | (CleanupProvider::OpenAI, WhisperProvider::OpenAI)
        )
    }

    /// Resolved cleanup API key.
    /// Always reuses the whisper key when both are the same cloud provider.
    pub fn cleanup_key(&self) -> &str {
        if self.cleanup_shares_whisper_key() {
            return &self.whisper_api_key;
        }
        &self.cleanup_api_key
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
        let mut cfg: Self = toml::from_str(&content).unwrap_or_default();
        // Migrate legacy sound_enabled=false to SoundPreset::Off
        if !cfg.sound_enabled && cfg.sound_preset == SoundPreset::Ping {
            // Old config had sound disabled; no sound_preset field was saved.
            // If sound_preset is still the default (Ping), the user intended Off.
            cfg.sound_preset = SoundPreset::Off;
        }
        if !SUPPORTED_KEYS.iter().any(|&(name, _)| name == cfg.record_key) {
            let default_key = if cfg!(target_os = "macos") { "F13" } else { "RightAlt" };
            eprintln!(
                "[phonix/config] invalid record_key '{}', resetting to {}",
                cfg.record_key, default_key
            );
            cfg.record_key = default_key.to_string();
        }
        cfg
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("No config dir"))?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}
