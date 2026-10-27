use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::history::History;
use crate::player::TrackInfo;
use crate::playlist::{Playlist, Track};
use crate::utils::display_name;

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
    pub fields: [String; TagEditor::COUNT], // [title, artist, album]
    pub active: usize,
}

impl TagEditor {
    const COUNT: usize = 3;
    pub const LABELS: [&'static str; Self::COUNT] = ["Title", "Artist", "Album"];
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

/// Where the currently playing track came from, so next/prev/repeat know
/// which set of tracks to advance through.
#[derive(Debug, Clone, PartialEq)]
pub enum NowPlaying {
    Playlist(usize, usize),
    Browser(PathBuf),
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
        if let Some(path) = self.entries.get(self.selected).cloned()
            && path.is_dir()
        {
            self.navigate_to(path);
        }
    }

    pub fn go_up(&mut self) {
        if let Some(parent) = self.current_dir.parent().map(|p| p.to_path_buf()) {
            self.navigate_to(parent);
        }
    }

    fn navigate_to(&mut self, dir: PathBuf) {
        self.current_dir = dir;
        self.entries = list_audio_and_dirs(&self.current_dir);
        self.selected = 0;
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
    pub show_help: bool,
    pub missing_tracks: HashSet<PathBuf>,
    last_missing_check: Option<Instant>,
    pub history_selected: usize,
    /// Accumulated play time for the current track (resets on track change).
    pub play_elapsed: std::time::Duration,
    /// Path of the track already logged to history this play session.
    pub history_logged_for: Option<PathBuf>,
    /// Path that was playing on the previous tick (used to detect track changes).
    pub last_playing: Option<PathBuf>,
    /// Where the currently playing track came from, for next/prev/repeat.
    pub playing_track: Option<NowPlaying>,
    /// Tick counter driving the visualizer animation (increments only while playing and not paused).
    pub viz_tick: u64,
    /// Path pending confirmation for permanent deletion from disk.
    pub delete_confirm: Option<PathBuf>,
}

impl App {
    pub fn new() -> Self {
        let start_dir = Some(Self::music_dir()).filter(|d| d.is_dir())
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        let mut playlists = Playlist::load_all();
        if playlists.is_empty() {
            playlists.push(Playlist::new("Default"));
        }
        Self {
            browser: BrowserState::new(start_dir),
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
            show_help: false,
            missing_tracks: HashSet::new(),
            last_missing_check: None,
            history_selected: 0,
            play_elapsed: std::time::Duration::ZERO,
            history_logged_for: None,
            last_playing: None,
            playing_track: None,
            viz_tick: 0,
            delete_confirm: None,
        }
    }

    /// The file highlighted in whichever panel has focus.
    pub fn highlighted_track(&self) -> Option<PathBuf> {
        match self.focused_panel {
            Panel::Browser => self.browser.selected_path().cloned(),
            Panel::Playlist => {
                let pl = self.current_playlist();
                pl.tracks.get(pl.selected).map(|t| t.path.clone())
            }
        }
    }

    /// Names of every playlist that contains `path`.
    pub fn playlists_containing(&self, path: &Path) -> Vec<&str> {
        self.playlists.iter()
            .filter(|pl| pl.tracks.iter().any(|t| t.path == path))
            .map(|pl| pl.name.as_str())
            .collect()
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
            .filter(|(_, p)| display_name(p).to_lowercase().contains(&q))
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
            pl.save();
        }
    }

    pub fn move_track_down(&mut self) {
        let pl = self.current_playlist_mut();
        if !pl.tracks.is_empty() && pl.selected + 1 < pl.tracks.len() {
            pl.tracks.swap(pl.selected, pl.selected + 1);
            pl.selected += 1;
            pl.save();
        }
    }


    // --- Repeat ---

    /// Call each tick with the tick duration. Logs to history after 30s of play.
    pub fn tick_play_time(&mut self, delta: std::time::Duration, now_playing: Option<&PathBuf>, paused: bool) {
        let Some(path) = now_playing else {
            self.play_elapsed = std::time::Duration::ZERO;
            self.history_logged_for = None;
            self.last_playing = None;
            return;
        };

        // reset accumulator when track changes
        if self.last_playing.as_ref() != Some(path) {
            self.play_elapsed = std::time::Duration::ZERO;
        }
        self.last_playing = Some(path.clone());

        if !paused {
            self.play_elapsed += delta;
        }

        if self.play_elapsed.as_secs() >= 30
            && self.history_logged_for.as_ref() != Some(path)
        {
            let name = display_name(path);
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
                let playlist = self.current_playlist_mut();
                if playlist.tracks.iter().any(|t| t.path == path) {
                    let pname = playlist.name.clone();
                    self.set_status(format!("Already in {}", pname));
                    return;
                }
                let name = display_name(&path);
                let pl = self.current_playlist_mut();
                pl.tracks.push(Track { name, path });
                let pname = pl.name.clone();
                pl.save();
                self.set_status(format!("Added to {}", pname));
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
            pl.save();
        }
    }

    /// Stage the browser's selected file for deletion; actual removal
    /// happens after the user confirms via `confirm_delete_file`.
    pub fn request_delete_selected(&mut self) {
        if let Some(path) = self.browser.selected_path().cloned() {
            if is_audio(&path) {
                self.delete_confirm = Some(path);
            } else {
                self.set_status("Not an audio file");
            }
        }
    }

    /// Stage the current playlist's selected track's underlying file for deletion.
    pub fn request_delete_selected_playlist_track(&mut self) {
        if let Some(track) = self.current_playlist().tracks.get(self.current_playlist().selected) {
            self.delete_confirm = Some(track.path.clone());
        }
    }

    pub fn cancel_delete(&mut self) {
        self.delete_confirm = None;
    }

    /// Permanently deletes the staged file from disk and removes it from every
    /// playlist that references it. Returns the deleted path on success.
    pub fn confirm_delete_file(&mut self) -> Option<PathBuf> {
        let path = self.delete_confirm.take()?;
        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.remove_track_from_all_playlists(&path);
                self.browser.refresh();
                self.set_status(format!("Deleted {}", display_name(&path)));
                Some(path)
            }
            Err(e) => {
                self.set_status(format!("Delete failed: {}", e));
                None
            }
        }
    }

    fn remove_track_from_all_playlists(&mut self, path: &Path) {
        for pl in &mut self.playlists {
            let before = pl.tracks.len();
            pl.tracks.retain(|t| t.path != path);
            if pl.tracks.len() != before {
                pl.selected = pl.selected.min(pl.tracks.len().saturating_sub(1));
                pl.save();
            }
        }
    }

    pub fn new_playlist(&mut self, name: &str) {
        self.playlists.push(Playlist::new(name));
        self.active_playlist = self.playlists.len() - 1;
        self.current_playlist().save();
    }

    pub fn next_playlist(&mut self) {
        self.active_playlist = (self.active_playlist + 1) % self.playlists.len();
    }

    /// The OS music folder (~/Music on Linux/macOS, the Music library on Windows),
    /// honoring XDG_MUSIC_DIR when it's set.
    pub fn music_dir() -> PathBuf {
        dirs::audio_dir().unwrap_or_else(|| {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")).join("Music")
        })
    }

    pub fn download_dir() -> PathBuf {
        Self::music_dir().join("tui-fi")
    }

    pub fn import_m3u(&mut self, path: &Path) -> Option<usize> {
        let pl = crate::m3u::import(path)?;
        let count = pl.tracks.len();
        pl.save();
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
        // Spotify audio is DRM-protected, so spotdl takes the tags and art from Spotify
        // and downloads the matching audio from YouTube Music.
        let (tool, args): (&str, Vec<String>) = if is_spotify_url(url) {
            ("spotdl", vec![
                "download".into(), url.into(),
                "--format".into(), "mp3".into(),
                "--bitrate".into(), "auto".into(),
                "--overwrite".into(), "skip".into(),
                "--output".into(), format!("{}/{{artists}} - {{title}}.{{output-ext}}", dir.display()),
            ])
        } else {
            ("yt-dlp", vec![
                "-x".into(),
                "--audio-format".into(), "mp3".into(),
                "--audio-quality".into(), "0".into(),
                "--embed-metadata".into(),
                "--embed-thumbnail".into(),
                "--convert-thumbnails".into(), "jpg".into(),
                "-o".into(), format!("{}/%(title)s.%(ext)s", dir.display()),
                url.into(),
            ])
        };
        match std::process::Command::new(tool)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                self.download_child = Some(child);
                self.status_msg = format!("Downloading via {}...", tool); // no expiry — stays until done
            }
            Err(_) => {
                self.set_status(format!("Error: {} not found", tool));
            }
        }
    }

    pub fn poll_download(&mut self) {
        let status = match &mut self.download_child {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => status,
                _ => return,
            },
            None => return,
        };
        self.download_child = None;
        if status.success() {
            self.set_status("Download complete");
        } else {
            self.set_status("Download failed");
        }
        if self.browser.current_dir == Self::download_dir() {
            self.browser.refresh();
        }
    }

    pub fn is_downloading(&self) -> bool {
        self.download_child.is_some()
    }

    /// Records which track is playing and returns its path.
    pub fn play_playlist_track(&mut self, pl_idx: usize, tr_idx: usize) -> Option<PathBuf> {
        let path = self.playlists.get(pl_idx)?.tracks.get(tr_idx)?.path.clone();
        self.playlists[pl_idx].selected = tr_idx;
        self.playing_track = Some(NowPlaying::Playlist(pl_idx, tr_idx));
        Some(path)
    }

    /// Records that a track played directly from the file browser (not a playlist).
    pub fn play_browser_track(&mut self, path: PathBuf) {
        self.playing_track = Some(NowPlaying::Browser(path));
    }

    /// Returns the path of the next/previous track to play, updating state.
    /// `dir`: 1 for next, -1 for previous. Tracks whose files no longer exist are skipped.
    pub fn advance_track(&mut self, dir: i32) -> Option<PathBuf> {
        match self.playing_track.clone()? {
            NowPlaying::Playlist(pl_idx, tr_idx) => {
                let tracks = &self.playlists.get(pl_idx)?.tracks;
                let next = pick_next(tracks.len(), tr_idx, dir, self.repeat, self.shuffle, |i| {
                    tracks[i].path.exists()
                })?;
                self.play_playlist_track(pl_idx, next)
            }
            NowPlaying::Browser(path) => {
                if self.repeat == RepeatMode::One {
                    return Some(path).filter(|p| p.exists());
                }
                let dir_path = path.parent()?.to_path_buf();
                let mut siblings: Vec<PathBuf> = list_audio_and_dirs(&dir_path)
                    .into_iter()
                    .filter(|p| is_audio(p))
                    .collect();
                siblings.sort();
                let cur_idx = siblings.iter().position(|p| p == &path)?;
                let next_idx = pick_next(siblings.len(), cur_idx, dir, self.repeat, self.shuffle, |i| {
                    siblings[i].exists()
                })?;
                let next_path = siblings[next_idx].clone();
                self.playing_track = Some(NowPlaying::Browser(next_path.clone()));
                Some(next_path)
            }
        }
    }

    /// Re-scan playlists for tracks whose files were deleted outside the app.
    pub fn tick_missing(&mut self) {
        if self.last_missing_check.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        self.last_missing_check = Some(Instant::now());
        self.missing_tracks = self.playlists.iter()
            .flat_map(|pl| &pl.tracks)
            .filter(|t| !t.path.exists())
            .map(|t| t.path.clone())
            .collect();
    }

    /// Drop tracks whose files no longer exist from the current playlist.
    pub fn prune_missing(&mut self) -> usize {
        let pl = self.current_playlist_mut();
        let before = pl.tracks.len();
        pl.tracks.retain(|t| t.path.exists());
        let removed = before - pl.tracks.len();
        if removed > 0 {
            pl.selected = pl.selected.min(pl.tracks.len().saturating_sub(1));
            pl.save();
        }
        self.last_missing_check = None;
        removed
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_msg = msg.into();
        self.status_msg_time = Some(Instant::now());
    }

    pub fn tick_status(&mut self) {
        if let Some(t) = self.status_msg_time
            && t.elapsed().as_secs() >= 2
        {
            self.status_msg.clear();
            self.status_msg_time = None;
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

fn is_spotify_url(url: &str) -> bool {
    url.contains("open.spotify.com/") || url.starts_with("spotify:")
}

pub fn is_audio(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(),
        Some("mp3" | "flac" | "ogg" | "wav" | "aac" | "m4a" | "opus")
    )
}

/// Picks the next index to play, skipping entries where `playable` is false.
/// Tries each candidate at most once, so it can't loop forever when nothing is playable.
fn pick_next(
    len: usize,
    cur: usize,
    dir: i32,
    repeat: RepeatMode,
    shuffle: bool,
    playable: impl Fn(usize) -> bool,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if repeat == RepeatMode::One {
        return Some(cur).filter(|&i| playable(i));
    }
    if shuffle {
        use rand::seq::SliceRandom;
        let live: Vec<usize> = (0..len).filter(|&i| playable(i)).collect();
        return live.choose(&mut rand::thread_rng()).copied();
    }
    (1..=len as i32)
        .map(|step| cur as i32 + dir * step)
        .take_while(|&c| repeat != RepeatMode::Off || (0..len as i32).contains(&c))
        .map(|c| c.rem_euclid(len as i32) as usize)
        .find(|&i| playable(i))
}

#[cfg(test)]
mod tests {
    use super::{RepeatMode, pick_next};

    #[test]
    fn skips_missing_tracks() {
        let live = |i: usize| i != 1 && i != 2;
        assert_eq!(pick_next(4, 0, 1, RepeatMode::Off, false, live), Some(3));
        assert_eq!(pick_next(4, 3, -1, RepeatMode::Off, false, live), Some(0));
    }

    #[test]
    fn stops_at_end_without_repeat() {
        assert_eq!(pick_next(3, 1, 1, RepeatMode::Off, false, |i| i != 2), None);
    }

    #[test]
    fn wraps_with_repeat_all() {
        assert_eq!(pick_next(3, 1, 1, RepeatMode::All, false, |i| i != 2), Some(0));
    }

    #[test]
    fn gives_up_when_nothing_playable() {
        assert_eq!(pick_next(5, 0, 1, RepeatMode::All, false, |_| false), None);
        assert_eq!(pick_next(5, 0, 1, RepeatMode::All, true, |_| false), None);
        assert_eq!(pick_next(5, 2, 1, RepeatMode::One, false, |_| false), None);
    }

    #[test]
    fn shuffle_only_picks_playable() {
        for _ in 0..50 {
            assert_eq!(pick_next(5, 0, 1, RepeatMode::Off, true, |i| i == 3), Some(3));
        }
    }
}
