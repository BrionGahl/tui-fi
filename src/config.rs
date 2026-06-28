use crate::utils::{load, save};
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub volume: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self { volume: 0.75 }
    }
}

impl Config {
    fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tui-fi/config.json")
    }

    pub fn load() -> Self {
        load(&Self::path())
    }

    pub fn save(&self) {
        save(self, &Self::path());
    }
}
