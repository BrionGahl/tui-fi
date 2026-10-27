use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use image::DynamicImage;
use lofty::file::{AudioFile, TaggedFile};
use lofty::picture::PictureType;
use lofty::prelude::{Accessor, TaggedFileExt};
use lofty::probe::Probe;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;

use crate::player::TrackInfo;

const FOLDER_COVERS: &[&str] = &["cover", "folder", "front", "album"];
const FOLDER_EXTS: &[&str] = &["jpg", "jpeg", "png"];

/// Tags and cover art for whichever song is highlighted in the focused panel.
pub struct Preview {
    picker: Picker,
    /// The file the current data was read from, and its mtime so tag edits get picked up.
    key: Option<(PathBuf, Option<SystemTime>)>,
    pub path: Option<PathBuf>,
    pub info: Option<TrackInfo>,
    pub duration: Option<Duration>,
    pub image: Option<StatefulProtocol>,
}

impl Preview {
    /// Must be called after entering raw mode so the terminal can be queried.
    pub fn new() -> Self {
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::from_fontsize((8, 16)));
        Self { picker, key: None, path: None, info: None, duration: None, image: None }
    }

    /// Reload when the highlighted song changes or its file is modified.
    pub fn sync(&mut self, selected: Option<&Path>) {
        let selected = selected.filter(|p| crate::app::is_audio(p));
        let key = selected.map(|p| (p.to_path_buf(), mtime(p)));
        if key == self.key {
            return;
        }
        self.key = key;
        self.path = selected.map(Path::to_path_buf);
        self.info = None;
        self.duration = None;
        self.image = None;

        let Some(path) = selected else { return };
        let tagged = Probe::open(path).ok().and_then(|p| p.read().ok());
        if let Some(tagged) = &tagged {
            self.info = tag_info(tagged);
            self.duration = Some(tagged.properties().duration()).filter(|d| !d.is_zero());
        }
        self.image = tagged.as_ref()
            .and_then(embedded_cover)
            .or_else(|| folder_cover(path))
            .map(|img| self.picker.new_resize_protocol(img));
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn tag_info(tagged: &TaggedFile) -> Option<TrackInfo> {
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    Some(TrackInfo {
        title: tag.title().map(|t| t.to_string()),
        artist: tag.artist().map(|a| a.to_string()),
        album: tag.album().map(|a| a.to_string()),
    })
}

fn embedded_cover(tagged: &TaggedFile) -> Option<DynamicImage> {
    let pictures: Vec<_> = tagged.tags().iter().flat_map(|t| t.pictures()).collect();
    let pic = pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())?;
    image::load_from_memory(pic.data()).ok()
}

fn folder_cover(path: &Path) -> Option<DynamicImage> {
    let dir = path.parent()?;
    let entries: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    FOLDER_COVERS.iter().find_map(|name| {
        entries.iter().find(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).map(str::to_lowercase);
            let ext = p.extension().and_then(|s| s.to_str()).map(str::to_lowercase);
            stem.as_deref() == Some(name) && ext.is_some_and(|e| FOLDER_EXTS.contains(&e.as_str()))
        })
    })
    .and_then(|p| image::open(p).ok())
}
