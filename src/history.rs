use crate::utils::{load, save};
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

const CAP: usize = 25;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct History {
    pub entries: Vec<HistoryEntry>,
}

impl History {
    fn path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tui-fi/history.json")
    }

    pub fn load() -> Self {
        load(&Self::path())
    }

    pub fn save(&self) {
        save(self, &Self::path());
    }

    pub fn add(&mut self, name: String, path: PathBuf) {
        // move to top if already present
        self.entries.retain(|e| e.path != path);
        self.entries.insert(0, HistoryEntry { name, path });
        // enforce cap
        self.entries.truncate(CAP);
        self.save();
    }
}
