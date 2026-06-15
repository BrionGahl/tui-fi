use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::history::History;
use crate::player::TrackInfo;
use crate::playlist::{Playlist, Track};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Panel {
    Browser,
    Playlist,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Normal,
    Naming,
    UrlInput,
    Searching,
    TagEditing,
}

pub struct TagEditor {
    pub path: PathBuf,
    pub fields: [String; 3], // [title, artist, album]
    pub active: usize,
}

impl TagEditor {
    pub const LABELS: [&'static str; 3] = ["Title", "Artist", "Album"];
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

pub struct BrowserState {
    pub current_dir: PathBuf,
    pub entries: Vec<PathBuf>,
    pub selected: usize,
}

impl BrowserState {
    pub fn new(dir: PathBuf) -> Self {
        let entries = list_audio_and_dirs(&dir);
        Self { current_dir: dir, entries, selected: 0 }
    }

    pub fn refresh(&mut self) {
        self.entries = list_audio_and_dirs(&self.current_dir);
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    pub fn enter_selected(&mut self) {
        if let Some(path) = self.entries.get(self.selected).cloned() {
            if path.is_dir() {
                self.current_dir = path;
                self.entries = list_audio_and_dirs(&self.current_dir);
                self.selected = 0;
            }
        }
    }

    pub fn go_up(&mut self) {
        if let Some(parent) = self.current_dir.parent().map(|p| p.to_path_buf()) {
            self.current_dir = parent;
            self.entries = list_audio_and_dirs(&self.current_dir);
            self.selected = 0;
        }
    }

    pub fn selected_path(&self) -> Option<&PathBuf> {
        self.entries.get(self.selected)
    }
}

pub struct App {
    pub browser: BrowserState,
    pub playlists: Vec<Playlist>,
    pub active_playlist: usize,
    pub focused_panel: Panel,
    pub mode: Mode,
    pub input_buffer: String,
    pub status_msg: String,
    pub status_msg_time: Option<Instant>,
    pub should_quit: bool,
    pub download_child: Option<std::process::Child>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub tag_editor: Option<TagEditor>,
    pub history: History,
    pub show_history: bool,
    pub history_selected: usize,
    /// Accumulated play time for the current track (resets on track change).
    pub play_elapsed: std::time::Duration,
    /// Path of the track already logged to history this play session.
    pub history_logged_for: Option<PathBuf>,
    /// (playlist_index, track_index) of the currently playing track
    pub playing_track: Option<(usize, usize)>,
}

impl App {
    pub fn new() -> Self {
        let home = dirs_home();
        let mut playlists = Playlist::load_all();
        if playlists.is_empty() {
            playlists.push(Playlist::new("Default"));
        }
        Self {
            browser: BrowserState::new(home),
            playlists,
            active_playlist: 0,
            focused_panel: Panel::Browser,
            mode: Mode::Normal,
            input_buffer: String::new(),
            status_msg: String::new(),
            status_msg_time: None,
            should_quit: false,
            download_child: None,
            shuffle: false,
            repeat: RepeatMode::Off,
            tag_editor: None,
            history: History::load(),
            show_history: false,
            history_selected: 0,
            play_elapsed: std::time::Duration::ZERO,
            history_logged_for: None,
            playing_track: None,
        }
    }

    pub fn current_playlist(&self) -> &Playlist {
        &self.playlists[self.active_playlist]
    }

    pub fn current_playlist_mut(&mut self) -> &mut Playlist {
        &mut self.playlists[self.active_playlist]
    }

    // --- Search / filtered navigation ---

    /// Indices into browser.entries that match the active search query.
    pub fn browser_visible_indices(&self) -> Vec<usize> {
        let q = self.active_search(Panel::Browser);
        if q.is_empty() {
            return (0..self.browser.entries.len()).collect();
        }
        self.browser.entries.iter().enumerate()
            .filter(|(_, p)| p.file_name().and_then(|n| n.to_str())
                .map(|n| n.to_lowercase().contains(&q)).unwrap_or(false))
            .map(|(i, _)| i)
            .collect()
    }

    /// Indices into current playlist tracks that match the active search query.
    pub fn playlist_visible_indices(&self) -> Vec<usize> {
        let q = self.active_search(Panel::Playlist);
        if q.is_empty() {
            return (0..self.current_playlist().tracks.len()).collect();
        }
        self.current_playlist().tracks.iter().enumerate()
            .filter(|(_, t)| t.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    fn active_search(&self, panel: Panel) -> String {
        if self.mode == Mode::Searching && self.focused_panel == panel {
            self.input_buffer.to_lowercase()
        } else {
            String::new()
        }
    }

    /// Navigate browser within filtered results. delta: +1 down, -1 up.
    pub fn browser_nav(&mut self, delta: i32) {
        let visible = self.browser_visible_indices();
        if visible.is_empty() { return; }
        let pos = visible.iter().position(|&i| i == self.browser.selected).unwrap_or(0);
        let new_pos = (pos as i32 + delta).rem_euclid(visible.len() as i32) as usize;
        self.browser.selected = visible[new_pos];
    }

    /// Navigate playlist within filtered results.
    pub fn playlist_nav(&mut self, delta: i32) {
        let visible = self.playlist_visible_indices();
        if visible.is_empty() { return; }
        let sel = self.current_playlist().selected;
        let pos = visible.iter().position(|&i| i == sel).unwrap_or(0);
        let new_pos = (pos as i32 + delta).rem_euclid(visible.len() as i32) as usize;
        self.current_playlist_mut().selected = visible[new_pos];
    }

    /// Snap browser cursor to first visible entry if current is filtered out.
    pub fn snap_browser_to_visible(&mut self) {
        let visible = self.browser_visible_indices();
        if !visible.is_empty() && !visible.contains(&self.browser.selected) {
            self.browser.selected = visible[0];
        }
    }

    /// Snap playlist cursor to first visible entry if current is filtered out.
    pub fn snap_playlist_to_visible(&mut self) {
        let visible = self.playlist_visible_indices();
        let sel = self.current_playlist().selected;
        if !visible.is_empty() && !visible.contains(&sel) {
            self.current_playlist_mut().selected = visible[0];
        }
    }

    // --- Reorder ---

    pub fn move_track_up(&mut self) {
        let pl = self.current_playlist_mut();
        if pl.selected > 0 && pl.tracks.len() > 1 {
            pl.tracks.swap(pl.selected - 1, pl.selected);
            pl.selected -= 1;
            let _ = pl.save();
        }
    }

    pub fn move_track_down(&mut self) {
        let pl = self.current_playlist_mut();
        if !pl.tracks.is_empty() && pl.selected + 1 < pl.tracks.len() {
            pl.tracks.swap(pl.selected, pl.selected + 1);
            pl.selected += 1;
            let _ = pl.save();
        }
    }


    // --- Repeat ---

    /// Call each tick with the tick duration. Logs to history after 30s of play.
    pub fn tick_play_time(&mut self, delta: std::time::Duration, now_playing: Option<&PathBuf>, paused: bool) {
        let Some(path) = now_playing else {
            self.play_elapsed = std::time::Duration::ZERO;
            self.history_logged_for = None;
            return;
        };

        // reset accumulator when track changes
        if self.history_logged_for.as_ref() != Some(path)
            && self.play_elapsed > std::time::Duration::ZERO
        {
            self.play_elapsed = std::time::Duration::ZERO;
        }

        if !paused {
            self.play_elapsed += delta;
        }

        if self.play_elapsed.as_secs() >= 30
            && self.history_logged_for.as_ref() != Some(path)
        {
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            self.history.add(name, path.clone());
            self.history_logged_for = Some(path.clone());
        }
    }

    pub fn open_tag_editor(&mut self, path: PathBuf, existing: Option<&TrackInfo>) {
        self.tag_editor = Some(TagEditor {
            fields: [
                existing.and_then(|t| t.title.clone()).unwrap_or_default(),
                existing.and_then(|t| t.artist.clone()).unwrap_or_default(),
                existing.and_then(|t| t.album.clone()).unwrap_or_default(),
            ],
            path,
            active: 0,
        });
        self.mode = Mode::TagEditing;
    }

    pub fn toggle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
        self.set_status(match self.repeat {
            RepeatMode::Off => "Repeat: off",
            RepeatMode::All => "Repeat: all",
            RepeatMode::One => "Repeat: one",
        });
    }

    pub fn add_selected_to_playlist(&mut self) {
        if let Some(path) = self.browser.selected_path().cloned() {
            if is_audio(&path) {
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                self.current_playlist_mut().tracks.push(Track { name, path });
                let pname = self.current_playlist().name.clone();
                self.set_status(format!("Added to {}", pname));
                let _ = self.current_playlist().save();
            } else {
                self.set_status("Not an audio file");
            }
        }
    }

    pub fn remove_selected_from_playlist(&mut self) {
        let pl = self.current_playlist_mut();
        if !pl.tracks.is_empty() && pl.selected < pl.tracks.len() {
            pl.tracks.remove(pl.selected);
            pl.selected = pl.selected.min(pl.tracks.len().saturating_sub(1));
            let _ = pl.save();
        }
    }

    pub fn new_playlist(&mut self, name: &str) {
        self.playlists.push(Playlist::new(name));
        self.active_playlist = self.playlists.len() - 1;
        let _ = self.current_playlist().save();
    }

    pub fn next_playlist(&mut self) {
        self.active_playlist = (self.active_playlist + 1) % self.playlists.len();
    }

    pub fn download_dir() -> PathBuf {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")).join("Music/cli-fi")
    }

    pub fn import_m3u(&mut self, path: &PathBuf) -> Option<usize> {
        let pl = crate::m3u::import(path)?;
        let count = pl.tracks.len();
        let _ = pl.save();
        self.playlists.push(pl);
        self.active_playlist = self.playlists.len() - 1;
        Some(count)
    }

    pub fn export_m3u(&self) -> std::io::Result<PathBuf> {
        let pl = self.current_playlist();
        let dir = Self::download_dir();
        std::fs::create_dir_all(&dir)?;
        let out = dir.join(format!("{}.m3u", pl.name));
        crate::m3u::export(pl, &out)?;
        Ok(out)
    }

    pub fn start_download(&mut self, url: &str) {
        let dir = Self::download_dir();
        let _ = std::fs::create_dir_all(&dir);
        match std::process::Command::new("yt-dlp")
            .args([
                "-x",
                "--audio-format", "mp3",
                "--audio-quality", "0",
                "-o", &format!("{}/%(title)s.%(ext)s", dir.display()),
                url,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                self.download_child = Some(child);
                self.status_msg = "Downloading...".to_string(); // no expiry — stays until done
            }
            Err(_) => {
                self.set_status("Error: yt-dlp not found");
            }
        }
    }

    pub fn poll_download(&mut self) {
        let finished = if let Some(child) = &mut self.download_child {
            matches!(child.try_wait(), Ok(Some(_)))
        } else {
            false
        };
        if finished {
            self.download_child = None;
            self.set_status("Download complete");
            if self.browser.current_dir == Self::download_dir() {
                self.browser.refresh();
            }
        }
    }

    pub fn is_downloading(&self) -> bool {
        self.download_child.is_some()
    }

    /// Records which track is playing and returns its path.
    pub fn play_playlist_track(&mut self, pl_idx: usize, tr_idx: usize) -> Option<PathBuf> {
        let path = self.playlists.get(pl_idx)?.tracks.get(tr_idx)?.path.clone();
        self.playlists[pl_idx].selected = tr_idx;
        self.playing_track = Some((pl_idx, tr_idx));
        Some(path)
    }

    /// Returns the path of the next/previous track to play, updating state.
    /// `dir`: 1 for next, -1 for previous.
    pub fn advance_track(&mut self, dir: i32) -> Option<PathBuf> {
        let (pl_idx, tr_idx) = self.playing_track?;
        let len = self.playlists.get(pl_idx)?.tracks.len();
        if len == 0 { return None; }
        let next = if self.repeat == RepeatMode::One {
            tr_idx
        } else if self.shuffle {
            use rand::Rng;
            rand::thread_rng().gen_range(0..len)
        } else {
            let candidate = tr_idx as i32 + dir;
            if self.repeat == RepeatMode::Off && (candidate < 0 || candidate >= len as i32) {
                return None;
            }
            candidate.rem_euclid(len as i32) as usize
        };
        self.play_playlist_track(pl_idx, next)
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_msg = msg.into();
        self.status_msg_time = Some(Instant::now());
    }

    pub fn tick_status(&mut self) {
        if let Some(t) = self.status_msg_time {
            if t.elapsed().as_secs() >= 2 {
                self.status_msg.clear();
                self.status_msg_time = None;
            }
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.set_status(if self.shuffle { "Shuffle on" } else { "Shuffle off" });
    }

    pub fn prev_playlist(&mut self) {
        if self.active_playlist == 0 {
            self.active_playlist = self.playlists.len() - 1;
        } else {
            self.active_playlist -= 1;
        }
    }
}

fn list_audio_and_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| !is_hidden(p) && (p.is_dir() || is_audio(p) || is_m3u(p)))
        .collect();
    entries.sort_by(|a, b| {
        let a_dir = a.is_dir();
        let b_dir = b.is_dir();
        b_dir.cmp(&a_dir).then(a.cmp(b))
    });
    entries
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.'))
        .unwrap_or(false)
}

pub fn is_m3u(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("m3u"))
        .unwrap_or(false)
}

pub fn is_audio(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(),
        Some("mp3" | "flac" | "ogg" | "wav" | "aac" | "m4a" | "opus")
    )
}

fn dirs_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}
