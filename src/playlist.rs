use crate::utils::{save};
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<Track>,
    #[serde(skip)]
    pub selected: usize,
}

impl Playlist {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), tracks: Vec::new(), selected: 0 }
    }

    fn data_dir() -> PathBuf {
        dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("tui-fi/playlists")
    }

    fn path_for(name: &str) -> PathBuf {
        Self::data_dir().join(format!("{}.json", name))
    }

    pub fn save(&self) {
        save(&self, &Self::path_for(&self.name));
    }

    pub fn load_all() -> Vec<Self> {
        let dir = Self::data_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
        let mut playlists: Vec<Self> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .filter_map(|e| {
                std::fs::read_to_string(e.path()).ok().and_then(|s| serde_json::from_str(&s).ok())
            })
            .collect();
        playlists.sort_by(|a, b| a.name.cmp(&b.name));
        playlists
    }

}
