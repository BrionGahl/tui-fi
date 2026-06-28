use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use lofty::config::WriteOptions;
use lofty::prelude::{Accessor, TagExt, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Tag, TagType};

pub struct TrackInfo {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

pub struct Player {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    sink: Option<Sink>,
    pub now_playing: Option<PathBuf>,
    pub track_info: Option<TrackInfo>,
    pub paused: bool,
    pub total_duration: Option<Duration>,
    pub volume: f32,
}

impl Player {
    pub fn new(volume: f32) -> Option<Self> {
        let (stream, handle) = OutputStream::try_default().ok()?;
        Some(Self { _stream: stream, handle, sink: None, now_playing: None, track_info: None, paused: false, total_duration: None, volume })
    }

    pub fn play(&mut self, path: PathBuf) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        let Ok(file) = File::open(&path) else { return };
        let Ok(source) = Decoder::new(BufReader::new(file)) else { return };
        let Ok(sink) = Sink::try_new(&self.handle) else { return };
        sink.set_volume(self.volume);
        self.total_duration = source.total_duration();
        sink.append(source);
        self.track_info = read_tags(&path);
        self.now_playing = Some(path);
        self.paused = false;
        self.sink = Some(sink);
    }

    pub fn toggle_pause(&mut self) {
        if let Some(sink) = &self.sink {
            if sink.is_paused() {
                sink.play();
                self.paused = false;
            } else {
                sink.pause();
                self.paused = true;
            }
        }
    }

    pub fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.now_playing = None;
        self.track_info = None;
        self.paused = false;
        self.total_duration = None;
    }

    pub fn volume_up(&mut self) {
        self.set_volume((self.volume + 0.05).min(2.0));
    }

    pub fn volume_down(&mut self) {
        self.set_volume((self.volume - 0.05).max(0.0));
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        if let Some(sink) = &self.sink {
            sink.set_volume(self.volume);
        }
    }

    pub fn position(&self) -> Option<Duration> {
        self.sink.as_ref().map(|s| s.get_pos())
    }

    pub fn is_finished(&self) -> bool {
        self.sink.as_ref().map(|s| s.empty()).unwrap_or(true)
    }

    pub fn reload_track_info(&mut self) {
        if let Some(path) = &self.now_playing {
            self.track_info = read_tags(&path);
        }
    }
}

/// Write title/artist/album to the file's tags. Returns true on success.
pub fn save_tags(path: &Path, title: &str, artist: &str, album: &str) -> bool {
    let Ok(mut tagged) = Probe::open(path).and_then(|p| p.read()) else { return false };

    if tagged.primary_tag().is_none() {
        // Pick a sensible default tag type based on extension
        let tag_type = match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
            Some("flac") => TagType::VorbisComments,
            Some("ogg" | "opus") => TagType::VorbisComments,
            _ => TagType::Id3v2,
        };
        tagged.insert_tag(Tag::new(tag_type));
    }

    let Some(tag) = tagged.primary_tag_mut() else { return false };
    tag.set_title(title.to_string());
    tag.set_artist(artist.to_string());
    tag.set_album(album.to_string());
    tag.save_to_path(path, WriteOptions::default()).is_ok()
}

pub fn read_tags(path: &Path) -> Option<TrackInfo> {
    let tagged = Probe::open(path).ok()?.read().ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    Some(TrackInfo {
        title: tag.title().map(|t| t.to_string()),
        artist: tag.artist().map(|a| a.to_string()),
        album: tag.album().map(|a| a.to_string()),
    })
}

pub fn fmt_duration(d: Duration) -> String {
    let secs = d.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}
