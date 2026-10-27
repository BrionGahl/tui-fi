use std::path::{Path, PathBuf};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// File name for display, with the extension dropped for audio files.
pub fn display_name(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    strip_audio_ext(&name).to_string()
}

/// Drops a trailing audio extension (".mp3", ".flac", ...) from a song name.
pub fn strip_audio_ext(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() && crate::app::is_audio(Path::new(name)) => stem,
        _ => name,
    }
}

pub fn save<T: Serialize>(value: &T, path: &PathBuf) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serde_json::to_string_pretty(value).unwrap());
}

pub fn load<T: Default + DeserializeOwned>(path: &PathBuf) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}