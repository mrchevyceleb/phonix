use anyhow::Result;
use chrono::{DateTime, Local};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// LLM-cleaned text
    pub text: String,
    /// Raw Whisper output (before cleanup)
    pub raw: String,
    pub timestamp: DateTime<Local>,
}

impl Entry {
    pub fn new(text: String, raw: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            text,
            raw,
            timestamp: Local::now(),
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Store {
    pub entries: Vec<Entry>,
}

impl Store {
    fn path() -> Option<PathBuf> {
        ProjectDirs::from("io", "phonix", "Phonix")
            .map(|d| d.data_dir().join("history.json"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        if !path.exists() {
            return Self::default();
        }
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("No data dir"))?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn push(&mut self, entry: Entry) {
        self.entries.insert(0, entry); // newest first
        self.entries.truncate(500);
        let _ = self.save();
    }

    pub fn remove(&mut self, id: &str) {
        self.entries.retain(|e| e.id != id);
        let _ = self.save();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        let _ = self.save();
    }
}
