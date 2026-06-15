use std::path::{Path, PathBuf};

use crate::playlist::{Playlist, Track};

pub fn export(playlist: &Playlist, m3u_path: &Path) -> std::io::Result<()> {
    let dir = m3u_path.parent().unwrap_or(Path::new("."));
    let mut lines = vec!["#EXTM3U".to_string()];
    for track in &playlist.tracks {
        let path_str = match track.path.strip_prefix(dir) {
            Ok(rel) => rel.to_string_lossy().to_string(),
            Err(_) => track.path.to_string_lossy().to_string(),
        };
        lines.push(format!("#EXTINF:-1,{}", track.name));
        lines.push(path_str);
    }
    lines.push(String::new());
    std::fs::write(m3u_path, lines.join("\n"))
}

pub fn import(m3u_path: &Path) -> Option<Playlist> {
    let dir = m3u_path.parent().unwrap_or(Path::new("."));
    let content = std::fs::read_to_string(m3u_path).ok()?;
    let name = m3u_path.file_stem()?.to_string_lossy().to_string();
    let mut tracks = Vec::new();
    let mut pending_name: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line == "#EXTM3U" {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let title = rest.splitn(2, ',').nth(1).unwrap_or("").trim().to_string();
            pending_name = Some(title);
        } else if !line.starts_with('#') {
            let raw = PathBuf::from(line);
            let path = if raw.is_absolute() { raw } else { dir.join(&raw) };
            if path.exists() {
                let track_name = pending_name
                    .take()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| {
                        path.file_stem().unwrap_or_default().to_string_lossy().to_string()
                    });
                tracks.push(Track { name: track_name, path });
            } else {
                pending_name = None;
            }
        }
    }

    let mut pl = Playlist::new(&name);
    pl.tracks = tracks;
    Some(pl)
}
