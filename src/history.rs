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
            .join("cli-fi/history.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, serde_json::to_string_pretty(self).unwrap());
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
