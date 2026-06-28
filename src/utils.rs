use std::path::PathBuf;
use serde::de::DeserializeOwned;
use serde::Serialize;

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