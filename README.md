# tui-fi

A terminal music player written in Rust. Browse your filesystem, manage playlists, and play audio.

## Features

- File browser with directory navigation and audio file filtering
- Multiple playlists with persistent storage
- Playback of MP3, FLAC, OGG, WAV, AAC, M4A, and Opus files
- Shuffle and repeat modes (off / all / one)
- Volume control (persisted between sessions)
- In-place tag editor (title, artist, album)
- Play history (tracks logged after 30 seconds of playback)
- Live search/filter in both browser and playlist panels
- M3U playlist import and export
- YouTube audio download via `yt-dlp` (press `y` to paste a URL)
- MPRIS2 integration

## Installation

### Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)
- `yt-dlp` on your `PATH` if you want the YouTube download feature (optional)

### Build from source

```sh
git clone git@github.com:BrionGahl/tui-fi.git
cd tui-fi
cargo install --path .
```

## Usage

```sh
tui-fi
```

### Key bindings

| Key | Action |
|-----|--------|
| `Tab` | Switch focus between Browser and Playlist panels |
| `j` / `k` or `↓` / `↑` | Move cursor |
| `Enter` | Play file / enter directory / import M3U |
| `l` / `→` | Enter directory |
| `h` / `←` / `Backspace` | Go up one directory |
| `a` | Add selected file to current playlist |
| `d` | Remove selected track from playlist |
| `J` / `K` | Move track down / up in playlist |
| `e` | Open tag editor for selected file or track |
| `Space` | Pause / resume |
| `s` | Stop playback |
| `[` / `]` | Previous / next track in playlist |
| `+` / `=` | Volume up |
| `-` | Volume down |
| `z` | Toggle shuffle |
| `p` | Cycle repeat mode (off → all → one) |
| `/` | Search / filter current panel |
| `H` | Show play history |
| `y` | Enter a YouTube URL to download audio |
| `r` | Refresh browser directory |
| `n` | Create a new playlist |
| `<` / `,` | Previous playlist |
| `>` / `.` | Next playlist |
| `X` | Export current playlist to M3U |
| `q` | Quit |

## Data locations

| Data | Path |
|------|------|
| Config (volume) | `~/.config/tui-fi/config.json` |
| Playlists | `~/.local/share/tui-fi/playlists/` |
| Downloaded audio | `~/Music/tui-fi/` |
